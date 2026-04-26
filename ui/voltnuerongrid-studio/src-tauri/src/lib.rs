use tauri::Manager;

// ─── Credential store (OS keychain via keyring) ─────────────────────────────

const SERVICE: &str = "com.voltnuerongrid.studio";

#[tauri::command]
fn store_credential(conn_id: String, key: String, value: String) -> Result<(), String> {
    let username = format!("{conn_id}:{key}");
    let entry = keyring::Entry::new(SERVICE, &username).map_err(|e| e.to_string())?;
    entry.set_password(&value).map_err(|e| e.to_string())
}

#[tauri::command]
fn get_credential(conn_id: String, key: String) -> Result<Option<String>, String> {
    let username = format!("{conn_id}:{key}");
    let entry = keyring::Entry::new(SERVICE, &username).map_err(|e| e.to_string())?;
    match entry.get_password() {
        Ok(pw) => Ok(Some(pw)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

#[tauri::command]
fn delete_credential(conn_id: String, key: String) -> Result<(), String> {
    let username = format!("{conn_id}:{key}");
    let entry = keyring::Entry::new(SERVICE, &username).map_err(|e| e.to_string())?;
    match entry.delete_password() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}

// ─── File I/O ────────────────────────────────────────────────────────────────

#[tauri::command]
async fn read_sql_file(path: String) -> Result<String, String> {
    tokio::fs::read_to_string(&path)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn write_sql_file(path: String, content: String) -> Result<(), String> {
    tokio::fs::write(&path, content.as_bytes())
        .await
        .map_err(|e| e.to_string())
}

// ─── Window controls (custom title bar) ─────────────────────────────────────

#[tauri::command]
fn window_minimize(app: tauri::AppHandle) {
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.minimize();
    }
}

#[tauri::command]
fn window_toggle_maximize(app: tauri::AppHandle) {
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.toggle_maximize();
    }
}

#[tauri::command]
fn window_close(app: tauri::AppHandle) {
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.close();
    }
}

// ─── App entry ───────────────────────────────────────────────────────────────

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            store_credential,
            get_credential,
            delete_credential,
            read_sql_file,
            write_sql_file,
            window_minimize,
            window_toggle_maximize,
            window_close,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
