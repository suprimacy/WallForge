import { useEffect, useState } from "react";
import type { TokenStatus } from "./types";

/**
 * Count the ManifestHub key down locally without polling the backend.
 *
 * The backend reports its clock (`measuredAt`) alongside `expiresAt`, so the
 * countdown anchors on those two and adds only locally elapsed seconds. It
 * stays correct when the system clock changes or the app is suspended.
 */
export function useTokenClock(status: TokenStatus | null): number {
  const [remaining, setRemaining] = useState(0);

  useEffect(() => {
    if (!status || !status.token || status.expiresAt <= 0) {
      setRemaining(0);
      return;
    }
    const anchorLocalMs = Date.now();
    const anchorServerSecs = status.measuredAt;
    const tick = () => {
      const elapsed = Math.floor((Date.now() - anchorLocalMs) / 1000);
      setRemaining(Math.max(0, status.expiresAt - (anchorServerSecs + elapsed)));
    };
    tick();
    const timer = window.setInterval(tick, 1000);
    return () => window.clearInterval(timer);
  }, [status]);

  return remaining;
}

export function formatCountdown(seconds: number): string {
  if (seconds <= 0) return "expired";
  const hours = Math.floor(seconds / 3600);
  const minutes = Math.floor((seconds % 3600) / 60);
  return `${hours}h ${String(minutes).padStart(2, "0")}m`;
}
