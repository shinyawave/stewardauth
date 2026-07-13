// SPDX-FileCopyrightText: 2026 shinyawave
// SPDX-License-Identifier: AGPL-3.0-or-later

import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { quitApp } from "../api";

interface MenuDropdownProps {
  onSettings: () => void;
}

export function MenuDropdown({ onSettings }: MenuDropdownProps) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const rootRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    const onDown = (e: MouseEvent) => {
      if (rootRef.current && !rootRef.current.contains(e.target as Node)) setOpen(false);
    };
    document.addEventListener("mousedown", onDown);
    return () => document.removeEventListener("mousedown", onDown);
  }, [open]);

  return (
    <div className="groups-dd" ref={rootRef}>
      <button
        type="button"
        className="toolbar-btn"
        onClick={() => setOpen((v) => !v)}
        aria-haspopup="menu"
        aria-expanded={open}
      >
        {t("menu.menu")}
      </button>
      {open && (
        <div className="groups-dd-menu" role="menu">
          <button
            className="groups-dd-item"
            role="menuitem"
            onClick={() => {
              setOpen(false);
              onSettings();
            }}
          >
            {t("menu.settings")}
          </button>
          <button
            className="groups-dd-item"
            role="menuitem"
            onClick={() => {
              setOpen(false);
              void quitApp();
            }}
          >
            {t("menu.quit")}
          </button>
        </div>
      )}
    </div>
  );
}
