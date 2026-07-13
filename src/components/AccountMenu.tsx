// SPDX-FileCopyrightText: 2026 shinyawave
// SPDX-License-Identifier: AGPL-3.0-or-later

import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { ImportButton } from "./ImportDialog";

interface Props {
  onImportedMany: () => void;
  onCreate: () => void;
}

export function AccountMenu({ onImportedMany, onCreate }: Props) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const rootRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    const onDoc = (e: MouseEvent) => {
      if (rootRef.current && !rootRef.current.contains(e.target as Node)) setOpen(false);
    };
    document.addEventListener("mousedown", onDoc);
    return () => document.removeEventListener("mousedown", onDoc);
  }, [open]);

  return (
    <div className="account-menu-root" ref={rootRef}>
      <button className="toolbar-btn" onClick={() => setOpen((o) => !o)} aria-haspopup="menu" aria-expanded={open}>
        {t("menu.account")} <span aria-hidden="true">▾</span>
      </button>
      {open && (
        <div className="account-menu-dropdown" role="menu">
          <ImportButton
            variant="menuitem"
            label={t("link.menuImport")}
            onImportedMany={() => { setOpen(false); onImportedMany(); }}
          />
          <button
            className="account-menu-item"
            role="menuitem"
            onClick={() => { setOpen(false); onCreate(); }}
          >
            {t("link.menuCreate")}
          </button>
        </div>
      )}
    </div>
  );
}
