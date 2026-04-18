## ADDED Requirements

### Requirement: 中繼端註冊

當下載端完成某檔案的下載後，系統 SHALL 自動將該下載端註冊為該檔案的可用中繼端，記錄其下載端 ID 與可提供的檔案 ID。

#### Scenario: 下載完成後自動註冊

- **WHEN** 下載端 A 完成檔案 X 的下載且 SHA-256 校驗通過
- **THEN** 系統 SHALL 將 A 註冊為檔案 X 的中繼端，加入排程引擎的可用中繼端清單

#### Scenario: 下載端斷線後取消註冊

- **WHEN** 已註冊為中繼端的下載端 A 斷線（SSE 連線中斷）
- **THEN** 系統 SHALL 將 A 從可用中繼端清單中移除，若 A 正在進行中繼傳輸則將該任務重新排程

### Requirement: WebRTC DataChannel 直連傳輸

已完成下載的中繼端 SHALL 透過 WebRTC DataChannel 直接將檔案傳送給目標下載端，不經過分享端。分享端僅負責信令交換（SDP/ICE 候選轉發）。

#### Scenario: 中繼端發起 WebRTC 連線

- **WHEN** 中繼端 A 收到 `relay-assign` 事件指派傳送檔案 X 給下載端 C
- **THEN** A SHALL 建立 RTCPeerConnection 與 DataChannel，產生 SDP offer 並透過分享端信令 API 轉發給 C

#### Scenario: 接收端完成 WebRTC 握手

- **WHEN** 下載端 C 收到中繼端 A 的 SDP offer
- **THEN** C SHALL 建立 RTCPeerConnection，產生 SDP answer 並透過分享端信令 API 回傳給 A，完成 WebRTC 連線建立

#### Scenario: 檔案資料透過 DataChannel 傳送

- **WHEN** WebRTC DataChannel 建立成功
- **THEN** 中繼端 A SHALL 從磁碟串流讀取檔案 X，以固定區塊大小（64KB）分段透過 DataChannel 傳送給 C

#### Scenario: LAN 環境不使用 STUN/TURN

- **WHEN** WebRTC 連線建立時進行 ICE 候選收集
- **THEN** 系統 SHALL 僅使用 host candidate（本機 IP），不配置 STUN/TURN Server

### Requirement: 中繼傳輸通知

下載端 SHALL 在檔案清單中顯示中繼狀態：當自身已下載完成的檔案正被用於中繼傳輸時，顯示「下載完成，分享中」狀態。

#### Scenario: 顯示分享中狀態

- **WHEN** 下載端 A 正在透過 WebRTC DataChannel 將已完成檔案傳送給其他下載端
- **THEN** A 的檔案清單中該檔案 SHALL 顯示「下載完成，分享中」狀態及目前傳輸的對象資訊與進度

#### Scenario: 中繼傳輸完成

- **WHEN** 中繼傳輸完成（目標端下載成功）
- **THEN** A 的檔案狀態 SHALL 更新為「下載完成」（若無其他中繼任務），WebRTC 連線 SHALL 關閉

### Requirement: 中繼端容量管理

排程引擎 SHALL 限制每個中繼端同時進行的 WebRTC 中繼傳輸數量為 1，避免中繼端頻寬過載。

#### Scenario: 中繼端忙碌

- **WHEN** 中繼端 A 正在進行一條 WebRTC 中繼傳輸，排程引擎需要分配新的中繼任務
- **THEN** 排程引擎 SHALL 跳過 A，選擇其他可用的中繼端或等待 A 完成

#### Scenario: 所有中繼端忙碌

- **WHEN** 所有可用中繼端都在進行中繼傳輸，仍有下載端在排隊
- **THEN** 排程引擎 SHALL 將多出的下載請求保持在佇列中等待，直到有中繼端空閒

### Requirement: 檔案完整性校驗

中繼傳輸完成後，目標下載端 SHALL 驗證接收到的檔案 SHA-256 校驗碼與分享端提供的校驗碼一致。

#### Scenario: 校驗成功

- **WHEN** 中繼傳輸完成且 SHA-256 校驗碼匹配
- **THEN** 系統 SHALL 將下載標記為成功，目標端可註冊為新的中繼端

#### Scenario: 校驗失敗

- **WHEN** 中繼傳輸完成但 SHA-256 校驗碼不匹配
- **THEN** 系統 SHALL 丟棄該檔案，將下載任務重新加入佇列，並標記該中繼端為不可靠

### Requirement: WebRTC 連線失敗處理

當 WebRTC DataChannel 連線建立失敗或傳輸中斷時，系統 SHALL 將受影響的傳輸任務重新排程。

#### Scenario: 連線建立逾時

- **WHEN** WebRTC 信令交換後 10 秒內 DataChannel 未成功開啟
- **THEN** 系統 SHALL 關閉 RTCPeerConnection，通知分享端連線失敗，排程引擎重新分配傳輸任務

#### Scenario: 傳輸中途中斷

- **WHEN** WebRTC DataChannel 在檔案傳輸過程中意外關閉
- **THEN** 系統 SHALL 丟棄不完整的資料，通知分享端傳輸失敗，排程引擎選擇其他來源端重新分配
