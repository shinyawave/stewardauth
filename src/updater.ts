// SPDX-FileCopyrightText: 2026 shinyawave
// SPDX-License-Identifier: AGPL-3.0-or-later

// ── Updater wrapper ─────────────────────────────────────────────────────────
//
// Thin typed layer over @tauri-apps/plugin-updater so the rest of the app never
// imports the plugin directly. check() hits the endpoint configured in
// tauri.conf.json > plugins.updater and verifies the artifact signature against
// the embedded public key before install() will run.

import { check, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";

/** Metadata about an available update, surfaced to the UI. */
export interface UpdateInfo {
  /** Version offered by the remote manifest, e.g. "0.2.0". */
  version: string;
  /** Version currently running, e.g. "0.1.0". */
  currentVersion: string;
  /** Release notes / changelog from latest.json `notes`, or null. */
  notes: string | null;
}

/** Byte counters for the download progress bar. */
export interface DownloadProgress {
  downloaded: number;
  /** Total content length in bytes, or null until the server reports it. */
  total: number | null;
}

/** A confirmed-available update the user can choose to install. */
export interface PendingUpdate {
  info: UpdateInfo;
  /** Download + install, reporting byte progress. Resolves when installed. */
  install(onProgress: (p: DownloadProgress) => void): Promise<void>;
}

/**
 * Check the configured endpoint for a newer version. Resolves to a
 * PendingUpdate when one is available, or null when up to date.
 * Rejects on network/verification errors — callers handle those.
 */
export async function checkForUpdate(): Promise<PendingUpdate | null> {
  const update: Update | null = await check();
  if (!update) return null;

  const info: UpdateInfo = {
    version: update.version,
    currentVersion: update.currentVersion,
    notes: update.body ?? null,
  };

  return {
    info,
    install: async (onProgress) => {
      let downloaded = 0;
      let total: number | null = null;
      await update.downloadAndInstall((event) => {
        switch (event.event) {
          case "Started":
            total = event.data.contentLength ?? null;
            onProgress({ downloaded, total });
            break;
          case "Progress":
            downloaded += event.data.chunkLength;
            onProgress({ downloaded, total });
            break;
          case "Finished":
            onProgress({ downloaded, total });
            break;
        }
      });
    },
  };
}

/** Restart the app into the freshly installed version. */
export function relaunchApp(): Promise<void> {
  return relaunch();
}
