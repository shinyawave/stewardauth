// SPDX-FileCopyrightText: 2026 shinyawave
// SPDX-License-Identifier: AGPL-3.0-or-later

// ── useUpdater ──────────────────────────────────────────────────────────────
//
// One state machine shared by the startup auto-check (App) and the manual
// "Check for update" button (StatusBar). `check(silent)` stays quiet on
// no-update/error when silent (startup), and reports upToDate/error when not
// (manual button).

import { useCallback, useRef, useState } from "react";
import {
  checkForUpdate,
  relaunchApp,
  type DownloadProgress,
  type PendingUpdate,
  type UpdateInfo,
} from "../updater";

export type UpdaterStatus =
  | "idle"
  | "checking"
  | "available"
  | "downloading"
  | "installing"
  | "upToDate"
  | "error";

export interface UseUpdater {
  status: UpdaterStatus;
  info: UpdateInfo | null;
  progress: DownloadProgress | null;
  check: (silent: boolean) => Promise<void>;
  install: () => Promise<void>;
  dismiss: () => void;
}

export function useUpdater(): UseUpdater {
  const [status, setStatus] = useState<UpdaterStatus>("idle");
  const [info, setInfo] = useState<UpdateInfo | null>(null);
  const [progress, setProgress] = useState<DownloadProgress | null>(null);
  const pendingRef = useRef<PendingUpdate | null>(null);

  const check = useCallback(async (silent: boolean) => {
    setStatus("checking");
    try {
      const pending = await checkForUpdate();
      if (pending) {
        pendingRef.current = pending;
        setInfo(pending.info);
        setStatus("available");
      } else {
        pendingRef.current = null;
        setInfo(null);
        setStatus(silent ? "idle" : "upToDate");
      }
    } catch {
      pendingRef.current = null;
      setStatus(silent ? "idle" : "error");
    }
  }, []);

  const install = useCallback(async () => {
    const pending = pendingRef.current;
    if (!pending) return;
    setProgress({ downloaded: 0, total: null });
    setStatus("downloading");
    try {
      await pending.install((p) => setProgress(p));
      setStatus("installing");
      await relaunchApp();
    } catch {
      setStatus("error");
    }
  }, []);

  const dismiss = useCallback(() => {
    setStatus("idle");
    setProgress(null);
  }, []);

  return { status, info, progress, check, install, dismiss };
}
