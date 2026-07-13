// SPDX-FileCopyrightText: 2026 shinyawave
// SPDX-License-Identifier: AGPL-3.0-or-later

import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import type { AccountSummary, Group } from "../types";
import { Icon } from "./icons";

interface AccountContextMenuProps {
  x: number;
  y: number;
  account: AccountSummary;
  groups: Group[];
  proxies: { id: string; host: string; port: number }[];
  onClose: () => void;
  onCopyLogin: () => void;
  onCopySteamId: () => void;
  onCopyMafile: () => void;
  onExportToFile: () => void;
  onExportSelected?: () => void;
  selectedCount: number;
  onAddToGroup: (name: string) => void;
  onCreateGroup: (name: string) => void;
  onRemoveFromGroup: () => void;
  onAssignProxy: (id: string) => void;
  onUnpinProxy: () => void;
  hasProxy: boolean;
  onDeleteAccount: () => void;
}

export function AccountContextMenu({
  x,
  y,
  account,
  groups,
  proxies,
  onClose,
  onCopyLogin,
  onCopySteamId,
  onCopyMafile,
  onExportToFile,
  onExportSelected,
  selectedCount,
  onAddToGroup,
  onCreateGroup,
  onRemoveFromGroup,
  onAssignProxy,
  onUnpinProxy,
  hasProxy,
  onDeleteAccount,
}: AccountContextMenuProps) {
  const { t } = useTranslation();
  const rootRef = useRef<HTMLDivElement>(null);
  const [showGroups, setShowGroups] = useState(false);
  const [showProxy, setShowProxy] = useState(false);
  const [creating, setCreating] = useState(false);
  const [newName, setNewName] = useState("");

  const inAnyGroup = groups.some((g) => g.members.includes(account.steamId));

  // Close on outside-click or Escape.
  useEffect(() => {
    const onDown = (e: MouseEvent) => {
      if (rootRef.current && !rootRef.current.contains(e.target as Node)) onClose();
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    document.addEventListener("mousedown", onDown);
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("mousedown", onDown);
      document.removeEventListener("keydown", onKey);
    };
  }, [onClose]);

  // Keep the menu on-screen.
  const style: React.CSSProperties = {
    top: Math.min(y, window.innerHeight - 260),
    left: Math.min(x, window.innerWidth - 220),
  };

  const run = (fn: () => void) => () => {
    fn();
    onClose();
  };

  return (
    <div className="ctx-menu" style={style} ref={rootRef} role="menu">
      <button className="ctx-item" role="menuitem" onClick={run(onCopyLogin)}>
        <span>{t("ctx.copyLogin")}</span>
        <span className="ctx-shortcut">Ctrl+C</span>
      </button>
      <button className="ctx-item" role="menuitem" disabled title={t("ctx.passwordNotStored")}>
        <span>{t("ctx.copyPassword")}</span>
        <span className="ctx-shortcut">Ctrl+Shift+C</span>
      </button>
      <button className="ctx-item" role="menuitem" onClick={run(onCopySteamId)}>
        <span>{t("ctx.copySteamId")}</span>
      </button>
      <button className="ctx-item" role="menuitem" onClick={run(onCopyMafile)}>
        <span>{t("ctx.copyMafile")}</span>
        <span className="ctx-shortcut">Ctrl+X</span>
      </button>
      <button className="ctx-item" role="menuitem" onClick={run(onExportToFile)}>
        <span>{t("export.toFile")}</span>
      </button>
      {selectedCount > 1 && onExportSelected && (
        <button className="ctx-item" role="menuitem" onClick={run(onExportSelected)}>
          <span>{t("export.selected", { count: selectedCount })}</span>
        </button>
      )}

      <div className="ctx-sep" />

      <div
        className="ctx-submenu-anchor"
        onMouseEnter={() => setShowGroups(true)}
        onMouseLeave={() => {
          setShowGroups(false);
          setCreating(false);
          setNewName("");
        }}
      >
        <button className="ctx-item ctx-item--submenu" role="menuitem">
          <span>{t("ctx.addToGroup")}</span>
          <span className="ctx-caret"><Icon name="chevron-right" size={12} stroke={2} /></span>
        </button>
        {showGroups && (
          <div className="ctx-submenu" role="menu">
            {groups.map((g) => (
              <button
                key={g.name}
                className="ctx-item"
                role="menuitem"
                onClick={run(() => onAddToGroup(g.name))}
              >
                <span>{g.name}</span>
              </button>
            ))}
            {groups.length > 0 && <div className="ctx-sep" />}
            {creating ? (
              <form
                className="ctx-create"
                onSubmit={(e) => {
                  e.preventDefault();
                  const name = newName.trim();
                  if (name === "") return;
                  onCreateGroup(name);
                  onClose();
                }}
              >
                <input
                  className="ctx-create-input"
                  autoFocus
                  placeholder={t("ctx.groupName")}
                  value={newName}
                  onChange={(e) => setNewName(e.target.value)}
                />
              </form>
            ) : (
              <button
                className="ctx-item ctx-item--accent"
                role="menuitem"
                onClick={() => setCreating(true)}
              >
                <span>＋ {t("ctx.createGroup")}</span>
              </button>
            )}
          </div>
        )}
      </div>

      <button
        className="ctx-item"
        role="menuitem"
        disabled={!inAnyGroup}
        onClick={run(onRemoveFromGroup)}
      >
        <span>{t("ctx.removeFromGroup")}</span>
      </button>

      <div
        className="ctx-submenu-anchor"
        onMouseEnter={() => setShowProxy(true)}
        onMouseLeave={() => setShowProxy(false)}
      >
        <button className="ctx-item ctx-item--submenu" role="menuitem">
          <span>{t("ctx.assignProxy")}</span>
          <span className="ctx-caret"><Icon name="chevron-right" size={12} stroke={2} /></span>
        </button>
        {showProxy && (
          <div className="ctx-submenu" role="menu">
            {proxies.map((p) => (
              <button
                key={p.id}
                className="ctx-item"
                role="menuitem"
                onClick={run(() => onAssignProxy(p.id))}
              >
                <span>{p.host}:{p.port}</span>
              </button>
            ))}
            {proxies.length === 0 && (
              <div className="ctx-item" style={{ opacity: 0.5, cursor: "default" }}>
                <span>{t("proxy.empty")}</span>
              </div>
            )}
          </div>
        )}
      </div>

      <button className="ctx-item" role="menuitem" disabled={!hasProxy} onClick={run(onUnpinProxy)}>
        <span>{t("ctx.unpinProxy")}</span>
      </button>

      <div className="ctx-sep" />

      <button
        className="ctx-item ctx-item--danger"
        role="menuitem"
        onClick={run(onDeleteAccount)}
      >
        <span>{t("ctx.deleteAccount")}</span>
      </button>
    </div>
  );
}
