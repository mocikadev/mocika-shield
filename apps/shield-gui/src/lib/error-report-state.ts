export type ReportPreview = {
  report_id: string;
  digest: string;
  payload_json: string;
  should_prompt: boolean;
  sent: boolean;
};

type ReportState = {
  latest: ReportPreview | null;
  preview: ReportPreview | null;
  open: boolean;
  sending: boolean;
  error: string | null;
  receipt: string | null;
};

type ReportAction =
  | { type: "receive" | "open"; preview: ReportPreview }
  | { type: "close" | "send" }
  | { type: "success"; id: string }
  | { type: "failure"; message: string };

export const initialReportState: ReportState = {
  latest: null, preview: null, open: false, sending: false, error: null, receipt: null,
};

/** 只管理确认交互，不持有原始错误，也不执行任何网络请求。 */
export function reportReducer(state: ReportState, action: ReportAction): ReportState {
  switch (action.type) {
    case "receive":
      if (state.open || state.sending || !action.preview.should_prompt) {
        return { ...state, latest: action.preview };
      }
      return { ...initialReportState, latest: action.preview, preview: action.preview, open: true };
    case "open":
      if (state.sending) return { ...state, open: true };
      return { ...state, preview: action.preview, open: true, error: null, receipt: action.preview.sent ? action.preview.report_id : null };
    case "close":
      return { ...state, open: false };
    case "send":
      return state.preview && !state.sending && !state.receipt ? { ...state, sending: true, error: null } : state;
    case "success":
      return { ...state, sending: false, receipt: action.id,
        latest: state.latest?.report_id === action.id ? { ...state.latest, sent: true } : state.latest };
    case "failure":
      return { ...state, sending: false, error: action.message };
  }
}
