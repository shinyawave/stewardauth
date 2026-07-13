// SPDX-FileCopyrightText: 2026 shinyawave
// SPDX-License-Identifier: AGPL-3.0-or-later

import { useEffect, useRef, useState } from "react";
import { getCode } from "../api";
import type { AppError } from "../types";
import { isAppError } from "../types";

export interface UseCodeResult {
  code: string;
  secondsRemaining: number;
  error: AppError | null;
  loading: boolean;
}

/**
 * Fetches the current Steam Guard TOTP code for `steamId` immediately on
 * mount (and whenever `steamId` changes), then counts down locally with a
 * 1-second interval.  When the countdown reaches 0 it re-invokes `getCode`
 * to obtain the fresh code + reset `secondsRemaining`.
 *
 * `steamId` may be `null` (no account selected) — in that case the hook is a
 * no-op: no fetch, empty code, `loading = false`. This lets callers lift the
 * hook above a possibly-empty selection without violating the rules of hooks.
 */
export function useCode(steamId: string | null): UseCodeResult {
  const [code, setCode] = useState<string>("");
  const [secondsRemaining, setSecondsRemaining] = useState<number>(30);
  const [error, setError] = useState<AppError | null>(null);
  const [loading, setLoading] = useState<boolean>(true);

  // Mutable ref so the interval callback always sees the latest value without
  // needing to be recreated on every tick.
  const secondsRef = useRef<number>(30);

  useEffect(() => {
    // No account selected → clear and stay idle.
    if (steamId === null) {
      setCode("");
      setError(null);
      setLoading(false);
      return;
    }

    let intervalId: ReturnType<typeof setInterval> | null = null;
    let cancelled = false;

    const fetchCode = async (isInitial: boolean) => {
      try {
        if (isInitial) setLoading(true);
        const result = await getCode(steamId);
        if (cancelled) return;
        setCode(result.code);
        setSecondsRemaining(result.secondsRemaining);
        secondsRef.current = result.secondsRemaining;
        setError(null);
      } catch (e: unknown) {
        if (cancelled) return;
        if (isAppError(e)) {
          setError(e);
        } else {
          setError({ kind: "SteamError", message: "Unknown error fetching code" });
        }
      } finally {
        if (!cancelled) setLoading(false);
      }
    };

    // Fetch immediately (initial load — show spinner)
    fetchCode(true).then(() => {
      if (cancelled) return;
      // Start 1-second countdown interval after the first fetch
      intervalId = setInterval(() => {
        secondsRef.current -= 1;
        if (secondsRef.current <= 0) {
          // Prevent re-entry while fetch is in-flight
          secondsRef.current = 30;
          // Reset displayed countdown immediately so the ring shows 30
          // rather than 0 while the fetch is in-flight.
          setSecondsRemaining(30);
          // Rollover refresh — do NOT show spinner (isInitial=false)
          fetchCode(false);
        } else {
          setSecondsRemaining(secondsRef.current);
        }
      }, 1000);
    });

    return () => {
      cancelled = true;
      if (intervalId !== null) clearInterval(intervalId);
    };
  }, [steamId]);

  return { code, secondsRemaining, error, loading };
}
