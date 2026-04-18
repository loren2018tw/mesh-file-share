## Context

本專案為電腦教室場景設計的區域網路檔案分享應用程式。教師端（分享端）需要將大型課程檔案分發給教室內多台學生電腦（下載端）。現有方案（隨身碟傳遞、雲端硬碟、P2P）在此場景下各有不足：隨身碟耗時、雲端受限於外網頻寬、P2P 碎片化傳輸在大檔案場景效率不佳。

目前專案已有 Tauri v2 + Vue 3 的基礎骨架（含 `src-tauri/` Rust 後端與 `src/` Vue 前端），需要在此基礎上建構完整的檔案分享功能。

## Goals / Non-Goals

**Goals:**

- 分享端為 Tauri 桌面應用程式，具備檔案選取與管理 GUI（Vuetify）
- 分享端內建 HTTPS Web Server（自行簽署憑證），下載端僅需瀏覽器即可連線
- 實作配對式順序分發策略，最大化區域網路頻寬利用率
- 下載完成的端自動成為中繼上傳源，以倍增方式加速分發
- 即時狀態同步：檔案清單變更、下載進度、排程狀態
- 下載端前端使用 Vuetify，與分享端 GUI 風格一致

**Non-Goals:**

- 不實作 P2P 碎片化傳輸協定（BitTorrent swarm 等）
- 不支援跨網段 / 外網分享
- 不實作使用者帳號與權限管理
- 不實作檔案加密傳輸（區域網路信任環境）
- 不支援斷點續傳（首版）

## Decisions

### 1. Web Server 框架：Axum

**選擇**: 使用 Axum 作為分享端內建 HTTP Server。

**理由**: Axum 基於 Tokio，與 Tauri v2 的非同步運行時相容。相較 actix-web，Axum 更輕量且社群活躍，API 設計更符合 Rust 慣例。

**替代方案**: actix-web（較成熟但較重）、warp（API 較難除錯）。

### 2. 即時通訊：Server-Sent Events (SSE)

**選擇**: 使用 SSE 推送即時狀態更新（檔案清單變更、下載進度、排程狀態）。

**理由**: 下載端為純瀏覽器，SSE 為單向推送且瀏覽器原生支援（EventSource API），不需額外函式庫。相較 WebSocket，SSE 更簡單且符合「伺服器推送狀態」的單向需求。

**替代方案**: WebSocket（雙向但此場景不需要）、輪詢（浪費頻寬且延遲高）。

### 3. 檔案傳輸方式：HTTP Chunked Transfer

**選擇**: 使用標準 HTTP chunked transfer encoding 進行檔案下載。

**理由**: 瀏覽器原生支援，不需額外協定。中繼端也透過 HTTP 提供檔案，架構統一。

### 4. 中繼端架構：WebRTC DataChannel 直連

**選擇**: 下載端之間透過 WebRTC DataChannel 建立直接連線，已完成下載的端可直接將檔案傳給其他下載端，不經過分享端。分享端僅作為信令伺服器（Signaling Server）協調 SDP/ICE 候選交換。

**理由**:

- 下載端為純瀏覽器環境，無法開啟 HTTP Server，但 WebRTC DataChannel 是瀏覽器原生支援的點對點傳輸技術
- LAN 環境不需要 STUN/TURN Server，ICE 候選直接使用 host candidate（本機 IP）
- 避免「分享端代理轉發」的頻寬瓶頸——5GB 檔案 30 端場景下，代理轉發會讓分享端承擔 ~150GB 流量，WebRTC 直連則僅需 ~25GB
- 與P2P 碎片化傳輸不同：WebRTC DataChannel 是一對一配對、完整檔案順序傳輸，由分享端統一協調配對

**信令流程**:

1. 排程引擎決定配對（例如 A → C）
2. 分享端透過 SSE 通知 A 與 C 開始 WebRTC 信令交換
3. A 建立 RTCPeerConnection，產生 SDP offer，透過分享端 API 轉發給 C
4. C 回傳 SDP answer，透過分享端轉發給 A
5. DataChannel 建立後，A 直接將檔案串流傳給 C（不經過分享端）

**替代方案**: 分享端代理轉發（分享端頻寬瓶頸）、下載端安裝 relay-agent 工具（增加部署複雜度）、Service Worker 中繼（瀏覽器限制多）。

### 5. 瀏覽器端大檔案儲存：File System Access API

**選擇**: 使用 File System Access API 讓使用者選擇下載資料夾，檔案直接寫入磁碟。

**理由**: File System Access API 支援串流式讀寫、檔案直接存在使用者指定的磁碟資料夾，不佔用瀏覽器記憶體。對於 5GB 級別的檔案，比 OPFS 更適合，因為下載完成後檔案已在磁碟上，不需額外「儲存檔案」操作。此外，HTTPS 安全上下文確保 API 可用。

**替代方案**: OPFS（需安全上下文且檔案需額外儲存操作）、IndexedDB Blob（5GB 可能超過部分瀏覽器限制）、Cache API（設計用於 HTTP 回應而非任意檔案）。

### 6. 排程策略：順序配對分發

**選擇**: 同一檔案的多個下載請求，採用順序服務 + 完成後倍增分發。

**流程**（5GB 檔案 × 30 端的分發）:

1. 輪次 1：分享端 HTTPS 傳給 A（單一通道）
2. 輪次 2：分享端 HTTPS → B，A 透過 WebRTC → C（2 條通道）
3. 輪次 3：分享端→D、A→E、B→F、C→G（4 條通道）
4. 輪次 4：8 條通道
5. 輪次 5：16 條通道 → 覆蓋剩餘所有端

→ 5 輪即可完成 30 端分發，分享端僅傳輸 5 次 = 25GB 總流量

**理由**: 最大化單一傳輸的頻寬利用，避免多路並行傳輸互搶頻寬。WebRTC 直連使中繼傳輸不經過分享端，真正實現倍增效果。

### 7. 前端架構：分享端 Vuetify SPA + 下載端 Vuetify SPA

**選擇**: 分享端 Tauri 內嵌 Vue+Vuetify SPA；下載端由分享端 Web Server 提供另一套 Vue+Vuetify SPA 靜態檔案。

**理由**: 統一技術棧，共用元件與樣式。Vite 建構兩套入口（分享端用 Tauri webview、下載端用瀏覽器）。

### 8. HTTPS 自行簽署憑證

**選擇**: 分享端使用 `rcgen` 在啟動時動態產生自行簽署 TLS 憑證，透過 `axum-server` 的 `tls-rustls` 提供 HTTPS 服務。

**理由**: HTTPS 安全上下文是 File System Access API 和 crypto.subtle（SHA-256 校驗）等現代瀏覽器 API 的必要條件。LAN 環境無法取得公開 CA 簽發的憑證，因此使用自行簽署憑證。憑證 SAN 包含 `localhost` 及本機 LAN IP 位址。

**替代方案**: mDNS + Let's Encrypt（LAN 環境不可行）、HTTP（無法使用 File System Access API）。

## Risks / Trade-offs

- **[自行簽署憑證瀏覽器警告]** → 瀏覽器會顯示「不安全」警告。緩解：使用者首次連線時手動接受憑證即可；電腦教室環境可透過 GPO 或教學引導處理。

- **[WebRTC DataChannel 訊息大小限制]** → DataChannel 單次訊息上限約 256KB，5GB 檔案需分段傳送（但仍為順序傳輸，非碎片化）。緩解：實作分段傳輸協定，每段 64KB-256KB，接收端依序寫入 OPFS。
- **[OPFS 瀏覽器相容性]** → 已改用 File System Access API，需要 Chromium 核心瀏覽器（Chrome 86+、Edge 86+）。緩解：電腦教室環境可控，統一使用 Chrome。
- **[WebRTC 連線建立延遲]** → SDP/ICE 交換需要數秒。緩解：LAN 環境 ICE 候選簡單（僅 host candidate），連線建立速度快；相對於 5GB 傳輸時間可忽略。
- **[SSE 連線數]** → 30 台同時連線 SSE。緩解：電腦教室規模在合理範圍內。
- **[檔案完整性]** → 傳輸過程可能損壞。緩解：使用 SHA-256 校驗碼驗證檔案完整性。
- **[DataChannel 背壓控制]** → 傳送方需監控 bufferedAmount 避免記憶體溢出。緩解：實作流量控制，當 bufferedAmount 超過閾值時暫停傳送。
