/** WebRTC DataChannel 中繼傳輸模組 */

import type { RelayAssignEvent, SignalingMessage } from "../src-shared/types";
import { fileStore } from "./fsaa";

const CHUNK_SIZE = 256 * 1024; // 256KB
const BACKPRESSURE_THRESHOLD = 8 * 1024 * 1024; // 8MB
const CONNECTION_TIMEOUT = 30_000; // 30s
/** ICE disconnected 寬限期：此狀態為暫態，可自行恢復；超過此時間未恢復才視為失敗 */
const ICE_DISCONNECT_GRACE_MS = 10_000;

type RelayReceiveCallback = (fileId: string) => Promise<void>;
type SendCompleteCallback = (fileId: string) => void;
type ProgressCallback = (fileId: string, downloadedBytes: number) => void;
type FileNameResolver = (fileId: string) => string;

interface PeerConnection {
  pc: RTCPeerConnection;
  dc: RTCDataChannel | null;
  fileId: string;
  role: "sender" | "receiver";
  event: RelayAssignEvent;
  pendingCandidates: RTCIceCandidateInit[];
  /** DataChannel 已開啟（傳輸已開始）；失敗時由 dc 回調處理，ICE handler 不重複通知 */
  transferActive: boolean;
  /** ICE disconnected 寬限計時器 ID */
  disconnectTimer: ReturnType<typeof setTimeout> | undefined;
}

class WebRTCManager {
  private clientId: string = "";
  private connections: Map<string, PeerConnection> = new Map();
  private onReceiveComplete: RelayReceiveCallback = async () => {};
  private onSendComplete: SendCompleteCallback = () => {};
  private onProgress: ProgressCallback = () => {};
  private resolveFileName: FileNameResolver = (id) => id;
  /** 信令處理佇列：確保同一連線的信令按序處理 */
  private signalingQueue: Promise<void> = Promise.resolve();
  /** 暫存尚未建立連線前到達的信令訊息 */
  private pendingSignaling: Map<string, SignalingMessage[]> = new Map();

  init(
    clientId: string,
    onReceiveComplete: RelayReceiveCallback,
    onSendComplete: SendCompleteCallback,
    onProgress: ProgressCallback,
    resolveFileName: FileNameResolver,
  ) {
    this.clientId = clientId;
    this.onReceiveComplete = onReceiveComplete;
    this.onSendComplete = onSendComplete;
    this.onProgress = onProgress;
    this.resolveFileName = resolveFileName;
  }

  /** 作為傳送端：建立連線並傳送檔案 */
  async startSending(event: RelayAssignEvent) {
    // peerKey 使用 channelId，每次 relay-assign 都是唯一 ID，完全避免舊連線信令汙染新連線
    const peerKey = event.channelId;

    // 清理已存在的同名連線（正常不應發生，防御用）
    if (this.connections.has(peerKey)) {
      console.warn("WebRTC 清理既有連線:", peerKey);
      this.cleanup(peerKey);
    }

    const pc = new RTCPeerConnection({
      iceServers: [],
    });

    const dc = pc.createDataChannel("file-transfer", {
      ordered: true,
    });

    this.connections.set(peerKey, {
      pc,
      dc,
      fileId: event.fileId,
      role: "sender",
      event,
      pendingCandidates: [],
      transferActive: false,
      disconnectTimer: undefined,
    });

    // 設定連線逾時（僅涵蓋 DataChannel 建立階段；開啟後由 ICE/dc 狀態管理）
    const timeout = setTimeout(() => {
      const conn = this.connections.get(peerKey);
      if (!conn || conn.transferActive) return; // 已開始傳輸，不由 timeout 處理
      console.error("WebRTC 連線逾時:", peerKey);
      this.cleanup(peerKey);
      void this.notifyTransferFailed(event.fileId, event.targetClientId);
      this.onSendComplete(event.fileId);
    }, CONNECTION_TIMEOUT);

    dc.onopen = async () => {
      clearTimeout(timeout);
      const conn = this.connections.get(peerKey);
      if (conn) conn.transferActive = true;
      try {
        console.log("WebRTC DataChannel 已開啟，開始傳送:", peerKey);
        await this.sendFile(dc, event.fileId, event.fileSize);
        await this.notifyRelayComplete(
          event.fileId,
          event.sourceClientId,
          event.targetClientId,
        );
      } catch (err) {
        console.error("WebRTC 傳送失敗:", err);
        await this.notifyTransferFailed(event.fileId, event.targetClientId);
      } finally {
        this.cleanup(peerKey);
        this.onSendComplete(event.fileId);
      }
    };

    dc.onerror = (ev) => {
      console.error("WebRTC DataChannel 錯誤 (sender):", peerKey, ev);
      clearTimeout(timeout);
      this.cleanup(peerKey);
      this.notifyTransferFailed(event.fileId, event.targetClientId);
      // 將 UI 狀態從「分享中」切回「完成」
      this.onSendComplete(event.fileId);
    };

    pc.onicecandidate = (e) => {
      if (e.candidate) {
        this.sendSignaling({
          type: "ice-candidate",
          fromClientId: this.clientId,
          toClientId: event.targetClientId,
          fileId: event.fileId,
          channelId: event.channelId,
          payload: e.candidate.toJSON(),
        });
      }
    };

    pc.oniceconnectionstatechange = () => {
      const state = pc.iceConnectionState;
      console.log("WebRTC ICE 狀態 (sender):", peerKey, state);
      const conn = this.connections.get(peerKey);
      if (!conn) return;

      if (state === "connected" || state === "completed") {
        // 恢復連線：取消斷線寬限計時器
        if (conn.disconnectTimer !== undefined) {
          clearTimeout(conn.disconnectTimer);
          conn.disconnectTimer = undefined;
        }
      } else if (state === "failed") {
        if (conn.disconnectTimer !== undefined)
          clearTimeout(conn.disconnectTimer);
        clearTimeout(timeout);
        const wasActive = conn.transferActive;
        this.cleanup(peerKey); // 關閉 dc → sendFile 拋例外 → dc.onopen catch 處理後續
        if (!wasActive) {
          // DataChannel 從未開啟，需手動通知（否則無人通知 server）
          console.error("WebRTC ICE 失敗（傳輸未啟動）(sender):", peerKey);
          void this.notifyTransferFailed(event.fileId, event.targetClientId);
          this.onSendComplete(event.fileId);
        } else {
          console.error("WebRTC ICE 失敗（傳輸中）(sender):", peerKey);
          // transferActive=true：dc close/error 會觸發 dc.onopen catch 處理
        }
      } else if (state === "disconnected") {
        // disconnected 為暫態，可自行恢復；給 ICE_DISCONNECT_GRACE_MS 緩衝
        if (conn.disconnectTimer !== undefined) return; // 已有計時器
        conn.disconnectTimer = setTimeout(() => {
          const current = this.connections.get(peerKey);
          if (!current) return; // 已被其他路徑清理
          const iceState = current.pc.iceConnectionState;
          if (iceState === "connected" || iceState === "completed") return; // 已恢復
          console.error("WebRTC ICE 斷線未恢復 (sender):", peerKey, iceState);
          clearTimeout(timeout);
          const wasActive = current.transferActive;
          this.cleanup(peerKey);
          if (!wasActive) {
            void this.notifyTransferFailed(event.fileId, event.targetClientId);
            this.onSendComplete(event.fileId);
          }
          // wasActive=true：cleanup 關閉 dc → backpressure 檢查 readyState 拋例外 → catch 處理
        }, ICE_DISCONNECT_GRACE_MS);
      }
    };

    // 建立 SDP offer
    const offer = await pc.createOffer();
    await pc.setLocalDescription(offer);

    console.log("WebRTC 傳送 offer:", peerKey);
    await this.sendSignaling({
      type: "offer",
      fromClientId: this.clientId,
      toClientId: event.targetClientId,
      fileId: event.fileId,
      channelId: event.channelId,
      payload: pc.localDescription!.toJSON(),
    });

    // 處理在連線建立前暫存的信令
    this.flushPendingSignaling(peerKey);
  }

  /** 作為接收端：等待連線並接收檔案 */
  async startReceiving(event: RelayAssignEvent) {
    // peerKey 使用 channelId，與傳送端一致
    const peerKey = event.channelId;

    // 清理已存在的同名連線
    if (this.connections.has(peerKey)) {
      console.warn("WebRTC 清理既有接收連線:", peerKey);
      this.cleanup(peerKey);
    }

    const pc = new RTCPeerConnection({
      iceServers: [],
    });

    this.connections.set(peerKey, {
      pc,
      dc: null,
      fileId: event.fileId,
      role: "receiver",
      event,
      pendingCandidates: [],
      transferActive: false,
      disconnectTimer: undefined,
    });

    const timeout = setTimeout(() => {
      const conn = this.connections.get(peerKey);
      if (!conn || conn.transferActive) return;
      console.error("WebRTC 接收端連線逾時:", peerKey);
      this.cleanup(peerKey);
      void this.notifyTransferFailed(event.fileId, event.targetClientId);
    }, CONNECTION_TIMEOUT);

    pc.ondatachannel = async (e) => {
      clearTimeout(timeout);
      const conn = this.connections.get(peerKey);
      if (conn) {
        conn.transferActive = true;
        conn.dc = e.channel;
      }
      const dc = e.channel;

      try {
        console.log("WebRTC DataChannel 已接收，開始接收檔案:", peerKey);
        await this.receiveFile(dc, event.fileId, event.fileSize);
        await this.onReceiveComplete(event.fileId);
      } catch (err) {
        console.error("WebRTC 接收失敗:", err);
        await fileStore.deleteFile(event.fileId);
        await this.notifyTransferFailed(event.fileId, event.targetClientId);
      } finally {
        this.cleanup(peerKey);
      }
    };

    pc.onicecandidate = (e) => {
      if (e.candidate) {
        this.sendSignaling({
          type: "ice-candidate",
          fromClientId: this.clientId,
          toClientId: event.sourceClientId,
          fileId: event.fileId,
          channelId: event.channelId,
          payload: e.candidate.toJSON(),
        });
      }
    };

    pc.oniceconnectionstatechange = () => {
      const state = pc.iceConnectionState;
      console.log("WebRTC ICE 狀態 (receiver):", peerKey, state);
      const conn = this.connections.get(peerKey);
      if (!conn) return;

      if (state === "connected" || state === "completed") {
        if (conn.disconnectTimer !== undefined) {
          clearTimeout(conn.disconnectTimer);
          conn.disconnectTimer = undefined;
        }
      } else if (state === "failed") {
        if (conn.disconnectTimer !== undefined)
          clearTimeout(conn.disconnectTimer);
        clearTimeout(timeout);
        const wasActive = conn.transferActive;
        this.cleanup(peerKey);
        if (!wasActive) {
          console.error("WebRTC ICE 失敗（傳輸未啟動）(receiver):", peerKey);
          void this.notifyTransferFailed(event.fileId, event.targetClientId);
        } else {
          console.error("WebRTC ICE 失敗（傳輸中）(receiver):", peerKey);
          // transferActive=true：cleanup 關閉 dc → receiveFile 的 dc.onclose 拋例外 → catch 處理
        }
      } else if (state === "disconnected") {
        if (conn.disconnectTimer !== undefined) return;
        conn.disconnectTimer = setTimeout(() => {
          const current = this.connections.get(peerKey);
          if (!current) return;
          const iceState = current.pc.iceConnectionState;
          if (iceState === "connected" || iceState === "completed") return;
          console.error("WebRTC ICE 斷線未恢復 (receiver):", peerKey, iceState);
          clearTimeout(timeout);
          const wasActive = current.transferActive;
          this.cleanup(peerKey);
          if (!wasActive) {
            void this.notifyTransferFailed(event.fileId, event.targetClientId);
          }
          // wasActive=true：cleanup 關閉 dc → dc.onclose → receiveFile reject → catch 處理
        }, ICE_DISCONNECT_GRACE_MS);
      }
    };

    // 處理在連線建立前暫存的信令
    this.flushPendingSignaling(peerKey);
  }

  /** 排隊處理信令訊息（確保按序、不併發） */
  queueHandleSignaling(msg: SignalingMessage) {
    this.signalingQueue = this.signalingQueue.then(() =>
      this.handleSignaling(msg).catch((err) =>
        console.error("WebRTC 信令處理錯誤:", err),
      ),
    );
  }

  /** 處理信令訊息 */
  private async handleSignaling(msg: SignalingMessage) {
    // 直接以 channelId 查找對應連線（該 ID 唯一且與本連線和舊連線完全障離）
    const peerKey = msg.channelId;
    const conn = this.connections.get(peerKey);

    if (!conn) {
      // 連線尚未建立，暫存信令
      const pending = this.pendingSignaling.get(peerKey) ?? [];
      pending.push(msg);
      this.pendingSignaling.set(peerKey, pending);
      console.log("WebRTC 暫存信令:", msg.type, peerKey);
      return;
    }

    if (msg.type === "offer") {
      await conn.pc.setRemoteDescription(
        msg.payload as RTCSessionDescriptionInit,
      );
      // 套用暫存的 ICE candidates
      for (const candidate of conn.pendingCandidates) {
        await conn.pc.addIceCandidate(candidate);
      }
      conn.pendingCandidates = [];

      const answer = await conn.pc.createAnswer();
      await conn.pc.setLocalDescription(answer);

      console.log("WebRTC 傳送 answer:", peerKey);
      await this.sendSignaling({
        type: "answer",
        fromClientId: this.clientId,
        toClientId: msg.fromClientId,
        fileId: msg.fileId,
        channelId: msg.channelId,
        payload: conn.pc.localDescription!.toJSON(),
      });
    } else if (msg.type === "answer") {
      // 確認 PC 狀態正確
      if (conn.pc.signalingState !== "have-local-offer") {
        console.warn(
          "WebRTC 忽略 answer（狀態不對）:",
          peerKey,
          conn.pc.signalingState,
        );
        return;
      }
      await conn.pc.setRemoteDescription(
        msg.payload as RTCSessionDescriptionInit,
      );
      // 套用暫存的 ICE candidates
      for (const candidate of conn.pendingCandidates) {
        await conn.pc.addIceCandidate(candidate);
      }
      conn.pendingCandidates = [];
    } else if (msg.type === "ice-candidate") {
      if (conn.pc.remoteDescription) {
        await conn.pc.addIceCandidate(msg.payload as RTCIceCandidateInit);
      } else {
        conn.pendingCandidates.push(msg.payload as RTCIceCandidateInit);
      }
    }
  }

  /** 處理在連線建立前暫存的信令訊息 */
  private flushPendingSignaling(peerKey: string) {
    // 現在以 channelId 為 key，可直接查找
    const pending = this.pendingSignaling.get(peerKey);
    if (pending?.length) {
      console.log("WebRTC 套用暫存信令:", peerKey, pending.length, "筆");
      this.pendingSignaling.delete(peerKey);
      for (const m of pending) {
        this.queueHandleSignaling(m);
      }
    }
  }

  /** 從 OPFS 讀取檔案並透過 DataChannel 分段傳送 */
  private async sendFile(
    dc: RTCDataChannel,
    fileId: string,
    _fileSize: number,
  ) {
    const stream = await fileStore.createReadStream(fileId);
    if (!stream) throw new Error("Cannot read file from storage");

    const reader = stream.getReader();

    // 讀取並分段傳送
    let buffer = new Uint8Array(0);

    while (true) {
      const { done, value } = await reader.read();
      if (done && buffer.byteLength === 0) break;

      if (value) {
        // 合併到 buffer
        const newBuf = new Uint8Array(buffer.byteLength + value.byteLength);
        newBuf.set(buffer);
        newBuf.set(value, buffer.byteLength);
        buffer = newBuf;
      }

      // 從 buffer 分段傳送 256KB 區塊
      while (
        buffer.byteLength >= CHUNK_SIZE ||
        (done && buffer.byteLength > 0)
      ) {
        const chunk = buffer.slice(0, Math.min(CHUNK_SIZE, buffer.byteLength));
        buffer = buffer.slice(chunk.byteLength);

        // 背壓控制：若 DataChannel 已關閉（ICE 失敗/斷線後被 cleanup 關閉）則立即中止傳輸
        while (dc.bufferedAmount > BACKPRESSURE_THRESHOLD) {
          if (dc.readyState !== "open")
            throw new Error("DataChannel 已關閉（背壓等待中）");
          await new Promise((r) => setTimeout(r, 10));
        }
        if (dc.readyState !== "open")
          throw new Error("DataChannel 已關閉（傳送前檢查）");

        dc.send(chunk);

        if (done && buffer.byteLength === 0) break;
      }

      if (done) break;
    }

    // 傳送結束標記（空訊息）
    dc.send(new ArrayBuffer(0));

    // 等待 buffer 清空，確保結束標記送達
    while (dc.bufferedAmount > 0) {
      if (dc.readyState !== "open")
        throw new Error("DataChannel 已關閉（傳送完成等待中）");
      await new Promise((r) => setTimeout(r, 10));
    }
    // 額外等待讓接收端處理結束標記
    await new Promise((r) => setTimeout(r, 500));
  }

  /** 透過 DataChannel 接收檔案並寫入磁碟 */
  private async receiveFile(
    dc: RTCDataChannel,
    fileId: string,
    _fileSize: number,
  ): Promise<void> {
    // 必須在 createWriter（async）之前設定 binaryType，
    // 否則 await createWriter 期間到達的訊息會以 Blob 而非 ArrayBuffer 傳入
    dc.binaryType = "arraybuffer";

    const writer = await fileStore.createWriter(
      fileId,
      this.resolveFileName(fileId),
    );

    let received = 0;
    let settled = false;
    /** 是否已收到傳送端的結束標記（end marker）
     * DataChannel 在傳送端送完後會「正常」close；此旗標區分正常關閉和異常關閉，
     * 防止 dc.onclose 把已正確 commit 的檔案刪除 */
    let endMarkerReceived = false;

    // 序列化寫入佇列：所有 write / close / abort 操作嚴格按到達順序執行
    let writeQueue: Promise<void> = Promise.resolve();

    return new Promise<void>((resolve, reject) => {
      /** 只呼叫一次 resolve 或 reject */
      const settle = (fn: () => void) => {
        if (settled) return;
        settled = true;
        fn();
      };

      dc.onmessage = (e) => {
        if (settled) return;
        const data = e.data as ArrayBuffer;

        if (data.byteLength === 0) {
          // 結束標記：標記已收到，並排入佇列尾端等所有 write 完成後才 close
          endMarkerReceived = true;
          writeQueue = writeQueue
            .then(async () => {
              await writer.close();
              settle(() => resolve());
            })
            .catch(async (err) => {
              // 前面某個 write 已失敗，中止並清理
              await writer.abort();
              await fileStore.deleteFile(fileId);
              settle(() => reject(err));
            });
        } else {
          // 資料區塊：排入佇列序列寫入（slice 複製 ArrayBuffer，避免 GC 回收原始資料）
          const chunk = new Uint8Array(data.slice(0));
          writeQueue = writeQueue.then(async () => {
            if (settled) return;
            await writer.write(chunk);
            received += chunk.byteLength;
            this.onProgress(fileId, received);
          });
        }
      };

      dc.onerror = () => {
        // 若已收到結束標記（表示傳輸成功，close/settle 正在進行中），忽略後續的 error 事件
        if (endMarkerReceived || settled) return;
        writeQueue = writeQueue.finally(async () => {
          await writer.abort();
          await fileStore.deleteFile(fileId);
          settle(() => reject(new Error("DataChannel error")));
        });
      };

      dc.onclose = () => {
        // 收到 end marker 後 DataChannel 會正常關閉，屬於預期行為，不應視為錯誤
        // 只有在「未收到 end marker 且尚未 settled」時才代表異常關閉
        if (endMarkerReceived || settled) return;
        writeQueue = writeQueue.finally(async () => {
          await writer.abort();
          await fileStore.deleteFile(fileId);
          settle(() => reject(new Error("DataChannel closed unexpectedly")));
        });
      };
    });
  }

  private async sendSignaling(msg: SignalingMessage) {
    try {
      const resp = await fetch(`/api/signaling/${msg.type}`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(msg),
      });
      if (!resp.ok) {
        console.error("WebRTC 信令 POST 失敗:", msg.type, resp.status);
      }
    } catch (err) {
      console.error("WebRTC 信令 POST 錯誤:", msg.type, err);
    }
  }

  private async notifyRelayComplete(
    fileId: string,
    sourceClientId: string,
    targetClientId: string,
  ) {
    await fetch(`/api/files/${fileId}/relay-complete`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ sourceClientId, targetClientId }),
    });
  }

  private async notifyTransferFailed(fileId: string, clientId: string) {
    await fetch(`/api/files/${fileId}/transfer-failed`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ clientId }),
    });
  }

  private cleanup(peerKey: string) {
    const conn = this.connections.get(peerKey);
    if (conn) {
      // 取消斷線寬限計時器，避免 cleanup 後計時器觸發殘留邏輯
      if (conn.disconnectTimer !== undefined) {
        clearTimeout(conn.disconnectTimer);
        conn.disconnectTimer = undefined;
      }
      conn.dc?.close();
      conn.pc.close();
      this.connections.delete(peerKey);
      // 清除此連線的暫存信令，防止舊信令汹入後續連線
      this.pendingSignaling.delete(peerKey);
    }
  }
}

export const webrtcManager = new WebRTCManager();
