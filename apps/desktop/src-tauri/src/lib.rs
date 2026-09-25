//! OwO AI Gateway desktop app: a dashboard and editor over the same config, keyring, and
//! call history the `owo` CLI uses.

mod commands;
mod config_edit;
mod mcp;
mod owo_cli;
mod skills;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            commands::status,
            commands::usage,
            commands::today,
            commands::calls,
            commands::call,
            commands::clear_history,
            commands::models,
            commands::providers,
            commands::presets,
            commands::apps,
            commands::gateway_start,
            commands::gateway_stop,
            commands::gateway_restart,
            commands::app_connect,
            commands::app_disconnect,
            commands::disconnect_all,
            commands::config_text,
            commands::check_config_text,
            commands::save_config_text,
            commands::reset_config,
            commands::general,
            commands::save_general,
            commands::save_client,
            commands::save_model,
            commands::delete_model,
            commands::save_provider,
            commands::delete_provider,
            commands::set_key,
            mcp::mcp_list,
            mcp::mcp_scan,
            mcp::mcp_save,
            mcp::mcp_remove,
            mcp::mcp_toggle,
            mcp::mcp_import,
            mcp::mcp_sync,
            skills::skills_list,
            skills::skills_discovered,
            skills::skills_repos,
            skills::skills_discover,
            skills::skills_install,
            skills::skills_read,
            skills::skills_apply,
            skills::skills_tree,
            skills::skills_sync,
            skills::skills_remove,
            skills::skills_toggle,
            skills::skills_adopt,
            skills::skills_update,
            skills::skills_repo_add,
            skills::skills_repo_remove,
            skills::skills_repo_reset,
            skills::skills_pick,
            skills::skills_open,
        ])
        .run(tauri::generate_context!())
        .expect("error while running the OwO AI Gateway desktop app");
}
