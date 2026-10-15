mod store;
mod sync;
mod system_theme;
mod watcher;

use std::sync::{Arc, LazyLock, Mutex};

use serde::Serialize;
use store::{Hit, Store};
use sync::{Status, Sync};
use system_theme::SystemTheme;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder, WindowEvent};

type Result<T> = std::result::Result<T, String>;

fn err(e: std::io::Error) -> String {
    e.to_string()
}

/// Emitted as "doc-changed" whenever a document changed on disk.
#[derive(Serialize, Clone)]
pub struct DocChanged {
    pub name: String,
    pub content: String,
}

/// The latest sync status, for a window that asks after it was emitted.
pub static LAST_STATUS: LazyLock<Mutex<Status>> = LazyLock::new(|| Mutex::new(Status { state: "off", ..Default::default() }));

/// `orthros --capture`: only the capture popup, then exit.
static CAPTURE_MODE: LazyLock<bool> = LazyLock::new(|| std::env::args().any(|a| a == "--capture"));

// ------------------------------------------------------------------ commands

#[tauri::command]
fn read_doc(store: tauri::State<Arc<Store>>, name: String) -> Result<String> {
    store.read(&name).map_err(err)
}

/// Save what the editor shows, based on `prev`; returns what is on disk now
/// (a merge, if the file changed underneath).
#[tauri::command]
fn save_doc(store: tauri::State<Arc<Store>>, name: String, content: String, prev: String) -> Result<String> {
    store.save(&name, &content, &prev).map_err(err)
}

#[tauri::command]
fn list_days(store: tauri::State<Arc<Store>>) -> Result<Vec<String>> {
    store.days().map_err(err)
}

#[tauri::command]
fn search(store: tauri::State<Arc<Store>>, query: String) -> Result<Vec<Hit>> {
    store.search(&query, 200).map_err(err)
}

#[tauri::command]
fn sync_now(app: AppHandle) {
    if let Some(s) = app.try_state::<Sync>() {
        s.now();
    }
}

#[tauri::command]
fn sync_status() -> Status {
    LAST_STATUS.lock().unwrap().clone()
}

/// Quick capture: one line into Later (a bullet) or today (a task).
#[tauri::command]
fn capture(app: AppHandle, store: tauri::State<Arc<Store>>, text: String, target: String, date: String) -> Result<()> {
    let text = text.trim();
    if !text.is_empty() {
        let (name, line) = match target.as_str() {
            "today" => (format!("days/{date}"), format!("- [ ] {text}")),
            _ => ("later".to_string(), format!("- {text}")),
        };
        store.append_line(&name, &line).map_err(err)?;
    }
    close_capture(app);
    Ok(())
}

#[tauri::command]
fn close_capture(app: AppHandle) {
    if *CAPTURE_MODE {
        app.exit(0);
    } else if let Some(w) = app.get_webview_window("capture") {
        let _ = w.hide();
    }
}

// ------------------------------------------------------------------ windows

fn main_window(app: &AppHandle) -> tauri::Result<()> {
    if let Some(w) = app.get_webview_window("main") {
        w.show()?;
        w.unminimize()?;
        return w.set_focus();
    }
    let b = WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
        .title("Orthros")
        .inner_size(960.0, 820.0)
        .min_inner_size(420.0, 400.0);
    #[cfg(target_os = "macos")]
    let b = b.title_bar_style(tauri::TitleBarStyle::Overlay).hidden_title(true);
    b.build()?;
    Ok(())
}

fn capture_window(app: &AppHandle) -> tauri::Result<()> {
    if let Some(w) = app.get_webview_window("capture") {
        w.center()?;
        w.show()?;
        return w.set_focus();
    }
    WebviewWindowBuilder::new(app, "capture", WebviewUrl::App("capture.html".into()))
        .title("Orthros capture")
        .inner_size(600.0, 128.0)
        .resizable(false)
        .decorations(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .center()
        .focused(true)
        .build()?;
    Ok(())
}

/// Open the capture popup from inside the running app. On Linux a separate
/// `orthros --capture` process: Wayland won't let a background app focus its
/// own window, but a freshly started one gets focus.
fn open_capture(app: &AppHandle) {
    #[cfg(target_os = "linux")]
    {
        let _ = app;
        if let Ok(exe) = std::env::current_exe() {
            let _ = std::process::Command::new(exe).arg("--capture").spawn();
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = capture_window(app);
    }
}

fn tray(app: &AppHandle) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "Open Orthros", true, None::<&str>)?;
    let capture = MenuItem::with_id(app, "capture", "Capture a thought…", true, None::<&str>)?;
    let sync = MenuItem::with_id(app, "sync", "Sync now", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit Orthros", true, None::<&str>)?;
    let sep = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(app, &[&open, &capture, &sync, &sep, &quit])?;

    // macOS draws a black template image in the menu bar's own colour; the
    // GNOME top bar is dark, so Linux gets a light glyph.
    #[cfg(target_os = "macos")]
    let icon = tauri::image::Image::from_bytes(include_bytes!("../icons/tray-template.png"))?;
    #[cfg(not(target_os = "macos"))]
    let icon = tauri::image::Image::from_bytes(include_bytes!("../icons/tray-light.png"))?;

    TrayIconBuilder::with_id("orthros")
        .icon(icon)
        .icon_as_template(cfg!(target_os = "macos"))
        .tooltip("Orthros")
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, e| match e.id().as_ref() {
            "open" => {
                let _ = main_window(app);
            }
            "capture" => open_capture(app),
            "sync" => sync_now(app.clone()),
            "quit" => app.exit(0),
            _ => {}
        })
        .build(app)?;
    Ok(())
}

// ------------------------------------------------------------------ run

/// `orthros --connect <server> <token>` sets up sync and exits.
fn connect_command() -> Option<i32> {
    let args: Vec<String> = std::env::args().collect();
    let i = args.iter().position(|a| a == "--connect")?;
    let (Some(server), Some(token)) = (args.get(i + 1), args.get(i + 2)) else {
        eprintln!("usage: orthros --connect https://your-server <token>");
        return Some(2);
    };
    match sync::connect(server, token) {
        Ok(path) => {
            println!("Sync set up ({}). Start Orthros; the dot at the top right turns green once it reaches the server.", path.display());
            Some(0)
        }
        Err(e) => {
            eprintln!("orthros: {e}");
            Some(1)
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    if let Some(code) = connect_command() {
        std::process::exit(code);
    }
    let store = Arc::new(Store::new(Store::default_root()).expect("cannot create the orthros data directory"));

    let mut builder = tauri::Builder::default().plugin(tauri_plugin_opener::init());

    if !*CAPTURE_MODE {
        // A second `orthros` brings the running one to the front.
        builder = builder.plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            let _ = main_window(app);
        }));
        #[cfg(target_os = "macos")]
        {
            use tauri_plugin_global_shortcut::{Builder as Shortcuts, ShortcutState};
            builder = builder.plugin(
                Shortcuts::new()
                    .with_shortcuts(["ctrl+alt+space"])
                    .expect("valid shortcut")
                    .with_handler(|app, _shortcut, event| {
                        if event.state == ShortcutState::Pressed {
                            open_capture(app);
                        }
                    })
                    .build(),
            );
        }
    }

    let app = builder
        .manage(store.clone())
        .manage(SystemTheme::default())
        .setup(move |app| {
            let handle = app.handle().clone();
            system_theme::watch(handle.clone());
            if *CAPTURE_MODE {
                capture_window(&handle)?;
                return Ok(());
            }
            main_window(&handle)?;
            // Not every desktop has a tray (GNOME without AppIndicator, some
            // Flatpak setups): Orthros then just runs without the icon.
            if let Err(e) = tray(&handle) {
                eprintln!("orthros: no tray icon: {e}");
            }
            app.manage(sync::start(handle.clone(), store.clone()));
            match watcher::start(handle.clone(), store.clone()) {
                Ok(w) => {
                    app.manage(w);
                }
                Err(e) => eprintln!("orthros: not watching the space for changes: {e}"),
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            // Closing the main window keeps Orthros running (tray, sync,
            // instant capture); quit from the tray menu.
            if let WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == "main" && !*CAPTURE_MODE {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            read_doc,
            save_doc,
            list_days,
            search,
            sync_now,
            sync_status,
            capture,
            close_capture,
            system_theme::system_theme
        ])
        .build(tauri::generate_context!())
        .expect("error while building orthros");

    app.run(|_app, _event| {
        // macOS: clicking the Dock icon brings the window back.
        #[cfg(target_os = "macos")]
        if let tauri::RunEvent::Reopen { .. } = _event {
            let _ = main_window(_app);
        }
    });
}
