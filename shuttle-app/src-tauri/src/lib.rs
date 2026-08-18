mod commands;
mod config;
mod connectors;
mod db;
mod env;
mod models;
mod notifications;
mod secrets;
mod telemetry;

use commands::{init_state, APP_HANDLE};
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    crate::env::load_dotenv();
    tracing_subscriber::fmt::init();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let state = init_state(app.handle());
            app.manage(state);
            *APP_HANDLE.lock() = Some(app.handle().clone());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_accounts,
            commands::list_connectors,
            commands::create_account,
            commands::delete_account,
            commands::update_account,
            commands::list_conversations,
            commands::get_messages,
            commands::send_message,
            commands::mark_read,
            commands::mark_unread,
            commands::update_conversation,
            commands::search_conversations,
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
            commands::export_backup,
            commands::restore_backup,
            commands::open_external,
            commands::open_devtools,
            commands::forward_message,
            commands::telemetry_track,
            commands::telemetry_error,
            commands::telemetry_performance,
            commands::telemetry_set_foreground,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
