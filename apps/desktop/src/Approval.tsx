// The approval popup. Shows the oldest waiting request and its countdown.

import { useCallback, useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { api, formatSize, type Answer, type PendingApproval } from "./api";

export function ApprovalPopup() {
  const [list, setList] = useState<PendingApproval[]>([]);
  const [now, setNow] = useState(Date.now());

  const refresh = useCallback(() => {
    api.pending().then(setList).catch(() => setList([]));
  }, []);

  useEffect(() => {
    refresh();
    const unlisten = listen("approvals-changed", refresh);
    const tick = setInterval(() => setNow(Date.now()), 500);
    return () => {
      void unlisten.then((f) => f());
      clearInterval(tick);
    };
  }, [refresh]);

  const current = list[0];
  if (!current) {
    return (
      <main className="popup">
        <p className="muted">Nothing is waiting for approval.</p>
      </main>
    );
  }

  const answer = (a: Answer) => {
    void api.answer(current.id, a).then(refresh);
  };
  const secondsLeft = Math.max(0, Math.ceil((current.expires_ms - now) / 1000));
  const isUpload = current.action === "upload";
  const site = current.site ?? "a web page";

  return (
    <main className="popup">
      <p className="eyebrow">{isUpload ? "Upload request" : "Download request"}</p>
      <h1 className="popup-title">
        {isUpload ? (
          <>
            An AI app wants to put <strong>{current.file_name}</strong> into {site}
          </>
        ) : (
          <>An AI app wants to receive the next file {site} downloads</>
        )}
      </h1>

      <dl className="facts">
        {isUpload && (
          <>
            <dt>File</dt>
            <dd title={current.path}>{current.path}</dd>
            <dt>Size</dt>
            <dd>{formatSize(current.size)}</dd>
          </>
        )}
        {!isUpload && (
          <>
            <dt>Saved to</dt>
            <dd title={current.path}>{current.path}</dd>
          </>
        )}
        <dt>Site</dt>
        <dd>{current.site ?? "unknown"}</dd>
      </dl>

      <div className="popup-actions">
        <button className="primary" onClick={() => answer("allow")} autoFocus>
          Allow once
        </button>
        {current.site && (
          <button onClick={() => answer("allow_site")}>
            Allow {current.site} for 1 hour
          </button>
        )}
        <button className="danger" onClick={() => answer("deny")}>
          Deny
        </button>
      </div>

      <p className="muted small">
        Denied automatically in {secondsLeft} s
        {list.length > 1 ? ` · ${list.length - 1} more waiting` : ""}
      </p>
    </main>
  );
}
