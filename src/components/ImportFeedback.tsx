// SPDX-FileCopyrightText: 2026 shinyawave
// SPDX-License-Identifier: AGPL-3.0-or-later

import { useState } from "react";
import { useTranslation } from "react-i18next";
import type { ImportState } from "../hooks/useImportPaths";

interface ImportFeedbackProps {
  state: ImportState;
  onSubmitPassword: (password: string) => void;
  onReset: () => void;
}

/**
 * Shared UI for the import lifecycle: SDA password prompt, result report, and
 * error banner. Renders nothing while idle or importing.
 */
export function ImportFeedback({ state, onSubmitPassword, onReset }: ImportFeedbackProps) {
  const { t } = useTranslation();
  const [password, setPassword] = useState("");

  if (state.phase === "need-password") {
    return (
      <form
        className="import-password-form"
        onSubmit={(e) => {
          e.preventDefault();
          onSubmitPassword(password);
        }}
        aria-label="SDA password required"
      >
        <p className="import-password-label">{t("import.sdaPrompt")}</p>
        <div className="import-password-row">
          <input
            className="import-password-input"
            type="password"
            value={password}
            onChange={(e) => setPassword(e.target.value)}
            placeholder={t("import.sdaPassword")}
            autoFocus
            autoComplete="off"
          />
          <button type="submit" className="import-password-submit" disabled={password.length === 0}>
            {t("import.unlock")}
          </button>
          <button
            type="button"
            className="import-password-cancel"
            onClick={() => {
              setPassword("");
              onReset();
            }}
          >
            {t("common.cancel")}
          </button>
        </div>
        {state.errorKind === "InvalidPassword" && (
          <p className="import-error import-error--inline">{t("import.errInvalidPassword")}</p>
        )}
      </form>
    );
  }

  if (state.phase === "done" && state.report !== null) {
    const r = state.report;
    return (
      <div className="import-report" role="status">
        <p>{t("import.sdaReportImported", { count: r.imported })}</p>
        {r.skippedExisting.length > 0 && (
          <p>{t("import.sdaReportSkipped", { count: r.skippedExisting.length })}</p>
        )}
        {r.failed.length > 0 && (
          <>
            <p>{t("import.sdaReportFailed", { count: r.failed.length })}</p>
            <ul className="import-report-failed">
              {r.failed.map((f) => (
                <li key={f}>{f}</li>
              ))}
            </ul>
          </>
        )}
        <button type="button" className="import-password-cancel" onClick={onReset}>
          {t("common.close")}
        </button>
      </div>
    );
  }

  if (state.phase === "error" && state.errorKind !== null) {
    return (
      <div className="import-error" role="alert">
        <span className="import-error-kind">{state.errorKind}</span>
        <span className="import-error-msg">{t(`import.err${state.errorKind}`)}</span>
      </div>
    );
  }

  if (state.phase === "error" && state.errorKind === null) {
    return (
      <div className="import-error" role="alert">
        <span className="import-error-msg">{t("import.unexpected")}</span>
      </div>
    );
  }

  return null;
}
