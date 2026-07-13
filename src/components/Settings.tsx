// SPDX-FileCopyrightText: 2026 shinyawave
// SPDX-License-Identifier: AGPL-3.0-or-later

// ── Settings panel ─────────────────────────────────────────────────────────
//
// Rendered as an overlay panel opened via the Menu button.
// Features:
//  - General tab: Language, Minimize to Tray, Auto-confirm, Encryption, Accounts, About
//  - Theme tab: Accent color hue slider

import { useEffect, useRef, useState, type FormEvent } from "react";
import { useTranslation } from "react-i18next";
import { setEncryption, removeAccount, getPollInterval, setPollInterval, getSettings, setSettings, renameMafiles, openMafilesFolder, getDataDir, setDataDir } from "../api";
import { isAppError } from "../types";
import type { AccountSummary } from "../types";
import { open as openDirPicker } from "@tauri-apps/plugin-dialog";
import { applyAccent } from "../theme";
import { LANGUAGES } from "../i18n";
import i18n from "../i18n";
import { Icon } from "./icons";
import { getVersion } from "@tauri-apps/api/app";

const MIN_POLL_INTERVAL = 15;

interface SettingsProps {
  accounts: AccountSummary[];
  encryptionEnabled: boolean;
  onEncryptionChanged: (enabled: boolean) => void;
  onAccountRemoved: (steamId: string) => void;
  onClose: () => void;
  /** Called after a successful data-directory move so the caller can reload accounts/settings. */
  onDataDirChanged?: () => void;
}

// ── Encryption sub-panel ────────────────────────────────────────────────────

interface EncryptionPanelProps {
  encryptionEnabled: boolean;
  onChanged: (enabled: boolean) => void;
}

function EncryptionPanel({ encryptionEnabled, onChanged }: EncryptionPanelProps) {
  const { t } = useTranslation();
  // "enable" flow: collect master + confirm
  const [showEnableForm, setShowEnableForm] = useState(false);
  const [newMaster, setNewMaster] = useState("");
  const [confirmMaster, setConfirmMaster] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [success, setSuccess] = useState<string | null>(null);

  const passwordsMatch = newMaster.length > 0 && newMaster === confirmMaster;

  const handleEnable = async (e: FormEvent) => {
    e.preventDefault();
    if (!passwordsMatch || busy) return;
    setBusy(true);
    setError(null);
    setSuccess(null);
    try {
      await setEncryption(true, newMaster);
      setNewMaster("");
      setConfirmMaster("");
      setShowEnableForm(false);
      setSuccess(t("settings.encEnabled"));
      onChanged(true);
    } catch (err: unknown) {
      if (isAppError(err)) {
        setError(`[${err.kind}] ${err.message}`);
      } else {
        setError(t("settings.encEnableFailed"));
      }
    } finally {
      setBusy(false);
    }
  };

  const handleDisable = async () => {
    if (busy) return;
    setBusy(true);
    setError(null);
    setSuccess(null);
    try {
      await setEncryption(false);
      setSuccess(t("settings.encDisabled"));
      onChanged(false);
    } catch (err: unknown) {
      if (isAppError(err)) {
        setError(`[${err.kind}] ${err.message}`);
      } else {
        setError(t("settings.encDisableFailed"));
      }
    } finally {
      setBusy(false);
    }
  };

  return (
    <section className="settings-section">
      <h2 className="settings-section-title">{t("settings.encryption")}</h2>

      <div className="settings-row">
        <div className="settings-row-info">
          <span className="settings-row-label">
            {t("settings.masterProtection")}
          </span>
          <span className={`settings-encryption-badge${encryptionEnabled ? " settings-encryption-badge--on" : ""}`}>
            {encryptionEnabled ? t("settings.on") : t("settings.off")}
          </span>
        </div>
        <div className="settings-row-actions">
          {encryptionEnabled ? (
            <button
              className="settings-btn settings-btn--danger"
              onClick={() => { void handleDisable(); }}
              disabled={busy}
            >
              {busy ? t("settings.disabling") : t("settings.disable")}
            </button>
          ) : (
            <button
              className="settings-btn settings-btn--accent"
              onClick={() => { setShowEnableForm((v) => !v); setError(null); setSuccess(null); }}
              disabled={busy}
            >
              {t("settings.enable")}
            </button>
          )}
        </div>
      </div>

      {showEnableForm && !encryptionEnabled && (
        <form className="settings-sub-form" onSubmit={(e) => { void handleEnable(e); }}>
          <p className="settings-hint">
            {t("settings.enableHint")}
          </p>
          <input
            type="password"
            className="settings-input"
            placeholder={t("settings.newMaster")}
            value={newMaster}
            onChange={(e) => setNewMaster(e.target.value)}
            disabled={busy}
            autoComplete="new-password"
            aria-label={t("settings.newMaster")}
          />
          <input
            type="password"
            className="settings-input"
            placeholder={t("settings.confirmMaster")}
            value={confirmMaster}
            onChange={(e) => setConfirmMaster(e.target.value)}
            disabled={busy}
            autoComplete="new-password"
            aria-label={t("settings.confirmMaster")}
          />
          {confirmMaster.length > 0 && !passwordsMatch && (
            <p className="settings-field-error">{t("settings.mismatch")}</p>
          )}
          <div className="settings-form-actions">
            <button
              type="button"
              className="settings-btn settings-btn--ghost"
              onClick={() => { setShowEnableForm(false); setNewMaster(""); setConfirmMaster(""); setError(null); }}
              disabled={busy}
            >
              {t("common.cancel")}
            </button>
            <button
              type="submit"
              className="settings-btn settings-btn--accent"
              disabled={busy || !passwordsMatch}
            >
              {busy ? t("settings.enabling") : t("common.save")}
            </button>
          </div>
        </form>
      )}

      {error !== null && (
        <div className="settings-error" role="alert">{error}</div>
      )}
      {success !== null && (
        <div className="settings-success" role="status">{success}</div>
      )}
    </section>
  );
}

// ── Account removal sub-panel ──────────────────────────────────────────────

interface RemoveAccountPanelProps {
  accounts: AccountSummary[];
  onRemoved: (steamId: string) => void;
}

function RemoveAccountPanel({ accounts, onRemoved }: RemoveAccountPanelProps) {
  const { t } = useTranslation();
  const [pendingSteamId, setPendingSteamId] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const pendingAccount = accounts.find((a) => a.steamId === pendingSteamId) ?? null;

  const handleRemove = async () => {
    if (!pendingSteamId || busy) return;
    setBusy(true);
    setError(null);
    try {
      await removeAccount(pendingSteamId);
      onRemoved(pendingSteamId);
      setPendingSteamId(null);
    } catch (err: unknown) {
      if (isAppError(err)) {
        setError(`[${err.kind}] ${err.message}`);
      } else {
        setError(t("settings.removeFailed"));
      }
    } finally {
      setBusy(false);
    }
  };

  if (accounts.length === 0) {
    return (
      <section className="settings-section">
        <h2 className="settings-section-title">{t("settings.accounts")}</h2>
        <p className="settings-hint">{t("settings.noAccounts")}</p>
      </section>
    );
  }

  return (
    <section className="settings-section">
      <h2 className="settings-section-title">{t("settings.accounts")}</h2>
      <ul className="settings-account-list">
        {accounts.map((account) => (
          <li key={account.steamId} className="settings-account-row">
            <div className="settings-account-info">
              <span className="settings-account-name">{account.accountName}</span>
              <span className="settings-account-id">{account.steamId}</span>
            </div>
            {pendingSteamId === account.steamId ? (
              <div className="settings-account-confirm">
                <span className="settings-confirm-label">{t("settings.removeQ")}</span>
                <button
                  className="settings-btn settings-btn--danger settings-btn--sm"
                  onClick={() => { void handleRemove(); }}
                  disabled={busy}
                >
                  {busy ? t("settings.removing") : t("settings.removeYes")}
                </button>
                <button
                  className="settings-btn settings-btn--ghost settings-btn--sm"
                  onClick={() => { setPendingSteamId(null); setError(null); }}
                  disabled={busy}
                >
                  {t("common.cancel")}
                </button>
              </div>
            ) : (
              <button
                className="settings-btn settings-btn--danger settings-btn--sm"
                onClick={() => { setPendingSteamId(account.steamId); setError(null); }}
              >
                {t("settings.remove")}
              </button>
            )}
          </li>
        ))}
      </ul>

      {pendingAccount !== null && (
        <p className="settings-hint settings-hint--warning">
          {t("settings.removeWarn", { name: pendingAccount.accountName })}
        </p>
      )}

      {error !== null && (
        <div className="settings-error" role="alert">{error}</div>
      )}
    </section>
  );
}

// ── Auto-confirm sub-panel ─────────────────────────────────────────────────

function AutoConfirmPanel() {
  const { t } = useTranslation();
  const [interval, setIntervalValue] = useState<string>("");
  const [loaded, setLoaded] = useState(false);
  const [busy, setBusy] = useState(false);
  const [note, setNote] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    void getPollInterval()
      .then((secs) => {
        if (!cancelled) {
          setIntervalValue(String(secs));
          setLoaded(true);
        }
      })
      .catch(() => {
        if (!cancelled) setLoaded(true);
      });
    return () => {
      cancelled = true;
    };
  }, []);

  const handleSave = async (e: FormEvent) => {
    e.preventDefault();
    const parsed = Number.parseInt(interval, 10);
    if (Number.isNaN(parsed) || busy) return;
    setBusy(true);
    setNote(null);
    try {
      const stored = await setPollInterval(parsed);
      setIntervalValue(String(stored));
      setNote(t("settings.pollSaved", { secs: stored }));
    } catch {
      setNote(t("settings.pollFailed"));
    } finally {
      setBusy(false);
    }
  };

  return (
    <section className="settings-section">
      <h2 className="settings-section-title">{t("settings.autoConfirm")}</h2>
      <p className="settings-hint">
        {t("settings.pollHint", { min: MIN_POLL_INTERVAL })}
      </p>
      <form className="settings-sub-form" onSubmit={(e) => { void handleSave(e); }}>
        <div className="settings-row">
          <div className="settings-row-info">
            <span className="settings-row-label">{t("settings.pollInterval")}</span>
          </div>
          <div className="settings-row-actions settings-interval-actions">
            <input
              type="number"
              min={MIN_POLL_INTERVAL}
              className="settings-input settings-input--interval"
              value={interval}
              onChange={(e) => setIntervalValue(e.target.value)}
              disabled={busy || !loaded}
              aria-label="Poll interval in seconds"
            />
            <button
              type="submit"
              className="settings-btn settings-btn--accent settings-btn--sm"
              disabled={busy || !loaded}
            >
              {busy ? t("common.save") + "…" : t("common.save")}
            </button>
          </div>
        </div>
      </form>
      {note !== null && <div className="settings-success" role="status">{note}</div>}
    </section>
  );
}

// ── Language sub-panel ─────────────────────────────────────────────────────

function LanguagePanel() {
  const { t, i18n: i18next } = useTranslation();
  const [lang, setLang] = useState(i18next.language);
  const change = (code: string) => {
    setLang(code);
    void i18next.changeLanguage(code);
    void setSettings({ language: code });
  };
  return (
    <section className="settings-section">
      <h2 className="settings-section-title">{t("settings.language")}</h2>
      <select
        className="settings-input"
        value={lang}
        onChange={(e) => change(e.target.value)}
        aria-label={t("settings.language")}
      >
        {LANGUAGES.map((l) => (
          <option key={l.code} value={l.code}>{l.label}</option>
        ))}
      </select>
    </section>
  );
}

// ── Tray sub-panel ─────────────────────────────────────────────────────────

function TrayPanel() {
  const { t } = useTranslation();
  const [on, setOn] = useState(false);
  useEffect(() => {
    let cancelled = false;
    void getSettings().then((s) => { if (!cancelled) setOn(s.minimizeToTray); }).catch(() => {});
    return () => { cancelled = true; };
  }, []);
  const toggle = () => {
    const next = !on;
    setOn(next);
    void setSettings({ minimizeToTray: next });
  };
  return (
    <section className="settings-section">
      <div className="settings-row">
        <div className="settings-row-info">
          <span className="settings-row-label">{t("settings.minimizeToTray")}</span>
        </div>
        <div className="settings-row-actions">
          <button
            className={`settings-btn ${on ? "settings-btn--accent" : "settings-btn--ghost"}`}
            onClick={toggle}
            aria-pressed={on}
          >
            {on ? t("settings.on") : t("settings.off")}
          </button>
        </div>
      </div>
    </section>
  );
}

// ── maFiles sub-panel ──────────────────────────────────────────────────────

function MafilesPanel() {
  const { t } = useTranslation();
  const [naming, setNaming] = useState("steamid");
  const [common, setCommon] = useState(true);
  const [note, setNote] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    void getSettings()
      .then((s) => {
        if (!cancelled) {
          setNaming(s.mafileNaming);
          setCommon(s.commonMafileFormat);
        }
      })
      .catch(() => {});
    return () => { cancelled = true; };
  }, []);

  const changeNaming = (v: string) => {
    setNaming(v);
    void setSettings({ mafileNaming: v });
  };
  const toggleCommon = () => {
    const next = !common;
    setCommon(next);
    void setSettings({ commonMafileFormat: next });
  };
  const applyRename = async () => {
    try {
      await renameMafiles();
      setNote(t("settings.mafiles.renamed"));
    } catch {
      setNote(t("settings.mafiles.renameFailed"));
    }
  };

  return (
    <section className="settings-section">
      <h2 className="settings-section-title">{t("settings.mafiles.title")}</h2>
      <div className="settings-row">
        <div className="settings-row-info">
          <span className="settings-row-label">{t("settings.mafiles.naming")}</span>
        </div>
        <div className="settings-row-actions">
          <select
            className="settings-input settings-input--interval"
            style={{ width: "auto" }}
            value={naming}
            onChange={(e) => changeNaming(e.target.value)}
            aria-label={t("settings.mafiles.naming")}
          >
            <option value="steamid">{t("settings.mafiles.steamId")}</option>
            <option value="login">{t("settings.mafiles.login")}</option>
          </select>
        </div>
      </div>
      <div className="settings-row">
        <div className="settings-row-info">
          <span className="settings-row-label">{t("settings.mafiles.commonFormat")}</span>
        </div>
        <div className="settings-row-actions">
          <button
            className={`settings-btn ${common ? "settings-btn--accent" : "settings-btn--ghost"}`}
            onClick={toggleCommon}
            aria-pressed={common}
          >
            {common ? t("settings.on") : t("settings.off")}
          </button>
        </div>
      </div>
      <div className="settings-form-actions">
        <button className="settings-btn settings-btn--ghost" onClick={() => void openMafilesFolder()}>
          {t("settings.mafiles.openFolder")}
        </button>
        <button className="settings-btn settings-btn--accent" onClick={() => void applyRename()}>
          {t("settings.mafiles.applyRename")}
        </button>
      </div>
      {note !== null && <div className="settings-success" role="status">{note}</div>}
    </section>
  );
}

// ── Data directory sub-panel ───────────────────────────────────────────────

interface DataDirPanelProps {
  onChanged?: () => void;
}

function DataDirPanel({ onChanged }: DataDirPanelProps) {
  const { t } = useTranslation();
  const [currentDir, setCurrentDir] = useState<string>("");
  const [busy, setBusy] = useState(false);
  const [note, setNote] = useState<string | null>(null);
  const [isError, setIsError] = useState(false);
  const mountedRef = useRef(true);

  useEffect(() => {
    mountedRef.current = true;
    void getDataDir()
      .then((dir) => { if (mountedRef.current) setCurrentDir(dir); })
      .catch(() => {});
    return () => { mountedRef.current = false; };
  }, []);

  const handleChange = async () => {
    if (busy) return;
    const picked = await openDirPicker({ directory: true, multiple: false });
    if (!picked || typeof picked !== "string") return;
    setBusy(true);
    setNote(null);
    setIsError(false);
    try {
      const newDir = await setDataDir(picked);
      if (mountedRef.current) {
        setCurrentDir(newDir);
        setNote(t("settings.dataDir.success"));
        onChanged?.();
      }
    } catch (err: unknown) {
      if (mountedRef.current) {
        setIsError(true);
        if (isAppError(err)) {
          setNote(`[${err.kind}] ${err.message}`);
        } else {
          setNote(t("settings.dataDir.error"));
        }
      }
    } finally {
      if (mountedRef.current) setBusy(false);
    }
  };

  return (
    <section className="settings-section">
      <h2 className="settings-section-title">{t("settings.dataDir.title")}</h2>
      <p className="settings-hint">{t("settings.dataDir.hint")}</p>
      <div className="settings-row">
        <div className="settings-row-info">
          <span className="settings-row-label settings-row-label--mono">
            {currentDir || "…"}
          </span>
        </div>
        <div className="settings-row-actions">
          <button
            className="settings-btn settings-btn--ghost"
            onClick={() => { void handleChange(); }}
            disabled={busy}
          >
            {busy ? t("settings.dataDir.moving") : t("settings.dataDir.change")}
          </button>
        </div>
      </div>
      {note !== null && (
        <div className={isError ? "settings-error" : "settings-success"} role={isError ? "alert" : "status"}>
          {note}
        </div>
      )}
    </section>
  );
}

// ── Accent color sub-panel ─────────────────────────────────────────────────

function AccentPanel() {
  const { t } = useTranslation();
  const [hue, setHue] = useState(250);
  useEffect(() => {
    let cancelled = false;
    void getSettings().then((s) => { if (!cancelled) setHue(s.accentHue); }).catch(() => {});
    return () => { cancelled = true; };
  }, []);
  const onInput = (v: number) => {
    setHue(v);
    applyAccent(v); // live preview
  };
  const persist = () => { void setSettings({ accentHue: hue }); };
  return (
    <section className="settings-section">
      <h2 className="settings-section-title">{t("settings.accentColor")}</h2>
      <p className="settings-hint">{t("settings.accentHint")}</p>
      <input
        type="range"
        min={0}
        max={360}
        value={hue}
        className="accent-slider"
        onChange={(e) => onInput(Number(e.target.value))}
        onPointerUp={persist}
        onBlur={persist}
        aria-label={t("settings.accentColor")}
      />
    </section>
  );
}

// ── Main Settings panel ─────────────────────────────────────────────────────

export function Settings({
  accounts,
  encryptionEnabled,
  onEncryptionChanged,
  onAccountRemoved,
  onClose,
  onDataDirChanged,
}: SettingsProps) {
  const { t } = useTranslation();
  const [tab, setTab] = useState<"general" | "theme">("general");
  const [version, setVersion] = useState("");

  useEffect(() => { void getVersion().then(setVersion).catch(() => {}); }, []);

  // Suppress unused import warning — i18n default export is used by LanguagePanel
  void i18n;

  return (
    <div className="settings-overlay" role="dialog" aria-modal="true" aria-label={t("settings.title")}>
      {/* Backdrop */}
      <div className="settings-backdrop" onClick={onClose} />

      <div className="settings-panel">
        <div className="settings-header">
          <h1 className="settings-title">{t("settings.title")}</h1>
          <button
            className="settings-close-btn"
            onClick={onClose}
            aria-label={t("common.close")}
          >
            <Icon name="close" size={15} stroke={2} />
          </button>
        </div>

        <div className="settings-tabs" role="tablist">
          <button role="tab" aria-selected={tab === "general"} className={`settings-tab${tab === "general" ? " settings-tab--active" : ""}`} onClick={() => setTab("general")}>{t("settings.tabGeneral")}</button>
          <button role="tab" aria-selected={tab === "theme"} className={`settings-tab${tab === "theme" ? " settings-tab--active" : ""}`} onClick={() => setTab("theme")}>{t("settings.tabTheme")}</button>
        </div>

        <div className="settings-body">
          {tab === "general" ? (
            <>
              <LanguagePanel />
              <TrayPanel />
              <MafilesPanel />
              <DataDirPanel onChanged={onDataDirChanged} />
              <AutoConfirmPanel />
              <EncryptionPanel encryptionEnabled={encryptionEnabled} onChanged={onEncryptionChanged} />
              <RemoveAccountPanel accounts={accounts} onRemoved={onAccountRemoved} />
              <section className="settings-section settings-section--about">
                <h2 className="settings-section-title">{t("settings.about")}</h2>
                <div className="settings-about-row">
                  <span className="settings-row-label">StewardAuth</span>
                  <span className="settings-version">v{version}</span>
                </div>
              </section>
            </>
          ) : (
            <AccentPanel />
          )}
        </div>
      </div>
    </div>
  );
}
