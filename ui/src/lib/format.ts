import { formatDistanceToNow, format } from "date-fns";

export function shortHash(h: string | null, chars = 8): string {
  if (!h) return "—";
  return `${h.slice(0, chars + 2)}…${h.slice(-4)}`;
}

export function ago(ts: string | null): string {
  if (!ts) return "—";
  return formatDistanceToNow(new Date(ts), { addSuffix: true });
}

export function ts(ts: string | null): string {
  if (!ts) return "—";
  return format(new Date(ts), "MMM d, HH:mm:ss");
}

export function usd(n: number | null): string {
  if (n === null) return "—";
  return new Intl.NumberFormat("en-US", {
    style: "currency", currency: "USD", maximumFractionDigits: 2,
  }).format(n);
}

export function pct(n: number): string {
  return `${(n * 100).toFixed(1)}%`;
}