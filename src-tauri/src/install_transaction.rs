use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::os::windows::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use sysinfo::{PidExt, ProcessExt, System, SystemExt};

pub fn file_error(action: &str, file: &Path, error: impl std::fmt::Display) -> String {
    format!("No se pudo {} «{}». {}", action, file.display(), error)
}

/// Check actual write access, including custom locations and existing ACLs.
pub fn probe_permissions(target: &Path) -> io::Result<()> {
    let mut directory = target;
    loop {
        match fs::metadata(directory) {
            Ok(info) if info.is_dir() => break,
            Ok(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::NotADirectory,
                    "La ruta no es una carpeta",
                ))
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                directory = directory.parent().ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "La carpeta no tiene un directorio válido",
                    )
                })?;
            }
            Err(error) => return Err(error),
        }
    }
    let probe = tempfile::Builder::new()
        .prefix(".battly-permission-")
        .tempdir_in(directory)?;
    fs::write(probe.path().join("write-test"), b"Battly")?;
    // Creating a subdirectory does not prove FILE_ADD_FILE on its parent.
    // Existing ACLs may allow the old probe while rejecting the new executable.
    let mut direct = tempfile::Builder::new()
        .prefix(".battly-file-permission-")
        .tempfile_in(directory)?;
    direct.write_all(b"Battly")?;
    direct.as_file().sync_all()?;
    direct.close()?;
    probe.close()
}

fn path_key(path: &Path) -> String {
    path.to_string_lossy().replace('/', "\\").to_lowercase()
}

/// Only stop executables from the installation being replaced.
pub fn close_battly(target: &Path, exe_name: &str) -> Result<(), String> {
    let target =
        fs::canonicalize(target).map_err(|e| file_error("comprobar la carpeta", target, e))?;
    let target_key = path_key(&target);
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut system = System::new_all();
    loop {
        system.refresh_processes();
        let mut running = false;
        for process in system.processes().values() {
            if process.pid().as_u32() == std::process::id() {
                continue;
            }
            let expected_name = process.name().eq_ignore_ascii_case(exe_name)
                || process.name().eq_ignore_ascii_case("BattlyLauncher.exe");
            if !expected_name {
                continue;
            }
            let executable = process.exe();
            let parent = executable.parent().and_then(|p| fs::canonicalize(p).ok());
            if parent.as_ref().is_some_and(|p| path_key(p) == target_key) {
                running = true;
                if !process.kill() {
                    return Err(file_error(
                        "cerrar Battly",
                        executable,
                        "Cierra Battly desde el Administrador de tareas y vuelve a intentarlo. Comprueba que tienes permisos para cerrarlo.",
                    ));
                }
            }
        }
        if !running {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(file_error(
                "cerrar Battly",
                &target.join(exe_name),
                "El proceso sigue abierto.",
            ));
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

pub fn check_replaceable(stage: &Path, target: &Path) -> Result<(), String> {
    for entry in walkdir::WalkDir::new(stage).min_depth(1) {
        let entry = entry.map_err(|e| e.to_string())?;
        let relative = entry
            .path()
            .strip_prefix(stage)
            .map_err(|e| e.to_string())?;
        let destination = target.join(relative);
        let info = match fs::symlink_metadata(&destination) {
            Ok(info) => info,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(file_error("comprobar el archivo", &destination, error)),
        };
        if info.file_type().is_symlink() || info.is_dir() != entry.file_type().is_dir() {
            return Err(file_error(
                "reemplazar el archivo",
                &destination,
                "El tipo de archivo no coincide con el paquete.",
            ));
        }
        if info.is_file() {
            if info.permissions().readonly() {
                return Err(file_error(
                    "reemplazar el archivo",
                    &destination,
                    "El archivo es de solo lectura. Revisa sus permisos.",
                ));
            }
            // Do not truncate the existing file just to discover a sharing violation.
            OpenOptions::new()
                .read(true)
                .write(true)
                .share_mode(0)
                .open(&destination)
                .map_err(|e| file_error("abrir el archivo para actualizarlo", &destination, e))?;
        }
    }
    Ok(())
}

pub fn validate_application(stage: &Path, exe_name: &str, arch: &str) -> Result<(), String> {
    let executable = stage.join(exe_name);
    let bytes =
        fs::read(&executable).map_err(|e| file_error("leer el ejecutable", &executable, e))?;
    let invalid = || {
        file_error(
            "validar el ejecutable",
            &executable,
            "El paquete está incompleto o no corresponde a esta arquitectura.",
        )
    };
    let offset =
        u32::from_le_bytes(bytes.get(60..64).ok_or_else(invalid)?.try_into().unwrap()) as usize;
    if offset > bytes.len().saturating_sub(6) {
        return Err(invalid());
    }
    let machine = u16::from_le_bytes(
        bytes
            .get(offset + 4..offset + 6)
            .ok_or_else(invalid)?
            .try_into()
            .unwrap(),
    );
    if bytes.get(..2) != Some(b"MZ")
        || bytes.get(offset..offset + 4) != Some(b"PE\0\0")
        || machine != if arch == "ia32" { 0x014c } else { 0x8664 }
    {
        return Err(invalid());
    }
    let app = stage.join("resources/app.asar");
    if !app.is_file() {
        return Err(file_error(
            "validar el paquete",
            &app,
            "Falta el archivo de la aplicación.",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::process::{Child, Command};

    const EXE: &str = "Battly Launcher.exe";

    fn pe(arch: &str) -> Vec<u8> {
        let mut bytes = vec![0u8; 128];
        bytes[..2].copy_from_slice(b"MZ");
        bytes[60..64].copy_from_slice(&64u32.to_le_bytes());
        bytes[64..68].copy_from_slice(b"PE\0\0");
        bytes[68..70]
            .copy_from_slice(&if arch == "ia32" { 0x014cu16 } else { 0x8664u16 }.to_le_bytes());
        bytes
    }

    fn package(root: &Path, arch: &str, extra: Option<&str>) -> PathBuf {
        let path = root.join("package.zip");
        let mut writer = zip::ZipWriter::new(fs::File::create(&path).unwrap());
        let options = zip::write::FileOptions::default();
        for (name, bytes) in [
            (EXE, pe(arch)),
            ("resources/app.asar", b"new-app".to_vec()),
            ("a.txt", b"new-a".to_vec()),
        ] {
            writer.start_file(name, options).unwrap();
            writer.write_all(&bytes).unwrap();
        }
        if let Some(name) = extra {
            writer.start_file(name, options).unwrap();
            writer.write_all(b"extra").unwrap();
        }
        writer.finish().unwrap();
        path
    }

    fn previous(root: &Path) -> PathBuf {
        let target = root.join("installed");
        fs::create_dir_all(target.join("resources")).unwrap();
        fs::write(target.join(EXE), b"previous-executable").unwrap();
        fs::write(target.join("resources/app.asar"), b"previous-app").unwrap();
        fs::write(target.join("a.txt"), b"previous-a").unwrap();
        fs::write(target.join("user-preferences.json"), b"preserved").unwrap();
        target
    }

    fn unchanged(target: &Path) {
        assert_eq!(fs::read(target.join(EXE)).unwrap(), b"previous-executable");
        assert_eq!(
            fs::read(target.join("resources/app.asar")).unwrap(),
            b"previous-app"
        );
        assert_eq!(fs::read(target.join("a.txt")).unwrap(), b"previous-a");
        assert_eq!(
            fs::read(target.join("user-preferences.json")).unwrap(),
            b"preserved"
        );
    }

    #[test]
    fn locked_file_reports_its_path_without_replacing_any_previous_bytes() {
        let root = tempfile::tempdir().unwrap();
        let target = previous(root.path());
        let archive = package(root.path(), "x64", None);
        let lock = OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(target.join("resources/app.asar"))
            .unwrap();
        let error = install_archive(&archive, &target, EXE, "x64", |_, _, _| {}).unwrap_err();
        assert!(error.contains("app.asar"), "{}", error);
        drop(lock);
        unchanged(&target);
        install_archive(&archive, &target, EXE, "x64", |_, _, _| {}).unwrap();
        assert_eq!(
            fs::read(target.join("resources/app.asar")).unwrap(),
            b"new-app"
        );
        assert_eq!(
            fs::read(target.join("user-preferences.json")).unwrap(),
            b"preserved"
        );
    }

    #[test]
    fn read_only_file_is_reported_before_the_previous_installation_changes() {
        let root = tempfile::tempdir().unwrap();
        let target = previous(root.path());
        let archive = package(root.path(), "x64", None);
        let file = target.join(EXE);
        let mut permissions = fs::metadata(&file).unwrap().permissions();
        permissions.set_readonly(true);
        fs::set_permissions(&file, permissions).unwrap();
        let result = install_archive(&archive, &target, EXE, "x64", |_, _, _| {});
        let mut permissions = fs::metadata(&file).unwrap().permissions();
        permissions.set_readonly(false);
        fs::set_permissions(&file, permissions).unwrap();
        assert!(result.unwrap_err().contains(EXE));
        unchanged(&target);
    }

    #[test]
    fn failed_installation_keeps_the_previous_executable_runnable() {
        let root = tempfile::tempdir().unwrap();
        let target = previous(root.path());
        let executable = target.join(EXE);
        fs::copy(std::env::current_exe().unwrap(), &executable).unwrap();
        let original = fs::read(&executable).unwrap();
        let archive = package(root.path(), "x64", None);
        let lock = OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(target.join("resources/app.asar"))
            .unwrap();
        assert!(install_archive(&archive, &target, EXE, "x64", |_, _, _| {}).is_err());
        drop(lock);
        assert_eq!(fs::read(&executable).unwrap(), original);
        assert_eq!(
            fs::read(target.join("resources/app.asar")).unwrap(),
            b"previous-app"
        );
        let status = Command::new(executable)
            .args([
                "--exact",
                "install_transaction::tests::sleeping_battly_process",
                "--ignored",
            ])
            .status()
            .unwrap();
        assert!(status.success());
    }

    #[test]
    fn wrong_architecture_or_bad_pe_does_not_touch_the_previous_installation() {
        let root = tempfile::tempdir().unwrap();
        let target = previous(root.path());
        let archive = package(root.path(), "ia32", None);
        assert!(install_archive(&archive, &target, EXE, "x64", |_, _, _| {})
            .unwrap_err()
            .contains(EXE));
        unchanged(&target);
        let stage = root.path().join("bad");
        fs::create_dir(&stage).unwrap();
        let mut bytes = pe("x64");
        bytes[60..64].copy_from_slice(&u32::MAX.to_le_bytes());
        fs::write(stage.join(EXE), bytes).unwrap();
        assert!(validate_application(&stage, EXE, "x64").is_err());
    }

    #[test]
    fn incomplete_zip_and_duplicate_entries_leave_the_old_installation_intact() {
        let root = tempfile::tempdir().unwrap();
        let target = previous(root.path());
        let archive = package(root.path(), "x64", Some("resources/app.asar"));
        assert!(install_archive(&archive, &target, EXE, "x64", |_, _, _| {})
            .unwrap_err()
            .contains("app.asar"));
        unchanged(&target);
        fs::write(&archive, b"incomplete zip").unwrap();
        assert!(install_archive(&archive, &target, EXE, "x64", |_, _, _| {}).is_err());
        unchanged(&target);
    }

    #[test]
    fn zip_cannot_escape_the_destination() {
        let root = tempfile::tempdir().unwrap();
        let target = previous(root.path());
        let archive = package(root.path(), "x64", Some("../escaped.txt"));
        assert!(install_archive(&archive, &target, EXE, "x64", |_, _, _| {}).is_err());
        assert!(!root.path().join("escaped.txt").exists());
        unchanged(&target);
    }

    #[test]
    fn both_architectures_install_and_do_not_overwrite_unrelated_user_files() {
        for arch in ["x64", "ia32"] {
            let root = tempfile::tempdir().unwrap();
            let target = previous(root.path());
            let archive = package(root.path(), arch, None);
            install_archive(&archive, &target, EXE, arch, |_, _, _| {}).unwrap();
            assert_eq!(fs::read(target.join(EXE)).unwrap(), pe(arch));
            assert_eq!(
                fs::read(target.join("resources/app.asar")).unwrap(),
                b"new-app"
            );
            assert_eq!(
                fs::read(target.join("user-preferences.json")).unwrap(),
                b"preserved"
            );
            assert!(fs::read_dir(&target).unwrap().all(|entry| !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".battly-")));
        }
    }

    #[test]
    fn permission_probe_checks_custom_destinations_and_cleans_up_after_itself() {
        let root = tempfile::tempdir().unwrap();
        probe_permissions(&root.path().join("new/nested/installation")).unwrap();
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
        let file = root.path().join("not-a-directory");
        fs::write(&file, "file").unwrap();
        assert!(probe_permissions(&file).is_err());
    }

    #[test]
    fn windows_acl_denial_is_detected_before_touching_the_old_installation() {
        let root = tempfile::tempdir().unwrap();
        let target = previous(root.path());
        let archive = package(root.path(), "x64", None);
        let user = Command::new("whoami")
            .args(["/user", "/fo", "csv", "/nh"])
            .output()
            .unwrap();
        assert!(user.status.success());
        let user = String::from_utf8_lossy(&user.stdout);
        let sid = user.trim().split(',').last().unwrap().trim_matches('"');
        assert!(sid.starts_with("S-1-"));
        let status = Command::new("icacls")
            .arg(&target)
            .args(["/deny", &format!("*{}:(W)", sid)])
            .output()
            .unwrap();
        assert!(status.status.success());
        let permissions = crate::check_install_permissions(target.to_string_lossy().into_owned());
        let result = install_archive(&archive, &target, EXE, "x64", |_, _, _| {});
        let restored = Command::new("icacls")
            .arg(&target)
            .args(["/remove:d", &format!("*{}", sid)])
            .output()
            .unwrap();
        assert!(restored.status.success());
        assert_eq!(permissions.unwrap(), false);
        assert!(result
            .unwrap_err()
            .contains(&target.to_string_lossy().to_string()));
        unchanged(&target);
    }

    #[test]
    #[ignore]
    fn sleeping_battly_process() {
        if std::env::var("BATTLY_INSTALLER_SLEEP_FIXTURE").as_deref() == Ok("1") {
            std::thread::sleep(Duration::from_secs(30));
        }
    }

    struct FixtureChild(Child);
    impl Drop for FixtureChild {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    fn start_fixture(target: &Path) -> FixtureChild {
        fs::create_dir_all(target).unwrap();
        let exe = target.join(EXE);
        fs::copy(std::env::current_exe().unwrap(), &exe).unwrap();
        FixtureChild(
            Command::new(exe)
                .args([
                    "--exact",
                    "install_transaction::tests::sleeping_battly_process",
                    "--ignored",
                ])
                .env("BATTLY_INSTALLER_SLEEP_FIXTURE", "1")
                .spawn()
                .unwrap(),
        )
    }

    #[test]
    fn closes_only_the_battly_process_in_the_destination_before_extracting() {
        let root = tempfile::tempdir().unwrap();
        let target = root.path().join("installed");
        let mut owned = start_fixture(&target);
        let mut other = start_fixture(&root.path().join("another-installation"));
        let archive = package(root.path(), "x64", None);
        install_archive(&archive, &target, EXE, "x64", |_, _, _| {}).unwrap();
        assert!(owned.0.try_wait().unwrap().is_some());
        assert!(other.0.try_wait().unwrap().is_none());
    }
}

/// Extraction never writes into the existing installation. Commit keeps a rollback copy.
pub fn install_archive(
    archive: &Path,
    target: &Path,
    exe_name: &str,
    arch: &str,
    mut progress: impl FnMut(usize, usize, &Path),
) -> Result<(), String> {
    probe_permissions(target).map_err(|e| file_error("escribir en la carpeta", target, e))?;
    fs::create_dir_all(target).map_err(|e| file_error("crear la carpeta", target, e))?;
    close_battly(target, exe_name)?;
    let transaction = tempfile::Builder::new()
        .prefix(".battly-install-")
        .tempdir_in(target)
        .map_err(|e| file_error("preparar la instalación", target, e))?;
    let stage = transaction.path().join("stage");
    let backup = transaction.path().join("backup");
    fs::create_dir(&stage).map_err(|e| file_error("crear la carpeta temporal", &stage, e))?;
    fs::create_dir(&backup).map_err(|e| file_error("crear la copia de seguridad", &backup, e))?;
    let file = fs::File::open(archive).map_err(|e| file_error("abrir el paquete", archive, e))?;
    let mut zip =
        zip::ZipArchive::new(file).map_err(|e| file_error("leer el paquete", archive, e))?;
    if zip.len() > 10_000 {
        return Err("El paquete contiene demasiados archivos.".into());
    }
    let count = zip.len();
    let mut expanded = 0u64;
    for index in 0..count {
        let mut entry = zip
            .by_index(index)
            .map_err(|e| file_error("leer el paquete", archive, e))?;
        let relative = entry
            .enclosed_name()
            .ok_or("El paquete contiene una ruta no permitida.")?
            .to_owned();
        if relative.as_os_str().is_empty()
            || relative.to_string_lossy().contains(':')
            || entry.unix_mode().is_some_and(|m| m & 0o170000 == 0o120000)
        {
            return Err(format!(
                "El paquete contiene una ruta no permitida «{}».",
                relative.display()
            ));
        }
        expanded = expanded
            .checked_add(entry.size())
            .ok_or("El paquete es demasiado grande.")?;
        if expanded > 2 * 1024 * 1024 * 1024 {
            return Err("El paquete descomprimido supera el tamaño permitido.".into());
        }
        let out = stage.join(&relative);
        let destination = target.join(&relative);
        progress(index + 1, count, &relative);
        if entry.is_dir() {
            fs::create_dir_all(&out)
                .map_err(|e| file_error("crear la carpeta", &destination, e))?;
        } else {
            fs::create_dir_all(out.parent().ok_or("Ruta de archivo no válida.")?)
                .map_err(|e| file_error("preparar el archivo", &destination, e))?;
            let mut file = OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&out)
                .map_err(|e| file_error("extraer el archivo", &destination, e))?;
            io::copy(&mut entry, &mut file)
                .map_err(|e| file_error("extraer el archivo", &destination, e))?;
            file.sync_all()
                .map_err(|e| file_error("guardar el archivo", &destination, e))?;
        }
    }
    validate_application(&stage, exe_name, arch)?;
    // Battly may have been reopened while the ZIP was being prepared.
    close_battly(target, exe_name)?;
    probe_permissions(target).map_err(|e| file_error("escribir en la carpeta", target, e))?;
    check_replaceable(&stage, target)?;
    if let Err(error) = crate::update_cli::commit(&stage, &backup, target) {
        if fs::read_dir(&backup)
            .map(|mut entries| entries.next().is_some())
            .unwrap_or(true)
        {
            let recovery: PathBuf = transaction.keep();
            return Err(format!(
                "{} La copia anterior se conserva en «{}».",
                error,
                recovery.display()
            ));
        }
        return Err(format!(
            "{} La instalación anterior se ha restaurado.",
            error
        ));
    }
    Ok(())
}
