# Battly Installer

Instalador oficial de Battly Launcher para Windows, escrito con Rust y Tauri.

Descarga `Battly-Launcher-win.zip` en Windows x64 y `Battly-Launcher-win-ia32.zip` en Windows x86 desde las releases de `BattlyLauncher/battlylauncher`. También reconoce los nombres versionados `Battly-Launcher-VERSION-x64.zip` e `-ia32.zip`. La selección no depende del orden de los adjuntos.

## Compilar

Se necesitan Visual Studio Build Tools, WebView2 y Rust 1.95.0 con los destinos `x86_64-pc-windows-msvc` e `i686-pc-windows-msvc`.

```powershell
cargo +1.95 test --locked --release --manifest-path src-tauri/Cargo.toml
cargo +1.95 build --locked --release --manifest-path src-tauri/Cargo.toml --target x86_64-pc-windows-msvc
cargo +1.95 build --locked --release --manifest-path src-tauri/Cargo.toml --target i686-pc-windows-msvc
```

Los ejecutables se distribuyen como `Battly-Launcher-Windows.exe` y `Battly-Launcher-win-ia32.exe`. El segundo se compila para x86; cambiar el nombre de un binario x64 no lo convierte en x86.

## Actualizaciones

El instalador admite los argumentos de electron-updater: `--updated`, `/S`, `--force-run` y `/D=RUTA`. Battly transmite el repositorio, el tag y la arquitectura de la oferta aceptada. El modo de actualización conserva el destino existente y no muestra ofertas de software adicional.

La descarga se prepara en una carpeta temporal del destino y se comprueban su tamaño, digest de GitHub, rutas y arquitectura. Los archivos anteriores se conservan hasta terminar la sustitución. Si falla, se restauran; si el destino requiere permisos de administrador, se solicita elevación antes de reemplazarlos. Cancelar el aviso conserva la instalación anterior.

La selección de archivos está en [release_assets.rs](src-tauri/src/release_assets.rs) y la sustitución y recuperación en [update_cli.rs](src-tauri/src/update_cli.rs).

## GitHub Actions y firma

El [workflow](.github/workflows/build.yml) compila ambas arquitecturas, ejecuta las comprobaciones, firma los EXE con Azure y verifica sus firmas. El [generador del manifiesto](scripts/installer-release-manifest.cjs) comprueba las arquitecturas y genera `installer-protocol.json` con protocolo 2, tamaños y hashes SHA-512.

La autenticación usa OIDC desde el environment `windows-signing`, restringido a `main`. La identidad solo puede firmar con el perfil asignado a Battly. Estos valores se guardan como secrets cifrados del environment:

- `AZURE_CLIENT_ID`, `AZURE_TENANT_ID` y `AZURE_SUBSCRIPTION_ID`.
- `AZURE_SIGNING_ENDPOINT`, `AZURE_SIGNING_ACCOUNT_NAME` y `AZURE_SIGNING_CERTIFICATE_PROFILE`.

No se almacenan contraseñas ni certificados privados en el repositorio. Los artifacts contienen únicamente los ejecutables y los metadatos previstos. La firma del EXE contiene el certificado público y la identidad del editor que Windows necesita verificar.

La publicación permanece desactivada mientras `BATTLY_INSTALLER_PUBLISH_ENABLED` no sea `true`. Al habilitarla, el workflow crea una release `rust_vVERSION`, adjunta los dos EXE firmados y el manifiesto, y después la hace pública. La ejecución manual permite compilar y verificar sin publicar.
