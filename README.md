# mesh-file-share

繁體中文 | [English](README.en.md)

mesh-file-share 是一套給區域網路環境使用的大檔案分發工具。

分享端使用 Tauri 桌面應用程式管理檔案與狀態，下載端只要開瀏覽器連線即可下載。

專案目標是讓教室或內網環境在大量裝置同時下載時，仍能保持高效率分發。

## 功能說明

- 分享端檔案管理
  - 支援多檔加入與移除分享清單
  - 顯示檔名、大小與下載端狀態
  - 顯示分享網址，支援一鍵複製
  - 支援顯示下載網址 QR Code，方便行動裝置或快速掃描連線

- 下載端瀏覽器介面
  - 顯示可下載檔案清單與即時狀態
  - 支援排程中、下載中、下載完成、分享中等狀態
  - 即時顯示下載進度、速度與排隊位置
  - 下載前可選擇目標資料夾，檔案直接寫入磁碟

- 分發與傳輸
  - 分享端提供 HTTPS 檔案傳輸
  - 透過 SSE 推送即時事件與狀態
  - 已下載完成的客戶端可作為中繼端，使用 WebRTC DataChannel 進行對等傳輸
  - 排程引擎支援多客戶端排隊與配對式分發，降低單一分享端壓力

## 專案特色

- 適合大檔案與多台裝置同時分發
- 下載完成節點可接力傳輸，提升整體擴散效率
- 分享端與下載端介面皆使用 Vue 與 Vuetify，操作一致
- 下載端不需安裝桌面程式，只要瀏覽器即可使用
- 採用 HTTPS，滿足現代瀏覽器功能需求

## 技術架構

- 分享端桌面應用
  - Tauri v2
  - Rust 後端
  - Vue 3 + Vuetify 前端

- 下載端前端
  - Vue 3 + Vuetify 單頁應用
  - 由分享端內建伺服器提供靜態頁面

- 網路通訊
  - REST API 提供檔案與排程操作
  - SSE 提供即時狀態推送
  - WebRTC DataChannel 提供中繼節點對等傳輸

## 主要流程

1. 分享端啟動後，建立 HTTPS 服務並顯示連線網址。
2. 管理者加入要分享的檔案。
3. 下載端用瀏覽器開啟網址後，看到可下載檔案清單。
4. 下載端送出下載請求，排程引擎安排傳輸順序。
5. 首批完成下載的節點加入中繼傳輸，協助後續節點下載。

## 開發與執行

需求環境

- Node.js 18+
- pnpm
- Rust toolchain
- Tauri 相依環境

安裝相依

    pnpm install

開發模式

    pnpm tauri dev

建置網頁端與桌面端

    pnpm tauri build

## 專案結構

- 分享端前端: [src](src)
- 下載端前端: [src-client](src-client)
- 共用型別: [src-shared](src-shared)
- Tauri Rust 後端: [src-tauri/src](src-tauri/src)
- OpenSpec 文件: [openspec](openspec)

## 推薦開發環境

- [VS Code](https://code.visualstudio.com/)
- [Vue - Official](https://marketplace.visualstudio.com/items?itemName=Vue.volar)
- [Tauri](https://marketplace.visualstudio.com/items?itemName=tauri-apps.tauri-vscode)
- [rust-analyzer](https://marketplace.visualstudio.com/items?itemName=rust-lang.rust-analyzer)
