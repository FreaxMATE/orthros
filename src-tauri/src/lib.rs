mod details;
mod links;
mod places;
mod store;
mod sync;
mod system_theme;
mod tiles;
mod watcher;

use std::sync::{Arc, LazyLock, Mutex};

use links::LinkPreview;
use serde::Serialize;
use store::{Hit, NoteInfo, Store};
use sync::{Status, Sync};
use system_theme::SystemTheme;
#[cfg(desktop)]
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
#[cfg(desktop)]
use tauri::tray::TrayIconBuilder;
#[cfg(desktop)]
use tauri::WindowEvent;
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

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
pub static LAST_STATUS: LazyLock<Mutex<Status>> = LazyLock::new(|| {
    Mutex::new(Status {
        state: "off",
        ..Default::default()
    })
});

/// `orthros --capture`: only the capture popup, then exit.
#[cfg(desktop)]
static CAPTURE_MODE: LazyLock<bool> = LazyLock::new(|| std::env::args().any(|a| a == "--capture"));

// ------------------------------------------------------------------ commands

#[tauri::command]
fn read_doc(store: tauri::State<Arc<Store>>, name: String) -> Result<String> {
    store.read(&name).map_err(err)
}

/// Save what the editor shows, based on `prev`; returns what is on disk now
/// (a merge, if the file changed underneath).
#[tauri::command]
fn save_doc(
    store: tauri::State<Arc<Store>>,
    name: String,
    content: String,
    prev: String,
) -> Result<String> {
    store.save(&name, &content, &prev).map_err(err)
}

#[tauri::command]
fn list_days(store: tauri::State<Arc<Store>>) -> Result<Vec<String>> {
    store.days().map_err(err)
}

#[tauri::command]
fn list_notes(store: tauri::State<Arc<Store>>) -> Result<Vec<NoteInfo>> {
    store.notes().map_err(err)
}

/// Title, description and image of a page, for the card under a bare URL.
/// None when it can't be reached. Blocking network I/O, so off the main thread.
#[tauri::command]
async fn link_preview(app: AppHandle, url: String) -> Option<LinkPreview> {
    let root = app.state::<Arc<Store>>().root().to_path_buf();
    tauri::async_runtime::spawn_blocking(move || links::preview(&root, &url))
        .await
        .ok()
        .flatten()
}

/// Coordinates for each map link, in the same order (None: not a place, or
/// the network failed). One blocking pass, since geocoding is rate limited.
#[tauri::command]
async fn resolve_places(app: AppHandle, urls: Vec<String>) -> Vec<Option<places::Place>> {
    let root = app.state::<Arc<Store>>().root().to_path_buf();
    tauri::async_runtime::spawn_blocking(move || {
        urls.iter().map(|u| places::resolve(&root, u)).collect()
    })
    .await
    .unwrap_or_default()
}

/// What is known about the place at a map pin: kind, address, opening hours,
/// a Wikipedia description and photo. None when nothing is known or nothing
/// could be fetched. Blocking network I/O, so off the main thread.
#[tauri::command]
async fn place_details(
    app: AppHandle,
    lat: f64,
    lng: f64,
    name: Option<String>,
    lang: String,
) -> Option<details::Details> {
    let root = app.state::<Arc<Store>>().root().to_path_buf();
    tauri::async_runtime::spawn_blocking(move || {
        details::lookup(&root, lat, lng, name.as_deref(), &lang)
    })
    .await
    .ok()
    .flatten()
}

/// Raw bytes of a map tile, style, sprite or glyph (allowed hosts only), from
/// the disk cache when possible. Empty when the tile does not exist.
#[tauri::command]
async fn fetch_cached(app: AppHandle, url: String) -> Result<tauri::ipc::Response> {
    let root = app.state::<Arc<Store>>().root().to_path_buf();
    tauri::async_runtime::spawn_blocking(move || tiles::fetch(&root, &url))
        .await
        .map_err(|e| e.to_string())?
        .map(tauri::ipc::Response::new)
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

/// The configured server and token, to show as a QR code for another device.
#[derive(Serialize)]
struct SyncInfo {
    server: String,
    token: String,
}

/// None when sync isn't configured (or its config is unusable).
#[tauri::command]
fn sync_info() -> Option<SyncInfo> {
    sync::info().map(|(server, token)| SyncInfo { server, token })
}

/// Set up sync from a scanned QR code (same as `orthros --connect`), then
/// sync right away.
#[tauri::command]
fn connect_sync(app: AppHandle, server: String, token: String) -> Result<()> {
    sync::connect(&server, &token)?;
    sync_now(app);
    Ok(())
}

/// Quick capture: one line into Later (a bullet) or today (a task).
#[tauri::command]
fn capture(
    app: AppHandle,
    store: tauri::State<Arc<Store>>,
    text: String,
    target: String,
    date: String,
) -> Result<()> {
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
    // There is no capture popup on mobile; the command stays so the frontend
    // can call it everywhere.
    #[cfg(desktop)]
    if *CAPTURE_MODE {
        app.exit(0);
    } else if let Some(w) = app.get_webview_window("capture") {
        let _ = w.hide();
    }
    #[cfg(mobile)]
    let _ = app;
}

// ------------------------------------------------------------------ windows

/// Mobile: one fullscreen webview, nothing to show or focus.
#[cfg(mobile)]
fn main_window(app: &AppHandle) -> tauri::Result<()> {
    if app.get_webview_window("main").is_none() {
        WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into())).build()?;
    }
    Ok(())
}

#[cfg(desktop)]
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
    let b = b
        .title_bar_style(tauri::TitleBarStyle::Overlay)
        .hidden_title(true);
    let window = b.build()?;
    #[cfg(target_os = "linux")]
    forward_pinch(&window)?;
    #[cfg(not(target_os = "linux"))]
    let _ = window;
    Ok(())
}

/// A touchpad pinch, as emitted to the page.
#[cfg(target_os = "linux")]
#[derive(Serialize, Clone)]
struct Pinch {
    begin: bool,
    scale: f64,
    x: f64,
    y: f64,
}

/// WebKitGTK zooms the whole page on a touchpad pinch, in native code that
/// JavaScript can't stop and never hears about. Take the pinch first and hand
/// it to the page, where the map uses it.
#[cfg(target_os = "linux")]
fn forward_pinch(window: &tauri::WebviewWindow) -> tauri::Result<()> {
    use gtk::gdk::EventTouchpadPinch;
    use gtk::prelude::WidgetExt;
    use tauri::Emitter;

    let target = window.clone();
    window.with_webview(move |webview| {
        webview.inner().connect_event(move |_, event| {
            let Some(pinch) = event.downcast_ref::<EventTouchpadPinch>() else {
                return glib::Propagation::Proceed;
            };
            let (x, y) = pinch.position();
            let begin = !pinch.is_phase(); // false only for GDK's "begin" (0)
            let scale = pinch.scale();
            // On Wayland the last event of a pinch resets the scale to 1 and
            // has no movement: obeying it would undo the zoom on release.
            let ended = !begin && scale == 1.0 && pinch.delta() == (0.0, 0.0) && pinch.angle_delta() == 0.0;
            if !ended {
                let _ = target.emit("touchpad-pinch", Pinch { begin, scale, x, y });
            }
            glib::Propagation::Stop
        });
    })
}

#[cfg(desktop)]
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
#[cfg(desktop)]
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

#[cfg(desktop)]
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
#[cfg(desktop)]
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

/// Linux: settings that have to be in place before GTK and WebKit start.
#[cfg(target_os = "linux")]
fn prepare_linux() {
    // WebKitGTK's DMA-BUF renderer fails on some GPU setups (NVIDIA, and
    // Flatpak when its GL driver doesn't match the system's): the window stays
    // blank or the app aborts with "Could not create default EGL display:
    // EGL_BAD_PARAMETER". A text app doesn't need it. Setting the variable
    // yourself (even to 0) keeps your choice.
    if std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none() {
        // SAFETY: called first thing in main, before any other thread exists.
        unsafe { std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1") };
    }
    // The tray library warns on every start that it is deprecated in favour
    // of a successor that Tauri doesn't use yet. It works fine; drop the noise.
    glib::log_set_writer_func(|level, fields| {
        if is_tray_deprecation(fields) {
            glib::LogWriterOutput::Handled
        } else {
            glib::log_writer_default(level, fields)
        }
    });
}

#[cfg(target_os = "linux")]
fn is_tray_deprecation(fields: &[glib::LogField<'_>]) -> bool {
    let field = |key: &str| {
        fields
            .iter()
            .find(|f| f.key() == key)
            .and_then(|f| f.value_str())
    };
    field("GLIB_DOMAIN") == Some("libayatana-appindicator")
        && field("MESSAGE").is_some_and(|m| m.contains("deprecated"))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    #[cfg(target_os = "linux")]
    prepare_linux();
    #[cfg(desktop)]
    if let Some(code) = connect_command() {
        std::process::exit(code);
    }

    let mut builder = tauri::Builder::default().plugin(tauri_plugin_opener::init());

    // QR scanner for pairing a phone with the desktop's sync settings.
    #[cfg(mobile)]
    {
        builder = builder.plugin(tauri_plugin_barcode_scanner::init());
    }

    #[cfg(desktop)]
    {
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
        builder = builder.on_window_event(|window, event| {
            // Closing the main window keeps Orthros running (tray, sync,
            // instant capture); quit from the tray menu.
            if let WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == "main" && !*CAPTURE_MODE {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        });
    }

    let app = builder
        .manage(SystemTheme::default())
        .setup(|app| {
            let handle = app.handle().clone();
            // Created here (not before the builder) because the mobile data
            // dir comes from the app's path resolver.
            #[cfg(desktop)]
            let root = Store::default_root();
            #[cfg(mobile)]
            let root = {
                let dir = app.path().app_data_dir()?;
                sync::set_config_path(dir.join("sync.json"));
                dir
            };
            let store = Arc::new(Store::new(root)?);
            app.manage(store.clone());
            system_theme::watch(handle.clone());
            #[cfg(desktop)]
            if *CAPTURE_MODE {
                capture_window(&handle)?;
                return Ok(());
            }
            main_window(&handle)?;
            // Not every desktop has a tray (GNOME without AppIndicator, some
            // Flatpak setups): Orthros then just runs without the icon.
            #[cfg(desktop)]
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
        .invoke_handler(tauri::generate_handler![
            read_doc,
            save_doc,
            list_days,
            list_notes,
            link_preview,
            resolve_places,
            place_details,
            fetch_cached,
            search,
            sync_now,
            sync_status,
            sync_info,
            connect_sync,
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
