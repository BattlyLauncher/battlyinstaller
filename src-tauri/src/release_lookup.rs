use crate::release_assets::{select_zip, REPOSITORY};
use reqwest::{header::LOCATION, redirect::Policy, Client};
use serde_json::{json, Value};
use std::time::Duration;

fn release_from_redirect(location: &str, arch: &str) -> Result<Value, String> {
    let prefix = format!("https://github.com/{}/releases/tag/", REPOSITORY);
    let tag = location
        .strip_prefix(&prefix)
        .ok_or("Unexpected release redirect")?;
    let version = tag.strip_prefix('v').unwrap_or(tag);
    if !version.starts_with(|c: char| c.is_ascii_digit())
        || !version
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || ".-+".contains(c))
    {
        return Err("Invalid release version".into());
    }
    let name = match arch {
        "x64" => "Battly-Launcher-win.zip",
        "ia32" => "Battly-Launcher-win-ia32.zip",
        _ => return Err("Unsupported Windows architecture".into()),
    };
    Ok(json!({"tag_name": tag, "assets": [{"name": name,
        "browser_download_url": format!("https://github.com/{}/releases/download/{}/{}", REPOSITORY, tag, name)}]}))
}

async fn lookup(
    client: &Client,
    web_url: &str,
    api_url: &str,
    arch: &str,
) -> Result<Value, String> {
    // The public latest-release redirect has no GitHub REST API quota.
    let web = async {
        let response = client
            .head(web_url)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        if !response.status().is_redirection() {
            return Err("Missing latest-release redirect".to_string());
        }
        let location = response
            .headers()
            .get(LOCATION)
            .and_then(|v| v.to_str().ok())
            .ok_or("Missing release location")?;
        let release = release_from_redirect(location, arch)?;
        let asset = select_zip(&release, arch)?;
        let response = client
            .head(asset["downloadUrl"].as_str().ok_or("Missing ZIP URL")?)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        if response.status().is_success() {
            return Ok(asset);
        }
        let destination = response
            .headers()
            .get(LOCATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| reqwest::Url::parse(v).ok());
        if response.status().is_redirection()
            && destination.is_some_and(|url| {
                url.scheme() == "https"
                    && url
                        .host_str()
                        .is_some_and(|host| host.ends_with(".githubusercontent.com"))
            })
        {
            return Ok(asset);
        }
        Err(format!("Windows ZIP: HTTP {}", response.status()))
    }
    .await;
    if let Ok(asset) = web {
        return Ok(asset);
    }

    let response = client.get(api_url).send().await.map_err(|_| {
        "No se pudo conectar con GitHub. Comprueba la conexión y vuelve a intentarlo.".to_string()
    })?;
    if !response.status().is_success() {
        return Err(format!("No se pudo consultar la descarga en GitHub (HTTP {}). Vuelve a intentarlo en unos minutos.", response.status()));
    }
    let release: Value = response
        .json()
        .await
        .map_err(|_| "GitHub devolvió una respuesta de versión no válida".to_string())?;
    select_zip(&release, arch)
}

pub async fn latest(arch: &str) -> Result<Value, String> {
    let client = Client::builder()
        .user_agent("BattlyInstaller")
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(10))
        .redirect(Policy::none())
        .build()
        .map_err(|e| e.to_string())?;
    lookup(
        &client,
        &format!("https://github.com/{}/releases/latest", REPOSITORY),
        &format!(
            "https://api.github.com/repos/{}/releases/latest",
            REPOSITORY
        ),
        arch,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn web_failure_uses_api_and_reports_rate_limit_if_both_fail() {
        use std::io::{Read, Write};
        use std::net::TcpListener;
        for status in [200, 403] {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let endpoint = format!("http://{}", listener.local_addr().unwrap());
            let server = std::thread::spawn(move || {
                for index in 0..2 {
                    let (mut stream, _) = listener.accept().unwrap();
                    let mut request = [0; 4096];
                    stream.read(&mut request).unwrap();
                    let body = if index == 1 && status == 200 {
                        release_from_redirect(
                            &format!("https://github.com/{}/releases/tag/v3.1.0", REPOSITORY),
                            "ia32",
                        )
                        .unwrap()
                        .to_string()
                    } else {
                        "{}".to_string()
                    };
                    let code = if index == 0 { 503 } else { status };
                    write!(stream, "HTTP/1.1 {} Result\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", code, body.len(), body).unwrap();
                }
            });
            let client = Client::builder()
                .redirect(Policy::none())
                .timeout(Duration::from_secs(3))
                .build()
                .unwrap();
            let result = lookup(&client, &endpoint, &endpoint, "ia32").await;
            server.join().unwrap();
            if status == 200 {
                assert_eq!(result.unwrap()["arch"], "ia32");
            } else {
                assert!(result.unwrap_err().contains("403"));
            }
        }
    }

    #[tokio::test]
    #[ignore = "Checks the public GitHub release without using the REST API"]
    async fn live_release_without_api() {
        let client = Client::builder()
            .user_agent("BattlyInstaller")
            .redirect(Policy::none())
            .timeout(Duration::from_secs(15))
            .build()
            .unwrap();
        for arch in ["x64", "ia32"] {
            let value = lookup(
                &client,
                &format!("https://github.com/{}/releases/latest", REPOSITORY),
                "http://127.0.0.1:1/unavailable-api",
                arch,
            )
            .await
            .unwrap();
            assert_eq!(value["arch"], arch);
            println!("{} {}", arch, value["version"]);
        }
    }

    #[test]
    fn redirect_selects_only_the_requested_windows_archive() {
        let url = format!("https://github.com/{}/releases/tag/v3.1.0", REPOSITORY);
        for (arch, name) in [
            ("x64", "Battly-Launcher-win.zip"),
            ("ia32", "Battly-Launcher-win-ia32.zip"),
        ] {
            let asset = select_zip(&release_from_redirect(&url, arch).unwrap(), arch).unwrap();
            assert_eq!(asset["version"], "3.1.0");
            assert!(asset["downloadUrl"].as_str().unwrap().ends_with(name));
        }
        for invalid in ["https://evil.test/releases/tag/v3.1.0", "https://github.com/BattlyLauncher/battlylauncher/releases/tag/../bad", "https://github.com/BattlyLauncher/battlylauncher/releases/tag/battly-patch-3.1.0-patch5"] {
            assert!(release_from_redirect(invalid, "x64").is_err());
        }
    }
}
