/** File System Access API 檔案儲存管理，使用者選擇下載資料夾後直接寫入磁碟 */

interface FileStoreWriter {
  write(chunk: Uint8Array): Promise<void>;
  close(): Promise<void>;
}

class FileStoreManager {
  private dirHandle: FileSystemDirectoryHandle | null = null;
  /** fileId -> FileSystemFileHandle */
  private fileHandles: Map<string, FileSystemFileHandle> = new Map();
  /** fileId -> fileName mapping */
  private fileNames: Map<string, string> = new Map();

  /** fileId -> 已寫入位元組數 */
  private writtenSizes: Map<string, number> = new Map();

  /** 是否已選擇下載資料夾 */
  get ready(): boolean {
    return this.dirHandle !== null;
  }

  /** 讓使用者選擇下載資料夾 */
  async pickDirectory(): Promise<boolean> {
    try {
      this.dirHandle = await (window as any).showDirectoryPicker({
        mode: "readwrite",
      });
      console.log("已選擇下載資料夾:", this.dirHandle!.name);
      return true;
    } catch (err) {
      // 使用者取消選擇
      console.warn("未選擇下載資料夾:", err);
      return false;
    }
  }

  async createWriter(
    fileId: string,
    fileName: string,
  ): Promise<FileStoreWriter> {
    if (!this.dirHandle) throw new Error("尚未選擇下載資料夾");

    // 避免覆蓋同名檔案：若已存在則加上序號
    const actualName = await this.getUniqueFileName(fileName);
    this.fileNames.set(fileId, actualName);
    const fileHandle = await this.dirHandle.getFileHandle(actualName, {
      create: true,
    });
    this.fileHandles.set(fileId, fileHandle);
    const writable = await fileHandle.createWritable();
    let offset = 0;
    const writtenSizes = this.writtenSizes;
    writtenSizes.set(fileId, 0);

    return {
      async write(chunk: Uint8Array) {
        await writable.write({
          type: "write",
          position: offset,
          data: chunk,
        });
        offset += chunk.byteLength;
        writtenSizes.set(fileId, offset);
      },
      async close() {
        await writable.close();
      },
    };
  }

  async readAsBlob(fileId: string): Promise<Blob | null> {
    const handle = this.fileHandles.get(fileId);
    if (!handle) return null;
    try {
      return await handle.getFile();
    } catch {
      return null;
    }
  }

  async createReadStream(
    fileId: string,
  ): Promise<ReadableStream<Uint8Array> | null> {
    const blob = await this.readAsBlob(fileId);
    if (!blob) return null;
    return blob.stream() as ReadableStream<Uint8Array>;
  }

  async getFileSize(fileId: string): Promise<number> {
    return this.writtenSizes.get(fileId) ?? -1;
  }

  async deleteFile(fileId: string): Promise<void> {
    const fileName = this.fileNames.get(fileId);
    if (this.dirHandle && fileName) {
      try {
        await this.dirHandle.removeEntry(fileName);
      } catch {
        // 檔案不存在，忽略
      }
    }
    this.fileHandles.delete(fileId);
    this.fileNames.delete(fileId);
    this.writtenSizes.delete(fileId);
  }

  /** 產生不重複的檔名：若 foo.txt 已存在，則嘗試 foo (1).txt, foo (2).txt ... */
  private async getUniqueFileName(name: string): Promise<string> {
    if (!this.dirHandle) return name;

    // 也要檢查目前已分配但尚未寫完的檔名
    const usedNames = new Set(this.fileNames.values());

    if (!usedNames.has(name) && !(await this.fileExists(name))) {
      return name;
    }

    const dotIdx = name.lastIndexOf(".");
    const base = dotIdx > 0 ? name.slice(0, dotIdx) : name;
    const ext = dotIdx > 0 ? name.slice(dotIdx) : "";

    for (let i = 1; ; i++) {
      const candidate = `${base}(${i})${ext}`;
      if (!usedNames.has(candidate) && !(await this.fileExists(candidate))) {
        return candidate;
      }
    }
  }

  private async fileExists(name: string): Promise<boolean> {
    if (!this.dirHandle) return false;
    try {
      await this.dirHandle.getFileHandle(name);
      return true;
    } catch {
      return false;
    }
  }
}

export const fileStore = new FileStoreManager();
