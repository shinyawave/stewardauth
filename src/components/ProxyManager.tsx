// SPDX-FileCopyrightText: 2026 shinyawave
// SPDX-License-Identifier: AGPL-3.0-or-later

import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  addProxies,
  assignProxiesByText,
  bulkAssignProxy,
  checkProxy,
  deleteProxy,
  distributeProxies,
  getDefaultProxy,
  listAccounts,
  listProxies,
  setDefaultProxy,
  setProxyFavorite,
} from "../api";
import type { AccountSummary, Proxy } from "../types";
import { Icon } from "./icons";

interface ProxyManagerProps {
  onClose: () => void;
  selectedSteamIds: string[];
  onProxiesChanged: () => void;
}

type CheckState = "idle" | "checking" | "ok" | "fail";

export function ProxyManager({ onClose, selectedSteamIds, onProxiesChanged }: ProxyManagerProps) {
  const { t } = useTranslation();
  const [proxies, setProxies] = useState<Proxy[]>([]);
  const [accounts, setAccounts] = useState<AccountSummary[]>([]);
  const [def, setDef] = useState("");
  const [showProto, setShowProto] = useState(false);
  const [showCreds, setShowCreds] = useState(false);
  const [input, setInput] = useState("");
  const [assignText, setAssignText] = useState("");
  const [note, setNote] = useState<string | null>(null);
  const [checks, setChecks] = useState<Record<string, { state: CheckState; ms?: number }>>({});

  const hasSelection = selectedSteamIds.length > 0;

  const refresh = () => {
    void listProxies().then(setProxies).catch(() => {});
    void getDefaultProxy().then(setDef).catch(() => {});
    void listAccounts().then(setAccounts).catch(() => {});
  };
  useEffect(() => { refresh(); }, []);

  // proxy id → number of accounts assigned to it.
  const countByProxy = useMemo(() => {
    const m: Record<string, number> = {};
    for (const a of accounts) {
      if (a.proxy) m[a.proxy] = (m[a.proxy] ?? 0) + 1;
    }
    return m;
  }, [accounts]);

  const display = (p: Proxy) => {
    let s = showProto ? `${p.scheme}://` : "";
    s += `${p.host}:${p.port}`;
    if (showCreds && p.user) s += `  ${p.user}:${p.pass}`;
    return s;
  };

  const add = async () => {
    if (input.trim() === "") return;
    await addProxies(input).catch(() => {});
    setInput("");
    refresh();
  };

  const runCheck = async (p: Proxy) => {
    setChecks((c) => ({ ...c, [p.id]: { state: "checking" } }));
    try {
      const r = await checkProxy(p.id);
      setChecks((c) => ({
        ...c,
        [p.id]: { state: r.ok ? "ok" : "fail", ms: r.latencyMs ?? undefined },
      }));
    } catch {
      setChecks((c) => ({ ...c, [p.id]: { state: "fail" } }));
    }
  };

  const checkGlyph = (id: string) => {
    const c = checks[id];
    if (!c || c.state === "idle") return "◇";
    if (c.state === "checking") return "…";
    if (c.state === "ok") return c.ms != null ? `✓ ${c.ms}ms` : "✓";
    return "✗";
  };

  const doBulkAssign = async (proxyId: string) => {
    if (!hasSelection) return;
    try {
      await bulkAssignProxy(selectedSteamIds, proxyId);
      setNote(t("proxy.distributeDone", { count: selectedSteamIds.length }));
      refresh();
      onProxiesChanged();
    } catch {
      setNote(t("proxy.distributeNone"));
    }
  };

  const doDistribute = async () => {
    if (!hasSelection) return;
    try {
      await distributeProxies(selectedSteamIds);
      setNote(t("proxy.distributeDone", { count: selectedSteamIds.length }));
      refresh();
      onProxiesChanged();
    } catch {
      setNote(t("proxy.distributeNone"));
    }
  };

  const doAssignByText = async () => {
    if (assignText.trim() === "") return;
    try {
      const r = await assignProxiesByText(assignText);
      setNote(t("proxy.assignReport", { assigned: r.assigned, unmatched: r.unmatched.length }));
      setAssignText("");
      refresh();
      onProxiesChanged();
    } catch {
      setNote(null);
    }
  };

  return (
    <div className="settings-overlay" role="dialog" aria-modal="true" aria-label={t("proxy.title")}>
      <div className="settings-backdrop" onClick={onClose} />
      <div className="settings-panel proxy-panel">
        <div className="settings-header">
          <h1 className="settings-title">{t("proxy.title")}</h1>
          <button className="settings-close-btn" onClick={onClose} aria-label={t("common.close")}>
            <Icon name="close" size={15} stroke={2} />
          </button>
        </div>

        <div className="settings-body">
          <section className="settings-section">
            <div className="proxy-default">
              {t("proxy.default")}: <strong>{def || t("proxy.none")}</strong>
            </div>
            <label className="proxy-toggle">
              <input type="checkbox" checked={showProto} onChange={(e) => setShowProto(e.target.checked)} />
              {t("proxy.showProtocol")}
            </label>
            <label className="proxy-toggle">
              <input type="checkbox" checked={showCreds} onChange={(e) => setShowCreds(e.target.checked)} />
              {t("proxy.showLogin")}
            </label>
            {hasSelection && (
              <button className="settings-btn settings-btn--accent" onClick={() => void doDistribute()}>
                {t("proxy.distribute")}
              </button>
            )}
          </section>

          <section className="settings-section">
            {proxies.length === 0 ? (
              <p className="settings-hint">{t("proxy.empty")}</p>
            ) : (
              <ul className="proxy-list">
                {proxies.map((p, i) => (
                  <li key={p.id} className="proxy-row">
                    <span className="proxy-index">{i}:</span>
                    <span className="proxy-url" title={p.id}>{display(p)}</span>
                    <span className="proxy-badge" title={t("proxy.badgeAccounts", { count: countByProxy[p.id] ?? 0 })}>
                      ({countByProxy[p.id] ?? 0})
                    </span>
                    {hasSelection && (
                      <button
                        className="proxy-bulk"
                        onClick={() => void doBulkAssign(p.id)}
                        title={t("proxy.bulkAssign", { count: selectedSteamIds.length })}
                      >
                        {t("proxy.bulkAssign", { count: selectedSteamIds.length })}
                      </button>
                    )}
                    <button
                      className={`proxy-check proxy-check--${checks[p.id]?.state ?? "idle"}`}
                      onClick={() => void runCheck(p)}
                      title={t("proxy.check")}
                    >
                      {checkGlyph(p.id)}
                    </button>
                    <button
                      className={`proxy-fav${p.favorite ? " proxy-fav--on" : ""}`}
                      onClick={() => { void setProxyFavorite(p.id, !p.favorite).then(refresh); }}
                      title={t("proxy.favorite")}
                      aria-pressed={p.favorite}
                    >
                      <Icon name="heart" size={14} filled={p.favorite} stroke={1.5} />
                    </button>
                    <button
                      className={`proxy-default-btn${def === p.id ? " proxy-default-btn--on" : ""}`}
                      onClick={() => { void setDefaultProxy(def === p.id ? "" : p.id).then(refresh); }}
                      title={t("proxy.setDefault")}
                    >
                      <Icon name="star" size={14} filled={def === p.id} stroke={1.5} />
                    </button>
                    <button
                      className="proxy-del"
                      onClick={() => { void deleteProxy(p.id).then(() => { refresh(); onProxiesChanged(); }); }}
                      title={t("proxy.delete")}
                    >
                      <Icon name="trash" size={14} />
                    </button>
                  </li>
                ))}
              </ul>
            )}
          </section>

          <section className="settings-section">
            <textarea
              className="proxy-input"
              placeholder={"IP:PORT\nIP:PORT:USER:PASS\nPROTOCOL://IP:PORT:USER:PASS\nIP:PORT:USER:PASS{ID}"}
              value={input}
              onChange={(e) => setInput(e.target.value)}
              rows={4}
            />
            <div className="settings-form-actions">
              <button className="settings-btn settings-btn--ghost" onClick={() => setInput("")}>
                {t("proxy.clear")}
              </button>
              <button className="settings-btn settings-btn--accent" onClick={() => void add()}>
                {t("proxy.add")}
              </button>
            </div>
          </section>

          <section className="settings-section">
            <div className="proxy-default">{t("proxy.assignByText")}</div>
            <textarea
              className="proxy-input"
              placeholder={t("proxy.assignByTextPlaceholder")}
              value={assignText}
              onChange={(e) => setAssignText(e.target.value)}
              rows={4}
            />
            <div className="settings-form-actions">
              <button className="settings-btn settings-btn--accent" onClick={() => void doAssignByText()}>
                {t("proxy.assignByTextApply")}
              </button>
            </div>
          </section>

          {note !== null && <p className="settings-hint">{note}</p>}
        </div>
      </div>
    </div>
  );
}
