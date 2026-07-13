// SPDX-FileCopyrightText: 2026 shinyawave
// SPDX-License-Identifier: AGPL-3.0-or-later

import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { getVersion } from "@tauri-apps/api/app";
import { openExternal } from "../api";
import { Icon } from "./icons";
import type { UpdaterStatus } from "../hooks/useUpdater";

interface StatusBarProps {
  count: number;
  currentName: string | null;
  /** Number of right-click multi-selected accounts (0 = none). */
  multiCount: number;
  /** Transient message (e.g. auto-confirm feedback); shown centered when present. */
  note: string | null;
  onCheckForUpdate: () => void;
  updateCheckStatus: UpdaterStatus;
}

const GITHUB_URL = "https://github.com/shinyawave";
const TELEGRAM_URL = "https://t.me/StewardAuth";

export function StatusBar({ count, currentName, multiCount, note, onCheckForUpdate, updateCheckStatus }: StatusBarProps) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const [version, setVersion] = useState("");
  const rootRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    void getVersion().then(setVersion).catch(() => {});
  }, []);

  useEffect(() => {
    if (!open) return;
    const onDown = (e: MouseEvent) => {
      if (rootRef.current && !rootRef.current.contains(e.target as Node)) setOpen(false);
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") setOpen(false);
    };
    document.addEventListener("mousedown", onDown);
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("mousedown", onDown);
      document.removeEventListener("keydown", onKey);
    };
  }, [open]);

  const link = (url: string) => () => {
    setOpen(false);
    void openExternal(url);
  };

  return (
    <footer className="status-bar">
      <span className="status-bar-left">
        {t("status.accounts", { count })}
        <span className="status-bar-sep" aria-hidden="true">·</span>
        {t("status.current", { name: currentName ?? t("common.dash") })}
        {multiCount > 0 && (
          <>
            <span className="status-bar-sep" aria-hidden="true">·</span>
            {t("status.selected", { count: multiCount })}
          </>
        )}
      </span>
      {note !== null && <span className="status-bar-note">{note}</span>}

      <div className="status-brand-root" ref={rootRef}>
        <button
          type="button"
          className="status-bar-right status-brand"
          onClick={() => setOpen((v) => !v)}
          aria-haspopup="menu"
          aria-expanded={open}
        >
          StewardAuth
        </button>
        {open && (
          <div className="status-popover" role="menu">
            <div className="status-popover-header">
              StewardAuth
              <span className="status-popover-version">v{version}</span>
            </div>
            <button
              type="button"
              className="status-popover-item"
              role="menuitem"
              onClick={link(GITHUB_URL)}
            >
              <Icon name="github" size={15} />
              <span className="status-popover-item-main">{t("about.author")}</span>
              <span className="status-popover-item-sub">shinyawave</span>
            </button>
            <button
              type="button"
              className="status-popover-item"
              role="menuitem"
              onClick={link(TELEGRAM_URL)}
            >
              <Icon name="telegram" size={15} />
              <span className="status-popover-item-main">{t("about.telegram")}</span>
              <span className="status-popover-item-sub">@StewardAuth</span>
            </button>
            <button
              type="button"
              className="status-popover-item"
              role="menuitem"
              onClick={() => { setOpen(false); onCheckForUpdate(); }}
            >
              <Icon name="refresh" size={15} />
              <span className="status-popover-item-main">{t("update.check")}</span>
              <span className="status-popover-item-sub">
                {updateCheckStatus === "checking"
                  ? t("update.checking")
                  : updateCheckStatus === "upToDate"
                    ? t("update.upToDate")
                    : updateCheckStatus === "error"
                      ? t("update.error")
                      : ""}
              </span>
            </button>
          </div>
        )}
      </div>
    </footer>
  );
}
