//! BreraOS shell entry. The web layer talks; the core decides.
//! No `unwrap()` crosses this file — failures become exit codes or IPC errors.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod protection;
mod state;

use state::AppState;

fn main() {
    if let Err(fatal) = run() {
        eprintln!("breraos: fatal: {fatal}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let app_state = AppState::new().map_err(|e| format!("core init failed: {e}"))?;

    tauri::Builder::default()
        .manage(app_state)
        .invoke_handler(tauri::generate_handler![
            commands::system_snapshot,
            commands::vault_generate,
            commands::vault_unlock,
            commands::vault_lock,
            commands::vault_status,
            commands::wallet_address,
            commands::wallet_sign,
            commands::proxy_start,
            commands::proxy_stop,
            commands::proxy_status,
            commands::protection_set,
            commands::protection_status,
            commands::memory_purge,
        ])
        .setup(|app| {
            // capture shield goes up before the first frame is shown
            if let Some(window) = app.get_webview_window("main") {
                if let Err(e) = protection::apply(&window, true) {
                    eprintln!("breraos: capture shield failed to arm: {e}");
                }
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .map_err(|e| format!("tauri runtime error: {e}"))?;
    Ok(())
}
