<script setup lang="ts">
import { ref, reactive, onMounted, onUnmounted } from "vue";
import type {
  FileInfo,
  DownloadStatus,
  ScheduleEvent,
  StateSnapshot,
  RelayAssignEvent,
  SignalingMessage,
} from "../src-shared/types";
import { fileStore } from "./fsaa";
import { webrtcManager } from "./webrtc";

// --- State ---
const files = ref<FileInfo[]>([]);
const downloadStatuses = reactive<Map<string, DownloadStatus>>(new Map());
const clientId = ref<string>("");
const sseConnected = ref(false);
const directoryReady = ref(false);
let eventSource: EventSource | null = null;
let sseReconnectTimer: ReturnType<typeof setTimeout> | null = null;

const fileHeaders = [
  { title: "檔名", key: "name" },
  { title: "大小", key: "size" },
  { title: "狀態", key: "status", sortable: false },
  { title: "操作", key: "actions", sortable: false },
];

// --- Helpers ---
function formatSize(bytes: number): string {
  if (bytes < 1024) return bytes + " B";
  if (bytes < 1024 * 1024) return (bytes / 1024).toFixed(1).padStart(6) + " KB";
  if (bytes < 1024 * 1024 * 1024)
    return (bytes / (1024 * 1024)).toFixed(1).padStart(7) + " MB";
  return (bytes / (1024 * 1024 * 1024)).toFixed(2).padStart(7) + " GB";
}

function formatSpeed(bytesPerSec: number): string {
  return formatSize(bytesPerSec) + "/s";
}

function getStatus(fileId: string): DownloadStatus | undefined {
  return downloadStatuses.get(fileId);
}

function getStatusText(fileId: string): string {
  const status = getStatus(fileId);
  if (!status) return "可下載";
  switch (status.state) {
    case "queued":
      return `排程中（第 ${status.queuePosition ?? "?"} 位）`;
    case "downloading":
      return "下載中";
    case "completed":
      return "下載完成";
    case "relaying":
      return "下載完成，分享中";
    default:
      return "可下載";
  }
}

function getStatusColor(fileId: string): string {
  const status = getStatus(fileId);
  if (!status) return "default";
  switch (status.state) {
    case "queued":
      return "warning";
    case "downloading":
      return "info";
    case "completed":
      return "success";
    case "relaying":
      return "purple";
    default:
      return "default";
  }
}

function getProgress(fileId: string): number {
  const status = getStatus(fileId);
  if (!status || !status.downloadedBytes) return 0;
  const file = files.value.find((f) => f.id === fileId);
  if (!file) return 0;
  return Math.round((status.downloadedBytes / file.size) * 100);
}

// --- SSE ---
function connectSSE() {
  if (eventSource) {
    eventSource.close();
  }

  eventSource = new EventSource("/api/events");

  eventSource.addEventListener("snapshot", (e) => {
    const data: StateSnapshot = JSON.parse(e.data);
    clientId.value = data.clientId;
    files.value = data.files;
    sseConnected.value = true;
    // 初始化 WebRTC 模組
    webrtcManager.init(
      clientId.value,
      handleRelayReceiveComplete,
      (fileId) => {
        // 分享傳送完成，切回「下載完成」
        const existing = downloadStatuses.get(fileId);
        if (existing && existing.state === "relaying") {
          downloadStatuses.set(fileId, { ...existing, state: "completed" });
        }
      },
      (fileId, downloadedBytes) => {
        const existing = downloadStatuses.get(fileId);
        if (existing) {
          downloadStatuses.set(fileId, { ...existing, downloadedBytes });
        }
      },
      (fileId) => {
        const file = files.value.find((f) => f.id === fileId);
        return file?.name ?? fileId;
      },
    );
  });

  eventSource.addEventListener("file-added", (e) => {
    const file: FileInfo = JSON.parse(e.data);
    const idx = files.value.findIndex((f) => f.id === file.id);
    if (idx === -1) files.value.push(file);
  });

  eventSource.addEventListener("file-removed", (e) => {
    const data = JSON.parse(e.data);
    files.value = files.value.filter((f) => f.id !== data.id);
    downloadStatuses.delete(data.id);
  });

  eventSource.addEventListener("schedule-update", (e) => {
    const event: ScheduleEvent = JSON.parse(e.data);
    if (event.clientId !== clientId.value) return;

    const existing = downloadStatuses.get(event.fileId);
    downloadStatuses.set(event.fileId, {
      fileId: event.fileId,
      state: event.state,
      queuePosition: event.queuePosition,
      downloadedBytes: existing?.downloadedBytes,
      speed: existing?.speed,
      channel: existing?.channel,
    });

    // 如果變為 downloading 且之前是 queued，自動開始 HTTP 下載
    if (
      event.state === "downloading" &&
      (!existing || existing.state === "queued")
    ) {
      startHttpDownload(event.fileId);
    }
  });

  eventSource.addEventListener("relay-assign", (e) => {
    const event: RelayAssignEvent = JSON.parse(e.data);
    if (event.sourceClientId === clientId.value) {
      // 我是傳送端
      webrtcManager.startSending(event);
      downloadStatuses.set(event.fileId, {
        ...downloadStatuses.get(event.fileId)!,
        state: "relaying",
      });
    } else if (event.targetClientId === clientId.value) {
      // 我是接收端
      webrtcManager.startReceiving(event);
      downloadStatuses.set(event.fileId, {
        fileId: event.fileId,
        state: "downloading",
        channel: "webrtc",
        downloadedBytes: 0,
      });
    }
  });

  eventSource.addEventListener("signaling", (e) => {
    const msg: SignalingMessage = JSON.parse(e.data);
    if (msg.toClientId === clientId.value) {
      webrtcManager.queueHandleSignaling(msg);
    }
  });

  eventSource.onerror = () => {
    sseConnected.value = false;
    eventSource?.close();
    eventSource = null;
    // 自動重連
    sseReconnectTimer = setTimeout(connectSSE, 3000);
  };
}

// --- HTTP Download ---
async function startHttpDownload(fileId: string) {
  const file = files.value.find((f) => f.id === fileId);
  if (!file) return;

  const status = downloadStatuses.get(fileId);
  if (status && status.state === "downloading" && status.channel === "http")
    return;

  downloadStatuses.set(fileId, {
    fileId,
    state: "downloading",
    downloadedBytes: 0,
    speed: 0,
    channel: "http",
  });

  try {
    const response = await fetch(`/api/files/${fileId}/download`);
    if (!response.ok || !response.body) throw new Error("Download failed");

    const reader = response.body.getReader();
    const writer = await fileStore.createWriter(fileId, file.name);

    let downloaded = 0;
    let lastTime = Date.now();
    let lastBytes = 0;

    while (true) {
      const { done, value } = await reader.read();
      if (done) break;

      await writer.write(value);
      downloaded += value.byteLength;

      // 計算速率
      const now = Date.now();
      const elapsed = (now - lastTime) / 1000;
      if (elapsed >= 0.5) {
        const speed = Math.round((downloaded - lastBytes) / elapsed);
        lastTime = now;
        lastBytes = downloaded;

        downloadStatuses.set(fileId, {
          fileId,
          state: "downloading",
          downloadedBytes: downloaded,
          speed,
          channel: "http",
        });
      }
    }

    await writer.close();

    // 校驗檔案大小
    const actualSize = await fileStore.getFileSize(fileId);
    if (actualSize !== file.size) {
      console.error(
        "檔案大小校驗失敗:",
        fileId,
        "expected",
        file.size,
        "got",
        actualSize,
      );
      await fileStore.deleteFile(fileId);
      downloadStatuses.set(fileId, { fileId, state: "available" });
      return;
    }

    downloadStatuses.set(fileId, {
      fileId,
      state: "completed",
      downloadedBytes: file.size,
    });

    // 通知伺服器完成
    await fetch(`/api/files/${fileId}/complete`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ clientId: clientId.value }),
    });

    // 註冊為中繼端
    await fetch("/api/relay/register", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ clientId: clientId.value, fileId }),
    });
  } catch (err) {
    console.error("下載失敗:", err);
    // 通知伺服器失敗
    await fetch(`/api/files/${fileId}/transfer-failed`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ clientId: clientId.value }),
    });
    downloadStatuses.set(fileId, { fileId, state: "available" });
  }
}

// --- Relay receive complete callback ---
async function handleRelayReceiveComplete(fileId: string) {
  const file = files.value.find((f) => f.id === fileId);
  if (!file) return;

  const actualSize = await fileStore.getFileSize(fileId);
  if (actualSize !== file.size) {
    console.error(
      "中繼接收檔案大小校驗失敗:",
      fileId,
      "expected",
      file.size,
      "got",
      actualSize,
    );
    await fileStore.deleteFile(fileId);
    downloadStatuses.set(fileId, { fileId, state: "available" });
    await fetch(`/api/files/${fileId}/transfer-failed`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ clientId: clientId.value }),
    });
    return;
  }

  downloadStatuses.set(fileId, {
    fileId,
    state: "completed",
    downloadedBytes: file.size,
  });

  // 通知伺服器中繼完成
  await fetch(`/api/files/${fileId}/complete`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ clientId: clientId.value }),
  });

  await fetch("/api/relay/register", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ clientId: clientId.value, fileId }),
  });
}

// --- Download Request ---
async function requestDownload(fileId: string) {
  const existing = downloadStatuses.get(fileId);
  if (existing && existing.state !== "available") return;

  const res = await fetch(`/api/files/${fileId}/request`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ clientId: clientId.value }),
  });

  if (!res.ok) return;

  const event: ScheduleEvent = await res.json();
  downloadStatuses.set(event.fileId, {
    fileId: event.fileId,
    state: event.state,
    queuePosition: event.queuePosition,
  });

  if (event.state === "downloading") {
    startHttpDownload(fileId);
  }
}

// --- Pick download directory ---
async function pickDirectory() {
  directoryReady.value = await fileStore.pickDirectory();
}

// --- Lifecycle ---
onMounted(() => {
  connectSSE();
});

onUnmounted(() => {
  eventSource?.close();
  if (sseReconnectTimer) clearTimeout(sseReconnectTimer);
});
</script>

<template>
  <v-app>
    <v-app-bar color="primary" density="compact">
      <v-app-bar-title
        >mesh-file-share 區域網路檔案分享 by
        Loren(loren.tw@gmail.com)</v-app-bar-title
      >
      <template #append>
        <v-chip
          :color="sseConnected ? 'success' : 'error'"
          variant="flat"
          size="small"
        >
          <v-icon start size="x-small">mdi-circle</v-icon>
          {{ sseConnected ? "已連線" : "已斷線" }}
        </v-chip>
      </template>
    </v-app-bar>

    <v-main>
      <v-container>
        <!-- 選擇下載資料夾 -->
        <v-alert
          v-if="!directoryReady"
          type="warning"
          variant="tonal"
          class="mb-4"
        >
          <div class="d-flex align-center">
            <span>請先選擇下載資料夾才可下載檔案</span>
            <v-btn
              color="warning"
              variant="flat"
              size="small"
              class="ml-4"
              @click="pickDirectory"
            >
              <v-icon start>mdi-folder-open</v-icon>
              選擇資料夾
            </v-btn>
          </div>
        </v-alert>

        <v-alert
          v-if="files.length === 0"
          type="info"
          variant="tonal"
          class="mb-4"
        >
          目前沒有可下載的檔案
        </v-alert>

        <v-data-table
          v-else
          :headers="fileHeaders"
          :items="files"
          item-value="id"
        >
          <template #item.size="{ item }">
            {{ formatSize(item.size) }}
          </template>

          <template #item.status="{ item }">
            <div>
              <v-chip
                :color="getStatusColor(item.id)"
                size="small"
                variant="flat"
              >
                {{ getStatusText(item.id) }}
              </v-chip>

              <!-- 下載進度 -->
              <div
                v-if="getStatus(item.id)?.state === 'downloading'"
                class="mt-2"
              >
                <v-progress-linear
                  :model-value="getProgress(item.id)"
                  color="info"
                  height="8"
                  rounded
                />
                <div
                  class="text-caption mt-1"
                  style="font-family: monospace; white-space: pre"
                >
                  {{ formatSize(getStatus(item.id)?.downloadedBytes ?? 0) }}
                  / {{ formatSize(item.size) }}
                  <span v-if="getStatus(item.id)?.speed">
                    · {{ formatSpeed(getStatus(item.id)!.speed!) }}
                  </span>
                  <v-chip
                    v-if="getStatus(item.id)?.channel"
                    size="x-small"
                    variant="outlined"
                    class="ml-2"
                  >
                    {{
                      getStatus(item.id)?.channel === "webrtc"
                        ? "WebRTC"
                        : "HTTP"
                    }}
                  </v-chip>
                </div>
              </div>
            </div>
          </template>

          <template #item.actions="{ item }">
            <v-btn
              v-if="
                !getStatus(item.id) || getStatus(item.id)?.state === 'available'
              "
              color="primary"
              variant="flat"
              size="small"
              :disabled="!directoryReady"
              @click="requestDownload(item.id)"
            >
              <v-icon start>mdi-download</v-icon>
              下載
            </v-btn>
            <v-chip
              v-else-if="
                getStatus(item.id)?.state === 'completed' ||
                getStatus(item.id)?.state === 'relaying'
              "
              color="success"
              variant="flat"
              size="small"
            >
              <v-icon start>mdi-check-circle</v-icon>
              已儲存至磁碟
            </v-chip>
          </template>
        </v-data-table>
      </v-container>
    </v-main>
  </v-app>
</template>
