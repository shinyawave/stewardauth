// SPDX-FileCopyrightText: 2026 shinyawave
// SPDX-License-Identifier: AGPL-3.0-or-later

// ── Typed IPC client — wraps Tauri `invoke` with full TypeScript types ──────
//
// Each function maps one-to-one to a Tauri command defined in
// src-tauri/src/commands.rs.  Arg object keys are camelCase; Tauri's IPC
// layer auto-converts them to the snake_case Rust parameter names.
//
// On error every function rejects with an `AppError` JSON object
// `{ kind: AppErrorKind; message: string }`.  Callers should use `isAppError`
// from `./types` to narrow the rejection before switching on `kind`.

import { invoke } from "@tauri-apps/api/core";
import { openPath, openUrl } from "@tauri-apps/plugin-opener";
import type {
  AccountSummary,
  AppSettings,
  CodeResponse,
  ConfirmationItem,
  Group,
  PendingLogin,
  Proxy,
  ProxyCheck,
} from "./types";

/**
 * Return all accounts stored in the vault manifest.
 *
 * Invoke: `list_accounts` (no args)
 */
export const listAccounts = (): Promise<AccountSummary[]> =>
  invoke<AccountSummary[]>("list_accounts");

/**
 * Return the current Steam Guard TOTP code for the account identified by
 * `steamId`.
 *
 * Invoke: `get_code { steamId }`
 */
export const getCode = (steamId: string): Promise<CodeResponse> =>
  invoke<CodeResponse>("get_code", { steamId });

/** Report returned by importPaths. */
export interface ImportReport {
  imported: number;
  skippedExisting: string[];
  failed: string[];
}

/**
 * Import maFiles from a set of filesystem paths. Each path may be an individual
 * `.maFile`/`.json`, a `.zip` archive, or a folder (with or without an SDA
 * `manifest.json`). Encrypted files use the shared `password`.
 *
 * @param paths    Absolute paths (files, zips, and/or folders).
 * @param password Optional shared SDA password (required for encrypted maFiles).
 * @param master   Optional vault master password (falls back to in-memory master).
 *
 * Invoke: `import_paths { paths, password?, master? }`
 */
export const importPaths = (
  paths: string[],
  password?: string,
  master?: string,
): Promise<ImportReport> =>
  invoke<ImportReport>("import_paths", { paths, password, master });

/**
 * Fetch pending mobile confirmations for the account identified by `steamId`.
 * Requires an active (non-expired) session — call `login` first.
 *
 * Invoke: `fetch_confirmations { steamId }`
 */
export const fetchConfirmations = (steamId: string): Promise<ConfirmationItem[]> =>
  invoke<ConfirmationItem[]>("fetch_confirmations", { steamId });

/**
 * Accept or deny a batch of confirmations by their IDs.
 *
 * @param steamId Steam account ID.
 * @param ids     Array of confirmation IDs to act on.
 * @param accept  `true` to accept, `false` to deny.
 *
 * Invoke: `respond_confirmation { steamId, ids, accept }`
 */
export const respondConfirmation = (
  steamId: string,
  ids: string[],
  accept: boolean,
): Promise<void> =>
  invoke<void>("respond_confirmation", { steamId, ids, accept });

/**
 * List pending Steam login sessions for the account identified by `steamId`.
 * Requires an active/restorable session (same path as fetchConfirmations).
 *
 * Invoke: `list_login_approvals { steamId }`
 */
export const listLoginApprovals = (steamId: string): Promise<PendingLogin[]> =>
  invoke<PendingLogin[]>("list_login_approvals", { steamId });

/**
 * Approve or deny a pending login by its `clientId` (string, u64-safe).
 *
 * Invoke: `respond_login_approval { steamId, clientId, approve }`
 */
export const respondLoginApproval = (
  steamId: string,
  clientId: string,
  approve: boolean,
): Promise<void> =>
  invoke<void>("respond_login_approval", { steamId, clientId, approve });

/**
 * Perform a Steam credential login and store the resulting session in memory.
 * Returns the status string `"ok"` on success.
 *
 * Invoke: `login { steamId, username, password }`
 */
export const login = (
  steamId: string,
  username: string,
  password: string,
): Promise<string> =>
  invoke<string>("login", { steamId, username, password });

/**
 * Unlock the encrypted vault by validating and storing the master password.
 *
 * Invoke: `unlock_vault { master }`
 */
export const unlockVault = (master: string): Promise<void> =>
  invoke<void>("unlock_vault", { master });

/**
 * Enable or disable vault encryption.
 *
 * @param enabled `true` to enable (requires `master`), `false` to disable.
 * @param master  Master password — required when enabling encryption.
 *
 * Invoke: `set_encryption { enabled, master? }`
 */
export const setEncryption = (
  enabled: boolean,
  master?: string,
): Promise<void> =>
  invoke<void>("set_encryption", { enabled, master });

/**
 * Remove an account from the vault and the macOS Keychain.
 *
 * Invoke: `remove_account { steamId }`
 */
export const removeAccount = (steamId: string): Promise<void> =>
  invoke<void>("remove_account", { steamId });

/**
 * Set per-account auto-confirm flags for a batch of accounts. `market`/`trade`
 * of `undefined` leaves that flag untouched. Returns the updated account list.
 *
 * Invoke: `set_auto_confirm { steamIds, market?, trade? }`
 */
export const setAutoConfirm = (
  steamIds: string[],
  market?: boolean,
  trade?: boolean,
): Promise<AccountSummary[]> =>
  invoke<AccountSummary[]>("set_auto_confirm", { steamIds, market, trade });

/**
 * Persist the auto-confirm poll interval (seconds). Clamped server-side to a safe
 * minimum. Returns the value actually stored.
 *
 * Invoke: `set_poll_interval { seconds }`
 */
export const setPollInterval = (seconds: number): Promise<number> =>
  invoke<number>("set_poll_interval", { seconds });

/**
 * Read the current auto-confirm poll interval (seconds).
 *
 * Invoke: `get_poll_interval` (no args)
 */
export const getPollInterval = (): Promise<number> =>
  invoke<number>("get_poll_interval");

/** List all account groups. Invoke: `list_groups` */
export const listGroups = (): Promise<Group[]> => invoke<Group[]>("list_groups");

/** Create an empty group (no-op if it exists). Invoke: `create_group { name }` */
export const createGroup = (name: string): Promise<void> =>
  invoke<void>("create_group", { name });

/** Delete a group. Invoke: `delete_group { name }` */
export const deleteGroup = (name: string): Promise<void> =>
  invoke<void>("delete_group", { name });

/** Add an account to a group (creates it if missing). Invoke: `add_to_group { steamId, name }` */
export const addToGroup = (steamId: string, name: string): Promise<void> =>
  invoke<void>("add_to_group", { steamId, name });

/** Remove an account from a group. Invoke: `remove_from_group { steamId, name }` */
export const removeFromGroup = (steamId: string, name: string): Promise<void> =>
  invoke<void>("remove_from_group", { steamId, name });

/** Export a standard maFile JSON for an account (contains secrets). Invoke: `export_mafile { steamId }` */
export const exportMafile = (steamId: string): Promise<string> =>
  invoke<string>("export_mafile", { steamId });

/**
 * Export one or many accounts' plaintext maFiles to `destPath` (contains
 * secrets). One id → single maFile written to destPath; many ids → a ZIP.
 * Returns the written path. Invoke: `export_mafiles { steamIds, destPath }`
 */
export const exportMafiles = (steamIds: string[], destPath: string): Promise<string> =>
  invoke<string>("export_mafiles", { steamIds, destPath });

/** Read persisted UI settings. Invoke: `get_settings` */
export const getSettings = (): Promise<AppSettings> =>
  invoke<AppSettings>("get_settings");

/** Update UI settings (partial). Invoke: `set_settings { language?, minimizeToTray?, accentHue?, mafileNaming?, commonMafileFormat? }` */
export const setSettings = (patch: {
  language?: string;
  minimizeToTray?: boolean;
  accentHue?: number;
  mafileNaming?: string;
  commonMafileFormat?: boolean;
}): Promise<AppSettings> => invoke<AppSettings>("set_settings", patch);

/** Migrate legacy Keychain accounts to files + import dropped maFiles. Invoke: `rescan_mafiles` */
export const rescanMafiles = (): Promise<AccountSummary[]> =>
  invoke<AccountSummary[]>("rescan_mafiles");

/** Rename all maFiles to the current naming mode. Invoke: `rename_mafiles` */
export const renameMafiles = (): Promise<AccountSummary[]> =>
  invoke<AccountSummary[]>("rename_mafiles");

/** Get the maFiles folder path (created if missing). Invoke: `mafiles_dir` */
export const mafilesDir = (): Promise<string> => invoke<string>("mafiles_dir");

/** Open the maFiles folder in Finder. */
export const openMafilesFolder = async (): Promise<void> => {
  const dir = await mafilesDir();
  await openPath(dir);
};

/** Open an external URL in the default browser. */
export const openExternal = (url: string): Promise<void> => openUrl(url);

/** Quit the app. Invoke: `quit_app` */
export const quitApp = (): Promise<void> => invoke<void>("quit_app");

export const listProxies = (): Promise<Proxy[]> => invoke<Proxy[]>("list_proxies");
export const addProxies = (text: string): Promise<Proxy[]> =>
  invoke<Proxy[]>("add_proxies", { text });
export const deleteProxy = (id: string): Promise<void> =>
  invoke<void>("delete_proxy", { id });
export const setProxyFavorite = (id: string, favorite: boolean): Promise<void> =>
  invoke<void>("set_proxy_favorite", { id, favorite });
export const setDefaultProxy = (id: string): Promise<void> =>
  invoke<void>("set_default_proxy", { id });
export const getDefaultProxy = (): Promise<string> => invoke<string>("get_default_proxy");
export const assignProxy = (steamId: string, id: string): Promise<void> =>
  invoke<void>("assign_proxy", { steamId, id });
export const unpinProxy = (steamId: string): Promise<void> =>
  invoke<void>("unpin_proxy", { steamId });
export const checkProxy = (line: string): Promise<ProxyCheck> =>
  invoke<ProxyCheck>("check_proxy", { line });

/** Report returned by assignProxiesByText. */
export interface AssignReport {
  /** Number of accounts that were successfully assigned a proxy. */
  assigned: number;
  /** Login names that could not be matched to any account. */
  unmatched: string[];
}

/** Assign a single proxy to multiple accounts at once. Invoke: `bulk_assign_proxy { steamIds, proxyId }` */
export const bulkAssignProxy = (steamIds: string[], proxyId: string): Promise<void> =>
  invoke<void>("bulk_assign_proxy", { steamIds, proxyId });

/** Distribute available proxies across a set of accounts. Invoke: `distribute_proxies { steamIds }` */
export const distributeProxies = (steamIds: string[]): Promise<void> =>
  invoke<void>("distribute_proxies", { steamIds });

/** Assign proxies to accounts by login:proxy text lines. Invoke: `assign_proxies_by_text { text }` */
export const assignProxiesByText = (text: string): Promise<AssignReport> =>
  invoke<AssignReport>("assign_proxies_by_text", { text });

/** Return the current resolved data directory path. Invoke: `get_data_dir` */
export const getDataDir = (): Promise<string> => invoke<string>("get_data_dir");

/** Move all data to `path`, update the pointer, and switch. Returns the new path. Invoke: `set_data_dir { newPath }` */
export const setDataDir = (path: string): Promise<string> =>
  invoke<string>("set_data_dir", { newPath: path });

// ── Authenticator linking ────────────────────────────────────────────────────

export interface LinkLoginResult {
  status: "ok" | "bad_credentials" | "rate_limited";
  needsEmailGuard: boolean;
}
export interface LinkStatusResult {
  status: string; // "ok" | "bad_code" | "rate_limited" | "failed"
}
export interface LinkStartResult {
  status: "code" | "need_phone" | "already_linked" | "rate_limited";
  confirmType?: "sms" | "email";
  phoneHint?: string;
}
export interface LinkAwaitResult {
  stillWaiting: boolean;
  seconds?: number;
}
export interface LinkFinalizeResult {
  status: "done" | "wrong_code" | "time_sync_failed";
  revocationCode?: string;
  mafileName?: string;
}

export const linkBeginLogin = (
  username: string,
  password: string,
  proxyId?: string,
): Promise<LinkLoginResult> =>
  invoke<LinkLoginResult>("link_begin_login", { username, password, proxyId });

export const linkSubmitEmailGuard = (code: string): Promise<LinkStatusResult> =>
  invoke<LinkStatusResult>("link_submit_email_guard", { code });

export const linkStart = (): Promise<LinkStartResult> =>
  invoke<LinkStartResult>("link_start");

export const linkSetPhone = (phone: string): Promise<LinkStatusResult> =>
  invoke<LinkStatusResult>("link_set_phone", { phone });

export const linkAwaitPhoneEmail = (): Promise<LinkAwaitResult> =>
  invoke<LinkAwaitResult>("link_await_phone_email");

export const linkSendSms = (): Promise<LinkStatusResult> =>
  invoke<LinkStatusResult>("link_send_sms");

export const linkFinalize = (code: string): Promise<LinkFinalizeResult> =>
  invoke<LinkFinalizeResult>("link_finalize", { code });

export const linkCancel = (): Promise<null> => invoke<null>("link_cancel");

export const linkSaveRevocation = (path: string, code: string): Promise<null> =>
  invoke<null>("link_save_revocation", { path, code });
