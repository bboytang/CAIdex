#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#[cfg(windows)]
mod native {
    use caidex_credentials::SystemStore;
    use caidex_desktop::{Bootstrap, Desktop, Preferences, Profile, Snapshot};
    use tauri::{Manager, State};
    use uuid::Uuid;
    type App = Desktop<SystemStore>;
    type Reply<T> = std::result::Result<T, &'static str>;
    #[tauri::command]
    fn bootstrap(app: State<App>) -> Reply<Bootstrap> {
        app.bootstrap()
    }
    #[tauri::command]
    fn preferences(app: State<App>, preferences: Preferences) -> Reply<()> {
        app.preferences(preferences)
    }
    #[tauri::command]
    fn save_profile(
        app: State<App>,
        profile: Profile,
        key: Option<String>,
        reuse_key: bool,
    ) -> Reply<()> {
        app.save_profile(profile, key, reuse_key)
    }
    #[tauri::command]
    fn select_profile(app: State<App>, id: Uuid) -> Reply<()> {
        app.select_profile(id)
    }
    #[tauri::command]
    fn remove_key(app: State<App>, id: Uuid) -> Reply<()> {
        app.remove_key(id)
    }
    #[tauri::command]
    fn create(app: State<App>, title: String) -> Reply<Uuid> {
        app.create(title)
    }
    #[tauri::command]
    fn snapshot(app: State<App>, conversation: Uuid, after: i64) -> Reply<Snapshot> {
        app.snapshot(conversation, after)
    }
    #[tauri::command]
    fn draft(app: State<App>, conversation: Uuid, body: Option<String>) -> Reply<String> {
        app.draft(conversation, body)
    }
    #[tauri::command]
    fn attach(
        app: State<App>,
        conversation: Uuid,
        name: String,
        media_type: String,
        bytes: Vec<u8>,
    ) -> Reply<Uuid> {
        app.attach(conversation, name, media_type, bytes)
    }
    #[tauri::command]
    async fn send(
        app: State<'_, App>,
        operation: Uuid,
        conversation: Uuid,
        parent: Option<Uuid>,
        text: String,
        attachments: Vec<Uuid>,
    ) -> Reply<caidex_chat::Message> {
        app.send(caidex_desktop::request(
            operation,
            conversation,
            parent,
            text,
            attachments,
        ))
        .await
    }
    #[tauri::command]
    fn cancel(app: State<App>, operation: Uuid) -> Reply<()> {
        app.cancel(operation)
    }
    #[tauri::command]
    fn export_chat(app: State<App>, conversation: Uuid) -> Reply<String> {
        app.export(conversation)
    }
    pub fn run() {
        let result = tauri::Builder::default()
            .setup(|app| {
                let root = app.path().app_local_data_dir()?;
                let service = App::open(&root, SystemStore).map_err(std::io::Error::other)?;
                app.manage(service);
                Ok(())
            })
            .invoke_handler(tauri::generate_handler![
                bootstrap,
                preferences,
                save_profile,
                select_profile,
                remove_key,
                create,
                snapshot,
                draft,
                attach,
                send,
                cancel,
                export_chat
            ])
            .run(tauri::generate_context!());
        if result.is_err() {
            // Startup failures cannot expose filesystem, platform or secret-bearing error payloads.
            eprintln!("CAIdex startup failed: local storage or desktop runtime unavailable");
            let message: Vec<u16> = "CAIdex 无法启动。请关闭其他 CAIdex 窗口并检查本地数据目录权限。\nCAIdex cannot start. Close other CAIdex windows and check local data permissions.\0".encode_utf16().collect();
            let title: Vec<u16> = "CAIdex\0".encode_utf16().collect();
            // SAFETY: both buffers are NUL-terminated and live for the synchronous dialog call.
            unsafe {
                windows_sys::Win32::UI::WindowsAndMessaging::MessageBoxW(
                    std::ptr::null_mut(),
                    message.as_ptr(),
                    title.as_ptr(),
                    windows_sys::Win32::UI::WindowsAndMessaging::MB_OK
                        | windows_sys::Win32::UI::WindowsAndMessaging::MB_ICONERROR,
                );
            }
            std::process::exit(1);
        }
    }
}
fn main() {
    #[cfg(windows)]
    native::run();
    #[cfg(not(windows))]
    {
        eprintln!("This CAIdex application targets Windows 11 x64");
        std::process::exit(1);
    }
}
