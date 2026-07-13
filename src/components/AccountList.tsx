// SPDX-FileCopyrightText: 2026 shinyawave
// SPDX-License-Identifier: AGPL-3.0-or-later

import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import type { AccountSummary } from "../types";
import { Icon } from "./icons";

interface AccountListProps {
  accounts: AccountSummary[];
  selectedSteamId: string | null;
  onSelect: (steamId: string) => void;
  /** Accounts multi-selected via right-click (targets for the toolbar toggles). */
  multiSelected: Set<string>;
  /** Toggle an account's membership in the multi-selection (right-click). */
  onToggleMultiSelect: (steamId: string) => void;
  /** Open the account context menu at the cursor (right-click). */
  onContextMenu: (steamId: string, x: number, y: number) => void;
  /** When set, only show accounts whose steamId is in this set (active group). */
  groupFilter: Set<string> | null;
}

export function AccountList({
  accounts,
  selectedSteamId,
  onSelect,
  multiSelected,
  onToggleMultiSelect,
  onContextMenu,
  groupFilter,
}: AccountListProps) {
  const { t } = useTranslation();
  const [query, setQuery] = useState("");

  // Case-insensitive substring match on the login (accountName); also match on
  // steamId so pasting a SteamID finds the account too.
  const filtered = useMemo(() => {
    const base = groupFilter
      ? accounts.filter((a) => groupFilter.has(a.steamId))
      : accounts;
    const q = query.trim().toLowerCase();
    if (q === "") return base;
    return base.filter(
      (a) =>
        a.accountName.toLowerCase().includes(q) ||
        a.steamId.toLowerCase().includes(q),
    );
  }, [accounts, query, groupFilter]);

  // Auto-select the top match as the user types. Only act when there IS a query
  // and the current selection has dropped out of the filtered set — that way we
  // never override a selection that still matches, and calling onSelect only on
  // a real change keeps this from looping (the resulting selectedSteamId change
  // re-runs the effect, but now the selection is visible so we no-op).
  useEffect(() => {
    if (query.trim() === "" || filtered.length === 0) return;
    const selectionVisible = filtered.some((a) => a.steamId === selectedSteamId);
    if (!selectionVisible) {
      onSelect(filtered[0].steamId);
    }
  }, [query, filtered, selectedSteamId, onSelect]);

  if (accounts.length === 0) {
    return (
      <div className="account-list-empty">
        <p>{t("sidebar.importPrompt")}</p>
      </div>
    );
  }

  return (
    <div className="account-list-container">
      {filtered.length === 0 ? (
        <div className="account-list-empty">
          <p>
            {query !== ""
              ? t("sidebar.noMatch", { query })
              : t("sidebar.noInGroup")}
          </p>
        </div>
      ) : (
        <ul className="account-list" role="listbox" aria-label="Accounts">
          {filtered.map((account) => {
            const isSelected = account.steamId === selectedSteamId;
            const isMulti = multiSelected.has(account.steamId);
            return (
              <li
                key={account.steamId}
                className={
                  "account-row" +
                  (isSelected ? " account-row--selected" : "") +
                  (isMulti ? " account-row--multi" : "")
                }
                role="option"
                aria-selected={isSelected}
                onClick={(e) => {
                  // Cmd/Ctrl+click toggles multi-selection; plain click selects.
                  if (e.metaKey || e.ctrlKey) onToggleMultiSelect(account.steamId);
                  else onSelect(account.steamId);
                }}
                onContextMenu={(e) => {
                  e.preventDefault();
                  onContextMenu(account.steamId, e.clientX, e.clientY);
                }}
              >
                <div className="account-row-main">
                  <span className="account-row-name">{account.accountName}</span>
                  <span className="account-row-steamid">{account.steamId}</span>
                </div>
                {/* Auto-confirm status column: two dots (market · trade). */}
                <div className="account-row-flags" aria-hidden="true">
                  <span
                    className={`ac-dot${account.autoConfirmMarket ? " ac-dot--on" : ""}`}
                    title="Auto-confirm market"
                  />
                  <span
                    className={`ac-dot${account.autoConfirmTrade ? " ac-dot--on" : ""}`}
                    title="Auto-confirm trades"
                  />
                </div>
              </li>
            );
          })}
        </ul>
      )}

      {/* Search pinned to the BOTTOM of the sidebar (mockup style). */}
      <div className="account-search">
        <span className="account-search-icon" aria-hidden="true">
          <Icon name="search" size={12} stroke={2} />
        </span>
        <input
          className="account-search-input"
          type="text"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Escape") {
              setQuery("");
            } else if (e.key === "Enter" && filtered.length > 0) {
              // Jump to the top match.
              onSelect(filtered[0].steamId);
            }
          }}
          placeholder={t("sidebar.search")}
          aria-label="Search accounts by login"
          spellCheck={false}
          autoCapitalize="off"
          autoCorrect="off"
        />
        {query !== "" && (
          <button
            type="button"
            className="account-search-clear"
            onClick={() => setQuery("")}
            aria-label="Clear search"
            title="Clear search"
          >
            <Icon name="close" size={13} stroke={2} />
          </button>
        )}
      </div>
    </div>
  );
}
