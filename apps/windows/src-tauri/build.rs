fn main() {
    #[cfg(all(windows, feature = "desktop"))]
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(
        tauri_build::AppManifest::new().commands(&[
            "bootstrap",
            "preferences",
            "save_profile",
            "select_profile",
            "remove_key",
            "create",
            "conversations",
            "snapshot",
            "draft",
            "attach",
            "send",
            "cancel",
            "export_chat",
        ]),
    ))
    .expect("desktop manifest generation failed");
}
