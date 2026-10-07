use anyhow::{Context, Result};
use is_gpt_nerfed::{
    platform::{self, Tray, TrayEvent},
    runtime::{self, Action, Effect, Handle, ProbeTarget, Snapshot},
    storage::{Settings, Store},
};
use serde::Serialize;
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread::JoinHandle,
    time::Duration,
};
use tauri::{Emitter, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

struct Desktop {
    store: Store,
    tray: Mutex<Option<Tray>>,
    tray_worker: Mutex<Option<JoinHandle<()>>>,
    exiting: Arc<AtomicBool>,
    pet: Mutex<PetLayout>,
}

fn memory_usage(window: &WebviewWindow, low: bool) {
    #[cfg(windows)]
    let _ = window.with_webview(move |webview| {
        use webview2_com::Microsoft::Web::WebView2::Win32::{
            COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL, ICoreWebView2_19,
        };
        use windows_core::Interface;
        // Keep the document and unsaved input intact; only ask the runtime to reclaim caches.
        // Older WebView2 runtimes may not expose this optional interface.
        unsafe {
            if let Ok(core) = webview
                .controller()
                .CoreWebView2()
                .and_then(|v| v.cast::<ICoreWebView2_19>())
            {
                let _ = core.SetMemoryUsageTargetLevel(COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL(
                    i32::from(low),
                ));
            }
        }
    });
    #[cfg(not(windows))]
    let _ = (window, low);
}

#[derive(Clone, Copy, Default, Serialize)]
struct PetLayout {
    expanded: bool,
    chip_x: f64,
    chip_y: f64,
}

async fn execute(handle: &Handle, action: Action) -> std::result::Result<(), String> {
    let handle = handle.clone();
    tauri::async_runtime::spawn_blocking(move || handle.execute(action))
        .await
        .map_err(|e| e.to_string())?
}
#[tauri::command]
fn get_snapshot(handle: tauri::State<'_, Handle>) -> Snapshot {
    handle.snapshot()
}
#[tauri::command]
async fn refresh(handle: tauri::State<'_, Handle>) -> std::result::Result<(), String> {
    execute(&handle, Action::Refresh).await
}
#[tauri::command]
async fn scan_thread(
    handle: tauri::State<'_, Handle>,
    thread_id: String,
) -> std::result::Result<(), String> {
    execute(&handle, Action::Scan(thread_id)).await
}
#[tauri::command]
async fn start_probe(
    handle: tauri::State<'_, Handle>,
    target: ProbeTarget,
) -> std::result::Result<(), String> {
    execute(&handle, Action::Probe(target)).await
}
#[tauri::command]
async fn cancel_probe(handle: tauri::State<'_, Handle>) -> std::result::Result<(), String> {
    execute(&handle, Action::Cancel).await
}
#[tauri::command]
async fn save_settings(
    handle: tauri::State<'_, Handle>,
    settings: Settings,
    confirm_automatic: bool,
) -> std::result::Result<(), String> {
    execute(
        &handle,
        Action::Save {
            settings,
            confirm_automatic,
        },
    )
    .await
}
#[tauri::command]
async fn confirm_automatic(
    handle: tauri::State<'_, Handle>,
    confirmed: bool,
) -> std::result::Result<(), String> {
    execute(&handle, Action::ConfirmAutomatic(confirmed)).await
}
#[tauri::command]
async fn set_companion(
    handle: tauri::State<'_, Handle>,
    visible: bool,
) -> std::result::Result<(), String> {
    execute(&handle, Action::Companion(visible)).await
}
#[tauri::command]
async fn show_main(
    handle: tauri::State<'_, Handle>,
    follow_status: bool,
) -> std::result::Result<(), String> {
    execute(&handle, Action::ShowMain { follow_status }).await
}
#[tauri::command]
fn open_data_dir(desktop: tauri::State<'_, Desktop>) -> std::result::Result<(), String> {
    platform::open_folder(&desktop.store.home).map_err(|e| e.to_string())
}
#[tauri::command]
async fn quit(handle: tauri::State<'_, Handle>) -> std::result::Result<(), String> {
    execute(&handle, Action::Quit).await
}

#[tauri::command]
fn window_ready(
    window: WebviewWindow,
    handle: tauri::State<'_, Handle>,
    desktop: tauri::State<'_, Desktop>,
) -> std::result::Result<(), String> {
    if !handle.snapshot().quitting
        && (window.label() == "main" || handle.snapshot().settings.companion)
    {
        if window.label() == "companion" {
            show_pet_window(&window, &desktop).map_err(|e| e.to_string())?;
        } else {
            memory_usage(&window, false);
            window.show().map_err(|e| e.to_string())?;
            let _ = window.set_focus();
        }
    }
    Ok(())
}

// Both sizes and offsets are logical; placement is computed in physical monitor coordinates.
fn placement(
    anchor: [f64; 2],
    bounds: [f64; 4],
    scale: f64,
    expanded: bool,
) -> ([f64; 2], PetLayout) {
    let chip = 96.0 * scale;
    let size = if expanded {
        [344.0 * scale, 184.0 * scale]
    } else {
        [chip, chip]
    };
    let ax = anchor[0].clamp(bounds[0], (bounds[2] - chip).max(bounds[0]));
    let ay = anchor[1].clamp(bounds[1], (bounds[3] - chip).max(bounds[1]));
    let x = if ax + size[0] > bounds[2] {
        ax + chip - size[0]
    } else {
        ax
    };
    let y = if ay + size[1] > bounds[3] {
        ay + chip - size[1]
    } else {
        ay
    };
    let x = x.clamp(bounds[0], (bounds[2] - size[0]).max(bounds[0]));
    let y = y.clamp(bounds[1], (bounds[3] - size[1]).max(bounds[1]));
    (
        [x, y],
        PetLayout {
            expanded,
            chip_x: (ax - x) / scale,
            chip_y: (ay - y) / scale,
        },
    )
}
fn layout_pet(window: &WebviewWindow, desktop: &Desktop, expanded: bool) -> Result<PetLayout> {
    let mut current = desktop.pet.lock().unwrap_or_else(|e| e.into_inner());
    let scale = window.scale_factor()?;
    let pos = window.outer_position()?;
    let anchor = [
        pos.x as f64 + current.chip_x * scale,
        pos.y as f64 + current.chip_y * scale,
    ];
    let monitor = window
        .available_monitors()?
        .into_iter()
        .find(|m| {
            let p = m.position();
            let s = m.size();
            anchor[0] >= p.x as f64
                && anchor[0] < p.x as f64 + s.width as f64
                && anchor[1] >= p.y as f64
                && anchor[1] < p.y as f64 + s.height as f64
        })
        .or(window.current_monitor()?)
        .or(window.primary_monitor()?)
        .context("无法读取显示器信息")?;
    let area = monitor.work_area();
    let (pos, layout) = placement(
        anchor,
        [
            area.position.x as f64,
            area.position.y as f64,
            (area.position.x + area.size.width as i32) as f64,
            (area.position.y + area.size.height as i32) as f64,
        ],
        scale,
        expanded,
    );
    let size = if expanded {
        [344.0, 184.0]
    } else {
        [96.0, 96.0]
    };
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            SWP_NOACTIVATE, SWP_NOZORDER, SetWindowPos,
        };
        // A single native operation preserves the chip anchor without an intermediate off-screen frame.
        if SetWindowPos(
            window.hwnd()?.0 as _,
            std::ptr::null_mut(),
            pos[0].round() as i32,
            pos[1].round() as i32,
            (size[0] * scale).round() as i32,
            (size[1] * scale).round() as i32,
            SWP_NOACTIVATE | SWP_NOZORDER,
        ) == 0
        {
            return Err(std::io::Error::last_os_error().into());
        }
    }
    #[cfg(not(windows))]
    {
        window.set_position(tauri::PhysicalPosition::new(pos[0] as i32, pos[1] as i32))?;
        window.set_size(tauri::LogicalSize::new(size[0], size[1]))?;
    }
    *current = layout;
    Ok(layout)
}
#[tauri::command]
fn companion_layout(
    window: WebviewWindow,
    desktop: tauri::State<'_, Desktop>,
    expanded: bool,
) -> std::result::Result<PetLayout, String> {
    if window.label() != "companion" {
        return Err("此操作仅适用于桌宠窗口".into());
    }
    layout_pet(&window, &desktop, expanded).map_err(|e| e.to_string())
}
#[tauri::command]
fn companion_menu(window: WebviewWindow, app: tauri::AppHandle) -> std::result::Result<(), String> {
    use tauri::menu::{Menu, MenuItem};
    let build = || -> tauri::Result<()> {
        let show = MenuItem::with_id(&app, "pet-show", "打开主界面", true, None::<&str>)?;
        let hide = MenuItem::with_id(&app, "pet-hide", "收起桌宠", true, None::<&str>)?;
        let quit = MenuItem::with_id(&app, "pet-quit", "退出程序", true, None::<&str>)?;
        window.popup_menu(&Menu::with_items(&app, &[&show, &hide, &quit])?)
    };
    build().map_err(|e| e.to_string())
}
fn navigation_allowed(url: &tauri::Url) -> bool {
    matches!(url.scheme(), "tauri" | "about")
        || url.host_str() == Some("tauri.localhost")
        || (cfg!(debug_assertions)
            && url.host_str() == Some("127.0.0.1")
            && url.port() == Some(5173))
}
fn build_main(app: &tauri::AppHandle, store: &Store) -> Result<WebviewWindow> {
    let window = WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
        .title("is-gpt-nerfed — Windows")
        .inner_size(1000.0, 680.0)
        .min_inner_size(800.0, 520.0)
        .center()
        .prevent_overflow()
        .decorations(false)
        .shadow(true)
        .transparent(true)
        .visible(false)
        .background_color(tauri::webview::Color(246, 248, 253, 0))
        .data_directory(store.home.join("webview"))
        .on_navigation(navigation_allowed)
        .build()?;
    let _ = window.set_effects(tauri::utils::config::WindowEffectsConfig {
        effects: vec![tauri::utils::WindowEffect::MicaLight],
        ..Default::default()
    });
    Ok(window)
}
fn show_main_window(app: &tauri::AppHandle) -> Result<()> {
    let window = if let Some(w) = app.get_webview_window("main") {
        w
    } else {
        build_main(app, &app.state::<Desktop>().store)?
    };
    memory_usage(&window, false);
    window.unminimize()?;
    window.show()?;
    window.set_focus()?;
    Ok(())
}
fn set_pet_window(app: &tauri::AppHandle, visible: bool) -> Result<()> {
    let desktop = app.state::<Desktop>();
    if let Some(window) = app.get_webview_window("companion") {
        if visible {
            show_pet_window(&window, &desktop)?;
        } else {
            window.hide()?;
            memory_usage(&window, true);
            let layout = layout_pet(&window, &desktop, false)?;
            window.emit("companion-layout", layout)?;
        }
        return Ok(());
    }
    if !visible {
        return Ok(());
    }
    *desktop.pet.lock().unwrap_or_else(|e| e.into_inner()) = PetLayout::default();
    let window = WebviewWindowBuilder::new(
        app,
        "companion",
        WebviewUrl::App("index.html?window=companion".into()),
    )
    .title("Nerfed · 桌面浮窗")
    .inner_size(96.0, 96.0)
    .min_inner_size(96.0, 96.0)
    .decorations(false)
    .shadow(false)
    .transparent(true)
    .resizable(false)
    .maximizable(false)
    .minimizable(false)
    .always_on_top(true)
    .skip_taskbar(true)
    .focused(false)
    .visible(false)
    .background_color(tauri::webview::Color(0, 0, 0, 0))
    .data_directory(desktop.store.home.join("webview"))
    .on_navigation(navigation_allowed)
    .build()?;
    if let Some(monitor) = window.primary_monitor()? {
        let area = monitor.work_area();
        let scale = monitor.scale_factor();
        window.set_position(tauri::PhysicalPosition::new(
            area.position.x + area.size.width as i32 - (120.0 * scale) as i32,
            area.position.y + area.size.height as i32 - (120.0 * scale) as i32,
        ))?;
    }
    Ok(())
}
fn show_pet_window(window: &WebviewWindow, desktop: &Desktop) -> Result<()> {
    memory_usage(window, false);
    window.show()?;
    // Showing a captionless Win32 window can restore the system's wider default frame.
    let expanded = desktop
        .pet
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .expanded;
    let layout = layout_pet(window, desktop, expanded)?;
    window.emit("companion-layout", layout)?;
    Ok(())
}
fn missing_webview() {
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            IDYES, MB_ICONINFORMATION, MB_YESNO, MessageBoxW,
        };
        let wide = |s: &str| s.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
        if MessageBoxW(std::ptr::null_mut(), wide("Nerfed 需要 Microsoft Edge WebView2 Runtime。\n\n请安装后重新打开本程序。是否打开微软官方安装页面？\n\nhttps://developer.microsoft.com/microsoft-edge/webview2/").as_ptr(), wide("需要 WebView2 运行时").as_ptr(), MB_YESNO | MB_ICONINFORMATION) == IDYES {
            let _ = platform::command(std::path::Path::new("explorer.exe"), &["https://developer.microsoft.com/microsoft-edge/webview2/".into()]).and_then(|mut c| c.spawn().map_err(Into::into));
        }
    }
}
pub fn run() -> Result<()> {
    if tauri::webview_version().is_err() {
        missing_webview();
        return Ok(());
    }
    let store = Store::local()?;
    let cfg = store.settings()?;
    let app = tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            get_snapshot,
            refresh,
            scan_thread,
            start_probe,
            cancel_probe,
            save_settings,
            confirm_automatic,
            set_companion,
            show_main,
            open_data_dir,
            quit,
            window_ready,
            companion_layout,
            companion_menu
        ])
        .setup(move |app| {
            let app_handle = app.handle().clone();
            let tray = Tray::new()?;
            let exiting = Arc::new(AtomicBool::new(false));
            let callback_app = app_handle.clone();
            let handle = runtime::launch(store.clone(), cfg.clone(), move |effect| {
                let app = &callback_app;
                let result = match effect {
                    Effect::State(state) => {
                        app.emit("app-state", &state).map_err(anyhow::Error::from)
                    }
                    Effect::Notify(message) => {
                        if let Some(state) = app.try_state::<Desktop>()
                            && let Some(tray) = state
                                .tray
                                .lock()
                                .unwrap_or_else(|e| e.into_inner())
                                .as_ref()
                        {
                            tray.notify(message);
                        }
                        Ok(())
                    }
                    Effect::ShowMain => show_main_window(app),
                    Effect::Companion(visible) => set_pet_window(app, visible),
                    Effect::Exit => {
                        let state = app.state::<Desktop>();
                        state.exiting.store(true, Ordering::Relaxed);
                        if let Some(worker) = state
                            .tray_worker
                            .lock()
                            .unwrap_or_else(|e| e.into_inner())
                            .take()
                        {
                            let _ = worker.join();
                        }
                        state.tray.lock().unwrap_or_else(|e| e.into_inner()).take();
                        app.exit(0);
                        Ok(())
                    }
                };
                if let Err(error) = result {
                    let _ = app.emit("desktop-error", format!("{error:#}"));
                }
            })?;
            app.manage(handle.clone());
            app.manage(Desktop {
                store: store.clone(),
                tray: Mutex::new(Some(tray)),
                tray_worker: Mutex::new(None),
                exiting: exiting.clone(),
                pet: Mutex::new(PetLayout::default()),
            });
            let worker_app = app_handle.clone();
            let worker = std::thread::spawn(move || {
                while !exiting.load(Ordering::Relaxed) {
                    let events = {
                        let state = worker_app.state::<Desktop>();
                        let tray = state.tray.lock().unwrap_or_else(|e| e.into_inner());
                        tray.as_ref()
                            .map(|t| t.events.try_iter().collect::<Vec<_>>())
                            .unwrap_or_default()
                    };
                    for event in events {
                        handle.post(match event {
                            TrayEvent::Show => Action::ShowMain {
                                follow_status: false,
                            },
                            TrayEvent::ToggleAutomatic => Action::ToggleAutomatic,
                            TrayEvent::ToggleCompanion => {
                                Action::Companion(!handle.snapshot().settings.companion)
                            }
                            TrayEvent::Quit => Action::Quit,
                        });
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
            });
            *app.state::<Desktop>()
                .tray_worker
                .lock()
                .unwrap_or_else(|e| e.into_inner()) = Some(worker);
            build_main(&app_handle, &store)?;
            if cfg.companion {
                set_pet_window(&app_handle, true)?;
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                if window.label() == "main" {
                    let _ = window.hide();
                    if let Some(webview) = window.get_webview_window("main") {
                        memory_usage(&webview, true);
                    }
                } else {
                    window.state::<Handle>().post(Action::Companion(false));
                }
            }
            if let tauri::WindowEvent::Focused(focused) = event
                && let Some(webview) = window.get_webview_window(window.label())
            {
                if *focused {
                    memory_usage(&webview, false);
                } else if window.is_minimized().unwrap_or(false) {
                    memory_usage(&webview, true);
                }
            }
        })
        .on_menu_event(|app, event| {
            let action = match event.id().as_ref() {
                "pet-show" => Some(Action::ShowMain {
                    follow_status: true,
                }),
                "pet-hide" => Some(Action::Companion(false)),
                "pet-quit" => Some(Action::Quit),
                _ => None,
            };
            if let Some(action) = action {
                app.state::<Handle>().post(action);
            }
        })
        .build(tauri::generate_context!())
        .context("无法启动 Windows 界面")?;
    app.run(|app, event| {
        if let tauri::RunEvent::ExitRequested { api, .. } = event
            && !app.state::<Desktop>().exiting.load(Ordering::Relaxed)
        {
            api.prevent_exit();
            app.state::<Handle>().post(Action::Quit);
        }
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn companion_expansion_keeps_anchor_inside_scaled_work_areas() {
        for scale in [1.0, 1.5, 2.0] {
            for bounds in [[0.0, 0.0, 1920.0, 1040.0], [-1920.0, -200.0, 0.0, 840.0]] {
                for anchor in [
                    [bounds[0] + 8.0, bounds[1] + 8.0],
                    [bounds[2] - 110.0 * scale, bounds[3] - 110.0 * scale],
                ] {
                    let (pos, layout) = placement(anchor, bounds, scale, true);
                    assert!((pos[0] + layout.chip_x * scale - anchor[0]).abs() < 0.001);
                    assert!((pos[1] + layout.chip_y * scale - anchor[1]).abs() < 0.001);
                    assert!(pos[0] >= bounds[0] && pos[0] + 344.0 * scale <= bounds[2]);
                    assert!(pos[1] >= bounds[1] && pos[1] + 184.0 * scale <= bounds[3]);
                    let (collapsed, _) = placement(anchor, bounds, scale, false);
                    assert_eq!(collapsed, anchor);
                }
            }
        }
    }
}
