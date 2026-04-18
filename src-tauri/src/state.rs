use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};
use uuid::Uuid;

/// 分享檔案資訊
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileInfo {
    pub id: String,
    pub name: String,
    pub size: u64,
    pub path: PathBuf,
}

/// 下載狀態
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum DownloadState {
    Available,
    Queued,
    Downloading,
    Completed,
    Relaying,
}

/// 排程更新事件
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduleEvent {
    pub file_id: String,
    pub client_id: String,
    pub state: DownloadState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub queue_position: Option<usize>,
}

/// 傳輸通道類型
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ChannelType {
    Http,
    Webrtc,
}

/// 傳輸通道記錄
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferChannel {
    pub channel_id: String,
    pub file_id: String,
    pub source: String, // "server" or client_id
    pub target: String, // client_id
    pub channel_type: ChannelType,
}

/// SSE 事件
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SseEvent {
    #[serde(rename = "type")]
    pub event_type: String,
    pub data: serde_json::Value,
}

/// WebRTC 信令訊息
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SignalingMessage {
    #[serde(rename = "type")]
    pub msg_type: String, // "offer" | "answer" | "ice-candidate"
    pub from_client_id: String,
    pub to_client_id: String,
    pub file_id: String,
    pub payload: serde_json::Value,
}

/// 中繼指派事件
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RelayAssignEvent {
    pub file_id: String,
    pub source_client_id: String,
    pub target_client_id: String,
    pub file_size: u64,
}

/// 下載進度事件
#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadProgressEvent {
    pub file_id: String,
    pub client_id: String,
    pub downloaded_bytes: u64,
    pub total_bytes: u64,
    pub speed: u64,
}

/// 已連線的下載端
#[derive(Debug, Clone)]
pub struct ConnectedClient {
    pub id: String,
    /// 此端已完成下載的檔案 ID 集合（可作為中繼端）
    pub completed_files: Vec<String>,
    /// 是否正在進行中繼傳輸
    pub is_relaying: bool,
}

/// 下載佇列項目
#[derive(Debug, Clone)]
pub struct QueueItem {
    pub client_id: String,
    pub state: DownloadState,
}

/// 每個檔案的下載佇列
#[derive(Debug, Clone, Default)]
pub struct FileQueue {
    pub items: Vec<QueueItem>,
}

/// 應用程式共享狀態
#[derive(Clone)]
pub struct AppState {
    /// 分享檔案清單
    pub files: Arc<RwLock<HashMap<String, FileInfo>>>,
    /// 已連線下載端
    pub clients: Arc<RwLock<HashMap<String, ConnectedClient>>>,
    /// 下載佇列 (file_id -> queue)
    pub queues: Arc<RwLock<HashMap<String, FileQueue>>>,
    /// 傳輸通道
    pub channels: Arc<RwLock<HashMap<String, TransferChannel>>>,
    /// SSE 廣播器
    pub sse_tx: broadcast::Sender<SseEvent>,
    /// 伺服器連接埠
    pub port: u16,
}

impl AppState {
    pub fn new(port: u16) -> Self {
        let (sse_tx, _) = broadcast::channel(256);
        Self {
            files: Arc::new(RwLock::new(HashMap::new())),
            clients: Arc::new(RwLock::new(HashMap::new())),
            queues: Arc::new(RwLock::new(HashMap::new())),
            channels: Arc::new(RwLock::new(HashMap::new())),
            sse_tx,
            port,
        }
    }

    /// 新增分享檔案
    pub async fn add_file(&self, name: String, size: u64, path: PathBuf) -> FileInfo {
        let id = Uuid::new_v4().to_string();
        let file = FileInfo {
            id: id.clone(),
            name,
            size,
            path,
        };
        self.files.write().await.insert(id.clone(), file.clone());
        let _ = self.sse_tx.send(SseEvent {
            event_type: "file-added".to_string(),
            data: serde_json::to_value(&file).unwrap(),
        });
        file
    }

    /// 移除分享檔案
    pub async fn remove_file(&self, id: &str) -> bool {
        let removed = self.files.write().await.remove(id).is_some();
        if removed {
            // 清除佇列
            self.queues.write().await.remove(id);
            let _ = self.sse_tx.send(SseEvent {
                event_type: "file-removed".to_string(),
                data: serde_json::json!({ "id": id }),
            });
        }
        removed
    }

    /// 取得檔案清單
    pub async fn list_files(&self) -> Vec<FileInfo> {
        self.files.read().await.values().cloned().collect()
    }

    /// 取得指定檔案
    pub async fn get_file(&self, id: &str) -> Option<FileInfo> {
        self.files.read().await.get(id).cloned()
    }

    /// 註冊下載端
    pub async fn register_client(&self) -> String {
        let id = Uuid::new_v4().to_string();
        self.clients.write().await.insert(
            id.clone(),
            ConnectedClient {
                id: id.clone(),
                completed_files: Vec::new(),
                is_relaying: false,
            },
        );
        id
    }

    /// 移除下載端
    #[allow(dead_code)]
    pub async fn remove_client(&self, client_id: &str) {
        self.clients.write().await.remove(client_id);
        // 從所有佇列移除此端
        let mut queues = self.queues.write().await;
        for queue in queues.values_mut() {
            queue.items.retain(|item| item.client_id != client_id);
        }
    }

    /// 下載端斷線：清理佇列、釋放中繼、重新排程
    pub async fn disconnect_client(&self, client_id: &str) {
        // 從所有佇列移除此端
        {
            let mut queues = self.queues.write().await;
            for queue in queues.values_mut() {
                queue.items.retain(|item| item.client_id != client_id);
            }
        }

        // 清理此端作為中繼來源的傳輸通道，釋放中繼目標端回佇列
        let mut relay_targets: Vec<(String, String)> = Vec::new();
        {
            let channels = self.channels.read().await;
            for channel in channels.values() {
                if channel.source == client_id && channel.channel_type == ChannelType::Webrtc {
                    relay_targets.push((channel.file_id.clone(), channel.target.clone()));
                }
            }
        }
        // 將被中繼的目標端標記為失敗（回到排隊）
        for (file_id, target_id) in &relay_targets {
            let mut queues = self.queues.write().await;
            if let Some(queue) = queues.get_mut(file_id.as_str()) {
                if let Some(item) = queue.items.iter_mut().find(|i| i.client_id == *target_id) {
                    item.state = DownloadState::Queued;
                }
            }
        }

        // 清理相關傳輸通道
        {
            let mut channels = self.channels.write().await;
            channels.retain(|_, ch| ch.source != client_id && ch.target != client_id);
        }

        // 移除下載端，並釋放任何被此端中繼鎖定的 relay 狀態
        {
            let mut clients = self.clients.write().await;
            clients.remove(client_id);
        }

        // 重新排程
        self.dispatch_all().await;
    }

    /// 發送 SSE 事件
    pub fn broadcast(&self, event: SseEvent) {
        let _ = self.sse_tx.send(event);
    }
}
