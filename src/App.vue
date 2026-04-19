<script setup lang="ts">
import { ref, onMounted } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { getVersion } from "@tauri-apps/api/app";

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
            <v-chip variant="outlined" class="mr-2">
              {{ serverInfo.url }}
            </v-chip>
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
  </v-app>
</template>
