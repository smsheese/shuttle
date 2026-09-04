mod commands;
mod components;
mod config;
mod connectors;
mod db;
mod env;
mod media_store;
mod models;
mod notifications;
mod secrets;
mod telemetry;
mod tray;

use commands::{init_state, AppState, APP_HANDLE};
use tauri::{Manager, RunEvent, WindowEvent};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    crate::env::load_dotenv();
    tracing_subscriber::fmt::init();

    let mut builder = tauri::Builder::default();

    #[cfg(desktop)]
    {
        // Must be registered first so a second launch focuses the existing window.
        builder = builder.plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(win) = app.get_webview_window("main") {
                let _ = win.show();
                let _ = win.unminimize();
                let _ = win.set_focus();
            }
        }));
    }

    builder
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let state = init_state(app.handle());
            app.manage(state);
            *APP_HANDLE.lock() = Some(app.handle().clone());
            // Set window icon explicitly so the taskbar/titlebar always shows the app icon
            #[cfg(not(target_os = "macos"))]
            if let Some(win) = app.get_webview_window("main") {
                let icon_bytes = include_bytes!("../icons/icon.png");
                if let Ok(img) = tauri::image::Image::from_bytes(icon_bytes) {
                    let _ = win.set_icon(img);
                }
            }
            tray::setup_tray(app)?;
            Ok(())
        })
        .on_window_event(|window, event| {
            if window.label() != "main" {
                return;
            }
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_accounts,
            commands::list_connectors,
            commands::create_account,
            commands::delete_account,
            commands::update_account,
            commands::list_conversations,
            commands::count_conversations,
            commands::list_contacts,
            commands::start_conversation,
            commands::create_group,
            commands::download_message_media,
            commands::download_status_media,
            commands::read_message_media,
            commands::shuttle_files_root,
            commands::fetch_conversation_avatar,
            commands::sync_conversation,
            commands::get_messages,
            commands::send_message,
            commands::send_attachment,
            commands::mark_read,
            commands::mark_unread,
            commands::update_conversation,
            commands::search_conversations,
            commands::search_messages,
            commands::star_message,
            commands::pin_message,
            commands::fetch_contact_profile,
            commands::start_call,
            commands::accept_call,
            commands::reject_call,
            commands::hangup_call,
            commands::total_unread,
            commands::connect_account,
            commands::submit_auth,
            commands::get_app_config,
            commands::save_app_config,
            commands::list_workspaces,
            commands::create_workspace,
            commands::rename_workspace,
            commands::delete_workspace,
            commands::list_priority_groups,
            commands::create_priority_group,
            commands::rename_priority_group,
            commands::delete_priority_group,
            commands::list_todos,
            commands::add_todo,
            commands::set_todo_done,
            commands::delete_todo,
            commands::list_reminders,
            commands::create_reminder,
            commands::delete_reminder,
            commands::list_forward_rules,
            commands::create_forward_rule,
            commands::update_forward_rule,
            commands::delete_forward_rule,
            commands::list_scheduled_messages,
            commands::schedule_message,
            commands::delete_scheduled_message,
            commands::update_scheduled_message,
            commands::export_backup,
            commands::restore_backup,
            commands::restart_app,
            commands::open_external,
            commands::open_devtools,
            commands::forward_message,
            commands::fetch_url_bytes,
            commands::telemetry_track,
            commands::telemetry_error,
            commands::telemetry_performance,
            commands::telemetry_set_foreground,
            commands::wake_account,
            commands::set_active_account,
            commands::fetch_tweakcn_theme,
            commands::get_connector_requirements,
            commands::get_installed_components,
            commands::ensure_connector_components,
            commands::cancel_component_install,
            tray::update_tray_unread,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            if let RunEvent::Exit = event {
                if let Some(state) = app.try_state::<AppState>() {
                    state.connectors.shutdown_all();
                }
            }
        });
}
