mod store;

use store::{Hit, Store};

type Result<T> = std::result::Result<T, String>;

fn err(e: std::io::Error) -> String {
    e.to_string()
}

#[tauri::command]
fn read_doc(store: tauri::State<Store>, name: String) -> Result<String> {
    store.read(&name).map_err(err)
}

#[tauri::command]
fn write_doc(store: tauri::State<Store>, name: String, content: String) -> Result<()> {
    store.write(&name, &content).map_err(err)
}

#[tauri::command]
fn list_days(store: tauri::State<Store>) -> Result<Vec<String>> {
    store.days().map_err(err)
}

#[tauri::command]
fn search(store: tauri::State<Store>, query: String) -> Result<Vec<Hit>> {
    store.search(&query, 200).map_err(err)
}

#[tauri::command]
fn space_dir(store: tauri::State<Store>) -> String {
    store.root().display().to_string()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let store = Store::new(Store::default_root()).expect("cannot create the orthros data directory");

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(store)
        .invoke_handler(tauri::generate_handler![read_doc, write_doc, list_days, search, space_dir])
        .run(tauri::generate_context!())
        .expect("error while running orthros");
}
