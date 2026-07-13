// SPDX-FileCopyrightText: 2026 shinyawave
// SPDX-License-Identifier: AGPL-3.0-or-later

// ── ImportButton — multi-select maFile/ZIP import via a native file picker ───
//
// Flow:
//   1. User clicks the button → native picker (multiple files: .maFile/.json/.zip).
//   2. importPaths(paths) runs (via useImportPaths).
//   3. If any file is SDA-encrypted → InvalidPassword → shared password prompt.
//   4. On success a report (imported / skipped / failed) is shown.
// Drag-and-drop is handled separately in ReadyView using the same hook.

import { useTranslation } from "react-i18next";
import { open as openFilePicker } from "@tauri-apps/plugin-dialog";
import { useImportPaths } from "../hooks/useImportPaths";
import { ImportFeedback } from "./ImportFeedback";

interface ImportButtonProps {
  /** Called after an import completes so the parent can refresh the account list. */
  onImportedMany: () => void;
  /** Visual variant: "sidebar" (pill), "toolbar" (compact + anchored dropdown), or "menuitem" (plain menu row). */
  variant?: "sidebar" | "toolbar" | "menuitem";
  /** Idle-state button label. Defaults to the translated "Import". */
  label?: string;
}

export function ImportButton({ onImportedMany, variant = "sidebar", label }: ImportButtonProps) {
  const { t } = useTranslation();
  const imp = useImportPaths(onImportedMany);
  const isToolbar = variant === "toolbar";
  const isMenuItem = variant === "menuitem";

  async function handleClick() {
    imp.reset();
    let picked: string | string[] | null;
    try {
      picked = await openFilePicker({
        multiple: true,
        filters: [{ name: "maFile / ZIP", extensions: ["maFile", "json", "zip"] }],
      });
    } catch {
      return; // picker failed → treat as cancel
    }
    if (picked === null) return; // cancelled
    const paths = Array.isArray(picked) ? picked : [picked];
    await imp.run(paths);
  }

  const busy = imp.phase === "importing";
  const buttonLabel = busy ? t("import.importing") : (label ?? t("import.button"));

  const hasDropdown =
    imp.phase === "need-password" || imp.phase === "done" || imp.phase === "error";

  return (
    <div className={`import-root${isToolbar ? " import-root--toolbar" : ""}`}>
      <button
        className={isMenuItem ? "account-menu-item" : isToolbar ? "toolbar-btn" : "import-btn"}
        onClick={() => void handleClick()}
        disabled={busy || imp.phase === "need-password"}
        aria-busy={busy}
      >
        {buttonLabel}
      </button>

      <div className={(isToolbar || isMenuItem) && hasDropdown ? "import-dropdown" : undefined}>
        <ImportFeedback
          state={{ phase: imp.phase, report: imp.report, errorKind: imp.errorKind }}
          onSubmitPassword={(pw) => void imp.submitPassword(pw)}
          onReset={imp.reset}
        />
      </div>
    </div>
  );
}
