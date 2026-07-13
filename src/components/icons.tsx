// SPDX-FileCopyrightText: 2026 shinyawave
// SPDX-License-Identifier: AGPL-3.0-or-later

// Unified line-icon set (from the redesign's icons.svg sprite).
// 24×24, 1.8 stroke, currentColor-driven so CSS controls the color.

import type { CSSProperties, ReactElement } from "react";

export type IconName =
  | "lock"
  | "shield-check"
  | "key"
  | "cart"
  | "person-plus"
  | "people"
  | "refresh"
  | "search"
  | "check"
  | "close"
  | "plus"
  | "copy"
  | "download"
  | "folder"
  | "trash"
  | "chevron-down"
  | "chevron-right"
  | "star"
  | "heart"
  | "mail"
  | "info"
  | "alert"
  | "check-circle"
  | "github"
  | "telegram";

// Brand marks are filled logos (no stroke), unlike the line-icon set.
const BRAND: ReadonlySet<IconName> = new Set(["github", "telegram"]);

// Path geometry per symbol. `fill` marks solid-filled icons (heart/star variants
// handle their own fill via the `filled` prop below).
const PATHS: Record<IconName, ReactElement> = {
  lock: (
    <>
      <rect x="5" y="11" width="14" height="9" rx="2" />
      <path d="M8 11V8a4 4 0 0 1 8 0v3" />
    </>
  ),
  "shield-check": (
    <>
      <path d="M12 3l7 3v5c0 4.5-3 8-7 10-4-2-7-5.5-7-10V6z" />
      <path d="M9.5 12l1.8 1.8 3.5-3.6" />
    </>
  ),
  key: (
    <>
      <circle cx="8" cy="12" r="4" />
      <path d="M12 12h9M18 12v3M21 12v2" />
    </>
  ),
  cart: (
    <>
      <circle cx="9" cy="20" r="1.4" />
      <circle cx="18" cy="20" r="1.4" />
      <path d="M2 3h3l2.5 12h11l2-8H6" />
    </>
  ),
  "person-plus": (
    <>
      <circle cx="10" cy="8" r="3.2" />
      <path d="M4 20c0-3.3 2.7-6 6-6M17 8v6M20 11h-6" />
    </>
  ),
  people: (
    <>
      <circle cx="9" cy="9" r="3" />
      <path d="M3 19c0-3 2.7-5 6-5s6 2 6 5M16 6a3 3 0 0 1 0 6M18 19c0-2-.8-3.6-2-4.6" />
    </>
  ),
  refresh: (
    <>
      <path d="M20 12a8 8 0 1 1-2.3-5.6" />
      <path d="M20 4v4h-4" />
    </>
  ),
  search: (
    <>
      <circle cx="10.5" cy="10.5" r="6.5" />
      <path d="M20 20l-4.5-4.5" />
    </>
  ),
  check: <path d="M5 13l4 4L19 7" />,
  close: <path d="M6 6l12 12M18 6L6 18" />,
  plus: <path d="M12 5v14M5 12h14" />,
  copy: (
    <>
      <rect x="9" y="9" width="11" height="11" rx="2" />
      <path d="M5 15V6a2 2 0 0 1 2-2h9" />
    </>
  ),
  download: (
    <>
      <path d="M12 15V3" />
      <path d="M7 10l5 5 5-5" />
      <path d="M4 17v2a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2v-2" />
    </>
  ),
  folder: <path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z" />,
  trash: <path d="M4 7h16M9 7V5a1 1 0 0 1 1-1h4a1 1 0 0 1 1 1v2M6 7l1 13a1 1 0 0 0 1 1h8a1 1 0 0 0 1-1l1-13" />,
  "chevron-down": <path d="M6 9l6 6 6-6" />,
  "chevron-right": <path d="M9 6l6 6-6 6" />,
  star: <path d="M12 3l2.4 5 5.6.7-4 4 1 5.6-5-2.7-5 2.7 1-5.6-4-4 5.6-.7z" />,
  heart: <path d="M12 4l6 6c2 2 2 5 0 7s-5 2-6-.5c-1 2.5-4 2.5-6 .5s-2-5 0-7z" />,
  mail: (
    <>
      <rect x="3" y="5" width="18" height="14" rx="2" />
      <path d="M3 7l9 6 9-6" />
    </>
  ),
  info: (
    <>
      <circle cx="12" cy="12" r="9" />
      <path d="M12 11v5M12 8h.01" />
    </>
  ),
  alert: (
    <>
      <circle cx="12" cy="12" r="9" />
      <path d="M12 8v5M12 16h.01" />
    </>
  ),
  "check-circle": (
    <>
      <circle cx="12" cy="12" r="9" />
      <path d="M8 12l2.5 2.5L16 9" />
    </>
  ),
  github: (
    <path d="M12 2C6.48 2 2 6.48 2 12c0 4.42 2.87 8.17 6.84 9.5.5.09.68-.22.68-.48 0-.24-.01-.87-.01-1.7-2.78.6-3.37-1.34-3.37-1.34-.45-1.16-1.11-1.47-1.11-1.47-.91-.62.07-.61.07-.61 1 .07 1.53 1.03 1.53 1.03.89 1.52 2.34 1.08 2.91.83.09-.65.35-1.08.63-1.33-2.22-.25-4.55-1.11-4.55-4.94 0-1.09.39-1.98 1.03-2.68-.1-.25-.45-1.27.1-2.64 0 0 .84-.27 2.75 1.02a9.6 9.6 0 0 1 5 0c1.91-1.29 2.75-1.02 2.75-1.02.55 1.37.2 2.39.1 2.64.64.7 1.03 1.59 1.03 2.68 0 3.84-2.34 4.68-4.57 4.93.36.31.68.92.68 1.85 0 1.34-.01 2.42-.01 2.75 0 .27.18.58.69.48A10.01 10.01 0 0 0 22 12c0-5.52-4.48-10-10-10z" />
  ),
  telegram: (
    <path d="M9.78 15.6 9.6 19.2c.36 0 .52-.15.71-.34l1.7-1.63 3.53 2.58c.65.36 1.11.17 1.28-.6l2.32-10.9c.2-.95-.34-1.32-.97-1.09L4.4 12.02c-.94.36-.92.88-.16 1.11l3.44 1.07 7.98-5.02c.38-.25.72-.11.44.14l-6.32 6.28z" />
  ),
};

interface IconProps {
  name: IconName;
  size?: number;
  /** Stroke width override (default 1.8). Use ~2.4 for tiny accept/deny checks. */
  stroke?: number;
  /** Render as a solid-filled glyph (heart/star favorites, filled states). */
  filled?: boolean;
  className?: string;
  style?: CSSProperties;
}

export function Icon({
  name,
  size = 18,
  stroke = 1.8,
  filled = false,
  className,
  style,
}: IconProps) {
  const brand = BRAND.has(name);
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill={brand || filled ? "currentColor" : "none"}
      stroke={brand ? "none" : "currentColor"}
      strokeWidth={stroke}
      strokeLinecap="round"
      strokeLinejoin="round"
      className={className}
      style={style}
      aria-hidden="true"
    >
      {PATHS[name]}
    </svg>
  );
}

// ── Backwards-compatible named wrappers (used by Toolbar / AccountList) ──────
interface LegacyProps {
  size?: number;
}
export const CartIcon = ({ size = 18 }: LegacyProps) => <Icon name="cart" size={size} />;
export const PersonPlusIcon = ({ size = 18 }: LegacyProps) => <Icon name="person-plus" size={size} />;
export const PersonIcon = ({ size = 18 }: LegacyProps) => <Icon name="person-plus" size={size} />;
export const PeopleIcon = ({ size = 18 }: LegacyProps) => <Icon name="people" size={size} />;
