/** 檔案資訊 */
export interface FileInfo {
  id: string;
  name: string;
  size: number;
}

/** 下載狀態 */
export type DownloadState =
  | "available" // 可下載
  | "queued" // 排程中
  | "downloading" // 下載中
  | "completed" // 下載完成
  | "relaying"; // 下載完成，分享中

export interface DownloadStatus {
  fileId: string;
  state: DownloadState;
  /** 排隊位置（state = queued 時有值） */
  queuePosition?: number;
  /** 已下載位元組數 */
  downloadedBytes?: number;
  /** 傳輸速率 bytes/s */
  speed?: number;
  /** 傳輸方式 */
  channel?: "http" | "webrtc";
}

/** SSE 排程更新事件 */
export interface ScheduleEvent {
  fileId: string;
  clientId: string;
  state: DownloadState;
  queuePosition?: number;
}

/** SSE 事件型別 */
export type SSEEventType =
  | "file-added"
  | "file-removed"
  | "schedule-update"
  | "download-progress"
  | "relay-assign"
  | "signaling"
  | "snapshot";

export interface SSEEvent {
  type: SSEEventType;
  data: unknown;
}

/** WebRTC 信令訊息 */
export interface SignalingMessage {
  type: "offer" | "answer" | "ice-candidate";
  fromClientId: string;
  toClientId: string;
  fileId: string;
  /** 對應 RelayAssignEvent.channelId，用於比對信令所屬的連線，過濾舊連線的殘留信令 */
  channelId: string;
  payload: RTCSessionDescriptionInit | RTCIceCandidateInit;
}

/** 中繼指派事件 */
export interface RelayAssignEvent {
  fileId: string;
  /** 傳送端 */
  sourceClientId: string;
  /** 接收端 */
  targetClientId: string;
  /** 檔案大小 */
  fileSize: number;
  /** 本次傳輸通道唯一 ID，用於 WebRTC 信令比對，防止舊信令汙染新連線 */
  channelId: string;
}

/** 下載進度事件 */
export interface DownloadProgressEvent {
  fileId: string;
  clientId: string;
  downloadedBytes: number;
  totalBytes: number;
  speed: number;
}

/** 狀態快照（SSE 連線時推送） */
export interface StateSnapshot {
  clientId: string;
  files: FileInfo[];
  schedules: ScheduleEvent[];
}
