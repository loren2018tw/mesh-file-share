## ADDED Requirements

### Requirement: 下載請求排隊機制

當多個下載端請求下載同一檔案時，下載排程引擎 SHALL 將請求加入佇列，同一時間僅允許一個傳輸通道對該檔案進行傳輸（從分享端傳出），其餘請求標記為「排程中」並記錄排隊順序。

#### Scenario: 第一個下載請求

- **WHEN** 下載端 A 請求下載檔案 X 且目前無其他端正在下載檔案 X
- **THEN** 排程引擎 SHALL 立即開始將檔案 X 傳送給 A，狀態標記為「下載中」

#### Scenario: 後續下載請求排隊

- **WHEN** 下載端 B 請求下載檔案 X 且 A 正在下載檔案 X
- **THEN** 排程引擎 SHALL 將 B 的請求加入佇列，狀態標記為「排程中（第 1 位）」，並透過 SSE 通知 B

#### Scenario: 多個請求同時排隊

- **WHEN** A 正在下載檔案 X，B 和 C 依序請求下載檔案 X
- **THEN** 排程引擎 SHALL 維護佇列順序 [B, C]，B 為第 1 位、C 為第 2 位

### Requirement: 配對式分發策略

當某個下載端完成檔案下載後，排程引擎 SHALL 啟動配對分發——分享端繼續服務佇列中的下一個請求，同時指派已完成的下載端透過 WebRTC DataChannel 直接傳輸給另一個請求，實現並行分發。

#### Scenario: 第一個下載端完成後的配對分發

- **WHEN** A 完成下載檔案 X，佇列中有 B 和 C 等待
- **THEN** 排程引擎 SHALL 分配「分享端 HTTPS → B」及「A 透過 WebRTC DataChannel → C」兩條傳輸通道同時進行

#### Scenario: 多個下載端完成後的倍增分發

- **WHEN** A 和 B 都已完成下載檔案 X，佇列中有 C、D、E 等待
- **THEN** 排程引擎 SHALL 分配最多 3 條傳輸通道（分享端 HTTPS→C、A WebRTC→D、B WebRTC→E）同時進行

#### Scenario: 無可用中繼端

- **WHEN** A 完成下載檔案 X，佇列中只有 B 等待
- **THEN** 排程引擎 SHALL 僅分配「分享端 → B」一條傳輸通道

### Requirement: 傳輸通道管理

排程引擎 SHALL 追蹤所有進行中的傳輸通道（分享端 HTTPS 直傳 + WebRTC DataChannel 中繼傳輸），記錄每條通道的來源端、目標端、傳輸進度、傳輸速率。

#### Scenario: 建立傳輸通道

- **WHEN** 排程引擎分配一條新的傳輸任務（無論是分享端 HTTPS 直傳或 WebRTC 中繼）
- **THEN** 系統 SHALL 建立傳輸通道記錄，包含：通道 ID、來源（分享端 / 中繼端 ID）、目標端 ID、檔案 ID、傳輸方式（HTTPS / WebRTC）、開始時間

#### Scenario: 傳輸通道完成

- **WHEN** 某傳輸通道的檔案傳輸完成
- **THEN** 系統 SHALL 將該通道標記為已完成，更新目標端狀態為「下載完成」，並將目標端加入可用中繼端清單

#### Scenario: 傳輸通道失敗

- **WHEN** 某傳輸通道的傳輸過程中連線中斷
- **THEN** 系統 SHALL 將該傳輸任務重新加入佇列，選擇其他可用來源端重新分配

### Requirement: 排程狀態通知

排程引擎 SHALL 透過 SSE 即時推送排程狀態變更給所有相關的下載端，事件類型為 `schedule-update`，內容包含：檔案 ID、下載端 ID、新狀態、排隊位置（若為排程中）。

#### Scenario: 狀態從排程中變為下載中

- **WHEN** 佇列中的下載端輪到下載
- **THEN** 排程引擎 SHALL 透過 SSE 發送 `schedule-update` 事件，通知該下載端狀態變更為「下載中」，同時更新其他排隊端的排隊位置

#### Scenario: 新增排隊通知

- **WHEN** 新的下載端加入某檔案的下載佇列
- **THEN** 排程引擎 SHALL 透過 SSE 通知該下載端目前的排隊位置

### Requirement: 不同檔案的獨立排程

排程引擎 SHALL 為每個檔案維護獨立的下載佇列，不同檔案的下載 SHALL 可同時進行，互不影響。

#### Scenario: 不同檔案同時下載

- **WHEN** A 請求下載檔案 X，B 請求下載檔案 Y
- **THEN** 排程引擎 SHALL 同時為 A 傳輸檔案 X、為 B 傳輸檔案 Y，兩者獨立排程

#### Scenario: 同一下載端請求多個檔案

- **WHEN** A 同時請求下載檔案 X 和檔案 Y
- **THEN** 排程引擎 SHALL 分別在檔案 X 和檔案 Y 的佇列中處理 A 的請求
