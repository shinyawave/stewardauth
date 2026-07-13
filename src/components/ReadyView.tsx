// SPDX-FileCopyrightText: 2026 shinyawave
// SPDX-License-Identifier: AGPL-3.0-or-later

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { save } from "@tauri-apps/plugin-dialog";
import {
  addToGroup,
  assignProxy,
  createGroup,
  exportMafile,
  exportMafiles,
  listGroups,
  listProxies,
  removeAccount,
  removeFromGroup,
  setAutoConfirm,
  unpinProxy,
} from "../api";
import { useCode } from "../hooks/useCode";
import { useImportPaths } from "../hooks/useImportPaths";
import type { AccountSummary, Group, Proxy } from "../types";
import { AccountContextMenu } from "./AccountContextMenu";
import { AccountList } from "./AccountList";
import { AccountMenu } from "./AccountMenu";
import { AddAuthenticatorWizard } from "./AddAuthenticatorWizard";
import { GroupsDropdown } from "./GroupsDropdown";
import { Icon } from "./icons";
import { ImportFeedback } from "./ImportFeedback";
import { ProxyManager } from "./ProxyManager";
import { Confirmations } from "./Confirmations";
import { ConfirmLogin } from "./ConfirmLogin";
import { MenuDropdown } from "./MenuDropdown";
import { Settings } from "./Settings";
import { StatusBar } from "./StatusBar";
import { Toolbar } from "./Toolbar";

interface ReadyViewProps {
  accounts: AccountSummary[];
  selectedSteamId: string | null;
  onSelect: (steamId: string) => void;
  refreshing: boolean;
  onRefresh: () => void;
  loadError: string | null;
  encryptionEnabled: boolean;
  onEncryptionChanged: (enabled: boolean) => void;
  onImported: (steamId: string) => void;
  onImportedMany: () => void;
  onAccountRemoved: (steamId: string) => void;
  onDataDirChanged?: () => void;
  onCheckForUpdate: () => void;
  updateCheckStatus: import("../hooks/useUpdater").UpdaterStatus;
}

interface ConfirmedEvent {
  steamId: string;
  count: number;
  market: number;
  trade: number;
}

/**
 * The main "ready" screen: toolbar + sidebar + merged code/confirmations pane +
 * status bar. Owns the multi-selection and auto-confirm toggling; `useCode` runs
 * here so the shared code result feeds both the toolbar countdown and the header.
 */
export function ReadyView({
  accounts,
  selectedSteamId,
  onSelect,
  refreshing,
  onRefresh,
  loadError,
  encryptionEnabled,
  onEncryptionChanged,
  onImportedMany,
  onAccountRemoved,
  onDataDirChanged,
  onCheckForUpdate,
  updateCheckStatus,
}: ReadyViewProps) {
  const { t } = useTranslation();
  const [showSettings, setShowSettings] = useState(false);
  const [showProxy, setShowProxy] = useState(false);
  const [wizardOpen, setWizardOpen] = useState(false);
  const [proxies, setProxies] = useState<Proxy[]>([]);
  const [copied, setCopied] = useState(false);
  const [multiSelected, setMultiSelected] = useState<Set<string>>(new Set());
  // When set, we're asking the user to confirm enabling *trade* auto-confirm for
  // these account ids (trades are irreversible, so we gate them behind a confirm).
  const [pendingTradeEnable, setPendingTradeEnable] = useState<string[] | null>(null);
  const [pendingDelete, setPendingDelete] = useState<AccountSummary | null>(null);
  const [statusNote, setStatusNote] = useState<string | null>(null);
  const [groups, setGroups] = useState<Group[]>([]);
  const [activeGroup, setActiveGroup] = useState<string | null>(null);
  const [contextMenu, setContextMenu] = useState<
    { account: AccountSummary; x: number; y: number } | null
  >(null);
  const [activePanel, setActivePanel] = useState<"confirmations" | "confirmLogin">(
    "confirmations",
  );
  const [isDragging, setIsDragging] = useState(false);
  const anyModalOpenRef = useRef(false);
  const dropImport = useImportPaths(onImportedMany);

  const selectedAccount =
    accounts.find((a) => a.steamId === selectedSteamId) ?? null;

  const { code, secondsRemaining, error: codeError, loading: codeLoading } =
    useCode(selectedSteamId);

  const hasCode = selectedAccount !== null && codeError === null && code !== "";

  // ── Auto-confirm target set ────────────────────────────────────────────────
  // Multi-selection wins; otherwise the toggles act on the current account.
  const targetIds = useMemo(() => {
    if (multiSelected.size > 0) return [...multiSelected];
    return selectedSteamId !== null ? [selectedSteamId] : [];
  }, [multiSelected, selectedSteamId]);

  const targetAccounts = accounts.filter((a) => targetIds.includes(a.steamId));
  const hasTarget = targetAccounts.length > 0;
  const marketActive = hasTarget && targetAccounts.every((a) => a.autoConfirmMarket);
  const tradeActive = hasTarget && targetAccounts.every((a) => a.autoConfirmTrade);

  const toggleMultiSelect = useCallback((steamId: string) => {
    setMultiSelected((prev) => {
      const next = new Set(prev);
      if (next.has(steamId)) next.delete(steamId);
      else next.add(steamId);
      return next;
    });
  }, []);

  const refreshGroups = useCallback(() => {
    void listGroups().then(setGroups).catch(() => {});
  }, []);

  useEffect(() => {
    refreshGroups();
  }, [refreshGroups]);

  const refreshProxies = useCallback(() => {
    void listProxies().then(setProxies).catch(() => {});
  }, []);

  useEffect(() => {
    refreshProxies();
  }, [refreshProxies]);

  const groupFilter = useMemo(() => {
    if (activeGroup === null) return null;
    const g = groups.find((gr) => gr.name === activeGroup);
    return g ? new Set(g.members) : null;
  }, [activeGroup, groups]);

  useEffect(() => {
    if (activeGroup !== null && !groups.some((g) => g.name === activeGroup)) {
      setActiveGroup(null);
    }
  }, [groups, activeGroup]);

  const copyText = useCallback(async (text: string, note: string) => {
    try {
      await navigator.clipboard.writeText(text);
      setStatusNote(note);
    } catch {
      setStatusNote(t("ctx.copyFailed"));
    }
  }, [t]);

  const handleCopyMafile = useCallback(
    async (steamId: string) => {
      try {
        const json = await exportMafile(steamId);
        await navigator.clipboard.writeText(json);
        setStatusNote(t("ctx.mafileCopied"));
      } catch {
        setStatusNote(t("ctx.copyFailed"));
      }
    },
    [t],
  );

  const handleExportToFile = useCallback(
    async (steamId: string, defaultName: string) => {
      try {
        const dest = await save({
          defaultPath: defaultName,
          filters: [{ name: "maFile", extensions: ["maFile"] }],
        });
        if (dest === null) return; // user cancelled
        await exportMafiles([steamId], dest);
        setStatusNote(t("export.done"));
      } catch {
        setStatusNote(t("export.failed"));
      }
    },
    [t],
  );

  const handleExportSelected = useCallback(
    async (steamIds: string[]) => {
      if (steamIds.length === 0) return;
      try {
        const dest = await save({
          defaultPath: "stewardauth-export.zip",
          filters: [{ name: "ZIP", extensions: ["zip"] }],
        });
        if (dest === null) return; // user cancelled
        await exportMafiles(steamIds, dest);
        setStatusNote(t("export.done"));
      } catch {
        setStatusNote(t("export.failed"));
      }
    },
    [t],
  );

  const handleAddToGroup = useCallback(
    async (steamId: string, name: string) => {
      await addToGroup(steamId, name).catch(() => {});
      refreshGroups();
    },
    [refreshGroups],
  );

  const handleCreateGroup = useCallback(
    async (steamId: string, name: string) => {
      await createGroup(name).catch(() => {});
      await addToGroup(steamId, name).catch(() => {});
      refreshGroups();
    },
    [refreshGroups],
  );

  const handleRemoveFromGroup = useCallback(
    async (steamId: string) => {
      const containing = groups.filter((g) => g.members.includes(steamId));
      for (const g of containing) {
        await removeFromGroup(steamId, g.name).catch(() => {});
      }
      refreshGroups();
    },
    [groups, refreshGroups],
  );

  const handleAssignProxy = useCallback(async (steamId: string, id: string) => {
    await assignProxy(steamId, id).catch(() => {});
    onRefresh();
  }, [onRefresh]);

  const handleUnpinProxy = useCallback(async (steamId: string) => {
    await unpinProxy(steamId).catch(() => {});
    onRefresh();
  }, [onRefresh]);

  const applyAutoConfirm = useCallback(
    async (market?: boolean, trade?: boolean) => {
      if (targetIds.length === 0) return;
      try {
        await setAutoConfirm(targetIds, market, trade);
        onRefresh(); // re-fetch accounts so the column + icon state update
      } catch {
        setStatusNote(t("autoconfirm.updateFailed"));
      }
    },
    [targetIds, onRefresh],
  );

  const handleToggleMarket = useCallback(() => {
    // If not every target has it on → enable all; else disable all.
    void applyAutoConfirm(!marketActive, undefined);
  }, [applyAutoConfirm, marketActive]);

  const handleToggleTrade = useCallback(() => {
    if (!tradeActive) {
      // Enabling trades → gate behind a confirm dialog.
      setPendingTradeEnable(targetIds);
    } else {
      void applyAutoConfirm(undefined, false);
    }
  }, [applyAutoConfirm, tradeActive, targetIds]);

  const confirmTradeEnable = useCallback(async () => {
    if (pendingTradeEnable === null) return;
    try {
      await setAutoConfirm(pendingTradeEnable, undefined, true);
      onRefresh();
    } catch {
      setStatusNote(t("autoconfirm.tradeFailed"));
    } finally {
      setPendingTradeEnable(null);
    }
  }, [pendingTradeEnable, onRefresh]);

  const confirmDelete = useCallback(async () => {
    if (pendingDelete === null) return;
    const steamId = pendingDelete.steamId;
    try {
      await removeAccount(steamId);
      onAccountRemoved(steamId);
      onRefresh();
    } catch {
      setStatusNote(t("deleteGate.failed"));
    } finally {
      setPendingDelete(null);
    }
  }, [pendingDelete, onAccountRemoved, onRefresh, t]);

  const handleCopy = async () => {
    if (!code) return;
    try {
      await navigator.clipboard.writeText(code);
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    } catch {
      // Clipboard write failed silently — nothing actionable here.
    }
  };

  // ── Auto-confirm engine feedback ───────────────────────────────────────────
  useEffect(() => {
    const unlistenConfirmed = listen<ConfirmedEvent>("auto-confirm-confirmed", (e) => {
      const { steamId, count, market, trade } = e.payload;
      const acc = accounts.find((a) => a.steamId === steamId);
      const who = acc?.accountName ?? steamId;
      const what =
        market > 0 && trade > 0
          ? t("autoconfirm.confirmations", { count })
          : market > 0
          ? t("autoconfirm.market", { count: market })
          : t("autoconfirm.trade", { count: trade });
      setStatusNote(t("autoconfirm.done", { what, who }));
    });
    return () => {
      void unlistenConfirmed.then((off) => off());
    };
  }, [accounts, t]);

  // Clear a transient status note after a few seconds.
  useEffect(() => {
    if (statusNote === null) return;
    const t = setTimeout(() => setStatusNote(null), 4000);
    return () => clearTimeout(t);
  }, [statusNote]);

  // Keep anyModalOpenRef in sync without re-subscribing the drag handler.
  useEffect(() => {
    anyModalOpenRef.current =
      showSettings || showProxy || pendingTradeEnable !== null || pendingDelete !== null || contextMenu !== null;
  }, [showSettings, showProxy, pendingTradeEnable, pendingDelete, contextMenu]);

  // ── Drag-and-drop maFile/ZIP import ─────────────────────────────────────────
  useEffect(() => {
    const promise = getCurrentWebview().onDragDropEvent((event) => {
      if (event.payload.type === "enter" || event.payload.type === "over") {
        setIsDragging(true);
      } else if (event.payload.type === "leave") {
        setIsDragging(false);
      } else if (event.payload.type === "drop") {
        setIsDragging(false);
        if (anyModalOpenRef.current) return; // ignore drops while a modal is open
        if (event.payload.paths.length > 0) {
          void dropImport.run(event.payload.paths);
        }
      }
    });
    return () => {
      void promise.then((unlisten) => unlisten());
    };
  }, [dropImport]);

  // ── Keyboard shortcuts: Ctrl+C = copy login, Ctrl+X = copy maFile ─────────
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const el = document.activeElement;
      const typing =
        el instanceof HTMLInputElement || el instanceof HTMLTextAreaElement;
      if (typing || selectedAccount === null) return;
      if (!(e.metaKey || e.ctrlKey)) return;
      if (e.key === "c") {
        e.preventDefault();
        void copyText(selectedAccount.accountName, t("ctx.loginCopied"));
      } else if (e.key === "x") {
        e.preventDefault();
        void handleCopyMafile(selectedAccount.steamId);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [selectedAccount, copyText, handleCopyMafile]);

  const pendingTradeNames =
    pendingTradeEnable === null
      ? []
      : accounts
          .filter((a) => pendingTradeEnable.includes(a.steamId))
          .map((a) => a.accountName);

  return (
    <div className="app-shell">
      <Toolbar
        secondsRemaining={secondsRemaining}
        showTimer={hasCode}
        menuSlot={<MenuDropdown onSettings={() => setShowSettings(true)} />}
        onRefresh={onRefresh}
        refreshing={refreshing}
        accountSlot={
          <AccountMenu onImportedMany={onImportedMany} onCreate={() => setWizardOpen(true)} />
        }
        groupsSlot={
          <GroupsDropdown
            groups={groups}
            activeGroup={activeGroup}
            onSelect={setActiveGroup}
          />
        }
        proxySlot={
          <button type="button" className="toolbar-btn toolbar-btn--muted toolbar-btn--dropdown" onClick={() => setShowProxy(true)}>
            {t("menu.proxy")}
          </button>
        }
        targetCount={targetAccounts.length}
        marketActive={marketActive}
        tradeActive={tradeActive}
        hasTarget={hasTarget}
        onToggleMarket={handleToggleMarket}
        onToggleTrade={handleToggleTrade}
      />

      <div className="app-body">
        <aside className="sidebar">
          {loadError !== null && <div className="sidebar-error">{loadError}</div>}
          <AccountList
            accounts={accounts}
            selectedSteamId={selectedSteamId}
            onSelect={onSelect}
            multiSelected={multiSelected}
            onToggleMultiSelect={toggleMultiSelect}
            onContextMenu={(steamId, x, y) => {
              const account = accounts.find((a) => a.steamId === steamId);
              if (account) setContextMenu({ account, x, y });
            }}
            groupFilter={groupFilter}
          />
        </aside>

        <main className="main-pane">
          {selectedAccount !== null ? (
            <div className="account-pane">
              {/* ── Code header: segmented Guard tiles + 30s progress ────── */}
              <div className="code-header">
                <div className="code-row">
                  {codeError !== null ? (
                    <div className="code-error">
                      <span className="code-error-kind">{codeError.kind}</span>
                      <span className="code-error-msg">{codeError.message}</span>
                    </div>
                  ) : copied ? (
                    <button
                      type="button"
                      className="code-copied"
                      onClick={() => void handleCopy()}
                    >
                      <span className="code-copied-value">{code}</span>
                      <span className="code-copied-label">
                        <Icon name="check" size={14} stroke={2.4} />
                        {t("code.copied")}
                      </span>
                    </button>
                  ) : hasCode ? (
                    <>
                      <button
                        type="button"
                        className="code-tiles"
                        onClick={() => void handleCopy()}
                        title={t("code.clickToCopy")}
                        aria-label="Steam Guard code — click to copy"
                      >
                        {code.split("").map((ch, i) => (
                          <span key={i} className="code-tile">{ch}</span>
                        ))}
                      </button>
                      <span className="code-hint">{t("code.clickToCopy")}</span>
                    </>
                  ) : (
                    <span className="code-loading">
                      {codeLoading ? "•••••" : t("common.dash")}
                    </span>
                  )}
                </div>
                {codeError === null && (
                  <div className="code-progress" aria-hidden="true">
                    <div
                      className="code-progress-fill"
                      style={{ width: `${hasCode ? (secondsRemaining / 30) * 100 : 0}%` }}
                    />
                  </div>
                )}
                <div className="code-sub">
                  {selectedAccount.accountName} ·{" "}
                  <span className="code-sub-id">{selectedAccount.steamId}</span>
                </div>
              </div>

              {/* ── Panel tabs: Confirmations | Confirm Login ───────────── */}
              <div className="panel-tabs" role="tablist">
                <button
                  type="button"
                  role="tab"
                  aria-selected={activePanel === "confirmations"}
                  className={`panel-tab${activePanel === "confirmations" ? " panel-tab--active" : ""}`}
                  onClick={() => setActivePanel("confirmations")}
                >
                  {t("tabs.confirmations")}
                </button>
                <button
                  type="button"
                  role="tab"
                  aria-selected={activePanel === "confirmLogin"}
                  className={`panel-tab${activePanel === "confirmLogin" ? " panel-tab--active" : ""}`}
                  onClick={() => setActivePanel("confirmLogin")}
                >
                  {t("tabs.confirmLogin")}
                </button>
              </div>

              {activePanel === "confirmations" ? (
                <Confirmations
                  steamId={selectedAccount.steamId}
                  accountName={selectedAccount.accountName}
                />
              ) : (
                <ConfirmLogin
                  steamId={selectedAccount.steamId}
                  accountName={selectedAccount.accountName}
                />
              )}
            </div>
          ) : (
            <div className="main-pane-empty">
              {accounts.length === 0
                ? t("pane.noAccounts")
                : t("pane.selectAccount")}
            </div>
          )}
        </main>
      </div>

      <StatusBar
        count={accounts.length}
        currentName={selectedAccount?.accountName ?? null}
        multiCount={multiSelected.size}
        note={statusNote}
        onCheckForUpdate={onCheckForUpdate}
        updateCheckStatus={updateCheckStatus}
      />

      {/* Delete account safety gate */}
      {pendingDelete !== null && (
        <div className="confirm-overlay" role="dialog" aria-modal="true">
          <div className="confirm-backdrop" onClick={() => setPendingDelete(null)} />
          <div className="confirm-card">
            <h2 className="confirm-title">{t("deleteGate.title")}</h2>
            <p className="confirm-body">
              {t("deleteGate.body", { name: pendingDelete.accountName || pendingDelete.steamId })}
            </p>
            <div className="confirm-actions">
              <button className="conf-btn conf-btn--ghost" onClick={() => setPendingDelete(null)}>
                {t("common.cancel")}
              </button>
              <button className="conf-btn conf-btn--danger-solid" onClick={() => void confirmDelete()}>
                {t("deleteGate.confirm")}
              </button>
            </div>
          </div>
        </div>
      )}

      {/* Trade auto-confirm safety gate */}
      {pendingTradeEnable !== null && (
        <div className="confirm-overlay" role="dialog" aria-modal="true">
          <div className="confirm-backdrop" onClick={() => setPendingTradeEnable(null)} />
          <div className="confirm-card">
            <h2 className="confirm-title">{t("tradeGate.title")}</h2>
            <p className="confirm-body">
              {pendingTradeNames.length === 1
                ? t("tradeGate.bodyOne", { name: pendingTradeNames[0] })
                : t("tradeGate.bodyMany", { count: pendingTradeNames.length })}
            </p>
            <div className="confirm-actions">
              <button
                className="conf-btn conf-btn--ghost"
                onClick={() => setPendingTradeEnable(null)}
              >
                {t("common.cancel")}
              </button>
              <button
                className="conf-btn conf-btn--warning"
                onClick={() => void confirmTradeEnable()}
              >
                {t("tradeGate.enable")}
              </button>
            </div>
          </div>
        </div>
      )}

      {showSettings && (
        <Settings
          accounts={accounts}
          encryptionEnabled={encryptionEnabled}
          onEncryptionChanged={onEncryptionChanged}
          onAccountRemoved={onAccountRemoved}
          onClose={() => setShowSettings(false)}
          onDataDirChanged={onDataDirChanged}
        />
      )}

      {showProxy && (
        <ProxyManager
          selectedSteamIds={[...multiSelected]}
          onProxiesChanged={onRefresh}
          onClose={() => { setShowProxy(false); refreshProxies(); }}
        />
      )}

      {contextMenu !== null && (
        <AccountContextMenu
          x={contextMenu.x}
          y={contextMenu.y}
          account={contextMenu.account}
          groups={groups}
          proxies={proxies.map((p) => ({ id: p.id, host: p.host, port: p.port }))}
          onClose={() => setContextMenu(null)}
          onCopyLogin={() =>
            void copyText(contextMenu.account.accountName, t("ctx.loginCopied"))
          }
          onCopySteamId={() =>
            void copyText(contextMenu.account.steamId, t("ctx.steamIdCopied"))
          }
          onCopyMafile={() => void handleCopyMafile(contextMenu.account.steamId)}
          onExportToFile={() =>
            void handleExportToFile(
              contextMenu.account.steamId,
              `${contextMenu.account.accountName || contextMenu.account.steamId}.maFile`,
            )
          }
          onExportSelected={
            multiSelected.size > 1
              ? () => void handleExportSelected([...multiSelected])
              : undefined
          }
          selectedCount={multiSelected.size}
          onAddToGroup={(name) =>
            void handleAddToGroup(contextMenu.account.steamId, name)
          }
          onCreateGroup={(name) =>
            void handleCreateGroup(contextMenu.account.steamId, name)
          }
          onRemoveFromGroup={() =>
            void handleRemoveFromGroup(contextMenu.account.steamId)
          }
          onAssignProxy={(id) => void handleAssignProxy(contextMenu.account.steamId, id)}
          onUnpinProxy={() => void handleUnpinProxy(contextMenu.account.steamId)}
          hasProxy={contextMenu.account.proxy !== ""}
          onDeleteAccount={() => setPendingDelete(contextMenu.account)}
        />
      )}

      {wizardOpen && (
        <AddAuthenticatorWizard proxies={proxies} onClose={() => setWizardOpen(false)} />
      )}

      {isDragging && (
        <div className="drop-overlay" role="presentation">
          <div className="drop-overlay-hint">
            <Icon name="download" size={34} stroke={1.6} />
            {t("import.dropHint")}
            <span className="drop-overlay-hint-sub">{t("import.dropTypes")}</span>
          </div>
        </div>
      )}

      {dropImport.phase !== "idle" && dropImport.phase !== "importing" && (
        <div className="confirm-overlay" role="dialog" aria-modal="true">
          <div className="confirm-backdrop" onClick={dropImport.reset} />
          <div className="confirm-card">
            <ImportFeedback
              state={{
                phase: dropImport.phase,
                report: dropImport.report,
                errorKind: dropImport.errorKind,
              }}
              onSubmitPassword={(pw) => void dropImport.submitPassword(pw)}
              onReset={dropImport.reset}
            />
          </div>
        </div>
      )}
    </div>
  );
}
