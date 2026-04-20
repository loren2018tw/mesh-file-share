# mesh-file-share

[繁體中文](README.md) | English

mesh-file-share is a large-file distribution tool for local area network environments.

The sharing side uses a Tauri desktop application to manage files and state, while the download side only needs a browser to connect and download.

The project goal is to maintain high distribution efficiency even when a large number of devices download simultaneously in a classroom or intranet environment.

## Features

- Share-side file management
  - Add and remove multiple files from the share list
  - Display file names, sizes, and downloader status
  - Display the share URL with one-click copy support
  - Display the download URL as a QR code for easy scanning from mobile devices

- Download-side browser interface
  - Show a list of downloadable files with real-time status
  - Support statuses: scheduled, downloading, completed, and relaying
  - Real-time display of download progress, speed, and queue position
  - Select a destination folder before downloading; files are written directly to disk

- Distribution and transmission
  - Share side provides HTTPS file transfer
  - Real-time events and status pushed via SSE
  - Clients that have finished downloading can act as relay nodes using WebRTC DataChannel for peer-to-peer transfer
  - Scheduling engine supports multi-client queuing and paired distribution to reduce load on the single share side

## Highlights

- Designed for large files and simultaneous distribution to many devices
- Completed-download nodes can relay transfers to improve overall propagation efficiency
- Both share-side and download-side UIs use Vue and Vuetify for a consistent experience
- No desktop app required on the download side — a browser is all that's needed
- Uses HTTPS to satisfy modern browser feature requirements

## Technical Architecture

- Share-side desktop application
  - Tauri v2
  - Rust backend
  - Vue 3 + Vuetify frontend

- Download-side frontend
  - Vue 3 + Vuetify single-page application
  - Served as static pages by the share side's built-in server

- Network communication
  - REST API for file and scheduling operations
  - SSE for real-time status push
  - WebRTC DataChannel for relay-node peer-to-peer transfer

## Main Flow

1. The share side starts up, creates an HTTPS service, and displays the connection URL.
2. The administrator adds the files to share.
3. The download side opens the URL in a browser and sees the list of available files.
4. The download side submits a download request; the scheduling engine arranges the transfer order.
5. The first nodes to finish downloading join as relay nodes, helping subsequent nodes download.

## Development & Running

Requirements

- Node.js 18+
- pnpm
- Rust toolchain
- Tauri dependencies

Install dependencies

    pnpm install

Development mode

    pnpm tauri dev

Build web and desktop

    pnpm tauri build

## Project Structure

- Share-side frontend: [src](src)
- Download-side frontend: [src-client](src-client)
- Shared types: [src-shared](src-shared)
- Tauri Rust backend: [src-tauri/src](src-tauri/src)
- OpenSpec documentation: [openspec](openspec)

## Recommended Development Environment

- [VS Code](https://code.visualstudio.com/)
- [Vue - Official](https://marketplace.visualstudio.com/items?itemName=Vue.volar)
- [Tauri](https://marketplace.visualstudio.com/items?itemName=tauri-apps.tauri-vscode)
- [rust-analyzer](https://marketplace.visualstudio.com/items?itemName=rust-lang.rust-analyzer)
