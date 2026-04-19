<script setup lang="ts">
import { ref, onMounted } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { getVersion } from "@tauri-apps/api/app";
import QRCode from "qrcode";

interface FileInfo {
  id: string;
  name: string;
  size: number;
  path: string;
}

interface ServerInfo {
  ip: string;
  port: number;
  url: string;
}

interface ClientInfo {
  id: string;
  completedFiles: string[];
  isRelaying: boolean;
}

const files = ref<FileInfo[]>([]);
const serverInfo = ref<ServerInfo | null>(null);
const clients = ref<ClientInfo[]>([]);
const appVersion = ref("");
const isLoading = ref(false);
const qrDialog = ref(false);
const qrCodeDataUrl = ref("");
const qrLoading = ref(false);
const snackbar = ref(false);
const snackbarText = ref("");

const QR_CODE_SIZE = 1024;

const fileHeaders = [
  { title: "檔名", key: "name" },
  { title: "大小", key: "size" },
  { title: "操作", key: "actions", sortable: false },
];

function formatSize(bytes: number): string {
  if (bytes < 1024) return bytes + " B";
  if (bytes < 1024 * 1024) return (bytes / 1024).toFixed(1) + " KB";
  if (bytes < 1024 * 1024 * 1024)
    return (bytes / (1024 * 1024)).toFixed(1) + " MB";
  return (bytes / (1024 * 1024 * 1024)).toFixed(2) + " GB";
}

async function loadFiles() {
  files.value = await invoke<FileInfo[]>("list_files");
}

async function loadServerInfo() {
  serverInfo.value = await invoke<ServerInfo>("get_server_info");
}

async function loadClients() {
  clients.value = await invoke<ClientInfo[]>("list_clients");
}

async function addFiles() {
  // 使用 Tauri 原生檔案對話框
  const { open: openDialog } = await import("@tauri-apps/plugin-dialog");
  const selected = await openDialog({
    multiple: true,
    title: "選取要分享的檔案",
  });
  if (!selected) return;

  isLoading.value = true;
  const paths = Array.isArray(selected) ? selected : [selected];
  for (const filePath of paths) {
    const path =
      typeof filePath === "string"
        ? filePath
        : (filePath as { path: string }).path;
    const file = await invoke<FileInfo>("add_file", { path });
    files.value.push(file);
  }
  isLoading.value = false;
}

async function removeFile(id: string) {
  await invoke<boolean>("remove_file", { id });
  await loadFiles();
}

async function copyToClipboard(text: string, successMessage: string) {
  if (!text) return;

  try {
    if (navigator.clipboard && window.isSecureContext) {
      await navigator.clipboard.writeText(text);
    } else {
      const textarea = document.createElement("textarea");
      textarea.value = text;
      textarea.style.position = "fixed";
      textarea.style.left = "-9999px";
      document.body.appendChild(textarea);
      textarea.focus();
      textarea.select();
      document.execCommand("copy");
      document.body.removeChild(textarea);
    }

    snackbarText.value = successMessage;
    snackbar.value = true;
  } catch (err) {
    console.error("複製失敗:", err);
    snackbarText.value = "複製失敗，請手動複製網址";
    snackbar.value = true;
  }
}

async function copyShareUrl() {
  await copyToClipboard(serverInfo.value?.url ?? "", "已複製分享網址");
}

async function showShareQrCode() {
  const shareUrl = serverInfo.value?.url ?? "";
  if (!shareUrl) return;

  await copyToClipboard(shareUrl, "已複製分享網址，並開啟 QR Code");
  qrDialog.value = true;
  qrLoading.value = true;

  try {
    qrCodeDataUrl.value = await QRCode.toDataURL(shareUrl, {
      width: QR_CODE_SIZE,
      margin: 1,
      errorCorrectionLevel: "M",
    });
  } catch (err) {
    console.error("QR Code 產生失敗:", err);
    qrCodeDataUrl.value = "";
    snackbarText.value = "QR Code 產生失敗";
    snackbar.value = true;
  } finally {
    qrLoading.value = false;
  }
}

onMounted(async () => {
  appVersion.value = await getVersion();
  await loadServerInfo();
  await loadFiles();
  // 定時更新下載端清單
  setInterval(loadClients, 3000);
});
</script>

<template>
  <v-app>
    <v-app-bar color="primary" density="compact">
      <v-app-bar-title>
        <div>mesh-file-share 區域網路檔案分享</div>
        <div class="text-caption" style="opacity: 0.8">
          v{{ appVersion }} by Loren(loren.tw@gmail.com)
        </div>
      </v-app-bar-title>
    </v-app-bar>

    <v-main>
      <v-container>
        <!-- 伺服器狀態 -->
        <v-card class="mb-4">
          <v-card-title>
            <v-icon start>mdi-server</v-icon>
            伺服器狀態
          </v-card-title>
          <v-card-text v-if="serverInfo">
            <v-chip color="success" variant="flat" class="mr-2">
              <v-icon start>mdi-circle</v-icon>
              已啟動
            </v-chip>
            <v-btn
              variant="outlined"
              class="mr-2"
              size="small"
              @click="copyShareUrl"
            >
              <v-icon start size="small">mdi-content-copy</v-icon>
              {{ serverInfo.url }}
            </v-btn>
            <v-btn
              icon="mdi-qrcode"
              size="x-small"
              variant="text"
              class="mr-2"
              @click="showShareQrCode"
            />
            <v-chip variant="outlined">
              <v-icon start>mdi-account-multiple</v-icon>
              已連線：{{ clients.length }} 台
            </v-chip>
          </v-card-text>
        </v-card>

        <!-- 分享檔案清單 -->
        <v-card class="mb-4">
          <v-card-title>
            <v-icon start>mdi-file-multiple</v-icon>
            分享檔案
            <v-btn
              class="ml-4"
              color="primary"
              variant="flat"
              size="small"
              :loading="isLoading"
              @click="addFiles"
            >
              <v-icon start>mdi-plus</v-icon>
              新增檔案
            </v-btn>
          </v-card-title>
          <v-data-table
            :headers="fileHeaders"
            :items="files"
            item-value="id"
            no-data-text="尚未加入分享檔案"
          >
            <template #item.size="{ item }">
              {{ formatSize(item.size) }}
            </template>
            <template #item.actions="{ item }">
              <v-btn
                icon="mdi-delete"
                variant="text"
                color="error"
                size="small"
                @click="removeFile(item.id)"
              />
            </template>
          </v-data-table>
        </v-card>

        <!-- 下載狀態面板 -->
        <v-card>
          <v-card-title>
            <v-icon start>mdi-download-multiple</v-icon>
            下載端狀態
          </v-card-title>
          <v-card-text v-if="clients.length === 0">
            <v-alert type="info" variant="tonal">
              目前沒有已連線的下載端
            </v-alert>
          </v-card-text>
          <v-list v-else>
            <v-list-item v-for="client in clients" :key="client.id">
              <template #prepend>
                <v-icon :color="client.isRelaying ? 'warning' : 'success'">
                  {{
                    client.isRelaying ? "mdi-swap-horizontal" : "mdi-monitor"
                  }}
                </v-icon>
              </template>
              <v-list-item-title>
                {{ client.id.substring(0, 8) }}…
              </v-list-item-title>
              <v-list-item-subtitle>
                已完成：{{ client.completedFiles.length }} 個檔案
                <v-chip
                  v-if="client.isRelaying"
                  size="x-small"
                  color="warning"
                  class="ml-2"
                >
                  中繼傳輸中
                </v-chip>
              </v-list-item-subtitle>
            </v-list-item>
          </v-list>
        </v-card>
      </v-container>
    </v-main>

    <v-dialog v-model="qrDialog" persistent max-width="none" class="qr-dialog">
      <v-card class="qr-dialog-card">
        <v-card-title class="d-flex align-center">
          <span>下載網址 QR Code</span>
          <v-spacer />
          <v-btn
            icon="mdi-close"
            variant="text"
            size="small"
            @click="qrDialog = false"
          />
        </v-card-title>
        <v-card-text class="d-flex flex-column align-center qr-dialog-content">
          <v-progress-circular
            v-if="qrLoading"
            indeterminate
            size="48"
            color="primary"
            class="my-8"
          />
          <v-img
            v-else-if="qrCodeDataUrl"
            :src="qrCodeDataUrl"
            class="qr-code-image"
            aspect-ratio="1"
            contain
          />
          <v-alert v-else type="error" variant="tonal" class="my-4">
            無法產生 QR Code
          </v-alert>
          <div class="text-caption mt-4 qr-url-text">
            {{ serverInfo?.url }}
          </div>
        </v-card-text>
      </v-card>
    </v-dialog>

    <v-snackbar v-model="snackbar" timeout="2200" location="bottom right">
      {{ snackbarText }}
    </v-snackbar>
  </v-app>
</template>

<style scoped>
.qr-dialog :deep(.v-overlay__content) {
  width: min(96vw, 1080px);
  max-height: 96vh;
  margin: 0;
}

.qr-dialog-card {
  max-height: 96vh;
  overflow: hidden;
}

.qr-dialog-content {
  height: calc(96vh - 64px);
  overflow: hidden;
  padding-top: 8px;
}

.qr-code-image {
  width: min(88vw, calc(96vh - 180px));
  max-width: 100%;
  max-height: 100%;
}

.qr-url-text {
  width: 100%;
  text-align: center;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
</style>
