//! The desktop's light/dark preference, for the "system" theme.
//!
//! On Linux the webview (WebKitGTK, a GTK3 app) only looks at the GTK3 theme
//! name, so with GNOME in dark mode and a light GTK3 theme like adw-gtk3 it
//! reports "light". The reliable source is the XDG desktop portal's
//! `org.freedesktop.appearance color-scheme` (1 = prefer dark, 2 = prefer
//! light), which also signals changes. We read it, watch it, and hand it to
//! the frontend. Elsewhere (macOS) the webview's own detection is correct, so
//! this reports nothing and the frontend falls back to `prefers-color-scheme`.

use std::sync::Mutex;

use tauri::{AppHandle, Emitter, Manager};

/// "dark", "light", or None when unknown / no preference.
#[derive(Default)]
pub struct SystemTheme(pub Mutex<Option<&'static str>>);

#[tauri::command]
pub fn system_theme(state: tauri::State<SystemTheme>) -> Option<&'static str> {
    *state.0.lock().unwrap()
}

#[cfg(target_os = "linux")]
pub fn watch(app: AppHandle) {
    use zbus::blocking::{Connection, Proxy};
    use zbus::zvariant::{OwnedValue, Value};

    fn scheme(v: &Value) -> Option<&'static str> {
        match v {
            Value::Value(inner) => scheme(inner),
            Value::U32(1) => Some("dark"),
            Value::U32(2) => Some("light"),
            _ => None,
        }
    }

    fn set(app: &AppHandle, s: Option<&'static str>) {
        *app.state::<SystemTheme>().0.lock().unwrap() = s;
        let _ = app.emit("system-theme", s);
    }

    std::thread::spawn(move || {
        let run = || -> zbus::Result<()> {
            let conn = Connection::session()?;
            let proxy = Proxy::new(
                &conn,
                "org.freedesktop.portal.Desktop",
                "/org/freedesktop/portal/desktop",
                "org.freedesktop.portal.Settings",
            )?;
            let initial: OwnedValue = proxy
                .call("ReadOne", &("org.freedesktop.appearance", "color-scheme"))
                // Portals older than v2 only have the deprecated Read.
                .or_else(|_| proxy.call("Read", &("org.freedesktop.appearance", "color-scheme")))?;
            set(&app, scheme(&initial));

            for msg in proxy.receive_signal("SettingChanged")? {
                let (ns, key, value): (String, String, OwnedValue) = msg.body().deserialize()?;
                if ns == "org.freedesktop.appearance" && key == "color-scheme" {
                    set(&app, scheme(&value));
                }
            }
            Ok(())
        };
        if let Err(e) = run() {
            eprintln!("orthros: cannot read the desktop color scheme: {e}");
        }
    });
}

#[cfg(not(target_os = "linux"))]
pub fn watch(_app: AppHandle) {}
