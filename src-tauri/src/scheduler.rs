use crate::state::{
    AppState, ChannelType, DownloadState, FileQueue, QueueItem, RelayAssignEvent, ScheduleEvent,
    SseEvent, TransferChannel,
};
use uuid::Uuid;

/// 同時進行中的 WebRTC 傳輸通道上限（跨所有檔案）
/// 控制瀏覽器同時開啟的 PeerConnection 數量，避免大檔案並發傳輸造成記憶體壓力及 Chrome 崩潰
const MAX_CONCURRENT_WEBRTC: usize = 4;

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
            failed_relays: Vec::new(),
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
        // 廣播中繼來源端切回「下載完成」（server 端同步，確保 UI 一致）
        self.broadcast(SseEvent {
            event_type: "schedule-update".to_string(),
            data: serde_json::to_value(&ScheduleEvent {
                file_id: file_id.to_string(),
                client_id: source_client_id.to_string(),
                state: DownloadState::Completed,
                queue_position: None,
            })
            .unwrap(),
        });
        self.mark_download_complete(file_id, target_client_id).await;
    }

    /// 傳輸失敗：回退為排隊中，觸發排程
    pub async fn mark_transfer_failed(&self, file_id: &str, client_id: &str) {
        // 防護：只處理目標端目前為 Downloading 的情況
        // （避免 sender/receiver 同時回報導致雙重處理，或誤重置已重新分配的狀態）
        {
            let queues = self.queues.read().await;
            let is_downloading = queues
                .get(file_id)
                .and_then(|q| q.items.iter().find(|i| i.client_id == client_id))
                .map(|item| item.state == DownloadState::Downloading)
                .unwrap_or(false);
            if !is_downloading {
                return;
            }
        }

        // 如果失敗的是中繼目標端，找出中繼來源端
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

        // 釋放中繼來源端
        if let Some(relay_id) = &release_relay {
            {
                let mut clients = self.clients.write().await;
                if let Some(relay) = clients.get_mut(relay_id.as_str()) {
                    relay.is_relaying = false;
                }
            }
            // 通知中繼來源端切回「下載完成」狀態
            self.broadcast(SseEvent {
                event_type: "schedule-update".to_string(),
                data: serde_json::to_value(&ScheduleEvent {
                    file_id: file_id.to_string(),
                    client_id: relay_id.clone(),
                    state: DownloadState::Completed,
                    queue_position: None,
                })
                .unwrap(),
            });
        }

        // 清理相關傳輸通道
        {
            let mut channels = self.channels.write().await;
            channels.retain(|_, ch| !(ch.file_id == file_id && ch.target == client_id));
        }

        // 回退為排隊中，並記錄失敗的中繼來源（避免重複指派相同失敗配對）
        {
            let mut queues = self.queues.write().await;
            if let Some(queue) = queues.get_mut(file_id) {
                if let Some(item) = queue.items.iter_mut().find(|i| i.client_id == client_id) {
                    item.state = DownloadState::Queued;
                    if let Some(relay_id) = &release_relay {
                        if !item.failed_relays.contains(relay_id) {
                            item.failed_relays.push(relay_id.clone());
                        }
                    }
                }
            }
        }

        // 立即廣播目標端回到排隊中（讓客戶端 UI 即時更新，確保下次 HTTP 分配時狀態正確）
        self.broadcast(SseEvent {
            event_type: "schedule-update".to_string(),
            data: serde_json::to_value(&ScheduleEvent {
                file_id: file_id.to_string(),
                client_id: client_id.to_string(),
                state: DownloadState::Queued,
                queue_position: None,
            })
            .unwrap(),
        });

        // 延遲 2 秒後再 dispatch：避免失敗後立即重試造成連線風暴
        // 同時讓 ICE 寬限計時器（10s）有充分時間先嘗試恢復，再由 server 重排
        let state_clone = self.clone();
        tokio::spawn(async move {
            tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
            state_clone.dispatch_all().await;
        });
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
            // 全域 WebRTC 並行上限：計算目前已在使用的通道數（跨所有檔案）
            let webrtc_slots: usize = {
                let channels = self.channels.read().await;
                let active = channels
                    .values()
                    .filter(|ch| ch.channel_type == ChannelType::Webrtc)
                    .count();
                MAX_CONCURRENT_WEBRTC.saturating_sub(active)
            };

            if webrtc_slots == 0 {
                // 已達上限，本輪不分配新的 WebRTC 傳輸；等現有傳輸完成後 dispatch 會再觸發
                return;
            }

            // 找出可用中繼端：已完成此檔案、目前未中繼
            let clients = self.clients.read().await;
            let mut available_relays: Vec<String> = clients
                .values()
                .filter(|c| c.completed_files.contains(&file_id.to_string()) && !c.is_relaying)
                .map(|c| c.id.clone())
                .collect();
            drop(clients);

            let mut slots_used = 0usize;
            for target_id in remaining {
                if slots_used >= webrtc_slots {
                    break; // 本次 dispatch 已用完可用 slot
                }
                // 取得此目標端的失敗中繼清單，跳過曾失敗的配對
                let failed_relays: Vec<String> = queue
                    .items
                    .iter()
                    .find(|i| i.client_id == target_id)
                    .map(|i| i.failed_relays.clone())
                    .unwrap_or_default();

                // 找第一個不在失敗清單中的中繼端
                let relay_pos = available_relays
                    .iter()
                    .rposition(|r| !failed_relays.contains(r));

                if let Some(pos) = relay_pos {
                    let relay_id = available_relays.remove(pos);

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
                    self.channels
                        .write()
                        .await
                        .insert(channel.channel_id.clone(), channel);

                    // 傳送 relay-assign（含 channel_id，用於 WebRTC 信令比對）
                    // 注意：不另外廣播 schedule-update(downloading) 給接收端；
                    // 接收端透過 relay-assign 已知要開始 WebRTC 下載。
                    // 額外的 schedule-update 會使接收端在 WebRTC 失敗重試後誤啟動 HTTP 下載。
                    self.broadcast(SseEvent {
                        event_type: "relay-assign".to_string(),
                        data: serde_json::to_value(&RelayAssignEvent {
                            file_id: file_id.to_string(),
                            source_client_id: relay_id.clone(),
                            target_client_id: target_id.clone(),
                            file_size: file.size,
                            channel_id: channel_id.clone(),
                        })
                        .unwrap(),
                    });

                    assigned.push(target_id);
                    slots_used += 1;
                } else {
                    // 所有可用中繼端皆曾失敗於此目標端 → 清除失敗記錄讓下次重新嘗試
                    // （避免永久卡住；若網路問題已恢復，下一輪 dispatch 可再試）
                    if let Some(item) = queue.items.iter_mut().find(|i| i.client_id == target_id) {
                        item.failed_relays.clear();
                    }
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
