mod applog;
mod catalog;
mod engine;
mod pfml;
mod selection;
mod settings;
mod state;
mod translate;

use serde::Serialize;
use settings::Settings;
use state::{AppState, EngineStatus};
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, RunEvent, WindowEvent};
use tauri_plugin_autostart::ManagerExt as AutostartExt;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

const MAIN: &str = "main";
const TRAY_ID: &str = "main-tray";

// ---------------------------------------------------------------------------
// ウィンドウ・ショートカット
// ---------------------------------------------------------------------------

fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window(MAIN) {
        let on_top = app.state::<AppState>().settings().always_on_top;
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_always_on_top(on_top);
        let _ = w.set_focus();
        #[cfg(windows)]
        if let Ok(hwnd) = w.hwnd() {
            let ok = selection::force_foreground(hwnd.0 as isize);
            applog::log(format!("show_main: foreground={}", ok));
        }
    }
}

/// グローバルショートカットが押されたとき
fn on_hotkey(app: &AppHandle) {
    static BUSY: AtomicBool = AtomicBool::new(false);
    if BUSY.swap(true, Ordering::SeqCst) {
        return;
    }
    let app = app.clone();
    std::thread::spawn(move || {
        applog::log("hotkey pressed");
        let state = app.state::<AppState>();
        let settings = state.settings();
        let own_focused = app
            .get_webview_window(MAIN)
            .map(|w| w.is_visible().unwrap_or(false) && w.is_focused().unwrap_or(false))
            .unwrap_or(false);

        let text = if settings.capture_selection && !own_focused {
            selection::capture_selection()
        } else {
            None
        };
        show_main(&app);
        match text {
            Some(t) => {
                *state.pending_selection.lock().unwrap() = Some(t);
                let _ = app.emit_to(MAIN, "selection-captured", ());
            }
            None => {
                let _ = app.emit_to(MAIN, "focus-input", ());
            }
        }
        BUSY.store(false, Ordering::SeqCst);
    });
}

fn register_shortcut(app: &AppHandle, shortcut: &str) -> Result<(), String> {
    let parsed = Shortcut::from_str(shortcut)
        .map_err(|e| format!("ショートカット「{}」を解釈できません: {}", shortcut, e))?;
    let gs = app.global_shortcut();
    let _ = gs.unregister_all();
    applog::log(format!("register shortcut: {} (id {})", parsed.into_string(), parsed.id()));
    gs.register(parsed).map_err(|e| {
        format!(
            "ショートカット「{}」を登録できません。他のアプリが使用している可能性があります。({})",
            shortcut, e
        )
    })?;
    update_tray_tooltip(app, shortcut);
    Ok(())
}

fn update_tray_tooltip(app: &AppHandle, shortcut: &str) {
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        let _ = tray.set_tooltip(Some(format!("PLaMo翻訳 ({})", shortcut)));
    }
}

// ---------------------------------------------------------------------------
// コマンド
// ---------------------------------------------------------------------------

#[tauri::command]
fn get_settings(state: tauri::State<AppState>) -> Settings {
    state.settings()
}

#[tauri::command]
async fn save_settings(app: AppHandle, settings: Settings) -> Result<Settings, String> {
    let state = app.state::<AppState>();
    let new = settings.sanitized();
    let old = state.settings();

    if new.shortcut != old.shortcut {
        if let Err(e) = register_shortcut(&app, &new.shortcut) {
            // 失敗したら元のショートカットに戻す
            let _ = register_shortcut(&app, &old.shortcut);
            return Err(e);
        }
        *state.shortcut_error.lock().unwrap() = None;
    } else if state.shortcut_error.lock().unwrap().is_some()
        && register_shortcut(&app, &new.shortcut).is_ok()
    {
        *state.shortcut_error.lock().unwrap() = None;
    }

    if new.launch_at_startup != old.launch_at_startup {
        let al = app.autolaunch();
        let r = if new.launch_at_startup { al.enable() } else { al.disable() };
        r.map_err(|e| format!("自動起動の設定に失敗しました: {}", e))?;
    }

    if let Some(w) = app.get_webview_window(MAIN) {
        if new.always_on_top != old.always_on_top {
            let _ = w.set_always_on_top(new.always_on_top);
        }
    }

    *state.settings.lock().unwrap() = new.clone();
    new.save(&state.settings_path)
        .map_err(|e| format!("設定を保存できません: {}", e))?;

    if new.engine_changed(&old) {
        let app2 = app.clone();
        tauri::async_runtime::spawn(async move { engine::start(&app2).await });
    }
    Ok(new)
}

#[tauri::command]
fn get_engine_status(state: tauri::State<AppState>) -> EngineStatus {
    state.status()
}

#[tauri::command]
async fn install_engine(app: AppHandle) -> Result<(), String> {
    engine::install_and_start(&app).await
}

#[tauri::command]
fn cancel_install(state: tauri::State<AppState>) {
    state.cancel_download.store(true, Ordering::SeqCst);
}

#[tauri::command]
async fn restart_engine(app: AppHandle) {
    engine::start(&app).await
}

#[tauri::command]
async fn translate(
    app: AppHandle,
    request: translate::TranslateRequest,
    request_id: u64,
) -> Result<Option<String>, String> {
    translate::translate(&app, request, request_id).await
}

#[tauri::command]
fn cancel_translation(state: tauri::State<AppState>) {
    state.translate_gen.store(0, Ordering::SeqCst);
}

#[tauri::command]
fn take_pending_selection(state: tauri::State<AppState>) -> Option<String> {
    state.pending_selection.lock().unwrap().take()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CatalogModel {
    #[serde(flatten)]
    info: catalog::ModelInfo,
    installed: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Catalog {
    models: Vec<CatalogModel>,
    backends: Vec<catalog::BackendInfo>,
    llama_cpp_tag: &'static str,
}

#[tauri::command]
fn get_catalog(state: tauri::State<AppState>) -> Catalog {
    let models_dir = state.data_dir.join("models");
    Catalog {
        models: catalog::MODELS
            .iter()
            .map(|m| CatalogModel {
                info: m.clone(),
                installed: models_dir.join(m.file_name()).exists(),
            })
            .collect(),
        backends: catalog::BACKENDS.to_vec(),
        llama_cpp_tag: catalog::LLAMA_CPP_TAG,
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AppInfo {
    version: String,
    data_dir: String,
    log_path: String,
    shortcut_error: Option<String>,
}

#[tauri::command]
fn get_app_info(app: AppHandle, state: tauri::State<AppState>) -> AppInfo {
    AppInfo {
        version: app.package_info().version.to_string(),
        data_dir: state.data_dir.display().to_string(),
        log_path: engine::server_log_path(&state).display().to_string(),
        shortcut_error: state.shortcut_error.lock().unwrap().clone(),
    }
}

/// ショートカット設定中は一時的にグローバルショートカットを無効にする
#[tauri::command]
fn suspend_shortcut(app: AppHandle, suspend: bool) -> Result<(), String> {
    if suspend {
        app.global_shortcut().unregister_all().map_err(|e| e.to_string())
    } else {
        let s = app.state::<AppState>().settings().shortcut;
        register_shortcut(&app, &s)
    }
}

#[tauri::command]
fn open_path(path: String) -> Result<(), String> {
    #[cfg(windows)]
    {
        std::process::Command::new("explorer.exe")
            .arg(&path)
            .spawn()
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
    #[cfg(not(windows))]
    {
        std::process::Command::new("xdg-open")
            .arg(&path)
            .spawn()
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
}

#[tauri::command]
fn hide_window(app: AppHandle) {
    if let Some(w) = app.get_webview_window(MAIN) {
        let _ = w.hide();
    }
}

// ---------------------------------------------------------------------------
// 起動
// ---------------------------------------------------------------------------

fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", "PLaMo翻訳を開く", true, None::<&str>)?;
    let settings = MenuItem::with_id(app, "settings", "設定...", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "終了", true, None::<&str>)?;
    let sep = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(app, &[&show, &settings, &sep, &quit])?;

    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip("PLaMo翻訳")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => show_main(app),
            "settings" => {
                show_main(app);
                let _ = app.emit_to(MAIN, "open-settings", ());
            }
            "quit" => {
                engine::stop_server(&app.state::<AppState>());
                app.exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            show_main(app);
        }))
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--minimized"]),
        ))
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, shortcut, event| {
                    applog::log(format!("shortcut event: {} {:?}", shortcut.into_string(), event.state()));
                    if event.state() == ShortcutState::Pressed {
                        on_hotkey(app);
                    }
                })
                .build(),
        )
        .setup(|app| {
            let handle = app.handle().clone();
            let config_dir = app.path().app_config_dir()?;
            let data_dir = app.path().app_local_data_dir()?;
            let log_dir = app.path().app_log_dir()?;
            applog::init(log_dir.clone());
            applog::log(format!("start v{}", app.package_info().version));
            let settings_path = config_dir.join("settings.json");
            let settings = Settings::load(&settings_path);

            let http = reqwest::Client::builder()
                .user_agent(concat!("plamo-translater/", env!("CARGO_PKG_VERSION")))
                .connect_timeout(std::time::Duration::from_secs(20))
                .build()?;
            let local_http = reqwest::Client::builder()
                .no_proxy()
                .connect_timeout(std::time::Duration::from_secs(5))
                .build()?;

            app.manage(AppState {
                settings: Mutex::new(settings.clone()),
                settings_path,
                data_dir,
                log_dir,
                status: Mutex::new(EngineStatus::Idle),
                server: Mutex::new(None),
                engine_lock: tokio::sync::Mutex::new(()),
                cancel_download: AtomicBool::new(false),
                http,
                local_http,
                shortcut_error: Mutex::new(None),
                translate_gen: AtomicU64::new(0),
                pending_selection: Mutex::new(None),
            });

            build_tray(&handle)?;

            if let Err(e) = register_shortcut(&handle, &settings.shortcut) {
                applog::log(&e);
                *app.state::<AppState>().shortcut_error.lock().unwrap() = Some(e);
            }

            // 自動起動の状態を設定と同期
            if settings.launch_at_startup {
                let _ = handle.autolaunch().enable();
            }

            let minimized = settings.start_minimized
                || std::env::args().any(|a| a == "--minimized");
            if !minimized {
                show_main(&handle);
            }

            tauri::async_runtime::spawn(async move { engine::start(&handle).await });
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                let app = window.app_handle();
                if app.state::<AppState>().settings().close_to_tray {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_settings,
            save_settings,
            get_engine_status,
            install_engine,
            cancel_install,
            restart_engine,
            translate,
            cancel_translation,
            take_pending_selection,
            get_catalog,
            get_app_info,
            suspend_shortcut,
            open_path,
            hide_window,
        ])
        .build(tauri::generate_context!())
        .expect("アプリケーションの初期化に失敗しました");

    app.run(|app, event| {
        if let RunEvent::Exit = event {
            if let Some(state) = app.try_state::<AppState>() {
                engine::stop_server(&state);
            }
        }
    });
}
