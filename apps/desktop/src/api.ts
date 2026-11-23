// Typed wrappers around the app's Rust commands (src-tauri/src/commands.rs).

import { invoke } from "@tauri-apps/api/core";

export type ApprovalMode = "ask" | "allow_all" | "deny_all";
export type Action = "upload" | "download";
export type AiApp = "claude_desktop" | "cursor";

export type HubState =
  | { state: "starting" }
  | { state: "running"; port: number }
  | { state: "port_busy"; port: number; detail: string };

export interface AppConnection {
  id: AiApp;
  name: string;
  installed: boolean;
  connected: boolean;
  config_file: string | null;
}

export interface Snapshot {
  version: string;
  hub: HubState;
  extension_connected: boolean;
  extension_detail: string;
  approval_mode: ApprovalMode;
  allowed_folders: string[];
  missing_folders: string[];
  native_host_path: string | null;
  relay_mcp_path: string | null;
  apps: AppConnection[];
  claude_code_command: string | null;
  pending: number;
  site_grants: { site: string; action: Action; until_ms: number }[];
  config_path: string;
}

export interface HistoryEvent {
  time_ms: number;
  action: Action;
  path: string;
  site?: string;
  result: "done" | "denied" | "failed";
  reason?: string;
}

export interface PendingApproval {
  id: string;
  action: Action;
  path: string;
  file_name: string;
  site: string | null;
  size: number | null;
  created_ms: number;
  expires_ms: number;
}

export type Answer = "allow" | "allow_site" | "deny";

export const api = {
  state: () => invoke<Snapshot>("get_state"),
  addFolder: (path: string) => invoke<void>("add_folder", { path }),
  removeFolder: (path: string) => invoke<void>("remove_folder", { path }),
  setMode: (mode: ApprovalMode) => invoke<void>("set_approval_mode", { mode }),
  history: (limit = 100) => invoke<HistoryEvent[]>("history", { limit }),
  pending: () => invoke<PendingApproval[]>("pending_approvals"),
  answer: (id: string, answer: Answer) => invoke<boolean>("answer_approval", { id, answer }),
  clearSiteGrants: () => invoke<void>("clear_site_grants"),
  installNativeHost: () => invoke<string[]>("install_native_host"),
  connectApp: (app: AiApp) => invoke<string>("connect_app", { app }),
  disconnectApp: (app: AiApp) => invoke<void>("disconnect_app", { app }),
};

export function formatSize(bytes: number | null | undefined): string {
  if (bytes === null || bytes === undefined) return "";
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}

export function fileName(path: string): string {
  return path.split(/[\\/]/).pop() || path;
}
