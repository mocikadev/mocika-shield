/** 仅协调本窗口的保存顺序与迟到刷新；跨进程一致性由后端文件锁和事务负责。 */
export class PreferenceSync {
  private revision = 0;
  private pending = 0;
  private tail: Promise<unknown> = Promise.resolve();
  get busy() { return this.pending > 0; }
  beginRead() { return this.busy ? undefined : ++this.revision; }
  canApply(token: number) { return !this.busy && token === this.revision; }
  invalidate() { ++this.revision; }
  async write<T>(operation: () => Promise<T>): Promise<T> {
    ++this.pending;
    ++this.revision;
    const task = this.tail.catch(() => undefined).then(operation);
    this.tail = task;
    try { return await task; }
    finally { --this.pending; ++this.revision; }
  }
}
