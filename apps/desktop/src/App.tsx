// The main window: status, setup steps, folders, activity and settings.

import { useCallback, useEffect, useState, type ReactNode } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { disable, enable, isEnabled } from "@tauri-apps/plugin-autostart";
import {
  api,
  fileName,
  type AiApp,
  type ApprovalMode,
  type HistoryEvent,
  type Snapshot,
} from "./api";
import { UpdateBanner } from "./Updates";

type Page = "overview" | "folders" | "activity" | "settings";

const PAGES: { id: Page; label: string }[] = [
  { id: "overview", label: "Overview" },
  { id: "folders", label: "Folders" },
  { id: "activity", label: "Activity" },
  { id: "settings", label: "Settings" },
];

export function App() {
  const [page, setPage] = useState<Page>("overview");
  const [state, setState] = useState<Snapshot | null>(null);
  const [notice, setNotice] = useState<{ text: string; error: boolean } | null>(null);

  const refresh = useCallback(() => {
    api.state().then(setState).catch((e) => setNotice({ text: String(e), error: true }));
  }, []);

  useEffect(() => {
    refresh();
    const t = setInterval(refresh, 1500);
    return () => clearInterval(t);
  }, [refresh]);

  /** Run an action, show its result or error, then refresh. */
  const run = useCallback(
    async (action: () => Promise<unknown>, ok?: string) => {
      try {
        const result = await action();
        setNotice(
          ok ? { text: ok, error: false } : typeof result === "string" ? { text: result, error: false } : null,
        );
      } catch (e) {
        setNotice({ text: String(e), error: true });
      }
      refresh();
    },
    [refresh],
  );

  if (!state) return <div className="loading">Starting…</div>;

  return (
    <div className="shell">
      <nav className="sidebar">
        <div className="brand">
          <span className="logo" aria-hidden="true" />
          AI File Relay
        </div>
        {PAGES.map((p) => (
          <button
            key={p.id}
            className={`nav ${page === p.id ? "active" : ""}`}
            onClick={() => setPage(p.id)}
          >
            {p.label}
            {p.id === "overview" && state.pending > 0 && <span className="badge">{state.pending}</span>}
          </button>
        ))}
        <div className="version">v{state.version}</div>
      </nav>

      <main className="content">
        <UpdateBanner />
        {notice && (
          <div className={`notice ${notice.error ? "error" : ""}`}>
            <span>{notice.text}</span>
            <button className="link" onClick={() => setNotice(null)} aria-label="Close">
              ×
            </button>
          </div>
        )}
        {page === "overview" && <Overview s={state} run={run} goFolders={() => setPage("folders")} />}
        {page === "folders" && <Folders s={state} run={run} />}
        {page === "activity" && <Activity />}
        {page === "settings" && <Settings s={state} run={run} />}
      </main>
    </div>
  );
}

type Run = (action: () => Promise<unknown>, ok?: string) => Promise<void>;

// ---------- Overview ----------

function Dot({ ok, warn }: { ok: boolean; warn?: boolean }) {
  return <span className={`dot ${ok ? "ok" : warn ? "warn" : "bad"}`} aria-hidden="true" />;
}

function StatusRow({ ok, warn, title, detail }: { ok: boolean; warn?: boolean; title: string; detail: string }) {
  return (
    <div className="status-row">
      <Dot ok={ok} warn={warn} />
      <div>
        <div className="status-title">{title}</div>
        <div className="muted small">{detail}</div>
      </div>
    </div>
  );
}

function Step({ done, title, children }: { done: boolean; title: string; children?: ReactNode }) {
  return (
    <li className={`step ${done ? "done" : ""}`}>
      <span className="check" aria-hidden="true">{done ? "✓" : ""}</span>
      <div className="step-body">
        <div className="step-title">{title}</div>
        {!done && children}
      </div>
    </li>
  );
}

function Overview({ s, run, goFolders }: { s: Snapshot; run: Run; goFolders: () => void }) {
  const hubOk = s.hub.state === "running";
  const hubDetail =
    s.hub.state === "running"
      ? `Listening on 127.0.0.1:${s.hub.port}`
      : s.hub.state === "port_busy"
        ? s.hub.detail
        : "Starting…";
  const modeText: Record<ApprovalMode, string> = {
    ask: "You approve each transfer",
    allow_all: "Every transfer is allowed without asking",
    deny_all: "All transfers are paused",
  };
  const anyApp = s.apps.some((a) => a.connected);

  return (
    <>
      <h1>Overview</h1>
      <section className="card">
        <StatusRow ok={hubOk} title="Hub" detail={hubDetail} />
        <StatusRow
          ok={s.extension_connected}
          title="Chrome extension"
          detail={s.extension_connected ? `Connected (${s.extension_detail})` : "Not connected. Follow the setup steps below."}
        />
        <StatusRow
          ok={s.approval_mode === "ask"}
          warn={s.approval_mode !== "ask"}
          title="Approvals"
          detail={modeText[s.approval_mode]}
        />
      </section>

      <h2>Setup</h2>
      <ol className="steps">
        <Step done={s.allowed_folders.length > 0} title="Choose a folder the AI may use">
          <p className="muted small">The AI can only reach files inside folders you choose.</p>
          <button onClick={goFolders}>Choose a folder</button>
        </Step>
        <Step done={s.native_host_path !== null || s.extension_connected} title="Let Chrome talk to this app">
          <p className="muted small">Registers AI File Relay with Chrome, Edge, Brave and Chromium.</p>
          <button onClick={() => run(api.installNativeHost, "Done. Restart Chrome to finish.")}>
            Set up Chrome
          </button>
        </Step>
        <Step done={s.extension_connected} title="Install the Chrome extension">
          <p className="muted small">
            Until it is in the Chrome Web Store: open <code>chrome://extensions</code>, turn on
            Developer mode, click <strong>Load unpacked</strong> and pick the extension folder. The
            README explains where to get it.
          </p>
        </Step>
        <Step done={anyApp} title="Connect an AI app">
          <AppButtons s={s} run={run} />
        </Step>
      </ol>
      {anyApp && (
        <>
          <h2>AI apps</h2>
          <section className="card">
            <AppButtons s={s} run={run} />
          </section>
        </>
      )}
    </>
  );
}

function AppButtons({ s, run }: { s: Snapshot; run: Run }) {
  const [copied, setCopied] = useState(false);
  return (
    <div className="apps">
      {s.apps.map((a) => (
        <div key={a.id} className="app-row">
          <div>
            <div className="status-title">{a.name}</div>
            <div className="muted small">
              {a.connected ? "Connected" : a.installed ? "Not connected" : "Not found on this computer"}
            </div>
          </div>
          {a.connected ? (
            <button className="link" onClick={() => run(() => api.disconnectApp(a.id as AiApp), `Removed from ${a.name}.`)}>
              Disconnect
            </button>
          ) : (
            <button onClick={() => run(() => api.connectApp(a.id as AiApp))} disabled={!s.relay_mcp_path}>
              Connect
            </button>
          )}
        </div>
      ))}
      {s.claude_code_command && (
        <div className="app-row">
          <div>
            <div className="status-title">Claude Code</div>
            <code className="small">{s.claude_code_command}</code>
          </div>
          <button
            onClick={() => {
              void navigator.clipboard.writeText(s.claude_code_command ?? "");
              setCopied(true);
              setTimeout(() => setCopied(false), 1500);
            }}
          >
            {copied ? "Copied" : "Copy command"}
          </button>
        </div>
      )}
    </div>
  );
}

// ---------- Folders ----------

function Folders({ s, run }: { s: Snapshot; run: Run }) {
  const choose = async () => {
    const picked = await open({ directory: true, multiple: false, title: "Choose a folder the AI may use" });
    if (typeof picked === "string") await run(() => api.addFolder(picked), `Allowed ${picked}`);
  };
  return (
    <>
      <h1>Folders</h1>
      <p className="muted">
        The AI can list and upload files only from these folders. Folders inside them are included.
        Hidden key folders such as <code>.ssh</code> are always blocked.
      </p>
      <section className="card list">
        {s.allowed_folders.length === 0 && <p className="muted">No folders yet.</p>}
        {s.allowed_folders.map((f) => (
          <div key={f} className="list-row">
            <div>
              <div className="status-title">{fileName(f)}</div>
              <div className="muted small">
                {f}
                {s.missing_folders.includes(f) && <span className="tag bad">missing</span>}
              </div>
            </div>
            <button className="link" onClick={() => run(() => api.removeFolder(f))}>
              Remove
            </button>
          </div>
        ))}
      </section>
      <button className="primary" onClick={choose}>
        Add folder…
      </button>
    </>
  );
}

// ---------- Activity ----------

function Activity() {
  const [events, setEvents] = useState<HistoryEvent[]>([]);
  useEffect(() => {
    const load = () => api.history(200).then(setEvents).catch(() => {});
    load();
    const t = setInterval(load, 2000);
    return () => clearInterval(t);
  }, []);
  return (
    <>
      <h1>Activity</h1>
      <p className="muted">Every transfer, newest first. Kept on this computer only.</p>
      <section className="card list">
        {events.length === 0 && <p className="muted">No transfers yet.</p>}
        {events.map((e, i) => (
          <div key={`${e.time_ms}-${i}`} className="list-row">
            <div>
              <div className="status-title">
                {e.action === "upload" ? "↑" : "↓"} {fileName(e.path)}
                {e.site && <span className="muted"> · {e.site}</span>}
              </div>
              <div className="muted small">
                {new Date(e.time_ms).toLocaleString()}
                {e.reason && ` · ${e.reason}`}
              </div>
            </div>
            <span className={`tag ${e.result === "done" ? "ok" : e.result === "denied" ? "warn" : "bad"}`}>
              {e.result}
            </span>
          </div>
        ))}
      </section>
    </>
  );
}

// ---------- Settings ----------

function Settings({ s, run }: { s: Snapshot; run: Run }) {
  const [atLogin, setAtLogin] = useState<boolean | null>(null);
  useEffect(() => {
    isEnabled().then(setAtLogin).catch(() => setAtLogin(null));
  }, []);

  const modes: { id: ApprovalMode; title: string; text: string }[] = [
    { id: "ask", title: "Ask me each time", text: "A popup asks before every transfer. Recommended." },
    { id: "allow_all", title: "Allow everything", text: "No popup. Only inside allowed folders. For testing." },
    { id: "deny_all", title: "Pause all transfers", text: "Every transfer is refused." },
  ];

  return (
    <>
      <h1>Settings</h1>
      <h2>Approvals</h2>
      <section className="card">
        {modes.map((m) => (
          <label key={m.id} className="radio-row">
            <input
              type="radio"
              name="mode"
              checked={s.approval_mode === m.id}
              onChange={() => run(() => api.setMode(m.id))}
            />
            <div>
              <div className="status-title">{m.title}</div>
              <div className="muted small">{m.text}</div>
            </div>
          </label>
        ))}
      </section>

      {s.site_grants.length > 0 && (
        <>
          <h2>Sites allowed for now</h2>
          <section className="card list">
            {s.site_grants.map((g) => (
              <div key={`${g.site}-${g.action}`} className="list-row">
                <div>
                  <div className="status-title">{g.site}</div>
                  <div className="muted small">
                    {g.action}s until {new Date(g.until_ms).toLocaleTimeString()}
                  </div>
                </div>
              </div>
            ))}
            <button className="link" onClick={() => run(api.clearSiteGrants, "Cleared.")}>
              Forget all
            </button>
          </section>
        </>
      )}

      <h2>App</h2>
      <section className="card">
        <label className="radio-row">
          <input
            type="checkbox"
            checked={atLogin === true}
            disabled={atLogin === null}
            onChange={async (e) => {
              const on = e.target.checked;
              await run(() => (on ? enable() : disable()));
              setAtLogin(await isEnabled());
            }}
          />
          <div>
            <div className="status-title">Start when I log in</div>
            <div className="muted small">Starts hidden in the menu bar / system tray.</div>
          </div>
        </label>
      </section>

      <h2>Details</h2>
      <section className="card details">
        <div><span className="muted">Settings file</span><code>{s.config_path}</code></div>
        <div><span className="muted">relay-mcp</span><code>{s.relay_mcp_path ?? "not found"}</code></div>
        <div><span className="muted">Chrome host</span><code>{s.native_host_path ?? "not set up"}</code></div>
      </section>
    </>
  );
}
