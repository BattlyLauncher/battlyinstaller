use crate::release_assets::{architecture, select_zip, REPOSITORY};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::os::windows::process::CommandExt;
use std::{
    env, fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};
fn setting(args: &[String], key: &str, variable: &str) -> Option<String> {
    args.iter()
        .find_map(|a| a.strip_prefix(&format!("{}=", key)).map(String::from))
        .or_else(|| env::var(variable).ok())
}
fn github_request(
    client: &reqwest::blocking::Client,
    url: &str,
    token: Option<&str>,
) -> reqwest::blocking::RequestBuilder {
    let request = client.get(url);
    if url.starts_with("https://api.github.com/") {
        if let Some(token) = token {
            return request.bearer_auth(token);
        }
    }
    request
}

fn move_path(from: &Path, to: &Path) -> Result<(), String> {
    for attempt in 0..24 {
        match fs::rename(from, to) {
            Ok(()) => return Ok(()),
            Err(error)
                if attempt < 23
                    && matches!(error.raw_os_error(), Some(5) | Some(32) | Some(33)) =>
            {
                std::thread::sleep(Duration::from_millis(250))
            }
            Err(error) => return Err(error.to_string()),
        }
    }
    unreachable!()
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
            move_path(&entry.path(), &destination)?;
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
fn elevate(args: &[String]) -> Result<(), String> {
    use windows_sys::Win32::{
        Foundation::CloseHandle,
        System::Threading::{GetExitCodeProcess, WaitForSingleObject},
        UI::Shell::{ShellExecuteExW, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW},
    };
    fn wide(value: &str) -> Vec<u16> {
        value.encode_utf16().chain(Some(0)).collect()
    }
    fn quote(value: &str) -> String {
        let mut result = String::from("\"");
        let mut backslashes = 0;
        for c in value.chars() {
            if c == '\\' {
                backslashes += 1;
                continue;
            }
            result.push_str(&"\\".repeat(if c == '"' {
                backslashes * 2 + 1
            } else {
                backslashes
            }));
            result.push(c);
            backslashes = 0;
        }
        result.push_str(&"\\".repeat(backslashes * 2));
        result.push('"');
        result
    }
    let exe = wide(
        &env::current_exe()
            .map_err(|e| e.to_string())?
            .to_string_lossy(),
    );
    let parameters = wide(&args.iter().map(|a| quote(a)).collect::<Vec<_>>().join(" "));
    let verb = wide("runas");
    unsafe {
        let mut info: SHELLEXECUTEINFOW = std::mem::zeroed();
        info.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
        info.fMask = SEE_MASK_NOCLOSEPROCESS;
        info.lpVerb = verb.as_ptr();
        info.lpFile = exe.as_ptr();
        info.lpParameters = parameters.as_ptr();
        info.nShow = 1;
        if ShellExecuteExW(&mut info) == 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
        WaitForSingleObject(info.hProcess, u32::MAX);
        let mut code = 1;
        GetExitCodeProcess(info.hProcess, &mut code);
        CloseHandle(info.hProcess);
        if code != 0 {
            return Err(format!("Elevated installer exited with {}", code));
        }
    }
    Ok(())
}
fn update_registration(target: &Path, version: &str) {
    use winreg::{enums::*, RegKey};
    let location = target
        .to_string_lossy()
        .replace('/', "\\")
        .to_ascii_lowercase();
    for hive in [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE] {
        if let Ok(root) = RegKey::predef(hive)
            .open_subkey("Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall")
        {
            for name in root.enum_keys().flatten() {
                if let Ok(key) = root.open_subkey_with_flags(name, KEY_READ | KEY_WRITE) {
                    if key
                        .get_value::<String, _>("InstallLocation")
                        .ok()
                        .is_some_and(|v| {
                            v.replace('/', "\\")
                                .trim_end_matches('\\')
                                .to_ascii_lowercase()
                                == location.trim_end_matches('\\')
                        })
                    {
                        let _ = key.set_value("DisplayVersion", &version);
                    }
                }
            }
        }
    }
}
pub fn run() -> Result<(), String> {
    run_with_progress(|_, _| {})
}
pub fn run_with_progress(mut progress: impl FnMut(f64, &str)) -> Result<(), String> {
    progress(5.0, "Preparando la actualización...");
    let args: Vec<String> = env::args().skip(1).collect();
    let target = PathBuf::from(
        args.iter()
            .find_map(|a| a.strip_prefix("/D="))
            .ok_or("Missing update directory")?,
    );
    if !target.is_absolute()
        || target.parent().is_none()
        || !target.join("Battly Launcher.exe").is_file()
    {
        return Err("Invalid existing Battly installation".into());
    }
    let repository = setting(
        &args,
        "--battly-repository",
        "BATTLY_INSTALLER_UPDATE_REPOSITORY",
    )
    .unwrap_or_else(|| REPOSITORY.into());
    if repository.split('/').count() != 2
        || !repository
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "/_.-".contains(c))
    {
        return Err("Invalid release repository".into());
    }
    let tag = setting(&args, "--battly-tag", "BATTLY_INSTALLER_UPDATE_TAG");
    if tag.as_ref().is_some_and(|t| {
        t.is_empty()
            || !t
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "_.-".contains(c))
    }) {
        return Err("Invalid release tag".into());
    }
    let arch = setting(&args, "--battly-arch", "BATTLY_INSTALLER_UPDATE_ARCH")
        .unwrap_or_else(|| architecture().into());
    let transaction = match tempfile::Builder::new()
        .prefix(".battly-update-")
        .tempdir_in(&target)
    {
        Ok(directory) => directory,
        Err(_) if !is_elevated::is_elevated() => {
            let mut elevated = args.clone();
            if !elevated.iter().any(|a| a == "/S") {
                elevated.push("/S".into());
            }
            elevated.push(format!("--battly-repository={}", repository));
            elevated.push(format!("--battly-arch={}", arch));
            if let Some(tag) = tag {
                elevated.push(format!("--battly-tag={}", tag));
            }
            return elevate(&elevated);
        }
        Err(error) => return Err(error.to_string()),
    };
    let endpoint = format!(
        "https://api.github.com/repos/{}/releases/{}",
        repository,
        tag.as_ref()
            .map(|t| format!("tags/{}", t))
            .unwrap_or_else(|| "latest".into())
    );
    let client = reqwest::blocking::Client::builder()
        .user_agent("BattlyInstaller")
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(300))
        .build()
        .map_err(|e| e.to_string())?;
    let token = env::var("BATTLY_INSTALLER_GITHUB_TOKEN")
        .or_else(|_| env::var("BATTLY_PATCH_GITHUB_TOKEN"))
        .ok();
    let release: Value = github_request(&client, &endpoint, token.as_deref())
        .send()
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?
        .json()
        .map_err(|e| e.to_string())?;
    if tag
        .as_ref()
        .is_some_and(|t| release["tag_name"].as_str() != Some(t))
    {
        return Err("Release tag mismatch".into());
    }
    let asset = select_zip(&release, &arch)?;
    let zip_path = transaction.path().join("download.zip");
    let mut url = asset["downloadUrl"].as_str().ok_or("Missing ZIP URL")?;
    let mut api_download = false;
    if token.is_some() {
        if let Some(api) = asset["apiDownloadUrl"].as_str() {
            if !api.starts_with("https://api.github.com/repos/")
                || !api.contains("/releases/assets/")
                || !api
                    .rsplit('/')
                    .next()
                    .is_some_and(|id| !id.is_empty() && id.chars().all(|c| c.is_ascii_digit()))
            {
                return Err("Unsafe GitHub asset API URL".into());
            }
            url = api;
            api_download = true;
        }
    }
    let mut request = github_request(&client, url, token.as_deref());
    if api_download {
        request = request.header("Accept", "application/octet-stream");
    }
    let mut response = request
        .send()
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?;
    let mut file = fs::File::create(&zip_path).map_err(|e| e.to_string())?;
    let mut hash = Sha256::new();
    let mut size = 0u64;
    let mut buffer = [0u8; 65536];
    let mut last_percent = 0.0;
    loop {
        let n = response.read(&mut buffer).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        size += n as u64;
        let total = asset["size"].as_u64().unwrap_or(size.max(1));
        let percent = 10.0 + 60.0 * (size as f64 / total.max(1) as f64).min(1.0);
        if percent - last_percent >= 1.0 {
            progress(percent, "Descargando Battly...");
            last_percent = percent;
        }
        if size > 512 * 1024 * 1024 {
            return Err("Windows ZIP exceeds limit".into());
        }
        hash.update(&buffer[..n]);
        file.write_all(&buffer[..n]).map_err(|e| e.to_string())?;
    }
    drop(file);
    if asset["size"]
        .as_u64()
        .is_some_and(|expected| expected != size)
    {
        return Err("Windows ZIP size mismatch".into());
    }
    if let Some(expected) = asset["digest"].as_str() {
        if expected != format!("sha256:{:x}", hash.finalize()) {
            return Err("Windows ZIP checksum mismatch".into());
        }
    }
    progress(75.0, "Comprobando y preparando los archivos...");
    let stage = transaction.path().join("stage");
    let backup = transaction.path().join("backup");
    fs::create_dir(&stage).map_err(|e| e.to_string())?;
    fs::create_dir(&backup).map_err(|e| e.to_string())?;
    let mut zip = zip::ZipArchive::new(fs::File::open(&zip_path).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    if zip.len() > 10000 {
        return Err("Too many ZIP files".into());
    }
    let mut expanded = 0u64;
    for index in 0..zip.len() {
        let mut entry = zip.by_index(index).map_err(|e| e.to_string())?;
        let relative = entry.enclosed_name().ok_or("Unsafe ZIP path")?.to_owned();
        if relative.to_string_lossy().contains(':')
            || entry.unix_mode().is_some_and(|m| m & 0o170000 == 0o120000)
        {
            return Err("Unsafe ZIP entry".into());
        }
        expanded += entry.size();
        if expanded > 2 * 1024 * 1024 * 1024 {
            return Err("Expanded ZIP exceeds limit".into());
        }
        let out = stage.join(relative);
        if entry.is_dir() {
            fs::create_dir_all(out).map_err(|e| e.to_string())?;
        } else {
            fs::create_dir_all(out.parent().ok_or("Invalid ZIP parent")?)
                .map_err(|e| e.to_string())?;
            let mut f = fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(out)
                .map_err(|e| e.to_string())?;
            std::io::copy(&mut entry, &mut f).map_err(|e| e.to_string())?;
        }
    }
    let exe = fs::read(stage.join("Battly Launcher.exe")).map_err(|e| e.to_string())?;
    let offset =
        u32::from_le_bytes(exe.get(60..64).ok_or("Invalid PE")?.try_into().unwrap()) as usize;
    let machine = u16::from_le_bytes(
        exe.get(offset + 4..offset + 6)
            .ok_or("Invalid PE machine")?
            .try_into()
            .unwrap(),
    );
    if exe.get(..2) != Some(b"MZ")
        || exe.get(offset..offset + 4) != Some(b"PE\0\0")
        || machine != if arch == "ia32" { 0x014c } else { 0x8664 }
        || !stage.join("resources/app.asar").is_file()
    {
        return Err("Windows ZIP architecture or application mismatch".into());
    }
    progress(95.0, "Aplicando la actualización...");
    if let Err(error) = commit(&stage, &backup, &target) {
        if fs::read_dir(&backup)
            .map_err(|e| e.to_string())?
            .next()
            .is_some()
        {
            let saved = transaction.keep();
            return Err(format!("{}; recovery files: {}", error, saved.display()));
        }
        return Err(error);
    }
    update_registration(&target, asset["version"].as_str().unwrap_or(""));
    progress(100.0, "Battly se ha actualizado.");
    if args.iter().any(|a| a == "--force-run") {
        Command::new(target.join("Battly Launcher.exe"))
            .creation_flags(0x00000008)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::os::windows::fs::OpenOptionsExt;
    #[test]
    fn private_release_credentials_never_go_to_a_zip_host_or_a_test_server() {
        let client = reqwest::blocking::Client::new();
        for url in [
            "http://127.0.0.1:18800/test",
            "https://github.com/test/asset",
            "https://api.github.com.evil.test/asset",
        ] {
            assert!(!github_request(&client, url, Some("fixture-only"))
                .build()
                .unwrap()
                .headers()
                .contains_key("authorization"));
        }
        assert_eq!(
            github_request(
                &client,
                "https://api.github.com/repos/test/private/releases/latest",
                Some("fixture-only")
            )
            .build()
            .unwrap()
            .headers()
            .get("authorization")
            .unwrap(),
            "Bearer fixture-only"
        );
    }
    #[test]
    fn locked_previous_file_restores_every_already_replaced_file() {
        let root = tempfile::tempdir().unwrap();
        let stage = root.path().join("stage");
        let backup = root.path().join("backup");
        let target = root.path().join("target");
        for directory in [&stage, &backup, &target] {
            fs::create_dir(directory).unwrap();
        }
        for name in ["a.txt", "b.txt"] {
            fs::write(stage.join(name), "new").unwrap();
            fs::write(target.join(name), "old").unwrap();
        }
        let lock = fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(target.join("b.txt"))
            .unwrap();
        assert!(commit(&stage, &backup, &target).is_err());
        drop(lock);
        assert!(
            !stage.join("a.txt").exists(),
            "The first file must have been committed before rollback"
        );
        assert_eq!(fs::read_to_string(target.join("a.txt")).unwrap(), "old");
        assert_eq!(fs::read_to_string(target.join("b.txt")).unwrap(), "old");
        assert_eq!(fs::read_dir(&backup).unwrap().count(), 0);
    }
}
