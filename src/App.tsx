// SPDX-FileCopyrightText: 2026 shinyawave
// SPDX-License-Identifier: AGPL-3.0-or-later

import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { useTranslation } from "react-i18next";
import { listAccounts, setEncryption, getSettings, rescanMafiles } from "./api";
import i18n from "./i18n";
import { applyAccent } from "./theme";
import { ReadyView } from "./components/ReadyView";
import { UpdateModal } from "./components/UpdateModal";
import { UnlockScreen } from "./components/UnlockScreen";
import { useUpdater } from "./hooks/useUpdater";
import { Icon } from "./components/icons";
import type { AccountSummary } from "./types";
import { isAppError } from "./types";
import "./App.css";

// ── App-level vault state ────────────────────────────────────────────────────
//
// On startup we attempt listAccounts().
//  - Success → vault is unlocked (either unencrypted, or already accessible).
//  - Failure with AppError.kind === "InvalidPassword" → vault is encrypted &
//    locked → show UnlockScreen gate.
//  - Any other failure → show a regular load error (not the unlock gate), as
//    it likely reflects a different problem (e.g., I/O error).
//
// Limitation / concern: there is currently no `vault_status` backend command
// that can definitively report "encrypted=true, locked=true" without a side
// effect. We rely entirely on the `InvalidPassword` rejection from
// `list_accounts` to detect the locked state. A follow-up backend task should
// add a `vault_status` command to remove this ambiguity.
//
// First-run encryption choice heuristic:
// We show the "Protect with a master password?" prompt when:
//   - listAccounts() succeeded (vault accessible)
//   - accounts.length === 0 (no accounts imported yet — first-run proxy)
//   - The vault isn't already encrypted (encryptionEnabled === false)
// Once the user makes a choice (yes → setEncryption(true, master); no →
// setEncryption(false)) the vault manifest records it; the prompt won't reappear
// because: (a) on yes, next startup listAccounts() will fail with
// InvalidPassword → UnlockScreen replaces first-run prompt; (b) on no, the
// empty-accounts condition won't hold once accounts are added, and even if it
// does the manifest already has encrypted=false so there is no ambiguity.
// This heuristic will show the prompt again if all accounts are deleted and
// the user re-opens with an unencrypted vault — acceptable edge case.

type VaultState =
  | "booting"       // Initial load in progress
  | "locked"        // Encrypted + locked → show UnlockScreen
  | "first-run"     // Unlocked, 0 accounts, no encryption choice yet
  | "ready";        // Normal app

function App() {
  const { t } = useTranslation();
  const [vaultState, setVaultState] = useState<VaultState>("booting");
  const [accounts, setAccounts] = useState<AccountSummary[]>([]);
  const [encryptionEnabled, setEncryptionEnabled] = useState(false);
  const [selectedSteamId, setSelectedSteamId] = useState<string | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [refreshing, setRefreshing] = useState<boolean>(false);

  const updater = useUpdater();

  // ── First-run encryption choice state ──────────────────────────────────────
  const [firstRunBusy, setFirstRunBusy] = useState(false);
  const [firstRunError, setFirstRunError] = useState<string | null>(null);
  const [firstRunMaster, setFirstRunMaster] = useState("");
  const [firstRunConfirm, setFirstRunConfirm] = useState("");
  const [firstRunStep, setFirstRunStep] = useState<"ask" | "password">("ask");

  const firstRunPasswordsMatch =
    firstRunMaster.length > 0 && firstRunMaster === firstRunConfirm;

  // ── Load / refresh accounts ─────────────────────────────────────────────────

  const fetchAccounts = useCallback(async (isInitial = false) => {
    if (!isInitial) setRefreshing(true);
    setLoadError(null);
    try {
      const result = await listAccounts();
      setAccounts(result);
      setSelectedSteamId((prev) => {
        if (prev !== null) return prev;
        return result.length > 0 ? result[0].steamId : null;
      });
      if (isInitial) {
        // Vault is accessible. Check first-run condition.
        if (result.length === 0) {
          // No accounts yet — show first-run encryption prompt.
          setVaultState("first-run");
        } else {
          setVaultState("ready");
        }
      }
    } catch (e: unknown) {
      if (isInitial) {
        if (isAppError(e) && e.kind === "InvalidPassword") {
          // Vault is encrypted and locked.
          setEncryptionEnabled(true);
          setVaultState("locked");
        } else if (isAppError(e)) {
          setLoadError(`[${e.kind}] ${e.message}`);
          setVaultState("ready"); // Show main UI with error banner
        } else {
          setLoadError("Unknown error loading accounts.");
          setVaultState("ready");
        }
      } else {
        if (isAppError(e)) {
          setLoadError(`[${e.kind}] ${e.message}`);
        } else {
          setLoadError("Unknown error loading accounts.");
        }
      }
    } finally {
      if (!isInitial) setRefreshing(false);
    }
  }, []);

  useEffect(() => {
    void fetchAccounts(true);
  }, [fetchAccounts]);

  useEffect(() => {
    void getSettings()
      .then((s) => {
        void i18n.changeLanguage(s.language);
        applyAccent(s.accentHue);
      })
      .catch(() => {});
  }, []);

  const rescannedRef = useRef(false);
  useEffect(() => {
    if (vaultState !== "ready" || rescannedRef.current) return;
    rescannedRef.current = true;
    void rescanMafiles()
      .then((accs) => {
        setAccounts(accs);
        setSelectedSteamId((prev) => prev ?? (accs.length > 0 ? accs[0].steamId : null));
      })
      .catch(() => {});
  }, [vaultState]);

  const updateCheckedRef = useRef(false);
  useEffect(() => {
    if (vaultState !== "ready" || updateCheckedRef.current) return;
    updateCheckedRef.current = true;
    void updater.check(true); // silent: no toast if up-to-date or offline
  }, [vaultState, updater]);

  // ── Unlock callback ─────────────────────────────────────────────────────────

  const handleUnlocked = useCallback(() => {
    setVaultState("ready");
    void fetchAccounts(false);
  }, [fetchAccounts]);

  // ── First-run: user chose YES (enable encryption) ───────────────────────────

  const handleFirstRunEnable = useCallback(async () => {
    if (!firstRunPasswordsMatch || firstRunBusy) return;
    setFirstRunBusy(true);
    setFirstRunError(null);
    try {
      await setEncryption(true, firstRunMaster);
      setEncryptionEnabled(true);
      // After enabling encryption the vault is locked; show the unlock gate
      // so the user verifies the master password works.
      setFirstRunMaster("");
      setFirstRunConfirm("");
      setVaultState("locked");
    } catch (e: unknown) {
      if (isAppError(e)) {
        setFirstRunError(`[${e.kind}] ${e.message}`);
      } else {
        setFirstRunError(t("firstRun.enableError"));
      }
    } finally {
      setFirstRunBusy(false);
    }
  }, [firstRunPasswordsMatch, firstRunBusy, firstRunMaster, t]);

  // ── First-run: user chose NO (skip encryption) ──────────────────────────────

  const handleFirstRunSkip = useCallback(async () => {
    if (firstRunBusy) return;
    setFirstRunBusy(true);
    setFirstRunError(null);
    try {
      await setEncryption(false);
      setEncryptionEnabled(false);
      setVaultState("ready");
    } catch (e: unknown) {
      if (isAppError(e)) {
        setFirstRunError(`[${e.kind}] ${e.message}`);
      } else {
        setFirstRunError(t("firstRun.skipError"));
      }
    } finally {
      setFirstRunBusy(false);
    }
  }, [firstRunBusy, t]);

  // Apply a fresh account list and repair selection if the selected account vanished.
  const applyAccounts = useCallback((result: AccountSummary[]) => {
    setAccounts(result);
    setSelectedSteamId((prev) =>
      prev !== null && result.some((a) => a.steamId === prev)
        ? prev
        : (result.length > 0 ? result[0].steamId : null),
    );
  }, []);

  // Live sync: backend watcher reconciled the manifest → refresh the list.
  useEffect(() => {
    const promise = listen("accounts-changed", () => {
      void listAccounts().then(applyAccounts).catch(() => {});
    });
    return () => {
      void promise.then((unlisten) => unlisten());
    };
  }, [applyAccounts]);

  // ── Import handler ──────────────────────────────────────────────────────────

  // Refresh the account list without changing the current selection.
  // Used by both onImported (single file) and onImportedMany (SDA folder).
  const refreshAccounts = useCallback(async () => {
    setRefreshing(true);
    setLoadError(null);
    try {
      const result = await listAccounts();
      setAccounts(result);
      // Transition out of first-run if we're still in it
      if (vaultState === "first-run") setVaultState("ready");
    } catch (e: unknown) {
      if (isAppError(e)) {
        setLoadError(`[${e.kind}] ${e.message}`);
      } else {
        setLoadError("Unknown error loading accounts.");
      }
    } finally {
      setRefreshing(false);
    }
  }, [vaultState]);

  const handleImported = useCallback(async (steamId: string) => {
    await refreshAccounts();
    setSelectedSteamId(steamId);
  }, [refreshAccounts]);

  // ── Settings callbacks ──────────────────────────────────────────────────────

  const handleAccountRemoved = useCallback((steamId: string) => {
    setAccounts((prev) => {
      const next = prev.filter((a) => a.steamId !== steamId);
      return next;
    });
    setSelectedSteamId((prev) => (prev === steamId ? null : prev));
  }, []);

  // ── Render gates ────────────────────────────────────────────────────────────

  if (vaultState === "booting") {
    return (
      <div className="app-booting">
        <span className="app-booting-spinner" aria-hidden="true" />
        <span className="app-booting-text">{t("common.loading")}</span>
      </div>
    );
  }

  if (vaultState === "locked") {
    return <UnlockScreen onUnlocked={handleUnlocked} />;
  }

  if (vaultState === "first-run") {
    return (
      <div className="first-run-screen">
        <div className="first-run-card">
          <div className="first-run-icon" aria-hidden="true">
            <Icon name="shield-check" size={20} />
          </div>
          <h1 className="first-run-title">{t("firstRun.title")}</h1>
          <p className="first-run-subtitle">
            {t("firstRun.subtitle")}
          </p>

          {firstRunStep === "ask" && (
            <div className="first-run-actions">
              <button
                className="first-run-btn first-run-btn--accent"
                onClick={() => setFirstRunStep("password")}
                disabled={firstRunBusy}
              >
                {t("firstRun.yes")}
              </button>
              <button
                className="first-run-btn first-run-btn--ghost"
                onClick={() => { void handleFirstRunSkip(); }}
                disabled={firstRunBusy}
              >
                {firstRunBusy ? t("firstRun.saving") : t("firstRun.no")}
              </button>
            </div>
          )}

          {firstRunStep === "password" && (
            <form
              className="first-run-form"
              onSubmit={(e) => {
                e.preventDefault();
                void handleFirstRunEnable();
              }}
            >
              <p className="first-run-hint">
                {t("firstRun.hint")}
              </p>
              <input
                type="password"
                className="first-run-input"
                placeholder={t("firstRun.master")}
                value={firstRunMaster}
                onChange={(e) => setFirstRunMaster(e.target.value)}
                disabled={firstRunBusy}
                autoComplete="new-password"
                aria-label={t("firstRun.master")}
                autoFocus
              />
              <input
                type="password"
                className="first-run-input"
                placeholder={t("firstRun.confirm")}
                value={firstRunConfirm}
                onChange={(e) => setFirstRunConfirm(e.target.value)}
                disabled={firstRunBusy}
                autoComplete="new-password"
                aria-label={t("firstRun.confirm")}
              />
              {firstRunConfirm.length > 0 && !firstRunPasswordsMatch && (
                <p className="first-run-field-error">{t("firstRun.mismatch")}</p>
              )}
              {firstRunError !== null && (
                <div className="first-run-error" role="alert">{firstRunError}</div>
              )}
              <div className="first-run-actions">
                <button
                  type="button"
                  className="first-run-btn first-run-btn--ghost"
                  onClick={() => {
                    setFirstRunStep("ask");
                    setFirstRunMaster("");
                    setFirstRunConfirm("");
                    setFirstRunError(null);
                  }}
                  disabled={firstRunBusy}
                >
                  {t("firstRun.back")}
                </button>
                <button
                  type="submit"
                  className="first-run-btn first-run-btn--accent"
                  disabled={firstRunBusy || !firstRunPasswordsMatch}
                >
                  {firstRunBusy ? t("firstRun.enabling") : t("firstRun.enable")}
                </button>
              </div>
            </form>
          )}

          {firstRunError !== null && firstRunStep === "ask" && (
            <div className="first-run-error" role="alert">{firstRunError}</div>
          )}
        </div>
      </div>
    );
  }

  // ── Normal "ready" UI ───────────────────────────────────────────────────────

  return (
    <>
      <ReadyView
        accounts={accounts}
        selectedSteamId={selectedSteamId}
        onSelect={(id) => setSelectedSteamId(id)}
        refreshing={refreshing}
        onRefresh={() => { void rescanMafiles().then(applyAccounts).catch(() => {}); }}
        loadError={loadError}
        encryptionEnabled={encryptionEnabled}
        onEncryptionChanged={(enabled) => setEncryptionEnabled(enabled)}
        onImported={(id) => { void handleImported(id); }}
        onImportedMany={() => { void refreshAccounts(); }}
        onAccountRemoved={handleAccountRemoved}
        onDataDirChanged={() => { void fetchAccounts(false); }}
        onCheckForUpdate={() => { void updater.check(false); }}
        updateCheckStatus={updater.status}
      />
      <UpdateModal
        status={updater.status}
        info={updater.info}
        progress={updater.progress}
        onInstall={() => { void updater.install(); }}
        onLater={updater.dismiss}
      />
    </>
  );
}

export default App;
