use std::{
    fs, io,
    path::{Path, PathBuf},
    time::Duration,
};
fn rename_with_retry(from: &Path, to: &Path) -> io::Result<()> {
    for attempt in 0..24 {
        match fs::rename(from, to) {
            Ok(()) => return Ok(()),
            Err(error)
                if attempt < 23
                    && matches!(error.raw_os_error(), Some(5) | Some(32) | Some(33)) =>
            {
                std::thread::sleep(Duration::from_millis(250))
            }
            Err(error) => return Err(error),
        }
    }
    unreachable!()
}
fn move_path(from: &Path, to: &Path) -> Result<(), String> {
    rename_with_retry(from, to).map_err(|error| {
        crate::install_transaction::file_error(
            "mover el archivo",
            from,
            format!("{} Destino «{}».", error, to.display()),
        )
    })
}
pub fn commit(stage: &Path, backup: &Path, target: &Path) -> Result<(), String> {
    let mut old = Vec::<PathBuf>::new();
    let mut installed = Vec::<PathBuf>::new();
    let result = (|| {
        let mut entries = fs::read_dir(stage)
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            let name = PathBuf::from(entry.file_name());
            let destination = target.join(&name);
            if destination.exists() {
                move_path(&destination, &backup.join(&name))?;
                old.push(name.clone());
            }
            install_path(&entry.path(), &destination)?;
            installed.push(name);
        }
        Ok(())
    })();
    if let Err(error) = result {
        for name in installed.iter().rev() {
            let file = target.join(name);
            let remove = if file.is_dir() {
                fs::remove_dir_all(&file)
            } else {
                fs::remove_file(&file)
            };
            remove.map_err(|e| format!("{}; rollback: {}", error, e))?;
        }
        for name in old.iter().rev() {
            move_path(&backup.join(name), &target.join(name))?;
        }
        return Err(error);
    }
    Ok(())
}

// A scanner may allow reading a staged executable while denying its rename.
// Copy only incoming files, then publish the complete copy without replacing a
// file that appeared concurrently. Backups and rollback always use strict moves.
fn install_path(from: &Path, to: &Path) -> Result<(), String> {
    match rename_with_retry(from, to) {
        Ok(()) => Ok(()),
        Err(rename_error) => {
            let regular = fs::symlink_metadata(from)
                .map(|info| info.file_type().is_file())
                .unwrap_or(false);
            if !matches!(rename_error.raw_os_error(), Some(5) | Some(32) | Some(33))
                || !regular
                || to.exists()
            {
                return Err(crate::install_transaction::file_error(
                    "mover el archivo",
                    from,
                    format!("{} Destino «{}».", rename_error, to.display()),
                ));
            }
            let copy = || -> io::Result<()> {
                let mut source = fs::File::open(from)?;
                let expected = source.metadata()?.len();
                let parent = to.parent().ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidInput, "Invalid destination")
                })?;
                let mut temporary = tempfile::Builder::new()
                    .prefix(".battly-copy-")
                    .tempfile_in(parent)?;
                let copied = io::copy(&mut source, temporary.as_file_mut())?;
                if copied != expected {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "Incomplete installation copy",
                    ));
                }
                temporary.as_file().sync_all()?;
                temporary
                    .persist_noclobber(to)
                    .map_err(|error| error.error)?;
                Ok(())
            };
            copy().map_err(|error| {
                crate::install_transaction::file_error(
                    "instalar el archivo",
                    from,
                    format!("{} Destino «{}». {}", error, to.display(), rename_error),
                )
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::windows::fs::OpenOptionsExt;
    use std::process::Command;

    struct DeniedAccess {
        paths: Vec<PathBuf>,
        sid: String,
    }
    impl DeniedAccess {
        fn new() -> Self {
            let output = Command::new("whoami")
                .args(["/user", "/fo", "csv", "/nh"])
                .output()
                .unwrap();
            assert!(output.status.success());
            let value = String::from_utf8_lossy(&output.stdout);
            let sid = value
                .trim()
                .split(',')
                .last()
                .unwrap()
                .trim_matches('"')
                .to_owned();
            assert!(sid.starts_with("S-1-"));
            Self {
                paths: Vec::new(),
                sid,
            }
        }
        fn deny(&mut self, path: &Path, rights: &str) {
            let output = Command::new("icacls")
                .arg(path)
                .args(["/deny", &format!("*{}:({})", self.sid, rights)])
                .output()
                .unwrap();
            assert!(output.status.success(), "Fixture ACL could not be set");
            self.paths.push(path.to_owned());
        }
    }
    impl Drop for DeniedAccess {
        fn drop(&mut self) {
            for path in self.paths.iter().rev() {
                let _ = Command::new("icacls")
                    .arg(path)
                    .args(["/remove:d", &format!("*{}", self.sid)])
                    .output();
            }
        }
    }
    fn fixture() -> (tempfile::TempDir, PathBuf, PathBuf, PathBuf) {
        let root = tempfile::tempdir().unwrap();
        let stage = root.path().join("stage");
        let backup = root.path().join("backup");
        let target = root.path().join("target");
        for directory in [&stage, &backup, &target] {
            fs::create_dir(directory).unwrap();
        }
        for name in ["a.txt", "Battly Launcher.exe"] {
            fs::write(stage.join(name), b"new").unwrap();
            fs::write(target.join(name), b"old").unwrap();
        }
        (root, stage, backup, target)
    }
    #[test]
    fn readable_source_with_access_denied_on_rename_installs_atomically() {
        let (_root, stage, backup, target) = fixture();
        let mut acl = DeniedAccess::new();
        acl.deny(&stage, "DC");
        acl.deny(&stage.join("Battly Launcher.exe"), "DE");
        let error =
            fs::rename(stage.join("Battly Launcher.exe"), target.join("probe.exe")).unwrap_err();
        assert_eq!(error.raw_os_error(), Some(5));
        assert_eq!(fs::read(stage.join("Battly Launcher.exe")).unwrap(), b"new");
        commit(&stage, &backup, &target).unwrap();
        assert_eq!(
            fs::read(target.join("Battly Launcher.exe")).unwrap(),
            b"new"
        );
        assert_eq!(
            fs::read(backup.join("Battly Launcher.exe")).unwrap(),
            b"old"
        );
    }
    #[test]
    fn a_source_that_cannot_be_read_restores_every_previous_file() {
        let (_root, stage, backup, target) = fixture();
        let _reader = fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(stage.join("Battly Launcher.exe"))
            .unwrap();
        assert!(commit(&stage, &backup, &target).is_err());
        for name in ["a.txt", "Battly Launcher.exe"] {
            assert_eq!(fs::read(target.join(name)).unwrap(), b"old");
        }
        assert_eq!(fs::read_dir(&backup).unwrap().count(), 0);
        assert!(!fs::read_dir(&target).unwrap().any(|e| e
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".battly-copy-")));
    }
    #[test]
    fn file_creation_denied_is_detected_even_when_subdirectories_are_writable() {
        let (_root, _stage, _backup, target) = fixture();
        let mut acl = DeniedAccess::new();
        acl.deny(&target, "WD");
        fs::create_dir(target.join("subdirectory")).unwrap();
        fs::write(target.join("subdirectory/file.txt"), b"allowed").unwrap();
        assert_eq!(
            fs::write(target.join("direct.txt"), b"denied")
                .unwrap_err()
                .raw_os_error(),
            Some(5)
        );
        assert!(crate::install_transaction::probe_permissions(&target).is_err());
        assert_eq!(
            fs::read(target.join("Battly Launcher.exe")).unwrap(),
            b"old"
        );
    }
}
