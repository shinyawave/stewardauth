// SPDX-FileCopyrightText: 2026 shinyawave
// SPDX-License-Identifier: AGPL-3.0-or-later

import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import type { Group } from "../types";

interface GroupsDropdownProps {
  groups: Group[];
  activeGroup: string | null;
  onSelect: (name: string | null) => void;
}

export function GroupsDropdown({ groups, activeGroup, onSelect }: GroupsDropdownProps) {
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

  const label = activeGroup ?? t("menu.groups");
  const pick = (name: string | null) => {
    onSelect(name);
    setOpen(false);
  };

  return (
    <div className="groups-dd" ref={rootRef}>
      <button
        type="button"
        className={`toolbar-btn toolbar-btn--dropdown${activeGroup ? " toolbar-btn--active-group" : " toolbar-btn--muted"}`}
        onClick={() => setOpen((v) => !v)}
        aria-haspopup="menu"
        aria-expanded={open}
      >
        {label}
      </button>
      {open && (
        <div className="groups-dd-menu" role="menu">
          <button
            className={`groups-dd-item${activeGroup === null ? " groups-dd-item--active" : ""}`}
            role="menuitem"
            onClick={() => pick(null)}
          >
            {t("menu.allAccounts")}
          </button>
          {groups.length > 0 && <div className="ctx-sep" />}
          {groups.map((g) => (
            <button
              key={g.name}
              className={`groups-dd-item${activeGroup === g.name ? " groups-dd-item--active" : ""}`}
              role="menuitem"
              onClick={() => pick(g.name)}
            >
              <span>{g.name}</span>
              <span className="groups-dd-count">{g.members.length}</span>
            </button>
          ))}
          {groups.length === 0 && (
            <div className="groups-dd-empty">{t("menu.noGroups")}</div>
          )}
        </div>
      )}
    </div>
  );
}
