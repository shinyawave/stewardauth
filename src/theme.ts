// SPDX-FileCopyrightText: 2026 shinyawave
// SPDX-License-Identifier: AGPL-3.0-or-later

/** Apply the accent color (derived from a 0–360 hue) as CSS variables.
 *  Keeps the weak/line accent tints in sync so the whole accent family
 *  tracks the user's chosen hue (selection fills, active borders, glow). */
export function applyAccent(hue: number): void {
  const root = document.documentElement;
  root.style.setProperty("--accent", `hsl(${hue} 92% 68%)`);
  root.style.setProperty("--accent-glow", `hsla(${hue}, 92%, 68%, 0.25)`);
  root.style.setProperty("--accent-weak", `hsla(${hue}, 92%, 68%, 0.12)`);
  root.style.setProperty("--accent-line", `hsla(${hue}, 92%, 68%, 0.30)`);
}
