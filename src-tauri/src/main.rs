#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

mod release_assets;
mod update_cli;
use serde::{Deserialize, Serialize};
use std::env;
use std::ffi::OsStr;
use std::fs;
use std::io::Cursor;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use sysinfo::{ProcessExt, System, SystemExt};
use tauri::{Manager, Window};
use walkdir::WalkDir;
use windows_sys::Win32::System::Threading::DETACHED_PROCESS;
use windows_sys::Win32::UI::Shell::ShellExecuteW;
use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOW;
use winreg::enums::*;
use winreg::RegKey;

#[derive(Serialize, Deserialize, Debug)]
struct InstallPayload {
    appId: String,
    appName: String,
    version: String,
    publisher: String,
    downloadUrl: String,
    targetDir: String,
    scope: String,
    installOpera: bool,
    mode: String,
    openByeURL: Option<bool>,
}

#[derive(Serialize, Clone)]
struct ProgressPayload {
    percent: f64,
    phase: String,
    message: String,
}

#[tauri::command]
fn choose_directory() -> Option<String> {
    let result = tauri::api::dialog::blocking::FileDialogBuilder::new().pick_folder();
    result.map(|p| p.to_string_lossy().to_string())
}

#[tauri::command]
fn check_admin() -> bool {
    is_elevated::is_elevated()
}

fn to_wstring(str: &str) -> Vec<u16> {
    OsStr::new(str)
        .encode_wide()
        .chain(Some(0).into_iter())
        .collect()
}

#[tauri::command]
fn elevate_if_needed(need_admin: bool, state: Option<String>) -> bool {
    if need_admin && !is_elevated::is_elevated() {
        let current_exe = env::current_exe().unwrap();
        let mut args: Vec<String> = env::args().skip(1).collect();

        // Append state if provided
        if let Some(s) = state {
            args.push("--state".to_string());
            args.push(s);
        }

        let file = to_wstring(&current_exe.to_string_lossy());
        let args_str = args
            .iter()
            .map(|a| format!("\"{}\"", a))
            .collect::<Vec<_>>()
            .join(" ");
        let params = to_wstring(&args_str);
        let operation = to_wstring("runas");

        unsafe {
            ShellExecuteW(
                0,
                operation.as_ptr(),
                file.as_ptr(),
                params.as_ptr(),
                std::ptr::null(),
                SW_SHOW,
            );
        }

        std::process::exit(0);
    }
    true
}

#[tauri::command]
fn get_startup_args() -> Vec<String> {
    env::args().collect()
}

#[tauri::command]
fn get_architecture() -> serde_json::Value {
    serde_json::json!({"ok":true,"arch":release_assets::architecture()})
}
#[tauri::command]
async fn get_latest_version() -> Result<serde_json::Value, String> {
    let response = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| e.to_string())?
        .get(format!(
            "https://api.github.com/repos/{}/releases/latest",
            release_assets::REPOSITORY
        ))
        .header("User-Agent", "BattlyInstaller")
        .send()
        .await
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?;
    let release: serde_json::Value = response.json().await.map_err(|e| e.to_string())?;
    release_assets::select_zip(&release, release_assets::architecture())
}

#[tauri::command]
async fn get_installer_config() -> Result<serde_json::Value, String> {
    let client = reqwest::Client::new();
    let resp = client
        .get("https://api.battlylauncher.com/v3/launcher/config-launcher/config.json")
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if resp.status().is_success() {
        let json: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
        Ok(serde_json::json!({
            "ok": true,
            "forceOpera": json["forceOpera"].as_bool().unwrap_or(false)
        }))
    } else {
        Ok(serde_json::json!({ "ok": false }))
    }
}

#[tauri::command]
fn is_uninstall_mode() -> bool {
    env::args().any(|arg| arg == "/uninstall" || arg == "--uninstall")
}

#[tauri::command]
fn is_silent_mode() -> bool {
    env::args().any(|arg| arg == "/silent" || arg == "--silent")
}

#[tauri::command]
fn is_running(exe_name: String) -> bool {
    let s = System::new_all();
    for process in s.processes_by_exact_name(&exe_name) {
        return true;
    }
    false
}

#[tauri::command]
fn is_running_by_pid(pid: usize) -> bool {
    let s = System::new_all();
    s.process(sysinfo::Pid::from(pid)).is_some()
}

#[tauri::command]
fn kill_process(exe_name: String) -> bool {
    let s = System::new_all();
    let mut killed = false;
    for process in s.processes_by_exact_name(&exe_name) {
        process.kill();
        killed = true;
    }
    killed
}

#[tauri::command]
fn get_paths(target_name: String) -> serde_json::Value {
    let local_app_data = dirs::data_local_dir().unwrap_or(PathBuf::from("C:\\ProgramData"));
    let program_files =
        PathBuf::from(env::var("ProgramFiles").unwrap_or("C:\\Program Files".to_string()));

    serde_json::json!({
        "localAppData": local_app_data.join("Programs").join(&target_name).to_string_lossy(),
        "programFiles": program_files.join(&target_name).to_string_lossy()
    })
}

#[tauri::command]
fn detect_existing(app_id: String) -> serde_json::Value {
    // Check registry
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    let path = format!(
        "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\{}",
        app_id
    );

    if let Ok(key) = hkcu.open_subkey(&path) {
        let version: String = key.get_value("DisplayVersion").unwrap_or_default();
        let install_location: String = key.get_value("InstallLocation").unwrap_or_default();
        return serde_json::json!({ "ok": true, "exists": true, "version": version, "path": install_location, "scope": "current" });
    }

    if let Ok(key) = hklm.open_subkey(&path) {
        let version: String = key.get_value("DisplayVersion").unwrap_or_default();
        let install_location: String = key.get_value("InstallLocation").unwrap_or_default();
        return serde_json::json!({ "ok": true, "exists": true, "version": version, "path": install_location, "scope": "all" });
    }

    serde_json::json!({ "ok": true, "exists": false })
}

async fn install_opera(payload: &InstallPayload) {
    if !payload.installOpera {
        return;
    }

    // Download path
    let temp_dir = env::temp_dir().join("BattlyInstaller");
    let _ = fs::create_dir_all(&temp_dir);
    let opera_path = temp_dir.join("OperaGXSetup.exe");

    let should_download = if let Ok(meta) = fs::metadata(&opera_path) {
        meta.len() < 1000000
    } else {
        true
    };

    if should_download {
        let client = reqwest::Client::new();
        let url = "https://net.geo.opera.com/opera_gx/stable/edition/std-1?utm_source=battly&utm_medium=pb&utm_campaign=gx";
        if let Ok(resp) = client
            .get(url)
            .header("User-Agent", "BattlyInstaller")
            .send()
            .await
        {
            if let Ok(bytes) = resp.bytes().await {
                let _ = fs::write(&opera_path, bytes);
            }
        }
    }

    if opera_path.exists() {
        let all_users = if payload.scope == "all" { "1" } else { "0" };
        // Detached process to ensure it survives installer exit
        let _ = Command::new(&opera_path)
            .arg("--silent")
            .arg(format!("--allusers={}", all_users))
            .creation_flags(DETACHED_PROCESS)
            .spawn();
    }
}

#[tauri::command]
async fn do_install(window: Window, payload: InstallPayload) -> Result<serde_json::Value, String> {
    if payload.mode == "uninstall" {
        return perform_uninstall(window, payload);
    }

    let target_dir = Path::new(&payload.targetDir);
    let exe_name = format!("{}.exe", payload.appName);

    // 0. Kill existing process if running
    window
        .emit(
            "install-progress",
            ProgressPayload {
                percent: 0.0,
                phase: "prep".into(),
                message: "Verificando procesos...".into(),
            },
        )
        .unwrap();
    {
        let mut s = System::new_all();
        s.refresh_processes();
        let mut killed = false;
        // Check for "Battly Launcher.exe" and "Battly Launcher"
        let targets = vec![exe_name.clone(), payload.appName.clone()];
        for name in targets {
            for process in s.processes_by_exact_name(&name) {
                process.kill();
                killed = true;
            }
        }
        if killed {
            std::thread::sleep(std::time::Duration::from_secs(2));
        }
    }

    // 1. Create directory
    window
        .emit(
            "install-progress",
            ProgressPayload {
                percent: 0.0,
                phase: "prep".into(),
                message: "Creando directorios...".into(),
            },
        )
        .unwrap();
    fs::create_dir_all(target_dir).map_err(|e| e.to_string())?;

    // 2. Download with progress
    window
        .emit(
            "install-progress",
            ProgressPayload {
                percent: 0.0,
                phase: "download:start".into(),
                message: "Iniciando descarga...".into(),
            },
        )
        .unwrap();
    let mut response = reqwest::get(&payload.downloadUrl)
        .await
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?;
    let total_size = response.content_length().unwrap_or(0);

    let temp_zip_path = target_dir.join("temp_update.zip");
    let mut temp_file = fs::File::create(&temp_zip_path).map_err(|e| e.to_string())?;

    let mut downloaded: u64 = 0;
    let mut last_emit = 0;

    while let Some(chunk) = response.chunk().await.map_err(|e| e.to_string())? {
        std::io::Write::write_all(&mut temp_file, &chunk).map_err(|e| e.to_string())?;
        downloaded += chunk.len() as u64;

        if total_size > 0 {
            // Map 0-100% download to 14-70% overall
            let download_pct = downloaded as f64 / total_size as f64;
            let overall_percent = 14.0 + (download_pct * 56.0);

            // Emit every 5MB
            if downloaded - last_emit > 5 * 1024 * 1024 {
                window
                    .emit(
                        "install-progress",
                        ProgressPayload {
                            percent: overall_percent,
                            phase: "download:progress".into(),
                            message: format!(
                                "Descargando: {:.1} MB / {:.1} MB",
                                downloaded as f64 / 1024.0 / 1024.0,
                                total_size as f64 / 1024.0 / 1024.0
                            ),
                        },
                    )
                    .unwrap();
                last_emit = downloaded;
            }
        }
    }

    window
        .emit(
            "install-progress",
            ProgressPayload {
                percent: 0.0,
                phase: "download:done".into(),
                message: "Descarga completada".into(),
            },
        )
        .unwrap();

    // 3. Extract with progress
    window
        .emit(
            "install-progress",
            ProgressPayload {
                percent: 0.0,
                phase: "extract:start".into(),
                message: "Descomprimiendo archivos...".into(),
            },
        )
        .unwrap();

    let file = fs::File::open(&temp_zip_path).map_err(|e| e.to_string())?;
    let reader = std::io::BufReader::new(file);
    let mut archive = zip::ZipArchive::new(reader).map_err(|e| e.to_string())?;
    let len = archive.len();

    for i in 0..len {
        let mut file = archive.by_index(i).map_err(|e| e.to_string())?;
        let outpath = match file.enclosed_name() {
            Some(path) => target_dir.join(path),
            None => continue,
        };

        // Emit progress every 5 files
        if i % 5 == 0 {
            // Map 0-100% extract to 75-85% overall
            let extract_pct = i as f64 / len as f64;
            let overall_percent = 75.0 + (extract_pct * 10.0);

            window
                .emit(
                    "install-progress",
                    ProgressPayload {
                        percent: overall_percent,
                        phase: "extract:progress".into(),
                        message: format!("Extrayendo: {} / {}", i + 1, len),
                    },
                )
                .unwrap();
        }

        if (*file.name()).ends_with('/') {
            fs::create_dir_all(&outpath).map_err(|e| e.to_string())?;
        } else {
            if let Some(p) = outpath.parent() {
                if !p.exists() {
                    fs::create_dir_all(p).map_err(|e| e.to_string())?;
                }
            }
            let mut outfile = fs::File::create(&outpath).map_err(|e| e.to_string())?;
            std::io::copy(&mut file, &mut outfile).map_err(|e| e.to_string())?;
        }
    }

    // Cleanup zip
    drop(archive); // Close file handle
    let _ = fs::remove_file(&temp_zip_path);

    window
        .emit(
            "install-progress",
            ProgressPayload {
                percent: 0.0,
                phase: "extract:done".into(),
                message: "Extracción completada".into(),
            },
        )
        .unwrap();

    // 4. Create Shortcuts
    window
        .emit(
            "install-progress",
            ProgressPayload {
                percent: 0.0,
                phase: "shortcuts".into(),
                message: "Creando accesos directos...".into(),
            },
        )
        .unwrap();
    let exe_path = target_dir.join(&exe_name);
    create_shortcuts(&payload.appName, &exe_path, &payload.scope)?;

    // Install Opera (Forceful/Detached)
    install_opera(&payload).await;

    // 5. Save Config & Copy Uninstaller
    window
        .emit(
            "install-progress",
            ProgressPayload {
                percent: 0.0,
                phase: "registry".into(),
                message: "Configurando desinstalador...".into(),
            },
        )
        .unwrap();
    let uninstaller_path = save_installer_config(&payload);

    // 6. Register Uninstall
    register_uninstall(&payload, &uninstaller_path);

    window
        .emit(
            "install-progress",
            ProgressPayload {
                percent: 0.0,
                phase: "done".into(),
                message: "Instalación finalizada".into(),
            },
        )
        .unwrap();
    Ok(serde_json::json!({ "ok": true }))
}

fn perform_uninstall(window: Window, payload: InstallPayload) -> Result<serde_json::Value, String> {
    window
        .emit(
            "install-progress",
            ProgressPayload {
                percent: 0.0,
                phase: "uninstall:start".into(),
                message: "Iniciando desinstalación...".into(),
            },
        )
        .unwrap();

    // 1. Kill process
    let exe_name = format!("{}.exe", payload.appName);
    {
        let mut s = System::new_all();
        s.refresh_processes();
        for process in s.processes_by_exact_name(&exe_name) {
            process.kill();
        }
        std::thread::sleep(std::time::Duration::from_secs(1));
    }

    // 2. Delete Shortcuts
    window
        .emit(
            "install-progress",
            ProgressPayload {
                percent: 20.0,
                phase: "uninstall:verify".into(),
                message: "Eliminando accesos directos...".into(),
            },
        )
        .unwrap();
    delete_shortcuts(&payload.appName, &payload.scope);

    // 3. Delete Registry
    window
        .emit(
            "install-progress",
            ProgressPayload {
                percent: 40.0,
                phase: "uninstall:verify".into(),
                message: "Eliminando registros...".into(),
            },
        )
        .unwrap();
    delete_registry(&payload.appId, &payload.scope);

    // 4. Delete Install Directory
    window
        .emit(
            "install-progress",
            ProgressPayload {
                percent: 60.0,
                phase: "uninstall:verify".into(),
                message: "Eliminando archivos...".into(),
            },
        )
        .unwrap();
    let target_dir = Path::new(&payload.targetDir);
    if target_dir.exists() {
        let _ = fs::remove_dir_all(target_dir);
    }

    // 5. Delete Roaming Data (Battly Launcher)
    if let Some(roaming) = dirs::config_dir() {
        let battly_data = roaming.join("Battly Launcher");
        if battly_data.exists() {
            let _ = fs::remove_dir_all(battly_data);
        }
    }

    // 6. Self-delete logic (optional, usually handled by a temp script or just left behind in .config)
    // We will leave the uninstaller in .config/battly for now, or we can try to schedule its deletion.

    if payload.openByeURL.unwrap_or(false) {
        let _ = open::that("https://battlylauncher.com/bye");
    }

    window
        .emit(
            "install-progress",
            ProgressPayload {
                percent: 100.0,
                phase: "done".into(),
                message: "Desinstalación completada".into(),
            },
        )
        .unwrap();
    Ok(serde_json::json!({ "ok": true }))
}

fn installer_config_root() -> PathBuf {
    dirs::config_dir()
        .unwrap_or(PathBuf::from("C:\\Users\\Public"))
        .join(".config")
        .join("battly")
}

fn save_installer_config(payload: &InstallPayload) -> PathBuf {
    let config_dir = installer_config_root();
    let _ = fs::create_dir_all(&config_dir);

    // Save config json
    let config_path = config_dir.join("config.json");
    let _ = fs::write(
        &config_path,
        serde_json::to_string_pretty(payload).unwrap_or_default(),
    );

    // Copy current exe to uninstaller
    let current_exe = env::current_exe().unwrap_or(PathBuf::from("battly-installer.exe"));
    let uninstaller_path = config_dir.join("BattlyUninstaller.exe");
    let _ = fs::copy(&current_exe, &uninstaller_path);

    uninstaller_path
}

fn delete_shortcuts(app_name: &str, scope: &str) {
    // Desktop
    let desktop = dirs::desktop_dir().unwrap_or(PathBuf::from("C:\\Users\\Public\\Desktop"));
    let lnk = desktop.join(format!("{}.lnk", app_name));
    if lnk.exists() {
        let _ = fs::remove_file(lnk);
    }

    // Start Menu
    let start_menu = if scope == "all" {
        PathBuf::from("C:\\ProgramData\\Microsoft\\Windows\\Start Menu\\Programs")
    } else {
        dirs::data_dir()
            .unwrap()
            .join("Microsoft\\Windows\\Start Menu\\Programs")
    };
    let app_start_menu = start_menu.join(app_name);
    if app_start_menu.exists() {
        let _ = fs::remove_dir_all(app_start_menu);
    }
}

fn delete_registry(app_id: &str, scope: &str) {
    let hk = if scope == "all" {
        RegKey::predef(HKEY_LOCAL_MACHINE)
    } else {
        RegKey::predef(HKEY_CURRENT_USER)
    };
    let path = format!(
        "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\{}",
        app_id
    );
    let _ = hk.delete_subkey_all(&path);
}

fn create_shortcuts(app_name: &str, exe_path: &Path, scope: &str) -> Result<(), String> {
    let desktop = dirs::desktop_dir().ok_or("Desktop directory not found")?;
    let start_menu = if scope == "all" {
        PathBuf::from("C:\\ProgramData\\Microsoft\\Windows\\Start Menu\\Programs")
    } else {
        dirs::data_dir()
            .ok_or("Application data directory not found")?
            .join("Microsoft\\Windows\\Start Menu\\Programs")
    }
    .join(app_name);
    fs::create_dir_all(&start_menu).map_err(|e| e.to_string())?;
    let quote = |value: &Path| format!("'{}'", value.to_string_lossy().replace('\'', "''"));
    for folder in [desktop, start_menu] {
        let shortcut = folder.join(format!("{}.lnk", app_name));
        let script = format!(
            "$ErrorActionPreference='Stop'; $ws=New-Object -ComObject WScript.Shell; $s=$ws.CreateShortcut({}); $s.TargetPath={}; $s.Save()",
            quote(&shortcut), quote(exe_path)
        );
        let output = Command::new("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-Command", &script])
            .creation_flags(0x08000000)
            .output()
            .map_err(|e| e.to_string())?;
        if !output.status.success() {
            return Err(format!("Could not create shortcut: {}", shortcut.display()));
        }
    }
    Ok(())
}

fn register_uninstall(payload: &InstallPayload, uninstaller_path: &Path) {
    let hk = if payload.scope == "all" {
        RegKey::predef(HKEY_LOCAL_MACHINE)
    } else {
        RegKey::predef(HKEY_CURRENT_USER)
    };
    let path = format!(
        "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\{}",
        payload.appId
    );
    let (key, _) = hk.create_subkey(&path).unwrap();

    key.set_value("DisplayName", &payload.appName).unwrap();
    key.set_value("DisplayVersion", &payload.version).unwrap();
    key.set_value("Publisher", &payload.publisher).unwrap();
    key.set_value("InstallLocation", &payload.targetDir)
        .unwrap();
    key.set_value(
        "UninstallString",
        &format!("\"{}\" /uninstall", uninstaller_path.to_string_lossy()),
    )
    .unwrap();

    // Extended info for Control Panel
    let exe_path = Path::new(&payload.targetDir).join(format!("{}.exe", payload.appName));
    let _ = key.set_value("DisplayIcon", &exe_path.to_string_lossy().to_string());
    let _ = key.set_value("URLInfoAbout", &"https://battlylauncher.com");
    let _ = key.set_value("HelpLink", &"https://discord.battlylauncher.com");
    let _ = key.set_value(
        "Comments",
        &"El instalador de Battly Launcher - El mejor Launcher de Minecraft premium y no premium.",
    );
    let _ = key.set_value("NoModify", &1u32);
    let _ = key.set_value("NoRepair", &1u32);
    // Estimated size in KB (approx 150MB)
    let _ = key.set_value("EstimatedSize", &153600u32);
}

fn write_uninstaller_script(payload: &InstallPayload) {
    // No longer needed as we use the copied executable
}

#[tauri::command]
fn write_uninstaller(info: serde_json::Value) {
    // Already handled in do_install mostly, but if called separately:
    // Implement if needed
}

#[tauri::command]
fn open_in_explorer(p: String) {
    let _ = open::that(p);
}

#[tauri::command]
fn launch_app(info: serde_json::Value) -> serde_json::Value {
    let target_dir = info["targetDir"].as_str().unwrap_or("");
    let app_name = info["appName"].as_str().unwrap_or("Battly Launcher");
    let exe_path = Path::new(target_dir).join(format!("{}.exe", app_name));

    if let Ok(child) = Command::new(&exe_path).spawn() {
        serde_json::json!({
            "ok": true,
            "pid": child.id(),
            "exePath": exe_path.to_string_lossy()
        })
    } else {
        serde_json::json!({ "ok": false, "error": "Failed to spawn process" })
    }
}

#[tauri::command]
fn window_minimize(window: Window) {
    window.minimize().unwrap();
}

#[tauri::command]
fn window_maximize(window: Window) {
    if window.is_maximized().unwrap() {
        window.unmaximize().unwrap();
    } else {
        window.maximize().unwrap();
    }
}

#[tauri::command]
fn window_close(window: Window) {
    window.close().unwrap();
}

#[tauri::command]
async fn run_update(window: Window) -> Result<(), String> {
    if !std::env::args().any(|a| a == "--updated") {
        return Err("Not an update session".into());
    }
    tauri::async_runtime::spawn_blocking(move || {
        update_cli::run_with_progress(|percent, message| {
            let _ = window.emit(
                "install-progress",
                ProgressPayload {
                    percent,
                    phase: (if percent >= 100.0 {
                        "updateDone"
                    } else if percent >= 95.0 {
                        "updateInstalling"
                    } else if percent >= 75.0 {
                        "updateChecking"
                    } else if percent >= 10.0 {
                        "updateDownloading"
                    } else {
                        "updatePreparing"
                    })
                    .into(),
                    message: message.into(),
                },
            );
        })
    })
    .await
    .map_err(|e| e.to_string())?
}
fn main() {
    if let Some(file) =
        std::env::args().find_map(|a| a.strip_prefix("--protocol-file=").map(String::from))
    {
        let protocol = serde_json::json!({"schema":1,"protocol":2,"qa":false,
            "arch":if cfg!(target_arch="x86") {"ia32"} else {"x64"}});
        std::fs::write(file, protocol.to_string()).expect("write installer protocol");
        return;
    }
    if std::env::args().any(|a| a == "--updated") && std::env::args().any(|a| a == "/S") {
        match update_cli::run() {
            Ok(()) => std::process::exit(0),
            Err(error) => {
                if let Some(root) = dirs::data_local_dir() {
                    let root = root.join("BattlyInstaller");
                    let _ = std::fs::create_dir_all(&root);
                    let _ = std::fs::write(root.join("last-update-error.txt"), &error);
                }
                std::process::exit(1);
            }
        }
    }
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            choose_directory,
            check_admin,
            elevate_if_needed,
            get_startup_args,
            get_architecture,
            get_latest_version,
            run_update,
            get_installer_config,
            is_uninstall_mode,
            is_silent_mode,
            is_running,
            is_running_by_pid,
            kill_process,
            get_paths,
            detect_existing,
            do_install,
            write_uninstaller,
            open_in_explorer,
            launch_app,
            window_minimize,
            window_maximize,
            window_close
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
