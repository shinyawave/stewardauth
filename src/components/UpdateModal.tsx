// SPDX-FileCopyrightText: 2026 shinyawave
// SPDX-License-Identifier: AGPL-3.0-or-later

import { useTranslation } from "react-i18next";
import type { DownloadProgress, UpdateInfo } from "../updater";
import type { UpdaterStatus } from "../hooks/useUpdater";
import "./UpdateModal.css";

interface UpdateModalProps {
  status: UpdaterStatus;
  info: UpdateInfo | null;
  progress: DownloadProgress | null;
  onInstall: () => void;
  onLater: () => void;
}

/** Format a byte count as "1.2 MB". */
function mb(bytes: number): string {
  return `${(bytes / 1_048_576).toFixed(1)} MB`;
}

export function UpdateModal({ status, info, progress, onInstall, onLater }: UpdateModalProps) {
  const { t } = useTranslation();

  const busy = status === "downloading" || status === "installing";
  if (status !== "available" && !busy) return null;
  if (info === null) return null;

  const pct =
    progress && progress.total
      ? Math.min(100, Math.round((progress.downloaded / progress.total) * 100))
      : null;

  return (
    <div className="confirm-overlay" role="dialog" aria-modal="true" aria-label={t("update.title")}>
      <div className="confirm-backdrop" onClick={busy ? undefined : onLater} />
      <div className="confirm-card">
        <h2 className="confirm-title">{t("update.title")}</h2>
        <div className="update-version-line">
          {t("update.versionLine", { version: info.version, current: info.currentVersion })}
        </div>

        {status === "available" && (
          <>
            {info.notes && <div className="update-notes">{info.notes}</div>}
            <div className="confirm-actions">
              <button className="conf-btn conf-btn--ghost" onClick={onLater}>
                {t("update.later")}
              </button>
              <button className="conf-btn conf-btn--accent" onClick={onInstall}>
                {t("update.install")}
              </button>
            </div>
          </>
        )}

        {busy && (
          <div className="update-progress">
            <div className="update-progress-track">
              <div
                className="update-progress-fill"
                style={{ width: pct === null ? "100%" : `${pct}%` }}
              />
            </div>
            <span className="update-progress-label">
              {status === "installing"
                ? t("update.restarting")
                : pct === null
                  ? t("update.downloading")
                  : t("update.downloadingPct", {
                      pct,
                      done: progress ? mb(progress.downloaded) : "",
                      total: progress?.total ? mb(progress.total) : "",
                    })}
            </span>
          </div>
        )}
      </div>
    </div>
  );
}
