// SPDX-FileCopyrightText: 2026 shinyawave
// SPDX-License-Identifier: AGPL-3.0-or-later

import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { Icon } from "./icons";

interface ToolbarProps {
  /** Seconds left in the selected account's current code window (1–30). */
  secondsRemaining: number;
  /** Whether a code is available to show a live countdown for. */
  showTimer: boolean;
  /** Re-fetches the account list. */
  onRefresh: () => void;
  refreshing: boolean;
  /** The Menu dropdown, rendered at the far left. */
  menuSlot: ReactNode;
  /** The import control, rendered as the "Account" toolbar button. */
  accountSlot: ReactNode;
  /** The Groups dropdown, rendered in the left toolbar group. */
  groupsSlot: ReactNode;
  /** The Proxy manager button, rendered next to the Groups dropdown. */
  proxySlot: ReactNode;
  /** How many accounts the market/trade toggles will act on (for the tooltip). */
  targetCount: number;
  /** Whether the target set currently has market / trade auto-confirm on. */
  marketActive: boolean;
  tradeActive: boolean;
  /** Whether there is any target at all (a current account or a multi-selection). */
  hasTarget: boolean;
  onToggleMarket: () => void;
  onToggleTrade: () => void;
}

export function Toolbar({
  secondsRemaining,
  showTimer,
  onRefresh,
  refreshing,
  menuSlot,
  accountSlot,
  groupsSlot,
  proxySlot,
  targetCount,
  marketActive,
  tradeActive,
  hasTarget,
  onToggleMarket,
  onToggleTrade,
}: ToolbarProps) {
  const { t } = useTranslation();
  const scope =
    targetCount > 1 ? t("toolbar.scopeSelected", { count: targetCount }) : t("toolbar.scopeCurrent");

  return (
    <header className="toolbar">
      <div className="toolbar-group toolbar-group--left">
        {menuSlot}
        {accountSlot}

        <span className="toolbar-divider" aria-hidden="true" />

        {groupsSlot}
        {proxySlot}
      </div>

      <div className="toolbar-group toolbar-group--right">
        <button
          type="button"
          className={`toolbar-pill${marketActive ? " toolbar-pill--active" : ""}`}
          onClick={onToggleMarket}
          disabled={!hasTarget}
          aria-pressed={marketActive}
          title={t("toolbar.autoMarket", { scope })}
        >
          <Icon name="cart" size={13} />
          {t("toolbar.market")}
        </button>
        <button
          type="button"
          className={`toolbar-pill${tradeActive ? " toolbar-pill--active" : ""}`}
          onClick={onToggleTrade}
          disabled={!hasTarget}
          aria-pressed={tradeActive}
          title={t("toolbar.autoTrade", { scope })}
        >
          <Icon name="person-plus" size={13} />
          {t("toolbar.trades")}
        </button>

        <span className="toolbar-timer" title={t("toolbar.sec")}>
          {showTimer ? `${secondsRemaining}s` : "--"}
        </span>

        <button
          type="button"
          className="toolbar-icon-btn"
          onClick={onRefresh}
          disabled={refreshing}
          aria-label={t("toolbar.refreshAccounts")}
          title={t("toolbar.refreshAccounts")}
        >
          <span className={refreshing ? "toolbar-refresh--spinning" : undefined}>
            <Icon name="refresh" size={15} />
          </span>
        </button>
      </div>
    </header>
  );
}
