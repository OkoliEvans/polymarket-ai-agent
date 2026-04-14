import clsx from "clsx";

const MAP: Record<string, string> = {
  queued:   "bg-zinc-700 text-zinc-300",
  running:  "bg-blue-900 text-blue-300",
  proving:  "bg-violet-900 text-violet-300",
  done:     "bg-emerald-900 text-emerald-300",
  settled:  "bg-green-900 text-green-300",
  failed:   "bg-red-900 text-red-400",
};

export function StatusBadge({ status }: { status: string }) {
  return (
    <span className={clsx(
      "inline-flex items-center px-2 py-0.5 rounded text-xs font-mono font-medium",
      MAP[status] ?? "bg-zinc-700 text-zinc-300"
    )}>
      {status}
    </span>
  );
}