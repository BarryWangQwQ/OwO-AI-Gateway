//! OwO AI Gateway desktop app: a dashboard and editor over the same config, keyring, and
//! call history the `owo` CLI uses.

mod commands;
mod config_edit;
mod mcp;
mod owo_cli;
mod skills;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
/// First argument that makes this executable run as the `owo` CLI instead of opening a window.
/// The app uses it to run `owo` commands, and the background gateway is started the same way.
pub const CLI_FLAG: &str = "--owo-cli";

/// If the arguments start with [`CLI_FLAG`], runs the rest as an `owo` command and returns its exit code.
pub fn cli_mode() -> Option<i32> {
    let mut args = std::env::args_os().skip(1);
    if args.next().as_deref() != Some(std::ffi::OsStr::new(CLI_FLAG)) {
        return None;
    }
    attach_parent_console();
    owo::set_self_args(vec![CLI_FLAG.into()]);
    Some(owo::main_from(std::iter::once(std::ffi::OsString::from("owo")).chain(args)))
}

/// Release builds are GUI-subsystem executables without a console, so output typed from a
/// terminal would vanish; borrow the terminal's console then. Pipes (the app itself running
/// `owo`, the gateway's log file) already provide handles and are left alone.
#[cfg(windows)]
fn attach_parent_console() {
    use windows_sys::Win32::System::Console::{AttachConsole, GetStdHandle, ATTACH_PARENT_PROCESS, STD_OUTPUT_HANDLE};
    // SAFETY: plain Win32 calls with constant arguments; a failed attach just leaves no console.
    unsafe {
        if GetStdHandle(STD_OUTPUT_HANDLE).is_null() {
            AttachConsole(ATTACH_PARENT_PROCESS);
        }
    }
}

#[cfg(not(windows))]
fn attach_parent_console() {}

pub fn run() {
    // Failing here only means the pages show the missing-config state; never block the window.
    if let Some(paths) = owo_config::OwoPaths::home() {
        if let Err(e) = config_edit::ensure_exists(&paths) {
            eprintln!("could not create {}: {e:#}", paths.config.display());
        }
    }
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
