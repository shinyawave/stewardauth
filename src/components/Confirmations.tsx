// SPDX-FileCopyrightText: 2026 shinyawave
// SPDX-License-Identifier: AGPL-3.0-or-later

import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { fetchConfirmations, login, respondConfirmation } from "../api";
import { isAppError } from "../types";
import type { ConfirmationItem } from "../types";
import { Icon } from "./icons";

interface Props {
  steamId: string;
  accountName: string;
}

type Status = "idle" | "loading" | "session-expired" | "error" | "ready";

export function Confirmations({ steamId, accountName }: Props) {
  const { t } = useTranslation();
  const [status, setStatus] = useState<Status>("idle");
  const [items, setItems] = useState<ConfirmationItem[]>([]);
  const [errorMsg, setErrorMsg] = useState<string | null>(null);
  const [working, setWorking] = useState<Set<string>>(new Set());
  const [batchWorking, setBatchWorking] = useState(false);
  const [password, setPassword] = useState("");
  const [loginError, setLoginError] = useState<string | null>(null);
  const [loginWorking, setLoginWorking] = useState(false);

  const genRef = useRef(0);

  const load = useCallback(async () => {
    const gen = ++genRef.current;
    setStatus("loading");
    setErrorMsg(null);
    try {
      const data = await fetchConfirmations(steamId);
      if (gen !== genRef.current) return; // stale — a newer load started
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
        setErrorMsg(t("conf.fetchError"));
        setStatus("error");
      }
    }
  }, [steamId, t]);

  // Confirmations load ONLY on explicit request (the "Load confirmations" button,
  // or after an accept/deny) — never automatically on mount or account switch.
  // Steam rate-limits the mobileconf "getlist" endpoint, and auto-fetching on
  // every account selection trips it ([SteamError] Rate limit exceeded). This
  // mirrors the standard Steam mobile-confirmation flow (fetch on open + Refresh +
  // after an action). On account change we drop any stale list and return to the
  // unloaded state, invalidating any in-flight load from the previous account.
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
      // The Steam login username is the account name (from the maFile); it is
      // fixed for the selected account and not user-editable.
      await login(steamId, accountName, password);
      setPassword("");
      await load();
    } catch (e: unknown) {
      if (isAppError(e)) {
        setLoginError(`[${e.kind}] ${e.message}`);
      } else {
        setLoginError(t("conf.loginFailed"));
      }
    } finally {
      setLoginWorking(false);
    }
  }, [steamId, accountName, password, load, t]);

  const respond = useCallback(
    async (ids: string[], accept: boolean) => {
      if (ids.length === 0) return;
      const isBatch = ids.length > 1 || ids.length === items.length;
      if (isBatch) {
        setBatchWorking(true);
      } else {
        setWorking((prev) => new Set([...prev, ...ids]));
      }
      try {
        await respondConfirmation(steamId, ids, accept);
        // Optimistically drop the responded rows instead of re-fetching. A
        // reload here would fire a 3rd getlist per action and trip Steam's
        // mobileconf rate limit; the backend already knows the outcome.
        const done = new Set(ids);
        setItems((prev) => prev.filter((c) => !done.has(c.id)));
      } catch (e: unknown) {
        if (isAppError(e)) {
          setErrorMsg(`[${e.kind}] ${e.message}`);
        } else {
          setErrorMsg(t("conf.respondError"));
        }
        setStatus("error");
      } finally {
        if (isBatch) {
          setBatchWorking(false);
        } else {
          setWorking((prev) => {
            const next = new Set(prev);
            ids.forEach((id) => next.delete(id));
            return next;
          });
        }
      }
    },
    [steamId, items.length, load, t],
  );

  // ── Render sub-UIs ────────────────────────────────────────────────────────

  // Unloaded — wait for an explicit request (avoids Steam's getlist rate limit).
  if (status === "idle") {
    return (
      <div className="conf-panel">
        <div className="conf-load-cta">
          <button
            type="button"
            className="conf-load-btn"
            onClick={() => void load()}
          >
            {t("conf.loadButton")}
          </button>
          <p className="conf-load-hint">
            {t("conf.loadHint")}
          </p>
        </div>
      </div>
    );
  }

  if (status === "loading") {
    return (
      <div className="conf-panel">
        <div className="conf-state">
          <span className="conf-spinner" aria-hidden="true" />
          <p className="conf-loading">{t("conf.loading")}</p>
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

  // Error keeps the SAME "Load confirmations" button (it must not disappear) and
  // shows the error inline. One click = exactly one getlist request; nothing
  // auto-retries, so the user stays in control of how often Steam is hit.
  if (status === "error") {
    return (
      <div className="conf-panel">
        <div className="conf-load-cta">
          <button
            type="button"
            className="conf-load-btn"
            onClick={() => void load()}
          >
            {t("conf.loadButton")}
          </button>
          <p className="conf-error-msg">{errorMsg}</p>
        </div>
      </div>
    );
  }

  // status === "ready"
  const allIds = items.map((c) => c.id);

  return (
    <div className="conf-panel">
      <div className="conf-header">
        <span className="conf-header-title">
          {items.length === 0
            ? t("conf.none")
            : t("conf.pending", { count: items.length })}
        </span>
        <div className="conf-header-actions">
          <button
            className="conf-btn conf-btn--icon"
            onClick={() => void load()}
            disabled={batchWorking || working.size > 0 || loginWorking}
            title="Refresh"
            aria-label="Refresh"
          >
            <Icon name="refresh" size={14} />
          </button>
          {items.length > 0 && (
            <>
              <button
                className="conf-btn conf-btn--danger"
                onClick={() => void respond(allIds, false)}
                disabled={batchWorking}
              >
                {batchWorking ? t("conf.working") : t("conf.denyAll")}
              </button>
              <button
                className="conf-btn conf-btn--accept"
                onClick={() => void respond(allIds, true)}
                disabled={batchWorking}
              >
                {batchWorking ? t("conf.working") : t("conf.acceptAll")}
              </button>
            </>
          )}
        </div>
      </div>

      {items.length === 0 ? (
        <div className="conf-state">
          <span className="conf-empty-icon" aria-hidden="true">
            <Icon name="check-circle" size={30} stroke={1.6} />
          </span>
          <div className="conf-empty">{t("conf.none")}</div>
        </div>
      ) : (
        <ul className="conf-list">
          {items.map((item) => {
            const isWorking = working.has(item.id);
            return (
              <li key={item.id} className="conf-row">
                {item.icon != null && item.icon !== "" ? (
                  <img
                    className="conf-row-icon"
                    src={item.icon}
                    alt=""
                    aria-hidden="true"
                  />
                ) : (
                  <span
                    className={`conf-row-dot conf-row-dot--${item.category}`}
                    aria-hidden="true"
                  />
                )}
                <div className="conf-row-body">
                  <div className="conf-row-kind">{item.kind}</div>
                  <div className="conf-row-title">{item.title}</div>
                  {item.summary !== "" && (
                    <div className="conf-row-summary">{item.summary}</div>
                  )}
                </div>
                <div className="conf-row-actions">
                  <button
                    className="conf-icon-btn conf-icon-btn--accept"
                    onClick={() => void respond([item.id], true)}
                    disabled={isWorking || batchWorking}
                    aria-label={t("conf.accept")}
                    title={t("conf.accept")}
                  >
                    <Icon name="check" size={13} stroke={2.4} />
                  </button>
                  <button
                    className="conf-icon-btn conf-icon-btn--deny"
                    onClick={() => void respond([item.id], false)}
                    disabled={isWorking || batchWorking}
                    aria-label={t("conf.deny")}
                    title={t("conf.deny")}
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
