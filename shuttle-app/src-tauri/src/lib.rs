mod clipboard;
mod commands;
mod components;
mod config;
mod connectors;
mod db;
mod env;
mod link_preview;
mod main_window;
mod media_store;
mod models;
mod notifications;
mod permissions;
mod secrets;
mod telemetry;
mod translate;
mod tray;
mod updates;

use commands::{init_state, AppState, APP_HANDLE};
use tauri::{Manager, RunEvent, WindowEvent};

#[cfg(not(target_os = "macos"))]
fn apply_window_icon<R: tauri::Runtime>(win: &tauri::WebviewWindow<R>) -> tauri::Result<()> {
    #[cfg(target_os = "linux")]
    let icon = tauri::image::Image::from_bytes(include_bytes!("../icons/icon-128.png"))
        .ok()
        .or_else(|| win.app_handle().default_window_icon().cloned());
    #[cfg(not(target_os = "linux"))]
    let icon = win.app_handle().default_window_icon().cloned();
    match icon {
        Some(icon) => win.set_icon(icon),
        None => Ok(()),
    }
}

/// Everything the main window needs that the config file alone cannot give it: engine tuning, its
/// window icon, and the microphone grant. Runs at startup and again whenever the tray rebuilds
/// the window after an unload.
pub(crate) fn prepare_main_window<R: tauri::Runtime>(
    win: &tauri::WebviewWindow<R>,
) -> tauri::Result<()> {
    #[cfg(target_os = "linux")]
    if commands::performance_config(win.app_handle()).low_memory_webview {
        apply_low_memory_webview(win);
    }
    #[cfg(not(target_os = "macos"))]
    apply_window_icon(win)?;
    permissions::allow_microphone(win)
}

/// Shrink WebKit's memory footprint for a local messaging UI: the smallest cache model, no
/// page cache, and no WebGL (the frontend draws 2D canvas only). See docs/platforms.md.
#[cfg(target_os = "linux")]
fn apply_low_memory_webview<R: tauri::Runtime>(win: &tauri::WebviewWindow<R>) {
    use webkit2gtk::{CacheModel, SettingsExt, WebContextExt, WebViewExt};

    let _ = win.with_webview(|platform| {
        let view = platform.inner();
        if let Some(settings) = view.settings() {
            settings.set_enable_page_cache(false);
            settings.set_enable_webgl(false);
        }
        if let Some(context) = view.web_context() {
            context.set_cache_model(CacheModel::DocumentViewer);
        }
    });
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    crate::env::load_dotenv();
    tracing_subscriber::fmt::init();

    let mut builder = tauri::Builder::default();

    #[cfg(desktop)]
    {
        // Must be registered first so a second launch focuses the existing window.
        builder = builder.plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            main_window::show(app);
        }));
    }

    builder
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let state = init_state(app.handle());
            // Downloads live in the app-private cache; older builds wrote to Documents, and
            // those files stay viewable.
            let cache = media_store::media_cache_root(state.db.data_dir());
            std::fs::create_dir_all(&cache)?;
            // Staged "Copy media" links from earlier runs would otherwise pile up.
            clipboard::prune_staging(
                &media_store::clipboard_stage_dir(state.db.data_dir()),
                clipboard::KEEP_STAGED_AT_STARTUP,
            );
            app.asset_protocol_scope().allow_directory(&cache, true)?;
            app.asset_protocol_scope()
                .allow_directory(media_store::shuttle_documents_root(), true)?;
            app.manage(state);
            *APP_HANDLE.lock() = Some(app.handle().clone());
            // macOS uses the application bundle icon (.icns, up to 1024 px) for the Dock. On
            // Windows and Linux, also apply a large icon to the native window: alt-tab
            // switchers read it (X11 `_NET_WM_ICON`) and a 32 px one looks pixelated.
            // Linux sets the 128 px PNG explicitly (GDK on X11 drops 512 px icons); Windows uses `default_window_icon()`,
            // which is the first `.ico` entry (256 px, see `bundle.icon` in tauri.conf.json).
            if let Some(win) = app.get_webview_window("main") {
                prepare_main_window(&win)?;
            }
            tray::setup_tray(app)?;
            Ok(())
        })
        .on_window_event(|window, event| {
            if window.label() != "main" {
                return;
            }
            // On X11 the icon set during setup (window still hidden/unrealized) can be dropped,
            // leaving no `_NET_WM_ICON`; re-apply once the window is actually shown and focused.
            #[cfg(not(target_os = "macos"))]
            if let WindowEvent::Focused(true) = event {
                static APPLIED: std::sync::atomic::AtomicBool =
                    std::sync::atomic::AtomicBool::new(false);
                if !APPLIED.swap(true, std::sync::atomic::Ordering::Relaxed) {
                    if let Some(win) = window.app_handle().get_webview_window("main") {
                        let _ = apply_window_icon(&win);
                    }
                }
            }
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                main_window::hide(window.app_handle());
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_accounts,
            commands::get_link_preview,
            commands::get_map_tile,
            commands::get_embed_host,
            commands::list_connectors,
            commands::create_account,
            commands::delete_account,
            commands::update_account,
            commands::list_conversations,
            commands::count_conversations,
            commands::list_contacts,
            commands::start_conversation,
            commands::check_whatsapp_number,
            commands::create_group,
            commands::download_message_media,
            commands::download_status_media,
            commands::read_message_media,
            commands::save_media_file,
            commands::copy_media_to_clipboard,
            commands::reveal_in_file_manager,
            commands::media_cache_info,
            commands::clear_media_cache,
            commands::fetch_conversation_avatar,
            commands::sync_conversation,
            commands::get_messages,
            commands::send_message,
            commands::send_attachment,
            commands::apply_message_op,
            commands::translate_text,
            commands::mark_read,
            commands::mark_unread,
            commands::update_conversation,
            commands::search_conversations,
            commands::search_messages,
            commands::star_message,
            commands::pin_message,
            commands::fetch_contact_profile,
            commands::sender_contact,
            commands::start_call,
            commands::accept_call,
            commands::reject_call,
            commands::hangup_call,
            commands::total_unread,
            commands::current_call,
            commands::unread_by_account,
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
            commands::list_notes,
            commands::add_note,
            commands::update_note,
            commands::delete_note,
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
            commands::forward_messages,
            commands::fetch_url_bytes,
            commands::telemetry_track,
            commands::telemetry_error,
            commands::telemetry_performance,
            commands::telemetry_set_foreground,
            commands::telemetry_installation_id,
            commands::wake_account,
            commands::set_active_account,
            commands::fetch_tweakcn_theme,
            commands::get_connector_requirements,
            commands::get_installed_components,
            commands::app_info,
            commands::components_overview,
            commands::check_for_update,
            commands::download_update,
            commands::run_update_installer,
            commands::ensure_connector_components,
            commands::cancel_component_install,
            tray::update_tray_unread,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| match event {
            RunEvent::ExitRequested { code, api, .. } => {
                // The last window closing is not a quit the user asked for: the tray is.
                if main_window::keeps_app_alive(code) {
                    api.prevent_exit();
                }
            }
            RunEvent::Exit => {
                if let Some(state) = app.try_state::<AppState>() {
                    state.connectors.shutdown_all();
                }
            }
            // macOS sends this when the Dock icon is clicked with no window open.
            #[cfg(target_os = "macos")]
            RunEvent::Reopen { .. } => main_window::show(app),
            _ => {}
        });
}
