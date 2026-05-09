"use client";

import { useCallback, useState } from "react";
import { usePublicConfig, usePublicConfigSubscription } from "@/lib/public-config";

export function useDateTimeFormatter() {
  const config = usePublicConfig();
  const [offsetMinutes, setOffsetMinutes] = useState(config.timeZoneOffsetMinutes);
  usePublicConfigSubscription((next) => setOffsetMinutes(next.timeZoneOffsetMinutes));

  return useCallback(
    (value?: string | number | Date | null) => formatDateTime(value, offsetMinutes),
    [offsetMinutes]
  );
}

export function useMarketTimeZoneOffset() {
  const config = usePublicConfig();
  const [offsetMinutes, setOffsetMinutes] = useState(config.timeZoneOffsetMinutes);
  usePublicConfigSubscription((next) => setOffsetMinutes(next.timeZoneOffsetMinutes));
  return offsetMinutes;
}

export function formatDateTime(value?: string | number | Date | null, offsetMinutes = 480): string {
  if (value === null || value === undefined || value === "") return "—";
  const date = value instanceof Date ? value : new Date(value);
  const time = date.getTime();
  if (!Number.isFinite(time)) return String(value);
  const shifted = new Date(time + offsetMinutes * 60_000);
  return [
    shifted.getUTCFullYear(),
    pad2(shifted.getUTCMonth() + 1),
    pad2(shifted.getUTCDate()),
  ].join("-") + " " + [
    pad2(shifted.getUTCHours()),
    pad2(shifted.getUTCMinutes()),
    pad2(shifted.getUTCSeconds()),
  ].join(":");
}

export function isSameConfiguredDay(value: string, offsetMinutes = 480): boolean {
  const date = new Date(value);
  const time = date.getTime();
  if (!Number.isFinite(time)) return false;
  const shifted = new Date(time + offsetMinutes * 60_000);
  const now = new Date(Date.now() + offsetMinutes * 60_000);
  return shifted.getUTCFullYear() === now.getUTCFullYear()
    && shifted.getUTCMonth() === now.getUTCMonth()
    && shifted.getUTCDate() === now.getUTCDate();
}

export function formatUtcOffset(offsetMinutes: number): string {
  const sign = offsetMinutes >= 0 ? "+" : "-";
  const abs = Math.abs(offsetMinutes);
  const hours = Math.floor(abs / 60);
  const minutes = abs % 60;
  return `UTC${sign}${hours}${minutes ? `:${pad2(minutes)}` : ""}`;
}

function pad2(value: number): string {
  return String(value).padStart(2, "0");
}
