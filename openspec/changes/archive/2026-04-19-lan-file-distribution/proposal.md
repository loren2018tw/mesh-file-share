## Why

電腦教室中需要分享大型課程檔案（5GB 級別）給 30 台以上學生電腦，傳統 P2P 技術因碎片化傳輸反而降低效率。本專案採用配對式順序分發策略——分享端先將完整檔案傳送給第一位下載端，下載完成後透過 WebRTC DataChannel 讓下載端之間直接傳輸（不經過分享端），以倍增方式快速擴散檔案，分享端僅負責排程協調與信令交換。

## What Changes

- 建立分享端 Tauri 桌面應用程式，使用 Vuetify 作為前端 GUI，可選取並動態新增要分享的檔案
- 分享端內建 HTTPS Web Server（使用自行簽署憑證），提供 REST API 供下載端查詢檔案清單與下載檔案
- 下載端透過瀏覽器連線分享端，使用 Vuetify 前端顯示可下載檔案清單
- 實作下載排程引擎：當多個下載端同時請求同一檔案時，依序分配（先服務 A，B/C 排隊等待並顯示排程狀態）
- 下載完成的端透過 WebRTC DataChannel 直接傳輸給其他下載端（不經過分享端），分享端協調配對——例如 A 下載完成後，分享端傳給 B，同時指派 A 透過 WebRTC 直連傳給 C
- 下載端使用 File System Access API 讓使用者選擇下載資料夾，檔案直接寫入磁碟，不佔用記憶體
- 下載端檔案清單即時更新：分享端新增檔案時下載端可即時看到；下載完成的檔案顯示「下載完成，分享中」狀態

## Capabilities

### New Capabilities

- `share-server`: 分享端核心——Tauri 桌面應用含 Vuetify GUI、檔案選取管理、內建 Web Server 提供檔案清單 API 與檔案傳輸
- `browser-client`: 下載端瀏覽器前端——連線分享端、顯示檔案清單、下載檔案、顯示下載進度與排程狀態、即時更新
- `download-scheduler`: 下載排程引擎——管理多下載端的下載順序、配對分配策略、狀態追蹤與通知
- `peer-relay`: 對等中繼機制——已下載完成的端透過 WebRTC DataChannel 直連傳輸給其他下載端、分享端作為信令伺服器協調配對、File System Access API 檔案儲存

### Modified Capabilities

（無現有 capabilities 需要修改）

## Impact

- **前端**：Vue + Vuetify 用於分享端 Tauri GUI 及下載端瀏覽器頁面
- **後端**：Rust（Tauri 框架）實作 Web Server、檔案傳輸、排程引擎
- **API**：REST API 提供檔案清單查詢、檔案下載、排程狀態查詢；WebRTC 信令 API 協調下載端配對
- **網路**：分享端↔下載端使用 HTTPS（自行簽署憑證）；下載端↔下載端使用 WebRTC DataChannel 直連（LAN 環境不需 STUN/TURN）
- **依賴**：Tauri v2、Vue 3、Vuetify 3、Axum、axum-server（TLS）、rcgen（自行簽署憑證）、瀏覽器原生 WebRTC API、File System Access API
