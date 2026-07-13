// SPDX-FileCopyrightText: 2026 shinyawave
// SPDX-License-Identifier: AGPL-3.0-or-later

// ── UnlockScreen ─────────────────────────────────────────────────────────────
//
// Full-screen gate shown when the vault is encrypted and locked.
// Calls `unlockVault(master)` and triggers `onUnlocked` on success.
// Wrong password (AppError.kind === "InvalidPassword") shows an inline error.

import { useState, useRef, useEffect, type FormEvent } from "react";
import { useTranslation } from "react-i18next";
import { unlockVault } from "../api";
import { isAppError } from "../types";
import { Icon } from "./icons";

interface UnlockScreenProps {
  onUnlocked: () => void;
}

export function UnlockScreen({ onUnlocked }: UnlockScreenProps) {
  const { t } = useTranslation();
  const [master, setMaster] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    inputRef.current?.focus();
  }, []);

  const handleSubmit = async (e: FormEvent) => {
    e.preventDefault();
    if (!master || busy) return;
    setBusy(true);
    setError(null);
    try {
      await unlockVault(master);
      onUnlocked();
    } catch (err: unknown) {
      if (isAppError(err) && err.kind === "InvalidPassword") {
        setError(t("unlock.wrong"));
      } else if (isAppError(err)) {
        setError(`[${err.kind}] ${err.message}`);
      } else {
        setError(t("unlock.unexpected"));
      }
      setMaster("");
      inputRef.current?.focus();
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="unlock-screen">
      <div className="unlock-card">
        <div className="unlock-icon" aria-hidden="true">
          <Icon name="lock" size={20} />
        </div>
        <h1 className="unlock-title">StewardAuth</h1>
        <p className="unlock-subtitle">{t("unlock.subtitle")}</p>

        <form className="unlock-form" onSubmit={(e) => { void handleSubmit(e); }}>
          <input
            ref={inputRef}
            type="password"
            className="unlock-input"
            placeholder={t("unlock.master")}
            value={master}
            onChange={(e) => setMaster(e.target.value)}
            disabled={busy}
            autoComplete="current-password"
            aria-label={t("unlock.master")}
          />

          {error !== null && (
            <div className="unlock-error" role="alert">{error}</div>
          )}

          <button
            type="submit"
            className="unlock-submit"
            disabled={busy || master.length === 0}
          >
            {busy ? t("unlock.unlocking") : t("unlock.unlock")}
          </button>
        </form>
      </div>
    </div>
  );
}
