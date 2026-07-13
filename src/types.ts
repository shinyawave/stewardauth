// SPDX-FileCopyrightText: 2026 shinyawave
// SPDX-License-Identifier: AGPL-3.0-or-later

// ── Domain types mirroring the Rust IPC surface ────────────────────────────
//
// All fields use camelCase to match `#[serde(rename_all = "camelCase")]` on
// the Rust side.  Keep in sync with src-tauri/src/commands.rs and
// src-tauri/src/steam/confirmations.rs.

/** A lightweight account summary returned by `list_accounts` and `import_mafile`. */
export interface AccountSummary {
  steamId: string;
  accountName: string;
  /** e.g. "active", "locked" — value set by the vault manifest */
  status: string;
  /** Auto-accept market-listing confirmations for this account. */
  autoConfirmMarket: boolean;
  /** Auto-accept trade confirmations for this account. */
  autoConfirmTrade: boolean;
  /** Proxy id assigned to this account, or empty string if none. */
  proxy: string;
}

export interface Proxy {
  id: string;
  scheme: string;
  host: string;
  port: number;
  user: string;
  pass: string;
  favorite: boolean;
}

export interface ProxyCheck {
  ok: boolean;
  latencyMs?: number | null;
  error?: string | null;
}

/** Current Steam Guard TOTP code returned by `get_code`. */
export interface CodeResponse {
  /** 5-character alphanumeric Steam Guard code. */
  code: string;
  /** Seconds remaining in the current 30-second TOTP window (1–30). */
  secondsRemaining: number;
}

/** A single pending mobile confirmation returned by `fetch_confirmations`. */
export interface ConfirmationItem {
  /** Confirmation ID — passed to `respond_confirmation`. */
  id: string;
  /** Human-readable type, e.g. "Trade Offer", "Market Listing". */
  kind: string;
  /** Short title line (maps to `headline` in the Steam response). */
  title: string;
  /** Joined summary lines, comma-separated. */
  summary: string;
  /** Optional icon URL. */
  icon?: string | null;
  /** Stable machine category: "trade" | "market" | "other". */
  category: string;
}

/** A pending Steam login session awaiting approval/denial. */
export interface PendingLogin {
  /** Pending session client id (u64 as string, JS-safe). */
  clientId: string;
  /** Device friendly name (may be empty). */
  device: string;
  /** Best-effort "City, Country" (may be empty). */
  location: string;
  /** Best-effort requestor IP (may be empty). */
  ip: string;
  /** Human platform label (e.g. "Web Browser", "Mobile App"). */
  platform: string;
}

/** A named collection of accounts, returned by `list_groups`. */
export interface Group {
  name: string;
  members: string[];
}

// ── Error types ─────────────────────────────────────────────────────────────

/** Discriminant union matching the Rust `AppError` variant names. */
export type AppErrorKind =
  | "Network"
  | "InvalidPassword"
  | "CorruptMaFile"
  | "SessionExpired"
  | "RateLimited"
  | "SteamError";

/**
 * Typed error object thrown by every IPC command on failure.
 *
 * Rust serialises `AppError` as `{ "kind": "...", "message": "..." }`.
 * Use `isAppError` to narrow an unknown rejection to this type.
 */
export interface AppError {
  kind: AppErrorKind;
  message: string;
}

/** UI settings from `get_settings` (unencrypted, read before unlock). */
export interface AppSettings {
  language: string;
  minimizeToTray: boolean;
  accentHue: number;
  mafileNaming: string;
  commonMafileFormat: boolean;
}

/**
 * Type guard — narrows an unknown rejection value to `AppError`.
 *
 * @example
 * try { await listAccounts(); }
 * catch (e) { if (isAppError(e)) console.error(e.kind, e.message); }
 */
export function isAppError(e: unknown): e is AppError {
  return (
    typeof e === "object" &&
    e !== null &&
    "kind" in e &&
    "message" in e &&
    typeof (e as Record<string, unknown>).kind === "string" &&
    typeof (e as Record<string, unknown>).message === "string"
  );
}
