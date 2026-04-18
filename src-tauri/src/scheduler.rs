use crate::state::{
    AppState, ChannelType, DownloadState, FileQueue, QueueItem, RelayAssignEvent, ScheduleEvent,
    SseEvent, TransferChannel,
};
use uuid::Uuid;

impl AppState {
    /// 下載端請求下載某檔案：加入排程，然後觸發排程
    pub async fn request_download(&self, file_id: &str, client_id: &str) -> Option<ScheduleEvent> {
        if self.get_file(file_id).await.is_none() {
            return None;
        }

        let mut queues = self.queues.write().await;
        let queue = queues
            .entry(file_id.to_string())
            .or_insert_with(FileQueue::default);

        // 已在佇列中，直接回傳目前狀態
        if let Some(item) = queue.items.iter().find(|item| item.client_id == client_id) {
            return Some(ScheduleEvent {
                file_id: file_id.to_string(),
                client_id: client_id.to_string(),
                state: item.state.clone(),
                queue_position: None,
            });
        }

        // 加入為排隊中
        queue.items.push(QueueItem {
            client_id: client_id.to_string(),
            state: DownloadState::Queued,
        });

        let event = ScheduleEvent {
            file_id: file_id.to_string(),
            client_id: client_id.to_string(),
            state: DownloadState::Queued,
            queue_position: None,
        };

        self.broadcast(SseEvent {
            event_type: "schedule-update".to_string(),
            data: serde_json::to_value(&event).unwrap(),
        });

        drop(queues);

        // 觸發排程
        self.dispatch_all().await;

        Some(event)
    }

    /// 標記下載完成（HTTP 或 WebRTC 接收端），觸發排程
    pub async fn mark_download_complete(&self, file_id: &str, client_id: &str) {
        {
            let mut queues = self.queues.write().await;
            if let Some(queue) = queues.get_mut(file_id) {
                if let Some(item) = queue.items.iter_mut().find(|i| i.client_id == client_id) {
                    item.state = DownloadState::Completed;
                }
            }
        }

        // 註冊為可中繼端
        {
            let mut clients = self.clients.write().await;
            if let Some(client) = clients.get_mut(client_id) {
                if !client.completed_files.contains(&file_id.to_string()) {
                    client.completed_files.push(file_id.to_string());
                }
            }
        }

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

        // 清理已完成的傳輸通道
        {
            let mut channels = self.channels.write().await;
            channels.retain(|_, ch| !(ch.file_id == file_id && ch.target == client_id));
        }

        self.dispatch_all().await;
    }

    /// 中繼端完成傳輸：釋放中繼端，標記目標端完成
    pub async fn mark_relay_complete(
        &self,
        file_id: &str,
        source_client_id: &str,
        target_client_id: &str,
    ) {
        {
            let mut clients = self.clients.write().await;
            if let Some(relay) = clients.get_mut(source_client_id) {
                relay.is_relaying = false;
            }
        }
        // 清理通道
        {
            let mut channels = self.channels.write().await;
            channels.retain(|_, ch| {
                !(ch.file_id == file_id
                    && ch.source == source_client_id
                    && ch.target == target_client_id)
            });
        }
        self.mark_download_complete(file_id, target_client_id).await;
    }

    /// 傳輸失敗：回退為排隊中，觸發排程
    pub async fn mark_transfer_failed(&self, file_id: &str, client_id: &str) {
        // 如果失敗的是中繼目標端，也釋放中繼來源端
        {
            let mut release_relay: Option<String> = None;
            {
                let channels = self.channels.read().await;
                for ch in channels.values() {
                    if ch.file_id == file_id
                        && ch.target == client_id
                        && ch.channel_type == ChannelType::Webrtc
                    {
                        release_relay = Some(ch.source.clone());
                        break;
                    }
                }
            }
            if let Some(relay_id) = release_relay {
                let mut clients = self.clients.write().await;
                if let Some(relay) = clients.get_mut(&relay_id) {
                    relay.is_relaying = false;
                }
            }
        }

        // 清理相關通道
        {
            let mut channels = self.channels.write().await;
            channels.retain(|_, ch| !(ch.file_id == file_id && ch.target == client_id));
        }

        // 回退為排隊中
        {
            let mut queues = self.queues.write().await;
            if let Some(queue) = queues.get_mut(file_id) {
                if let Some(item) = queue.items.iter_mut().find(|i| i.client_id == client_id) {
                    item.state = DownloadState::Queued;
                }
            }
        }

        self.dispatch_all().await;
    }

    /// 全域排程：遍歷所有檔案的佇列，分配 HTTP 及 WebRTC 傳輸
    pub async fn dispatch_all(&self) {
        let file_ids: Vec<String> = self.files.read().await.keys().cloned().collect();
        for file_id in file_ids {
            self.dispatch_file(&file_id).await;
        }
    }

    /// 單一檔案排程
    async fn dispatch_file(&self, file_id: &str) {
        let file = match self.get_file(file_id).await {
            Some(f) => f,
            None => return,
        };

        let mut queues = self.queues.write().await;
        let queue = match queues.get_mut(file_id) {
            Some(q) => q,
            None => return,
        };

        // 排隊中的端
        let waiting: Vec<String> = queue
            .items
            .iter()
            .filter(|i| i.state == DownloadState::Queued)
            .map(|i| i.client_id.clone())
            .collect();

        if waiting.is_empty() {
            return;
        }

        // 檢查是否有 HTTP 傳輸正在進行
        let has_http_active = {
            let channels = self.channels.read().await;
            channels
                .values()
                .any(|ch| ch.file_id == file_id && ch.channel_type == ChannelType::Http)
        };

        let mut assigned = Vec::new();

        // 規則 1：如果沒有 HTTP 下載中，分配第一個排隊者走 HTTP
        if !has_http_active {
            if let Some(target_id) = waiting.first() {
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
                self.channels
                    .write()
                    .await
                    .insert(channel.channel_id.clone(), channel);

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

        // 規則 2：找空閒中繼端，分配給剩餘排隊者走 WebRTC
        let remaining: Vec<String> = waiting
            .iter()
            .filter(|id| !assigned.contains(id))
            .cloned()
            .collect();

        if !remaining.is_empty() {
            // 找出可用中繼端：已完成此檔案、目前未中繼
            let clients = self.clients.read().await;
            let mut available_relays: Vec<String> = clients
                .values()
                .filter(|c| c.completed_files.contains(&file_id.to_string()) && !c.is_relaying)
                .map(|c| c.id.clone())
                .collect();
            drop(clients);

            for target_id in remaining {
                if let Some(relay_id) = available_relays.pop() {
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
                    self.channels
                        .write()
                        .await
                        .insert(channel.channel_id.clone(), channel);

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

                    assigned.push(target_id);
                } else {
                    break;
                }
            }
        }

        // 更新剩餘排隊者的位置
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
}
