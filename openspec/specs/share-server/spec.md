## ADDED Requirements

### Requirement: 分享端啟動 HTTPS Web Server

分享端 Tauri 應用程式啟動時，系統 SHALL 在本機啟動 HTTPS Web Server（使用執行時動態產生的自行簽署憑證），監聽可配置的連接埠（預設 8080），並在分享端 GUI 顯示存取網址（含本機 IP 與連接埠）。

#### Scenario: 應用程式啟動後 Web Server 就緒

- **WHEN** 分享端 Tauri 應用程式啟動完成
- **THEN** HTTPS Web Server SHALL 在指定連接埠上監聽，GUI 顯示「伺服器已啟動：https://<本機IP>:<連接埠>」

#### Scenario: 連接埠被佔用

- **WHEN** 指定連接埠已被其他程式佔用
- **THEN** 系統 SHALL 顯示錯誤訊息並允許使用者變更連接埠

### Requirement: 檔案選取與管理

分享端 SHALL 提供 Vuetify GUI 介面讓使用者選取要分享的檔案，支援多檔選取，並以清單形式顯示已選取的檔案（含檔名、大小、SHA-256 校驗碼）。

#### Scenario: 選取檔案加入分享清單

- **WHEN** 使用者透過檔案選取對話框選取一個或多個檔案
- **THEN** 系統 SHALL 將選取的檔案加入分享清單，計算 SHA-256 校驗碼，並在 GUI 顯示檔案資訊

#### Scenario: 移除分享檔案

- **WHEN** 使用者在分享清單中選擇移除某個檔案
- **THEN** 系統 SHALL 從分享清單移除該檔案，並透過 SSE 通知所有已連線的下載端

#### Scenario: 動態新增檔案

- **WHEN** Web Server 已啟動且有下載端已連線的狀態下，使用者新增檔案至分享清單
- **THEN** 系統 SHALL 立即透過 SSE 通知所有已連線的下載端更新檔案清單

### Requirement: 檔案清單 API

分享端 Web Server SHALL 提供 REST API 端點 `GET /api/files`，回傳目前分享的檔案清單（JSON 格式），每筆包含檔案 ID、檔名、大小（位元組）、SHA-256 校驗碼。

#### Scenario: 查詢檔案清單

- **WHEN** 下載端向 `GET /api/files` 發送請求
- **THEN** 系統 SHALL 回傳 200 狀態碼及 JSON 陣列，包含所有目前分享中的檔案資訊

#### Scenario: 無分享檔案

- **WHEN** 分享清單為空時下載端查詢 `GET /api/files`
- **THEN** 系統 SHALL 回傳 200 狀態碼及空 JSON 陣列 `[]`

### Requirement: 檔案下載 API

分享端 Web Server SHALL 提供 `GET /api/files/:id/download` 端點，以 HTTP chunked transfer 方式串流傳輸指定檔案。

#### Scenario: 下載檔案

- **WHEN** 下載端請求 `GET /api/files/:id/download` 且該檔案存在
- **THEN** 系統 SHALL 以 chunked transfer encoding 串流回傳檔案內容，Content-Type 為 `application/octet-stream`

#### Scenario: 檔案不存在

- **WHEN** 下載端請求下載不存在的檔案 ID
- **THEN** 系統 SHALL 回傳 404 狀態碼

### Requirement: SSE 即時事件推送

分享端 Web Server SHALL 提供 `GET /api/events` SSE 端點，推送以下事件類型：`file-added`、`file-removed`、`schedule-update`、`download-progress`、`relay-assign`、`signaling`。

#### Scenario: 下載端訂閱 SSE 事件

- **WHEN** 下載端連線 `GET /api/events`
- **THEN** 系統 SHALL 建立 SSE 連線，立即推送目前的完整狀態快照

#### Scenario: 檔案清單變更事件

- **WHEN** 分享端新增或移除分享檔案
- **THEN** 系統 SHALL 向所有 SSE 連線推送 `file-added` 或 `file-removed` 事件

#### Scenario: 中繼配對指派事件

- **WHEN** 排程引擎決定由已完成的下載端 A 透過 WebRTC 傳輸給下載端 C
- **THEN** 系統 SHALL 透過 SSE 向 A 推送 `relay-assign` 事件（包含目標端 ID 與檔案 ID），向 C 推送 `relay-assign` 事件（包含來源端 ID 與檔案 ID）

### Requirement: WebRTC 信令 API

分享端 Web Server SHALL 提供信令 API 端點，作為 WebRTC 連線建立時 SDP 與 ICE 候選的中轉通道。

#### Scenario: 轉發 SDP Offer

- **WHEN** 中繼端 A 向 `POST /api/signaling/offer` 發送 SDP offer（包含目標端 ID）
- **THEN** 系統 SHALL 透過 SSE `signaling` 事件將 SDP offer 轉發給目標下載端

#### Scenario: 轉發 SDP Answer

- **WHEN** 下載端 C 向 `POST /api/signaling/answer` 發送 SDP answer（包含來源端 ID）
- **THEN** 系統 SHALL 透過 SSE `signaling` 事件將 SDP answer 轉發給中繼端 A

#### Scenario: 轉發 ICE 候選

- **WHEN** 任一端向 `POST /api/signaling/ice-candidate` 發送 ICE 候選資訊
- **THEN** 系統 SHALL 透過 SSE `signaling` 事件將 ICE 候選轉發給配對的另一端

### Requirement: 分享端下載狀態面板

分享端 GUI SHALL 顯示所有下載端的連線狀態與下載進度面板，包含：已連線下載端清單、各檔案的下載排程狀態、傳輸進度。

#### Scenario: 顯示已連線下載端

- **WHEN** 有下載端連線至分享端
- **THEN** 分享端 GUI SHALL 顯示該下載端的 IP 位址與連線時間

#### Scenario: 顯示傳輸進度

- **WHEN** 檔案正在傳輸給某下載端
- **THEN** 分享端 GUI SHALL 即時顯示傳輸進度百分比與傳輸速率

### Requirement: 提供下載端靜態前端

分享端 Web Server SHALL 在根路徑 `/` 提供下載端 Vue+Vuetify SPA 的靜態檔案，使下載端只需在瀏覽器開啟分享端網址即可使用。

#### Scenario: 瀏覽器存取分享端網址

- **WHEN** 下載端在瀏覽器輸入分享端的 URL（如 `https://192.168.1.100:8080`）
- **THEN** 系統 SHALL 回傳下載端 SPA 的 HTML/JS/CSS 靜態檔案
