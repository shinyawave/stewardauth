// SPDX-FileCopyrightText: 2026 shinyawave
// SPDX-License-Identifier: AGPL-3.0-or-later

import { useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { save } from "@tauri-apps/plugin-dialog";
import { Icon } from "./icons";
import {
  linkBeginLogin,
  linkSubmitEmailGuard,
  linkStart,
  linkSetPhone,
  linkAwaitPhoneEmail,
  linkSendSms,
  linkFinalize,
  linkCancel,
  linkSaveRevocation,
} from "../api";
import type { Proxy } from "../types";

type Step =
  | "login"
  | "emailGuard"
  | "phone"
  | "confirmPhoneEmail"
  | "sms"
  | "email"
  | "done";

interface Props {
  proxies: Proxy[];
  onClose: () => void;
}

// Which of the 6 visible steps we're on (login/emailGuard share the auth phase).
const STEP_NUMBER: Record<Step, number> = {
  login: 1,
  emailGuard: 2,
  phone: 3,
  confirmPhoneEmail: 4,
  sms: 5,
  email: 5,
  done: 6,
};

// Segmented OTP entry (email / SMS codes) — a hidden input feeds N display boxes.
function OtpBoxes({
  value,
  onChange,
  length = 5,
}: {
  value: string;
  onChange: (v: string) => void;
  length?: number;
}) {
  const ref = useRef<HTMLInputElement>(null);
  const chars = value.toUpperCase().slice(0, length).split("");
  const activeIndex = chars.length < length ? chars.length : -1;
  return (
    <div className="link-code-field" onClick={() => ref.current?.focus()}>
      <input
        ref={ref}
        className="link-code-input"
        value={value}
        maxLength={length}
        autoFocus
        autoComplete="one-time-code"
        onChange={(e) => onChange(e.target.value.toUpperCase().slice(0, length))}
      />
      <div className="link-code-boxes" aria-hidden="true">
        {Array.from({ length }).map((_, i) => (
          <span
            key={i}
            className={`link-code-box${i === activeIndex ? " link-code-box--active" : ""}`}
          >
            {chars[i] ?? ""}
          </span>
        ))}
      </div>
    </div>
  );
}

export function AddAuthenticatorWizard({ proxies, onClose }: Props) {
  const { t } = useTranslation();
  const [step, setStep] = useState<Step>("login");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // form fields
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [proxyId, setProxyId] = useState<string>("");
  const [emailCode, setEmailCode] = useState("");
  const [phone, setPhone] = useState("");
  const [code, setCode] = useState("");

  // link results
  const [phoneHint, setPhoneHint] = useState("");
  const [revocation, setRevocation] = useState("");
  const [saved, setSaved] = useState(false);
  const [copied, setCopied] = useState(false);

  const fail = (_e: unknown) => setError(t("link.failed"));

  // Advance from login/emailGuard into the AddAuthenticator step.
  const runStart = async () => {
    const r = await linkStart();
    if (r.status === "code") {
      setPhoneHint(r.phoneHint ?? "");
      setStep(r.confirmType === "sms" ? "sms" : "email");
    } else if (r.status === "need_phone") {
      setStep("phone");
    } else if (r.status === "already_linked") {
      setError(t("link.alreadyLinked"));
    } else {
      setError(t("link.rateLimited"));
    }
  };

  const submitLogin = async () => {
    setBusy(true);
    setError(null);
    try {
      const r = await linkBeginLogin(username, password, proxyId || undefined);
      if (r.status === "bad_credentials") setError(t("link.badCredentials"));
      else if (r.status === "rate_limited") setError(t("link.rateLimited"));
      else if (r.needsEmailGuard) setStep("emailGuard");
      else await runStart();
    } catch (e) {
      fail(e);
    } finally {
      setBusy(false);
    }
  };

  const submitEmailGuard = async () => {
    setBusy(true);
    setError(null);
    try {
      const r = await linkSubmitEmailGuard(emailCode);
      if (r.status === "bad_code") setError(t("link.badCode"));
      else if (r.status === "rate_limited") setError(t("link.rateLimited"));
      else await runStart();
    } catch (e) {
      fail(e);
    } finally {
      setBusy(false);
    }
  };

  const submitPhone = async () => {
    setBusy(true);
    setError(null);
    try {
      if (!phone.trim().startsWith("+")) {
        setError(t("link.invalidPhone"));
        return;
      }
      const r = await linkSetPhone(phone);
      if (r.status === "ok") setStep("confirmPhoneEmail");
      else setError(t("link.failed"));
    } catch (e) {
      fail(e);
    } finally {
      setBusy(false);
    }
  };

  const confirmPhoneContinue = async () => {
    setBusy(true);
    setError(null);
    try {
      const a = await linkAwaitPhoneEmail();
      if (a.stillWaiting) {
        setError(t("link.confirmPhoneEmailBody"));
        return;
      }
      const s = await linkSendSms();
      if (s.status !== "ok") {
        setError(t("link.failed"));
        return;
      }
      await runStart();
    } catch (e) {
      fail(e);
    } finally {
      setBusy(false);
    }
  };

  const submitCode = async () => {
    setBusy(true);
    setError(null);
    try {
      const r = await linkFinalize(code);
      if (r.status === "done") {
        setRevocation(r.revocationCode ?? "");
        setStep("done");
      } else if (r.status === "wrong_code") {
        setError(t("link.badCode"));
      } else {
        setError(t("link.timeSyncFailed"));
      }
    } catch (e) {
      fail(e);
    } finally {
      setBusy(false);
    }
  };

  const copyRevocation = async () => {
    await navigator.clipboard.writeText(revocation);
    setCopied(true);
    setTimeout(() => setCopied(false), 1500);
  };

  const saveTxt = async () => {
    try {
      const path = await save({ defaultPath: "revocation-code.txt" });
      if (!path) return;
      await linkSaveRevocation(path, revocation);
    } catch {
      /* user cancelled or write failed — ignore */
    }
  };

  const cancel = () => {
    void linkCancel().catch(() => {});
    onClose();
  };

  return (
    <div className="settings-overlay" role="dialog" aria-modal="true" aria-label={t("link.title")}>
      <div className="settings-backdrop" onClick={step === "done" ? undefined : cancel} />
      <div className="settings-panel link-panel">
        <div className="settings-header">
          <h1 className="settings-title">{t("link.title")}</h1>
          <span className="link-step-counter">{STEP_NUMBER[step]} / 6</span>
        </div>

        <div className="settings-body link-body">
          {step === "login" && (
            <>
              <p className="settings-hint">{t("link.signIn")}</p>
              <input className="settings-input" placeholder={t("link.username")} value={username}
                onChange={(e) => setUsername(e.target.value)} autoFocus />
              <input className="settings-input" type="password" placeholder={t("link.password")} value={password}
                onChange={(e) => setPassword(e.target.value)} />
              <select className="settings-input" value={proxyId} onChange={(e) => setProxyId(e.target.value)}>
                <option value="">{t("link.proxyNone")}</option>
                {proxies.map((p) => (
                  <option key={p.id} value={p.id}>{p.host}:{p.port}</option>
                ))}
              </select>
            </>
          )}

          {step === "emailGuard" && (
            <>
              <p className="settings-hint">{t("link.emailGuardHint")}</p>
              <OtpBoxes value={emailCode} onChange={setEmailCode} />
            </>
          )}

          {step === "phone" && (
            <>
              <p className="settings-hint">{t("link.phoneTitle")}</p>
              <input className="settings-input" placeholder={t("link.phonePlaceholder")} value={phone}
                onChange={(e) => setPhone(e.target.value)} autoFocus />
            </>
          )}

          {step === "confirmPhoneEmail" && (
            <div className="link-center-step">
              <Icon name="mail" size={30} stroke={1.6} />
              <p className="settings-hint">{t("link.confirmPhoneEmailTitle")}</p>
              <p className="settings-hint">{t("link.confirmPhoneEmailBody")}</p>
            </div>
          )}

          {(step === "sms" || step === "email") && (
            <>
              <p className="settings-hint">
                {step === "sms" ? t("link.smsTitle") : t("link.emailCodeTitle")}
              </p>
              {step === "sms" && phoneHint && (
                <p className="settings-hint">{t("link.smsHint", { hint: phoneHint })}</p>
              )}
              <OtpBoxes value={code} onChange={setCode} />
            </>
          )}

          {step === "done" && (
            <>
              <p className="settings-hint">{t("link.doneTitle")}</p>
              <label className="link-revocation-label">{t("link.revocationLabel")}</label>
              <div className="link-revocation-code">{revocation}</div>
              <div className="settings-form-actions">
                <button className="settings-btn settings-btn--ghost" onClick={() => void copyRevocation()}>
                  {copied ? t("link.copied") : t("link.copy")}
                </button>
                <button className="settings-btn settings-btn--ghost" onClick={() => void saveTxt()}>
                  {t("link.saveTxt")}
                </button>
              </div>
              <p className="link-revocation-warn">{t("link.revocationWarn")}</p>
              <label className="link-saved-check">
                <input type="checkbox" checked={saved} onChange={(e) => setSaved(e.target.checked)} />
                {t("link.savedCheckbox")}
              </label>
            </>
          )}

          {error !== null && <div className="unlock-error" role="alert">{error}</div>}
        </div>

        <div className="settings-form-actions link-actions">
          {step === "login" && (
            <button className="settings-btn settings-btn--accent" disabled={busy || !username || !password}
              onClick={() => void submitLogin()}>
              {busy ? t("link.working") : t("link.next")}
            </button>
          )}
          {step === "emailGuard" && (
            <button className="settings-btn settings-btn--accent" disabled={busy || !emailCode}
              onClick={() => void submitEmailGuard()}>
              {busy ? t("link.working") : t("link.next")}
            </button>
          )}
          {step === "phone" && (
            <button className="settings-btn settings-btn--accent" disabled={busy || !phone}
              onClick={() => void submitPhone()}>
              {busy ? t("link.working") : t("link.next")}
            </button>
          )}
          {step === "confirmPhoneEmail" && (
            <button className="settings-btn settings-btn--accent" disabled={busy}
              onClick={() => void confirmPhoneContinue()}>
              {busy ? t("link.working") : t("link.continue")}
            </button>
          )}
          {(step === "sms" || step === "email") && (
            <button className="settings-btn settings-btn--accent" disabled={busy || !code}
              onClick={() => void submitCode()}>
              {busy ? t("link.working") : t("link.next")}
            </button>
          )}
          {step === "done" && (
            <button className="settings-btn settings-btn--accent" disabled={!saved} onClick={onClose}>
              {t("link.finish")}
            </button>
          )}
        </div>
      </div>
    </div>
  );
}
