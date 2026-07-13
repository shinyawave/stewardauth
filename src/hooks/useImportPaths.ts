// SPDX-FileCopyrightText: 2026 shinyawave
// SPDX-License-Identifier: AGPL-3.0-or-later

import { useCallback, useRef, useState } from "react";
import { importPaths } from "../api";
import type { ImportReport } from "../api";
import { isAppError } from "../types";
import type { AppErrorKind } from "../types";

export type ImportUiPhase = "idle" | "importing" | "need-password" | "done" | "error";

export interface ImportState {
  phase: ImportUiPhase;
  report: ImportReport | null;
  errorKind: AppErrorKind | null;
}

/**
 * Owns the maFile import lifecycle: run → (need-password) → done/error.
 * `onDone` is called after a successful import so the caller can refresh accounts.
 */
export function useImportPaths(onDone: () => void) {
  const [phase, setPhase] = useState<ImportUiPhase>("idle");
  const [report, setReport] = useState<ImportReport | null>(null);
  const [errorKind, setErrorKind] = useState<AppErrorKind | null>(null);
  const pending = useRef<string[]>([]);

  const run = useCallback(
    async (paths: string[], password?: string) => {
      if (paths.length === 0) return;
      pending.current = paths;
      setPhase("importing");
      setErrorKind(null);
      try {
        const r = await importPaths(paths, password, undefined);
        setReport(r);
        setPhase("done");
        onDone();
      } catch (e: unknown) {
        if (isAppError(e) && e.kind === "InvalidPassword") {
          setPhase("need-password");
          setErrorKind(password !== undefined ? e.kind : null);
        } else if (isAppError(e)) {
          setPhase("error");
          setErrorKind(e.kind);
        } else {
          setPhase("error");
          setErrorKind(null);
        }
      }
    },
    [onDone],
  );

  const submitPassword = useCallback(
    (password: string) => run(pending.current, password),
    [run],
  );

  const reset = useCallback(() => {
    setPhase("idle");
    setReport(null);
    setErrorKind(null);
    pending.current = [];
  }, []);

  return { phase, report, errorKind, run, submitPassword, reset };
}
