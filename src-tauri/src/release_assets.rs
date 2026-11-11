use serde_json::{json, Value};
pub const REPOSITORY: &str = "BattlyLauncher/battlylauncher";
pub fn architecture() -> &'static str {
    let native = std::env::var("PROCESSOR_ARCHITEW6432")
        .or_else(|_| std::env::var("PROCESSOR_ARCHITECTURE"))
        .unwrap_or_else(|_| std::env::consts::ARCH.into());
    match native.to_ascii_lowercase().as_str() {
        "amd64" | "x86_64" | "arm64" | "aarch64" => "x64",
        _ => "ia32",
    }
}
pub fn select_zip(release: &Value, arch: &str) -> Result<Value, String> {
    let tag = release["tag_name"].as_str().ok_or("Missing release tag")?;
    let version = tag.strip_prefix('v').unwrap_or(tag);
    let preferred = match arch {
        "x64" => "Battly-Launcher-win.zip",
        "ia32" => "Battly-Launcher-win-ia32.zip",
        _ => return Err("Unsupported Windows architecture".into()),
    };
    let versioned = format!("Battly-Launcher-{}-{}.zip", version, arch);
    let assets = release["assets"]
        .as_array()
        .ok_or("Missing release assets")?;
    for name in [preferred, versioned.as_str()] {
        let matches: Vec<_> = assets
            .iter()
            .filter(|a| a["name"].as_str() == Some(name))
            .collect();
        if matches.len() > 1 {
            return Err("Duplicate Windows ZIP".into());
        }
        if let Some(asset) = matches.first() {
            let url = asset["browser_download_url"]
                .as_str()
                .ok_or("Missing ZIP URL")?;
            if !url.starts_with("https://github.com/") || !url.ends_with(&format!("/{}", name)) {
                return Err("Unsafe Windows ZIP URL".into());
            }
            return Ok(json!({"ok":true,"version":version,"downloadUrl":url,
                "arch":arch,"size":asset["size"],"digest":asset["digest"],"apiDownloadUrl":asset["url"]}));
        }
    }
    Err(format!("No Windows {} ZIP in release {}", arch, tag))
}
#[cfg(test)]
mod tests {
    use super::*;
    fn release(names: &[&str]) -> Value {
        json!({"tag_name":"v3.1.0","assets":names.iter().map(|name|json!({"name":name,
            "browser_download_url":format!("https://github.com/{}/releases/download/v3.1.0/{}", REPOSITORY,name)})).collect::<Vec<_>>()})
    }
    #[test]
    fn mixed_release_never_selects_a_mac_or_foreign_architecture_zip() {
        let r = release(&[
            "Battly-Launcher-3.1.0-mac-arm64.zip",
            "Battly-Launcher-win-ia32.zip",
            "Battly-Launcher-win.zip",
        ]);
        assert!(select_zip(&r, "x64").unwrap()["downloadUrl"]
            .as_str()
            .unwrap()
            .ends_with("/Battly-Launcher-win.zip"));
        assert!(select_zip(&r, "ia32").unwrap()["downloadUrl"]
            .as_str()
            .unwrap()
            .ends_with("/Battly-Launcher-win-ia32.zip"));
    }
    #[test]
    fn missing_architecture_does_not_fall_back_to_an_unrelated_zip() {
        assert!(select_zip(
            &release(&["Battly-Launcher-win.zip", "Battly-Launcher-mac-x64.zip"]),
            "ia32"
        )
        .is_err());
        assert!(select_zip(&release(&["Battly-Launcher-3.1.0-x64.zip"]), "x64").is_ok());
        assert!(select_zip(
            &release(&["Battly-Launcher-win.zip", "Battly-Launcher-win.zip"]),
            "x64"
        )
        .is_err());
    }
}
