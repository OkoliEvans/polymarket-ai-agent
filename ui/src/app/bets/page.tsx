"use client";

import { useEffect, useState, useCallback } from "react";
import { fetchBets, type Bet } from "@/lib/api";
import { HashCell } from "@/components/HashCell";
import { StatCard } from "@/components/StatCard";
import { ago, usd, pct } from "@/lib/format";
import { useSSE } from "@/hooks/useSSE";

export default function BetsPage() {
  const [bets, setBets]     = useState<Bet[]>([]);
  const [loading, setLoading] = useState(true);

  const refresh = useCallback(async () => {
    const b = await fetchBets(200);
    setBets(b);
    setLoading(false);
  }, []);

  useEffect(() => { refresh(); }, [refresh]);

  useSSE(useCallback((event: any) => {
    if (event.type === "bets") refresh();
  }, [refresh]));

  const resolved    = bets.filter(b => b.outcome !== null);
  const wins        = resolved.filter(b =>
    (b.side === "YES" && b.outcome === true) ||
    (b.side === "NO"  && b.outcome === false)
  );
  const totalPnl    = bets.reduce((s, b) => s + (b.pnl_usdc ?? 0), 0);
  const totalStaked = bets.reduce((s, b) => s + b.size_usdc, 0);
  const winRate     = resolved.length > 0 ? wins.length / resolved.length : null;

  if (loading) {
    return (
      <div className="flex items-center justify-center h-64">
        <div className="text-zinc-500 text-sm font-mono animate-pulse">Loading…</div>
      </div>
    );
  }

  return (
    <div className="space-y-8">
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-2xl font-bold tracking-tight">Bet History</h1>
          <p className="text-zinc-500 text-sm mt-1">
            All agent bets — placed only after SP1 attestation
          </p>
        </div>
        <div className="flex items-center gap-2 text-xs text-zinc-500 font-mono">
          <span className="w-1.5 h-1.5 rounded-full bg-emerald-400 animate-pulse" />
          live
        </div>
      </div>

      <div className="grid grid-cols-2 md:grid-cols-4 gap-4">
        <StatCard label="Total Bets"   value={bets.length} />
        <StatCard label="Total Staked" value={usd(totalStaked)} />
        <StatCard
          label="Win Rate"
          value={winRate !== null ? pct(winRate) : "—"}
          sub={`${wins.length} / ${resolved.length} resolved`}
          accent="green"
        />
        <StatCard
          label="Total P&L"
          value={usd(totalPnl)}
          accent={totalPnl >= 0 ? "green" : "red"}
        />
      </div>

      <div className="bg-[#181c2a] border border-white/5 rounded-xl overflow-hidden">
        <div className="overflow-x-auto">
          <table className="w-full text-sm">
            <thead>
              <tr className="border-b border-white/5 text-zinc-500 text-xs uppercase tracking-wider">
                <th className="text-left px-4 py-3">Market</th>
                <th className="text-left px-4 py-3">Side</th>
                <th className="text-left px-4 py-3">Price</th>
                <th className="text-left px-4 py-3">Size</th>
                <th className="text-left px-4 py-3">Confidence</th>
                <th className="text-left px-4 py-3">Attestation</th>
                <th className="text-left px-4 py-3">Settlement</th>
                <th className="text-left px-4 py-3">Outcome</th>
                <th className="text-left px-4 py-3">P&amp;L</th>
                <th className="text-left px-4 py-3">Placed</th>
              </tr>
            </thead>
            <tbody>
              {bets.map((b) => (
                <tr
                  key={b.id}
                  className="border-b border-white/5 hover:bg-white/[0.02] transition-colors"
                >
                  <td className="px-4 py-3 max-w-xs">
                    <p className="text-white truncate" title={b.question}>{b.question}</p>
                    <p className="text-zinc-600 font-mono text-xs mt-0.5">
                      {b.market_id.slice(0, 16)}…
                    </p>
                  </td>
                  <td className="px-4 py-3">
                    <span className={`font-mono font-bold text-xs px-2 py-0.5 rounded ${
                      b.side === "YES"
                        ? "bg-emerald-900 text-emerald-300"
                        : "bg-red-900 text-red-300"
                    }`}>
                      {b.side}
                    </span>
                  </td>
                  <td className="px-4 py-3 font-mono text-zinc-300">{pct(b.price)}</td>
                  <td className="px-4 py-3 font-mono text-zinc-300">{usd(b.size_usdc)}</td>
                  <td className="px-4 py-3 font-mono text-zinc-300">{pct(b.confidence)}</td>
                  <td className="px-4 py-3"><HashCell hash={b.attestation_hash} /></td>
                  <td className="px-4 py-3">
                    <HashCell
                      hash={b.tx_hash}
                      href={b.tx_hash
                        ? `https://testnet-explorer.hsk.xyz/tx/${b.tx_hash}`
                        : undefined}
                    />
                  </td>
                  <td className="px-4 py-3">
                    {b.outcome !== null ? (
                      <span className={`text-xs font-mono font-bold ${
                        (b.side === "YES" && b.outcome) || (b.side === "NO" && !b.outcome)
                          ? "text-emerald-400"
                          : "text-red-400"
                      }`}>
                        {b.outcome ? "YES" : "NO"}
                      </span>
                    ) : (
                      <span className="text-zinc-600 text-xs">pending</span>
                    )}
                  </td>
                  <td className="px-4 py-3 font-mono">
                    {b.pnl_usdc !== null ? (
                      <span className={b.pnl_usdc >= 0 ? "text-emerald-400" : "text-red-400"}>
                        {usd(b.pnl_usdc)}
                      </span>
                    ) : (
                      <span className="text-zinc-600">—</span>
                    )}
                  </td>
                  <td className="px-4 py-3 text-zinc-500 text-xs">{ago(b.placed_at)}</td>
                </tr>
              ))}
            </tbody>
          </table>
          {bets.length === 0 && (
            <p className="text-center text-zinc-600 py-12">No bets yet</p>
          )}
        </div>
      </div>
    </div>
  );
}