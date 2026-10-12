# CAIdex Windows · J-1

Windows 11 x64 Tauri 2 application using the existing local Chat, model adapters and Windows credential Broker. Version 0.1.1 adds provider model catalogs and composer model/effort/speed selection while retaining local conversations, Windows key storage, light/dark/system themes and Chinese/English. Account, cloud Memory/sync and Codex execution are outside this milestone.

User trial correction: [catalogs and reasoning controls](../../docs/CAIdex-J1-试用修正-模型目录与推理控制-V1.md). Provider catalogs are fetched after key save; manual IDs are an advanced fallback. Declared controls send actual API parameters; unknown controls stay at provider defaults.

Source and acceptance boundaries: [J-1 design and verification](../../docs/CAIdex-J1-Windows-设计与验收-V1.md).

On a Windows development machine with the pinned Rust toolchain, Node 22.23.3, MSVC build tools and WebView2:

```powershell
cd apps/windows
npm ci
npm run build
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --locked -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml --locked
npm run tauri -- build --no-binary-patching -- --locked
```

`npm run tauri -- dev` runs the native development app. `npm test` runs renderer tests against mocked IPC; it is not native application verification. The Windows workflow separately drives the actual release WebView and installs/starts/uninstalls the NSIS package using a synthetic loopback model.

Installers are under `src-tauri/target/release/bundle/nsis` and `bundle/msi`. The CI artifact contains installers and `build-manifest.json` with source SHA and hashes; use only the final verified artifact referenced by the acceptance document. Installers are unsigned previews.

Only configure keys inside the installed application's masked settings form. Do not put keys in repo files, fixtures, logs or screenshots. Configuration and Chat SQLite use the app's private local data directory; keys use Windows Credential Manager. No account or production service is contacted by this milestone's tests.
