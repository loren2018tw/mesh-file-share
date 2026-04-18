/** WebRTC DataChannel 中繼傳輸模組 */

import type { RelayAssignEvent, SignalingMessage } from "../src-shared/types";
import { fileStore } from "./fsaa";

const CHUNK_SIZE = 256 * 1024; // 256KB
const BACKPRESSURE_THRESHOLD = 8 * 1024 * 1024; // 8MB
const CONNECTION_TIMEOUT = 30_000; // 30s

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
    const peerKey = `${event.fileId}-${event.targetClientId}`;

    // 清理已存在的同 peerKey 連線（避免重複）
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
    });

    // 設定連線逾時
    const timeout = setTimeout(() => {
      console.error("WebRTC 連線逾時:", peerKey);
      this.cleanup(peerKey);
      this.notifyTransferFailed(event.fileId, event.targetClientId);
    }, CONNECTION_TIMEOUT);

    dc.onopen = async () => {
      clearTimeout(timeout);
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
    };

    pc.onicecandidate = (e) => {
      if (e.candidate) {
        this.sendSignaling({
          type: "ice-candidate",
          fromClientId: this.clientId,
          toClientId: event.targetClientId,
          fileId: event.fileId,
          payload: e.candidate.toJSON(),
        });
      }
    };

    pc.oniceconnectionstatechange = () => {
      console.log("WebRTC ICE 狀態 (sender):", peerKey, pc.iceConnectionState);
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
      payload: pc.localDescription!.toJSON(),
    });

    // 處理在連線建立前暫存的信令
    this.flushPendingSignaling(peerKey);
  }

  /** 作為接收端：等待連線並接收檔案 */
  async startReceiving(event: RelayAssignEvent) {
    const peerKey = `${event.fileId}-${event.sourceClientId}`;

    // 清理已存在的同 peerKey 連線
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
    });

    const timeout = setTimeout(() => {
      console.error("WebRTC 接收端連線逾時:", peerKey);
      this.cleanup(peerKey);
      this.notifyTransferFailed(event.fileId, event.targetClientId);
    }, CONNECTION_TIMEOUT);

    pc.ondatachannel = async (e) => {
      clearTimeout(timeout);
      const dc = e.channel;
      const conn = this.connections.get(peerKey);
      if (conn) conn.dc = dc;

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
          payload: e.candidate.toJSON(),
        });
      }
    };

    pc.oniceconnectionstatechange = () => {
      console.log(
        "WebRTC ICE 狀態 (receiver):",
        peerKey,
        pc.iceConnectionState,
      );
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
    // 找對應的連線
    let peerKey: string | undefined;
    for (const [key, conn] of this.connections) {
      if (
        conn.fileId === msg.fileId &&
        ((conn.role === "sender" &&
          conn.event.targetClientId === msg.fromClientId) ||
          (conn.role === "receiver" &&
            conn.event.sourceClientId === msg.fromClientId))
      ) {
        peerKey = key;
        break;
      }
    }

    if (!peerKey) {
      // 連線尚未建立，暫存信令
      const bufferKey = `${msg.fileId}-${msg.fromClientId}`;
      const pending = this.pendingSignaling.get(bufferKey) || [];
      pending.push(msg);
      this.pendingSignaling.set(bufferKey, pending);
      console.log("WebRTC 暫存信令:", msg.type, bufferKey);
      return;
    }

    const conn = this.connections.get(peerKey)!;

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
    // peerKey 可能匹配多種 bufferKey 格式
    const conn = this.connections.get(peerKey);
    if (!conn) return;

    for (const [bufferKey, msgs] of this.pendingSignaling) {
      const matches = msgs.filter(
        (m) =>
          m.fileId === conn.fileId &&
          ((conn.role === "sender" &&
            conn.event.targetClientId === m.fromClientId) ||
            (conn.role === "receiver" &&
              conn.event.sourceClientId === m.fromClientId)),
      );
      if (matches.length > 0) {
        console.log("WebRTC 套用暫存信令:", bufferKey, matches.length, "筆");
        this.pendingSignaling.delete(bufferKey);
        for (const m of matches) {
          this.queueHandleSignaling(m);
        }
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

        // 背壓控制
        while (dc.bufferedAmount > BACKPRESSURE_THRESHOLD) {
          await new Promise((r) => setTimeout(r, 10));
        }

        dc.send(chunk);

        if (done && buffer.byteLength === 0) break;
      }

      if (done) break;
    }

    // 傳送結束標記（空訊息）
    dc.send(new ArrayBuffer(0));

    // 等待 buffer 清空，確保結束標記送達
    while (dc.bufferedAmount > 0) {
      await new Promise((r) => setTimeout(r, 10));
    }
    // 額外等待讓接收端處理結束標記
    await new Promise((r) => setTimeout(r, 500));
  }

  /** 透過 DataChannel 接收檔案並寫入 OPFS */
  private async receiveFile(
    dc: RTCDataChannel,
    fileId: string,
    _fileSize: number,
  ): Promise<void> {
    const writer = await fileStore.createWriter(
      fileId,
      this.resolveFileName(fileId),
    );
    let received = 0;
    let completed = false;

    return new Promise((resolve, reject) => {
      dc.binaryType = "arraybuffer";

      dc.onmessage = async (e) => {
        const data = e.data as ArrayBuffer;
        if (data.byteLength === 0) {
          // 結束標記
          completed = true;
          await writer.close();
          resolve();
          return;
        }
        try {
          await writer.write(new Uint8Array(data));
          received += data.byteLength;
          this.onProgress(fileId, received);
        } catch (err) {
          reject(err);
        }
      };

      dc.onerror = () => {
        if (!completed) reject(new Error("DataChannel error"));
      };
      dc.onclose = () => {
        if (!completed) reject(new Error("DataChannel closed unexpectedly"));
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
      conn.dc?.close();
      conn.pc.close();
      this.connections.delete(peerKey);
    }
  }
}

export const webrtcManager = new WebRTCManager();
