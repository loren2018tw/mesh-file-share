mod files;
mod scheduler;
mod server;
mod state;

use state::AppState;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::Manager;

fn resolve_client_dist_dir(app: &tauri::App) -> Option<String> {
    let mut candidates: Vec<PathBuf> = vec![];

    // 開發模式常見路徑（工作目錄在專案根目錄）
    candidates.push(PathBuf::from("dist-client"));
    candidates.push(PathBuf::from("../dist-client"));

    // 打包後優先從 resource 目錄尋找
    if let Ok(resource_dir) = app.path().resource_dir() {
        candidates.push(resource_dir.join("dist-client"));
        candidates.push(resource_dir.join("_up_/dist-client"));
    }

    // 直接執行 release binary 時，嘗試由執行檔位置回推
    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(exe_dir) = exe_path.parent() {
            candidates.push(exe_dir.join("dist-client"));
            candidates.push(exe_dir.join("_up_/dist-client"));
            candidates.push(exe_dir.join("../dist-client"));
            candidates.push(exe_dir.join("../_up_/dist-client"));
            candidates.push(exe_dir.join("../../dist-client"));
            candidates.push(exe_dir.join("../../_up_/dist-client"));
        }
    }

    candidates
        .into_iter()
        .find(|dir| dir.join("client.html").exists())
        .map(|dir| dir.to_string_lossy().to_string())
}

/// Tauri 管理的共享狀態
struct TauriState {
    app_state: AppState,
}

/// Tauri Command: 新增分享檔案
#[tauri::command]
async fn add_file(
    state: tauri::State<'_, TauriState>,
    path: String,
) -> Result<state::FileInfo, String> {
    let path = PathBuf::from(&path);
    let (name, size) = files::get_file_metadata(&path)
        .await
        .map_err(|e| e.to_string())?;
    let file = state.app_state.add_file(name, size, path.clone()).await;
    Ok(file)
}

/// Tauri Command: 移除分享檔案
#[tauri::command]
async fn remove_file(state: tauri::State<'_, TauriState>, id: String) -> Result<bool, String> {
    Ok(state.app_state.remove_file(&id).await)
}

/// Tauri Command: 取得分享檔案清單
#[tauri::command]
async fn list_files(state: tauri::State<'_, TauriState>) -> Result<Vec<state::FileInfo>, String> {
    Ok(state.app_state.list_files().await)
}

/// Tauri Command: 取得伺服器資訊
#[tauri::command]
async fn get_server_info(state: tauri::State<'_, TauriState>) -> Result<serde_json::Value, String> {
    let port = state.app_state.port;
    let ip = local_ip_address::local_ip().map_err(|e| e.to_string())?;
    Ok(serde_json::json!({
        "ip": ip.to_string(),
        "port": port,
        "url": format!("https://{}:{}", ip, port),
    }))
}

/// Tauri Command: 取得已連線的下載端清單
#[tauri::command]
async fn list_clients(
    state: tauri::State<'_, TauriState>,
) -> Result<Vec<serde_json::Value>, String> {
    let clients = state.app_state.clients.read().await;
    let result: Vec<serde_json::Value> = clients
        .values()
        .map(|c| {
            serde_json::json!({
                "id": c.id,
                "completedFiles": c.completed_files,
                "isRelaying": c.is_relaying,
            })
        })
        .collect();
    Ok(result)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let port: u16 = 8080;
    let app_state = AppState::new(port);
    let server_state = app_state.clone();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(TauriState {
            app_state: app_state.clone(),
        })
        .invoke_handler(tauri::generate_handler![
            add_file,
            remove_file,
            list_files,
            get_server_info,
            list_clients,
        ])
        .setup(move |app| {
            // 啟動 Axum HTTPS Server（自行簽署憑證）
            let state = server_state.clone();
            let client_dir = resolve_client_dist_dir(app);
            tauri::async_runtime::spawn(async move {
                let router = server::create_router(state, client_dir);
                let addr = format!("0.0.0.0:{}", port);

                // 產生自行簽署 TLS 憑證
                let ip = local_ip_address::local_ip()
                    .map(|ip| ip.to_string())
                    .unwrap_or_else(|_| "127.0.0.1".to_string());
                let mut params = rcgen::CertificateParams::new(vec!["localhost".to_string()])
                    .expect("Failed to create cert params");
                if let Ok(ip_addr) = ip.parse::<std::net::IpAddr>() {
                    params
                        .subject_alt_names
                        .push(rcgen::SanType::IpAddress(ip_addr));
                }
                let key_pair = rcgen::KeyPair::generate().expect("Failed to generate key pair");
                let cert = params
                    .self_signed(&key_pair)
                    .expect("Failed to generate self-signed certificate");
                let cert_pem = cert.pem().into_bytes();
                let key_pem = key_pair.serialize_pem().into_bytes();

                let tls_config = axum_server::tls_rustls::RustlsConfig::from_pem(cert_pem, key_pem)
                    .await
                    .expect("Failed to load TLS certificate/key");

                // 強制 HTTP/1.1 only：修改 ALPN 避免瀏覽器協商 h2 導致 POST 405
                let mut rustls_config = (*tls_config.get_inner()).clone();
                rustls_config.alpn_protocols = vec![b"http/1.1".to_vec()];
                let tls_config =
                    axum_server::tls_rustls::RustlsConfig::from_config(Arc::new(rustls_config));

                println!("HTTPS Server 已啟動：https://{}:{}", ip, port);
                let addr: std::net::SocketAddr = addr.parse().unwrap();
                if let Err(e) = axum_server::bind_rustls(addr, tls_config)
                    .serve(router.into_make_service())
                    .await
                {
                    eprintln!("HTTPS Server 錯誤: {:?}", e);
                }
            });
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
