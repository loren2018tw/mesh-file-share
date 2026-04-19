## 1. 專案基礎設定

- [x] 1.1 加入 Rust 依賴：axum、tokio、serde、serde_json、sha2、uuid、tower-http（CORS、靜態檔案）、axum-server（TLS）、rcgen（自行簽署憑證）
- [x] 1.2 設定 Vite 多入口建構：分享端 SPA（Tauri webview）與下載端 SPA（瀏覽器）
- [x] 1.3 加入前端依賴：vuetify、@mdi/font，設定 Vuetify plugin
- [x] 1.4 建立前端共用型別定義（FileInfo、DownloadStatus、ScheduleEvent、SignalingMessage 等）

## 2. 分享端 Web Server 核心

- [x] 2.1 實作 Axum HTTPS Server 啟動邏輯（自行簽署憑證），可配置連接埠，與 Tauri 應用程式整合
- [x] 2.2 實作 `GET /api/files` 端點：回傳目前分享的檔案清單（JSON）
- [x] 2.3 實作 `GET /api/files/:id/download` 端點：以 chunked transfer 串流傳輸檔案
- [x] 2.4 實作 `GET /api/events` SSE 端點：建立 SSE 連線、推送狀態快照、支援 relay-assign 與 signaling 事件
- [x] 2.5 實作 SSE 事件推送邏輯：file-added、file-removed、schedule-update、download-progress、relay-assign、signaling 事件
- [x] 2.6 設定靜態檔案服務：根路徑 `/` 提供下載端 SPA 靜態檔案

## 3. 分享端 WebRTC 信令 API

- [x] 3.1 實作 `POST /api/signaling/offer` 端點：接收 SDP offer 並透過 SSE 轉發給目標下載端
- [x] 3.2 實作 `POST /api/signaling/answer` 端點：接收 SDP answer 並透過 SSE 轉發給來源端
- [x] 3.3 實作 `POST /api/signaling/ice-candidate` 端點：接收 ICE 候選並透過 SSE 轉發給配對端

## 4. 分享端 Tauri 前端（Vuetify GUI）

- [x] 4.1 實作檔案選取元件：多檔選取對話框，加入分享清單
- [x] 4.2 實作分享檔案清單元件：顯示檔名、大小、SHA-256 校驗碼，支援移除操作
- [x] 4.3 實作動態新增檔案功能：新增後觸發 SSE 事件通知下載端
- [x] 4.4 實作伺服器狀態顯示：顯示存取網址（IP + 連接埠）、伺服器啟動/停止控制
- [x] 4.5 實作下載狀態面板：已連線下載端清單、各檔案傳輸進度（含 HTTP 與 WebRTC 通道）、排程狀態總覽

## 5. 分享端檔案管理後端

- [x] 5.1 實作檔案管理模組：新增/移除分享檔案、計算 SHA-256 校驗碼、產生檔案 ID
- [x] 5.2 實作 Tauri Command 橋接：前端透過 invoke 呼叫 Rust 後端的檔案管理功能
- [x] 5.3 實作連線管理：追蹤已連線的下載端（透過 SSE 連線狀態），分配下載端 ID

## 6. 下載排程引擎

- [x] 6.1 實作下載佇列資料結構：每個檔案獨立佇列，記錄排隊順序與狀態
- [x] 6.2 實作下載請求 API `POST /api/files/:id/request`：下載端發送下載請求，排程引擎決定立即下載或排隊
- [x] 6.3 實作排程狀態通知：佇列變更時透過 SSE 推送 schedule-update 事件
- [x] 6.4 實作配對式分發策略：下載完成後啟動並行分發，分享端 HTTPS + 中繼端 WebRTC 同時服務
- [x] 6.5 實作傳輸通道管理：追蹤 HTTPS 直傳與 WebRTC 中繼通道、處理通道完成與失敗事件
- [x] 6.6 實作中繼端可用性追蹤：維護可用中繼端清單、容量管理（每端同時最多 1 條中繼）

## 7. 下載端瀏覽器前端（Vuetify SPA）

- [x] 7.1 實作檔案清單頁面：Vuetify v-data-table 顯示可下載檔案（檔名、大小、狀態）
- [x] 7.2 實作 SSE 連線管理：連線 /api/events、自動重連、連線狀態指示燈
- [x] 7.3 實作即時檔案清單更新：監聽 file-added / file-removed 事件自動更新表格
- [x] 7.4 實作下載按鈕與請求邏輯：點擊下載 → 呼叫 POST /api/files/:id/request
- [x] 7.5 實作 HTTPS 下載功能：從分享端 HTTPS 串流下載檔案，直接寫入磁碟
- [x] 7.6 實作下載進度顯示：進度條、已下載大小/總大小、傳輸速率
- [x] 7.7 實作排程狀態顯示：排程中（排隊位置）、下載中、下載完成（分享中）
- [x] 7.8 實作 File System Access API 資料夾選擇：下載前要求使用者選擇下載資料夾，檔案直接存入磁碟

## 8. File System Access API 檔案儲存模組

- [x] 8.1 實作 File System Access API 資料夾選擇與管理：showDirectoryPicker、FileHandle 管理
- [x] 8.2 實作串流寫入磁碟：HTTPS 下載與 WebRTC 接收的資料即時寫入使用者選擇的資料夾
- [x] 8.3 實作串流讀取磁碟：供中繼傳輸時讀取檔案資料
- [x] 8.4 實作 SHA-256 校驗：下載完成後驗證檔案完整性

## 9. WebRTC DataChannel 中繼模組

- [x] 9.1 實作 WebRTC 連線管理：RTCPeerConnection 建立、DataChannel 建立、ICE 候選處理（僅 host candidate）
- [x] 9.2 實作信令處理：監聽 SSE signaling 事件、透過 /api/signaling/\* API 發送 SDP/ICE
- [x] 9.3 實作中繼端傳送邏輯：收到 relay-assign 事件後建立 WebRTC 連線，從磁碟串流讀取並分段傳送（64KB 區塊）
- [x] 9.4 實作接收端接收邏輯：收到 relay-assign 事件後等待 WebRTC 連線，接收資料串流寫入磁碟
- [x] 9.5 實作 DataChannel 背壓控制：監控 bufferedAmount，超過閾值（1MB）時暫停傳送
- [x] 9.6 實作中繼端註冊：下載完成且校驗通過後透過 API 通知分享端可作為中繼端
- [x] 9.7 實作 WebRTC 失敗處理：連線逎時（10秒）、傳輸中斷時通知分享端重新排程

## 10. 整合測試與驗證

- [x] 10.1 測試分享端啟動 Web Server 並提供下載端 SPA 頁面
- [x] 10.2 測試單一下載端完整下載流程（連線 → 選擇下載資料夾 → 查看清單 → HTTPS 下載 → SHA-256 校驗 → 磁碟儲存）
- [x] 10.3 測試多下載端排程：A 下載中，B/C 排隊，驗證排程狀態顯示
- [x] 10.4 測試 WebRTC 配對分發：A 完成後，分享端 HTTPS→B 同時 A WebRTC→C 並行傳輸
- [x] 10.5 測試倍增分發：多個中繼端同時透過 WebRTC 傳輸給不同下載端
- [x] 10.6 測試動態檔案管理：分享端新增/移除檔案，下載端即時更新
- [x] 10.7 測試斷線處理：下載端斷線、WebRTC 連線中斷的重新排程行為
