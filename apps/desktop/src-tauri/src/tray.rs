use std::sync::Mutex;
use std::time::Duration;

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{App, AppHandle, Emitter, Manager, Wry};
use tauri_plugin_positioner::{Position, WindowExt};

use crate::commands;

const TRAY_ID: &str = "owo-tray";
const EVENT_NAVIGATE: &str = "tray:navigate";

#[derive(Clone, Copy)]
enum Language {
    English,
    Chinese,
    Japanese,
}

impl Language {
    fn parse(value: &str) -> Self {
        if value.starts_with("zh") {
            Self::Chinese
        } else if value.starts_with("ja") {
            Self::Japanese
        } else {
            Self::English
        }
    }
}

#[derive(Default)]
struct Snapshot {
    language: Option<Language>,
    gateway_running: bool,
    config_exists: bool,
}

struct TrayMenuState {
    gateway: MenuItem<Wry>,
    open: MenuItem<Wry>,
    popover: MenuItem<Wry>,
    quit: MenuItem<Wry>,
    snapshot: Mutex<Snapshot>,
}

struct Labels {
    gateway: String,
    open: &'static str,
    popover: &'static str,
    quit: &'static str,
}

fn labels(snapshot: &Snapshot) -> Labels {
    let language = snapshot.language.unwrap_or(Language::English);
    match language {
        Language::English => Labels {
            gateway: if snapshot.gateway_running {
                "Stop Gateway"
            } else {
                "Start Gateway"
            }
            .into(),
            open: "Open dashboard",
            popover: "Quick controls",
            quit: "Quit",
        },
        Language::Chinese => Labels {
            gateway: if snapshot.gateway_running {
                "停止网关"
            } else {
                "启动网关"
            }
            .into(),
            open: "打开仪表盘",
            popover: "打开快速控制面板",
            quit: "退出",
        },
        Language::Japanese => Labels {
            gateway: if snapshot.gateway_running {
                "ゲートウェイを停止"
            } else {
                "ゲートウェイを起動"
            }
            .into(),
            open: "ダッシュボードを開く",
            popover: "クイックパネルを開く",
            quit: "終了",
        },
    }
}

fn apply_labels(state: &TrayMenuState) {
    let snapshot = state.snapshot.lock().unwrap();
    let labels = labels(&snapshot);
    let _ = state.gateway.set_text(labels.gateway);
    let _ = state.open.set_text(labels.open);
    let _ = state.popover.set_text(labels.popover);
    let _ = state.quit.set_text(labels.quit);
    let _ = state.gateway.set_enabled(snapshot.config_exists);
}

pub fn setup(app: &mut App<Wry>) -> tauri::Result<()> {
    let initial = labels(&Snapshot::default());
    let gateway = MenuItem::with_id(app, "gateway-toggle", initial.gateway, false, None::<&str>)?;
    let open = MenuItem::with_id(app, "open", initial.open, true, None::<&str>)?;
    let popover = MenuItem::with_id(app, "open-popover", initial.popover, true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", initial.quit, true, None::<&str>)?;
    let separator1 = PredefinedMenuItem::separator(app)?;
    let separator2 = PredefinedMenuItem::separator(app)?;
    let _menu = Menu::with_items(
        app,
        &[&popover, &open, &separator1, &gateway, &separator2, &quit],
    )?;

    app.manage(TrayMenuState {
        gateway,
        open,
        popover,
        quit,
        snapshot: Mutex::new(Snapshot::default()),
    });

    let builder = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip("OwO AI Gateway")
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "open" => show_main(app, Some("dashboard")),
            "open-popover" => show_popover(app),
            "gateway-toggle" => {
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    toggle_gateway(&app).await;
                });
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            tauri_plugin_positioner::on_tray_event(tray.app_handle(), &event);
            if let TrayIconEvent::Click {
                button,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                if button == MouseButton::Left {
                    toggle_popover(tray.app_handle());
                }
                #[cfg(target_os = "macos")]
                if button == MouseButton::Right {
                    toggle_popover(tray.app_handle());
                }
            }
        });

    // macOS associates the menu with the status item itself and can show it before its click
    // handler runs. Leave it unattached there so a left click can open the custom popover.
    // Linux does not emit tray click events, so its native menu is also the entry point
    // for quick controls and restoring the main window after it has been closed.
    #[cfg(any(target_os = "windows", target_os = "linux"))]
    let builder = builder.menu(&_menu);

    let mut builder = builder;
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;

    let app_handle = app.handle().clone();
    tauri::async_runtime::spawn(async move {
        loop {
            refresh(&app_handle).await;
            tokio::time::sleep(Duration::from_secs(30)).await;
        }
    });

    Ok(())
}

async fn refresh(app: &AppHandle<Wry>) {
    let status = commands::status().await;
    let Some(state) = app.try_state::<TrayMenuState>() else {
        return;
    };
    {
        let mut snapshot = state.snapshot.lock().unwrap();
        if let Ok(status) = status {
            snapshot.gateway_running = status.gateway_running;
            snapshot.config_exists = status.config_exists;
        }
    }
    apply_labels(&state);
}

async fn toggle_gateway(app: &AppHandle<Wry>) {
    let result = match commands::status().await {
        Ok(status) if status.gateway_running => commands::gateway_stop().await,
        Ok(_) => commands::gateway_start().await,
        Err(error) => {
            eprintln!("could not read gateway state from tray: {error}");
            refresh(app).await;
            return;
        }
    };
    match result {
        Ok(result) if !result.ok => eprintln!("gateway toggle action was rejected"),
        Err(error) => eprintln!("could not toggle gateway from tray: {error}"),
        _ => {}
    }
    refresh(app).await;
}

pub fn show_main(app: &AppHandle<Wry>, page: Option<&str>) {
    hide_popover(app);
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    let _ = window.unminimize();
    let _ = window.show();
    let _ = window.set_focus();
    if let Some(page) = page {
        let _ = app.emit(EVENT_NAVIGATE, page);
    }
}

fn toggle_popover(app: &AppHandle<Wry>) {
    let Some(window) = app.get_webview_window("tray-popover") else {
        return;
    };
    if window.is_visible().unwrap_or(false) {
        let _ = window.hide();
        return;
    }
    show_popover(app);
}

fn show_popover(app: &AppHandle<Wry>) {
    let Some(window) = app.get_webview_window("tray-popover") else {
        eprintln!("tray popover window is not configured");
        return;
    };
    if let Err(error) = window
        .as_ref()
        .window()
        .move_window_constrained(Position::TrayCenter)
    {
        eprintln!("could not position tray popover: {error}");
    }
    if let Err(error) = window.show() {
        eprintln!("could not show tray popover: {error}");
    }
    if let Err(error) = window.set_focus() {
        eprintln!("could not focus tray popover: {error}");
    }
}

fn hide_popover(app: &AppHandle<Wry>) {
    if let Some(window) = app.get_webview_window("tray-popover") {
        let _ = window.hide();
    }
}

#[tauri::command]
pub fn tray_open_main(app: AppHandle<Wry>, page: Option<String>) {
    show_main(&app, page.as_deref());
}

#[tauri::command]
pub fn tray_close_popover(app: AppHandle<Wry>) {
    hide_popover(&app);
}

#[tauri::command]
pub fn tray_quit(app: AppHandle<Wry>) {
    app.exit(0);
}

#[tauri::command]
pub fn set_tray_language(app: AppHandle<Wry>, language: String) {
    let Some(state) = app.try_state::<TrayMenuState>() else {
        return;
    };
    state.snapshot.lock().unwrap().language = Some(Language::parse(&language));
    apply_labels(&state);
    let _ = app.emit("tray:language", language);
}

#[tauri::command]
pub async fn tray_refresh(app: AppHandle<Wry>) {
    refresh(&app).await;
}
