import { useEffect, useReducer, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { MessageSquareWarning, X } from "lucide-react";
import { Button } from "@/components/ui/button";
import { onTauriEvent } from "@/lib/tauri";
import { initialReportState, reportReducer, type ReportPreview } from "@/lib/error-report-state";
import { notifyError } from "@/lib/notify";

/** 应用级单实例：避免页面重挂载重复提示或把新错误替换进正在确认的报告。 */
export function ErrorReportDialog({ telemetryEnabled }: { telemetryEnabled: boolean }) {
  const [state, dispatch] = useReducer(reportReducer, initialReportState);
  const sending = useRef(false);

  useEffect(() => {
    const unlisten = onTauriEvent<ReportPreview>("failure-diagnostic", (preview) => {
      dispatch({ type: "receive", preview });
    });
    // 重挂载只恢复手动入口，不自动再次提示。
    void invoke<ReportPreview | null>("latest_error_report").then((preview) => {
      if (preview) dispatch({ type: "receive", preview: { ...preview, should_prompt: false } });
    }).catch(() => undefined);
    return () => { void unlisten.then((stop) => stop()); };
  }, []);

  async function openLatest() {
    if (sending.current) {
      if (state.preview) dispatch({ type: "open", preview: state.preview });
      return;
    }
    if (!state.latest) return;
    try {
      const preview = await invoke<ReportPreview>("preview_error_report", { reportId: state.latest.report_id });
      dispatch({ type: "open", preview });
    } catch (error) { notifyError(String(error)); }
  }

  async function send() {
    if (!state.preview || sending.current || state.receipt) return;
    const preview = state.preview;
    sending.current = true;
    dispatch({ type: "send" });
    try {
      // 仅发送不可变快照的引用，正文完全由后端控制。
      const id = await invoke<string>("send_error_report", { reportId: preview.report_id, digest: preview.digest });
      dispatch({ type: "success", id });
    } catch (error) {
      dispatch({ type: "failure", message: String(error) });
    } finally { sending.current = false; }
  }

  return <>
    {state.latest && <Button variant="outline" size="sm" className="fixed bottom-4 right-5 z-30 shadow-sm" onClick={() => void openLatest()}>
      <MessageSquareWarning className="mr-2 h-4 w-4" />反馈最近错误
    </Button>}
    {state.open && <aside role="complementary" aria-labelledby="error-report-title" className="fixed bottom-4 right-5 z-40 max-h-[calc(100vh-32px)] w-[min(680px,calc(100vw-40px))] overflow-y-auto rounded-2xl border bg-background p-6 shadow-xl">
          <h2 id="error-report-title" className="pr-8 text-lg font-semibold">操作失败，是否发送错误报告？</h2>
          <p className="mt-3 text-sm leading-6 text-muted-foreground">
            报告包含软件版本、系统平台、失败阶段、固定错误类别及可用的工具退出码。不会上传 APK、证书文件、密码或完整日志。发送失败不影响本地操作。
          </p>
          <p className="mt-2 text-xs leading-5 text-muted-foreground">发送至项目 Cloudflare 服务，主库保存 30 天；网络服务提供方会接触连接 IP。不发送匿名安装标识。</p>
          {!telemetryEnabled && <p className="mt-2 text-sm">本次主动发送不会开启匿名统计。</p>}
          <details className="my-4 rounded-lg border p-3">
            <summary className="cursor-pointer text-sm font-medium">查看全部待发送内容</summary>
            <p className="mt-2 text-xs leading-5 text-muted-foreground">app_version：软件版本；flow / operation / stage：流程、操作和阶段；code：固定错误码；occurred_at：发生时间（UTC 秒）；null 表示未取得可靠信息。没有安全的额外错误细节时 evidence 为空。</p>
            <pre className="mt-3 max-h-60 overflow-auto whitespace-pre-wrap break-all text-xs">{state.preview ? JSON.stringify(JSON.parse(state.preview.payload_json), null, 2) : ""}</pre>
          </details>
          {state.error && <p role="alert" className="mb-3 text-sm text-destructive">{state.error}</p>}
          {state.receipt && <p role="status" className="mb-3 break-all text-sm">已发送。报告编号：<span className="select-all font-mono">{state.receipt}</span>。可在 QQ 群或 Issue 中引用此编号。</p>}
          {state.sending && <p className="mb-3 text-xs text-muted-foreground">正在发送，最长等待 10 秒；此时关闭窗口不会撤回已经发出的请求。</p>}
          <div className="flex justify-end gap-3">
            <Button variant="outline" onClick={() => dispatch({ type: "close" })}>{state.receipt ? "关闭" : "暂不发送"}</Button>
            {!state.receipt && <Button disabled={state.sending || !state.preview} onClick={() => void send()}>{state.sending ? "发送中…" : state.error ? "手动重试" : "发送报告"}</Button>}
          </div>
          <button type="button" className="absolute right-4 top-4 rounded-sm p-1 text-muted-foreground" aria-label="关闭错误报告" onClick={() => dispatch({ type: "close" })}><X className="h-4 w-4" /></button>
        </aside>}
  </>;
}
