// SPDX-FileCopyrightText: 2026 shinyawave
// SPDX-License-Identifier: AGPL-3.0-or-later

import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { listLoginApprovals, login, respondLoginApproval } from "../api";
import { isAppError } from "../types";
import type { PendingLogin } from "../types";
import { Icon } from "./icons";

interface Props {
  steamId: string;
  accountName: string;
}

type Status = "idle" | "loading" | "session-expired" | "error" | "ready";

export function ConfirmLogin({ steamId, accountName }: Props) {
  const { t } = useTranslation();
  const [status, setStatus] = useState<Status>("idle");
  const [items, setItems] = useState<PendingLogin[]>([]);
  const [errorMsg, setErrorMsg] = useState<string | null>(null);
  const [working, setWorking] = useState<Set<string>>(new Set());
  const [password, setPassword] = useState("");
  const [loginError, setLoginError] = useState<string | null>(null);
  const [loginWorking, setLoginWorking] = useState(false);

  const genRef = useRef(0);

  const load = useCallback(async () => {
    const gen = ++genRef.current;
    setStatus("loading");
    setErrorMsg(null);
    try {
      const data = await listLoginApprovals(steamId);
      if (gen !== genRef.current) return; // stale
      setItems(data);
      setStatus("ready");
    } catch (e: unknown) {
      if (gen !== genRef.current) return; // stale
      if (isAppError(e) && e.kind === "SessionExpired") {
        setStatus("session-expired");
      } else if (isAppError(e)) {
        setErrorMsg(`[${e.kind}] ${e.message}`);
        setStatus("error");
      } else {
        setErrorMsg(t("confirmLogin.fetchError"));
        setStatus("error");
      }
    }
  }, [steamId, t]);

  // Reset to the unloaded state on account change; invalidate in-flight loads.
  useEffect(() => {
    genRef.current++;
    setStatus("idle");
    setItems([]);
    setErrorMsg(null);
  }, [steamId]);

  const handleLogin = useCallback(async () => {
    setLoginWorking(true);
    setLoginError(null);
    try {
      await login(steamId, accountName, password);
      setPassword("");
      await load();
    } catch (e: unknown) {
      if (isAppError(e)) {
        setLoginError(`[${e.kind}] ${e.message}`);
      } else {
        setLoginError(t("confirmLogin.loginFailed"));
      }
    } finally {
      setLoginWorking(false);
    }
  }, [steamId, accountName, password, load, t]);

  const respond = useCallback(
    async (clientId: string, approve: boolean) => {
      setWorking((prev) => new Set([...prev, clientId]));
      try {
        await respondLoginApproval(steamId, clientId, approve);
        // Optimistically drop the responded row; do not re-fetch.
        setItems((prev) => prev.filter((p) => p.clientId !== clientId));
      } catch (e: unknown) {
        if (isAppError(e)) {
          setErrorMsg(`[${e.kind}] ${e.message}`);
        } else {
          setErrorMsg(t("confirmLogin.respondError"));
        }
        setStatus("error");
      } finally {
        setWorking((prev) => {
          const next = new Set(prev);
          next.delete(clientId);
          return next;
        });
      }
    },
    [steamId, t],
  );

  // ── Render ────────────────────────────────────────────────────────────────

  if (status === "idle") {
    return (
      <div className="conf-panel">
        <div className="conf-load-cta">
          <button
            type="button"
            className="conf-load-btn"
            onClick={() => void load()}
          >
            {t("confirmLogin.loadButton")}
          </button>
          <p className="conf-load-hint">{t("confirmLogin.loadHint")}</p>
        </div>
      </div>
    );
  }

  if (status === "loading") {
    return (
      <div className="conf-panel">
        <div className="conf-state">
          <span className="conf-spinner" aria-hidden="true" />
          <p className="conf-loading">{t("confirmLogin.loading")}</p>
        </div>
      </div>
    );
  }

  if (status === "session-expired") {
    return (
      <div className="conf-panel">
        <div className="conf-login-prompt">
          <p className="conf-login-title">{t("conf.sessionTitle")}</p>
          <div className="conf-login-fields">
            <input
              className="conf-input conf-input--locked"
              type="text"
              value={accountName}
              readOnly
              disabled
              aria-label="Steam username (locked to the selected account)"
              autoComplete="username"
            />
            <input
              className="conf-input"
              type="password"
              placeholder={t("conf.password")}
              value={password}
              onChange={(e) => setPassword(e.target.value)}
              disabled={loginWorking}
              autoComplete="current-password"
              autoFocus
              onKeyDown={(e) => {
                if (e.key === "Enter" && !loginWorking) void handleLogin();
              }}
            />
          </div>
          {loginError !== null && (
            <p className="conf-error-inline">{loginError}</p>
          )}
          <button
            className="conf-btn conf-btn--accent"
            onClick={() => void handleLogin()}
            disabled={loginWorking || password === ""}
          >
            {loginWorking ? t("conf.signingIn") : t("conf.signIn")}
          </button>
        </div>
      </div>
    );
  }

  if (status === "error") {
    return (
      <div className="conf-panel">
        <div className="conf-load-cta">
          <button
            type="button"
            className="conf-load-btn"
            onClick={() => void load()}
          >
            {t("confirmLogin.loadButton")}
          </button>
          <p className="conf-error-msg">{errorMsg}</p>
        </div>
      </div>
    );
  }

  // status === "ready"
  return (
    <div className="conf-panel">
      <div className="conf-header">
        <span className="conf-header-title">
          {items.length === 0
            ? t("confirmLogin.none")
            : t("confirmLogin.pending", { count: items.length })}
        </span>
        <div className="conf-header-actions">
          <button
            className="conf-btn conf-btn--icon"
            onClick={() => void load()}
            disabled={working.size > 0 || loginWorking}
            title={t("confirmLogin.refresh")}
            aria-label={t("confirmLogin.refresh")}
          >
            <Icon name="refresh" size={14} />
          </button>
        </div>
      </div>

      {items.length === 0 ? (
        <div className="conf-state">
          <span className="conf-empty-icon" aria-hidden="true">
            <Icon name="check-circle" size={30} stroke={1.6} />
          </span>
          <div className="conf-empty">{t("confirmLogin.none")}</div>
        </div>
      ) : (
        <ul className="conf-list">
          {items.map((item) => {
            const isWorking = working.has(item.clientId);
            return (
              <li key={item.clientId} className="conf-row">
                <span className="conf-row-glyph" aria-hidden="true">
                  <Icon name="key" size={16} />
                </span>
                <div className="conf-row-body">
                  <div className="conf-row-kind">
                    {item.device !== "" ? item.device : t("confirmLogin.unknownDevice")}
                  </div>
                  <div className="conf-row-title">
                    {item.platform}
                    {item.location !== "" ? ` · ${item.location}` : ""}
                  </div>
                  {item.ip !== "" && (
                    <div className="conf-row-summary">{item.ip}</div>
                  )}
                </div>
                <div className="conf-row-actions">
                  <button
                    className="conf-icon-btn conf-icon-btn--accept"
                    onClick={() => void respond(item.clientId, true)}
                    disabled={isWorking}
                    aria-label={t("confirmLogin.approve")}
                    title={t("confirmLogin.approve")}
                  >
                    <Icon name="check" size={13} stroke={2.4} />
                  </button>
                  <button
                    className="conf-icon-btn conf-icon-btn--deny"
                    onClick={() => void respond(item.clientId, false)}
                    disabled={isWorking}
                    aria-label={t("confirmLogin.deny")}
                    title={t("confirmLogin.deny")}
                  >
                    <Icon name="close" size={12} stroke={2.4} />
                  </button>
                </div>
              </li>
            );
          })}
        </ul>
      )}
    </div>
  );
}
