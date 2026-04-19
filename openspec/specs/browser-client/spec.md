## ADDED Requirements

### Requirement: 連線分享端並顯示檔案清單

下載端瀏覽器前端 SHALL 在載入時自動向分享端 `GET /api/files` 請求檔案清單，並以 Vuetify 資料表格（v-data-table）顯示所有可下載的檔案，欄位包含：檔名、大小、狀態。

#### Scenario: 載入檔案清單

- **WHEN** 下載端頁面在瀏覽器中載入完成
- **THEN** 系統 SHALL 自動請求檔案清單並以表格呈現，每筆檔案顯示檔名、人類可讀的檔案大小、及「可下載」狀態

#### Scenario: 分享清單為空

- **WHEN** 分享端尚未加入任何分享檔案
- **THEN** 系統 SHALL 顯示「目前沒有可下載的檔案」提示訊息

### Requirement: 即時更新檔案清單

下載端 SHALL 透過 SSE 連線（`GET /api/events`）接收檔案清單變更事件，自動新增或移除清單中的檔案，無需使用者手動重新整理。

#### Scenario: 分享端新增檔案

- **WHEN** 下載端已連線且分享端新增一個檔案
- **THEN** 下載端檔案清單 SHALL 自動新增該檔案項目，無需重新整理頁面

#### Scenario: 分享端移除檔案

- **WHEN** 下載端已連線且分享端移除一個檔案
- **THEN** 下載端檔案清單 SHALL 自動移除該檔案項目

### Requirement: 請求下載檔案

下載端 SHALL 提供每個檔案的「下載」按鈕，點擊後向分享端發送下載請求，並根據排程狀態顯示對應的 UI 狀態。

#### Scenario: 點擊下載且立即開始傳輸

- **WHEN** 使用者點擊某檔案的「下載」按鈕，且該檔案目前無其他下載端排隊
- **THEN** 系統 SHALL 開始下載檔案，顯示進度條（百分比）與傳輸速率

#### Scenario: 點擊下載但需排隊

- **WHEN** 使用者點擊某檔案的「下載」按鈕，但該檔案目前有其他下載端正在下載
- **THEN** 系統 SHALL 顯示「排程中（排隊位置：第 N 位）」狀態，並持續透過 SSE 更新排隊狀態

### Requirement: 下載進度顯示

下載端 SHALL 在檔案下載過程中即時顯示下載進度，包含：進度百分比、已下載大小 / 總大小、傳輸速率。

#### Scenario: 下載進行中

- **WHEN** 檔案正在下載
- **THEN** 系統 SHALL 以 Vuetify 進度條顯示下載百分比，文字顯示「已下載 X MB / Y MB · Z MB/s」

#### Scenario: 下載完成

- **WHEN** 檔案下載完成且 SHA-256 校驗通過
- **THEN** 系統 SHALL 將檔案儲存至使用者選擇的下載資料夾，將該檔案狀態更新為「下載完成，分享中」，並顯示「已儲存至磁碟」狀態

### Requirement: 排程狀態顯示

下載端 SHALL 為每個檔案顯示目前的排程狀態，狀態包含：可下載、下載中、排程中（含排隊位置）、下載完成（分享中）。

#### Scenario: 顯示排程中狀態

- **WHEN** 使用者已請求下載某檔案但正在排隊等待
- **THEN** 系統 SHALL 顯示「排程中」標籤及排隊位置，例如「排程中（第 2 位）」

#### Scenario: 從排程中轉為下載中

- **WHEN** 排隊中的檔案輪到下載（收到 SSE `schedule-update` 事件）
- **THEN** 系統 SHALL 自動將狀態從「排程中」切換為「下載中」，開始顯示進度條

### Requirement: 連線狀態指示

下載端 SHALL 在頁面頂端顯示與分享端的連線狀態，包含 SSE 連線狀態指示燈（綠色已連線 / 紅色斷線）。

#### Scenario: SSE 連線中斷

- **WHEN** 與分享端的 SSE 連線意外中斷
- **THEN** 系統 SHALL 將連線狀態指示切換為紅色「已斷線」，並自動嘗試重新連線

#### Scenario: SSE 重新連線成功

- **WHEN** SSE 自動重新連線成功
- **THEN** 系統 SHALL 將連線狀態指示切換回綠色「已連線」，並重新取得最新檔案清單與排程狀態

### Requirement: File System Access API 檔案儲存

下載端 SHALL 使用 File System Access API 讓使用者選擇下載資料夾，下載完成的檔案直接寫入磁碟，不佔用瀏覽器記憶體。

#### Scenario: 選擇下載資料夾

- **WHEN** 下載端頁面載入完成
- **THEN** 系統 SHALL 顯示「請先選擇下載資料夾才可下載」提示，提供「選擇資料夾」按鈕，未選擇資料夾前停用所有下載按鈕

#### Scenario: 串流寫入磁碟

- **WHEN** 檔案正在下載（來自分享端 HTTPS 或來自中繼端 WebRTC DataChannel）
- **THEN** 系統 SHALL 將接收到的資料即時串流寫入使用者選擇的資料夾，不在記憶體中累積完整檔案

#### Scenario: 從磁碟讀取檔案供中繼傳輸

- **WHEN** 排程引擎指派本端作為中繼端傳輸檔案給其他下載端
- **THEN** 系統 SHALL 從磁碟串流讀取檔案資料，透過 WebRTC DataChannel 傳送

### Requirement: WebRTC DataChannel 中繼傳輸

下載端 SHALL 支援透過 WebRTC DataChannel 接收或傳送檔案，由分享端作為信令伺服器協調連線建立。

#### Scenario: 作為中繼端（傳送方）

- **WHEN** 收到 SSE `relay-assign` 事件指派本端為中繼端，需將檔案傳給目標下載端
- **THEN** 系統 SHALL 建立 RTCPeerConnection，透過分享端信令 API 交換 SDP/ICE，建立 DataChannel 後從磁碟串流讀取檔案並分段傳送

#### Scenario: 作為接收端（透過 WebRTC）

- **WHEN** 收到 SSE `relay-assign` 事件指派本端為接收端，將從中繼端接收檔案
- **THEN** 系統 SHALL 建立 RTCPeerConnection，完成信令交換後等待 DataChannel 開啟，接收資料並串流寫入磁碟

#### Scenario: WebRTC 連線建立失敗

- **WHEN** WebRTC DataChannel 連線建立逾時或失敗
- **THEN** 系統 SHALL 通知分享端連線失敗，排程引擎將重新分配傳輸任務

#### Scenario: 中繼傳輸進度顯示

- **WHEN** 本端正在透過 WebRTC DataChannel 傳送檔案給其他下載端
- **THEN** 系統 SHALL 在檔案清單中顯示「下載完成，分享中」狀態及傳輸進度

#### Scenario: DataChannel 背壓控制

- **WHEN** 傳送方的 DataChannel bufferedAmount 超過閾值（1MB）
- **THEN** 系統 SHALL 暫停讀取磁碟資料，等待 bufferedAmount 降低後繼續傳送
