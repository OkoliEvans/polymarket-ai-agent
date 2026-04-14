import { shortHash } from "@/lib/format";

export function HashCell({ hash, href }: { hash: string | null; href?: string }) {
  if (!hash) return <span className="text-zinc-600">—</span>;

  const display = shortHash(hash);

  if (href) {
    return (
      <a
        href={href}
        target="_blank"
        rel="noreferrer"
        className="font-mono text-xs text-brand-500 hover:text-brand-400 underline underline-offset-2"
      >
        {display}
      </a>
    );
  }

  return (
    <span className="font-mono text-xs text-zinc-400" title={hash}>
      {display}
    </span>
  );
}