use crate::state::{
    AppState, ChannelType, DownloadState, FileQueue, QueueItem, RelayAssignEvent, ScheduleEvent,
    SseEvent, TransferChannel,
};
use uuid::Uuid;

impl AppState {
    /// 下載端請求下載某檔案
    pub async fn request_download(&self, file_id: &str, client_id: &str) -> Option<ScheduleEvent> {
        // 確認檔案存在
        if self.get_file(file_id).await.is_none() {
            return None;
        }

        let mut queues = self.queues.write().await;
        let queue = queues
            .entry(file_id.to_string())
            .or_insert_with(FileQueue::default);

        // 檢查是否已在佇列中
        if queue.items.iter().any(|item| item.client_id == client_id) {
            let pos = queue
                .items
                .iter()
                .position(|item| item.client_id == client_id)
                .unwrap();
            let item = &queue.items[pos];
            return Some(ScheduleEvent {
                file_id: file_id.to_string(),
                client_id: client_id.to_string(),
                state: item.state.clone(),
                queue_position: if item.state == DownloadState::Queued {
                    // 排隊位置 = 在所有非 downloading/completed 之後
                    let downloading_count = queue
                        .items
                        .iter()
                        .filter(|i| i.state == DownloadState::Downloading)
                        .count();
                    Some(pos.saturating_sub(downloading_count))
                } else {
                    None
                },
            });
        }

        // 判斷是否有正在進行的傳輸
        let active_count = queue
            .items
            .iter()
            .filter(|item| item.state == DownloadState::Downloading)
            .count();

        let state = if active_count == 0 {
            DownloadState::Downloading
        } else {
            DownloadState::Queued
        };

        let queue_position = if state == DownloadState::Queued {
            Some(
                queue
                    .items
                    .iter()
                    .filter(|i| i.state == DownloadState::Queued)
                    .count(),
            )
        } else {
            None
        };

        queue.items.push(QueueItem {
            client_id: client_id.to_string(),
            state: state.clone(),
        });

        let event = ScheduleEvent {
            file_id: file_id.to_string(),
            client_id: client_id.to_string(),
            state,
            queue_position,
        };

        // 廣播排程更新
        self.broadcast(SseEvent {
            event_type: "schedule-update".to_string(),
            data: serde_json::to_value(&event).unwrap(),
        });

        Some(event)
    }

    /// 標記下載完成，觸發配對分發
    pub async fn mark_download_complete(&self, file_id: &str, client_id: &str) {
        // 更新佇列狀態
        {
            let mut queues = self.queues.write().await;
            if let Some(queue) = queues.get_mut(file_id) {
                if let Some(item) = queue.items.iter_mut().find(|i| i.client_id == client_id) {
                    item.state = DownloadState::Completed;
                }
            }
        }

        // 註冊為中繼端
        {
            let mut clients = self.clients.write().await;
            if let Some(client) = clients.get_mut(client_id) {
                if !client.completed_files.contains(&file_id.to_string()) {
                    client.completed_files.push(file_id.to_string());
                }
            }
        }

        // 廣播完成事件
        self.broadcast(SseEvent {
            event_type: "schedule-update".to_string(),
            data: serde_json::to_value(&ScheduleEvent {
                file_id: file_id.to_string(),
                client_id: client_id.to_string(),
                state: DownloadState::Completed,
                queue_position: None,
            })
            .unwrap(),
        });

        // 觸發配對分發
        self.dispatch_next(file_id).await;
    }

    /// 配對式分發：分配下一輪傳輸
    pub async fn dispatch_next(&self, file_id: &str) {
        let file = match self.get_file(file_id).await {
            Some(f) => f,
            None => return,
        };

        let mut queues = self.queues.write().await;
        let queue = match queues.get_mut(file_id) {
            Some(q) => q,
            None => return,
        };

        // 找出排隊中的端
        let waiting: Vec<String> = queue
            .items
            .iter()
            .filter(|i| i.state == DownloadState::Queued)
            .map(|i| i.client_id.clone())
            .collect();

        if waiting.is_empty() {
            return;
        }

        // 目前正在下載的數量
        let active_count = queue
            .items
            .iter()
            .filter(|i| i.state == DownloadState::Downloading)
            .count();

        // 找出可用中繼端（已完成下載且未在中繼中）
        let clients = self.clients.read().await;
        let mut available_relays: Vec<String> = clients
            .values()
            .filter(|c| c.completed_files.contains(&file_id.to_string()) && !c.is_relaying)
            .map(|c| c.id.clone())
            .collect();

        drop(clients);

        // 分享端 HTTP 通道（若尚未有 active HTTP 傳輸，可開一條）
        let mut assigned = Vec::new();

        // 分配分享端 HTTP 通道（最多 1 條）
        if active_count == 0 {
            if let Some(target_id) = waiting.first() {
                // 更新為 downloading
                if let Some(item) = queue.items.iter_mut().find(|i| i.client_id == *target_id) {
                    item.state = DownloadState::Downloading;
                }
                assigned.push(target_id.clone());

                let channel = TransferChannel {
                    channel_id: Uuid::new_v4().to_string(),
                    file_id: file_id.to_string(),
                    source: "server".to_string(),
                    target: target_id.clone(),
                    channel_type: ChannelType::Http,
                };

                let channel_id = channel.channel_id.clone();
                self.channels.write().await.insert(channel_id, channel);

                self.broadcast(SseEvent {
                    event_type: "schedule-update".to_string(),
                    data: serde_json::to_value(&ScheduleEvent {
                        file_id: file_id.to_string(),
                        client_id: target_id.clone(),
                        state: DownloadState::Downloading,
                        queue_position: None,
                    })
                    .unwrap(),
                });
            }
        }

        // 分配 WebRTC 中繼通道
        let remaining: Vec<String> = waiting
            .iter()
            .filter(|id| !assigned.contains(id))
            .cloned()
            .collect();
        for target_id in remaining {
            if let Some(relay_id) = available_relays.pop() {
                // 更新佇列狀態
                if let Some(item) = queue.items.iter_mut().find(|i| i.client_id == target_id) {
                    item.state = DownloadState::Downloading;
                }

                // 標記中繼端忙碌
                {
                    let mut clients = self.clients.write().await;
                    if let Some(relay) = clients.get_mut(&relay_id) {
                        relay.is_relaying = true;
                    }
                }

                let channel = TransferChannel {
                    channel_id: Uuid::new_v4().to_string(),
                    file_id: file_id.to_string(),
                    source: relay_id.clone(),
                    target: target_id.clone(),
                    channel_type: ChannelType::Webrtc,
                };

                let channel_id = channel.channel_id.clone();
                self.channels.write().await.insert(channel_id, channel);

                // 發送 relay-assign 事件
                self.broadcast(SseEvent {
                    event_type: "relay-assign".to_string(),
                    data: serde_json::to_value(&RelayAssignEvent {
                        file_id: file_id.to_string(),
                        source_client_id: relay_id.clone(),
                        target_client_id: target_id.clone(),
                        file_size: file.size,
                    })
                    .unwrap(),
                });

                self.broadcast(SseEvent {
                    event_type: "schedule-update".to_string(),
                    data: serde_json::to_value(&ScheduleEvent {
                        file_id: file_id.to_string(),
                        client_id: target_id.clone(),
                        state: DownloadState::Downloading,
                        queue_position: None,
                    })
                    .unwrap(),
                });

                assigned.push(target_id.clone());
            } else {
                break; // 沒有更多可用中繼端
            }
        }

        // 更新剩餘排隊端的位置
        let mut pos = 0;
        for item in queue
            .items
            .iter()
            .filter(|i| i.state == DownloadState::Queued)
        {
            pos += 1;
            self.broadcast(SseEvent {
                event_type: "schedule-update".to_string(),
                data: serde_json::to_value(&ScheduleEvent {
                    file_id: file_id.to_string(),
                    client_id: item.client_id.clone(),
                    state: DownloadState::Queued,
                    queue_position: Some(pos),
                })
                .unwrap(),
            });
        }
    }

    /// 中繼端完成傳輸
    pub async fn mark_relay_complete(
        &self,
        file_id: &str,
        source_client_id: &str,
        target_client_id: &str,
    ) {
        // 釋放中繼端
        {
            let mut clients = self.clients.write().await;
            if let Some(relay) = clients.get_mut(source_client_id) {
                relay.is_relaying = false;
            }
        }

        // 標記目標端完成
        self.mark_download_complete(file_id, target_client_id).await;
    }

    /// 傳輸失敗重新排程
    pub async fn mark_transfer_failed(&self, file_id: &str, client_id: &str) {
        {
            let mut queues = self.queues.write().await;
            if let Some(queue) = queues.get_mut(file_id) {
                if let Some(item) = queue.items.iter_mut().find(|i| i.client_id == client_id) {
                    item.state = DownloadState::Queued;
                }
            }
        }

        // 重新觸發分發
        self.dispatch_next(file_id).await;
    }
}
