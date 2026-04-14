"use client";

import { useEffect, useState, useCallback } from "react";
import { useSearchParams, useRouter } from "next/navigation";
import Link from "next/link";
import { fetchJobs, type Job } from "@/lib/api";
import { StatusBadge } from "@/components/StatusBadge";
import { HashCell } from "@/components/HashCell";
import { StatCard } from "@/components/StatCard";
import { ago, ts } from "@/lib/format";
import { useSSE } from "@/hooks/useSSE";

const PAGE_SIZE = 20;

export default function ProofsPage() {
  const searchParams  = useSearchParams();
  const router        = useRouter();
  const page          = Math.max(1, parseInt(searchParams.get("page") ?? "1", 10));
  const statusFilter  = searchParams.get("status") ?? "";
  const hasTxFilter   = searchParams.get("has_tx") === "true";

  const [allJobs, setAllJobs]   = useState<Job[]>([]);
  const [loading, setLoading]   = useState(true);

  const refresh = useCallback(async () => {
    const j = await fetchJobs(500);
    setAllJobs(j);
    setLoading(false);
  }, []);

  useEffect(() => { refresh(); }, [refresh]);

  useSSE(useCallback((event: any) => {
    if (event.type === "jobs") refresh();
  }, [refresh]));

  const filtered = allJobs.filter((j) => {
    if (statusFilter && j.status !== statusFilter) return false;
    if (hasTxFilter  && !j.tx_hash)               return false;
    return true;
  });

  const byStatus = filtered.reduce((acc, j) => {
    acc[j.status] = (acc[j.status] ?? 0) + 1;
    return acc;
  }, {} as Record<string, number>);

  const avgSettleMs = (() => {
    const settled = filtered.filter(j => j.settled_at && j.submitted_at);
    if (!settled.length) return null;
    const total = settled.reduce((s, j) =>
      s + (new Date(j.settled_at!).getTime() - new Date(j.submitted_at).getTime()), 0
    );
    return Math.round(total / settled.length / 1000);
  })();

  const totalPages = Math.ceil(filtered.length / PAGE_SIZE);
  const jobs       = filtered.slice((page - 1) * PAGE_SIZE, page * PAGE_SIZE);

  const buildHref = (overrides: Record<string, string | number>) => {
    const p = new URLSearchParams();
    if (statusFilter)  p.set("status", statusFilter);
    if (hasTxFilter)   p.set("has_tx", "true");
    p.set("page", String(page));
    Object.entries(overrides).forEach(([k, v]) => p.set(k, String(v)));
    return `/proofs?${p.toString()}`;
  };

  const filterLink = (label: string, overrides: Record<string, string>) => {
    const p = new URLSearchParams(overrides);
    p.set("page", "1");
    const isActive = Object.entries(overrides).every(([k, v]) => searchParams.get(k) === v);
    return (
      <Link
        href={`/proofs?${p.toString()}`}
        className={`text-xs px-3 py-1 rounded-md transition-colors ${
          isActive
            ? "bg-[#4f6ef7] text-white"
            : "text-zinc-500 hover:text-white hover:bg-white/5"
        }`}
      >
        {label}
      </Link>
    );
  };

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
          <h1 className="text-2xl font-bold tracking-tight">Proof Explorer</h1>
          <p className="text-zinc-500 text-sm mt-1">
            SP1 inference proofs settled on HashKey testnet
          </p>
        </div>
        <div className="flex items-center gap-2 text-xs text-zinc-500 font-mono">
          <span className="w-1.5 h-1.5 rounded-full bg-emerald-400 animate-pulse" />
          live
        </div>
      </div>

      {/* Filters */}
      <div className="flex gap-2 flex-wrap">
        <Link
          href="/proofs?page=1"
          className={`text-xs px-3 py-1 rounded-md transition-colors ${
            !statusFilter && !hasTxFilter
              ? "bg-[#4f6ef7] text-white"
              : "text-zinc-500 hover:text-white hover:bg-white/5"
          }`}
        >
          All
        </Link>
        {filterLink("Settled",  { status: "settled" })}
        {filterLink("Proving",  { status: "proving" })}
        {filterLink("Running",  { status: "running" })}
        {filterLink("Failed",   { status: "failed"  })}
        {filterLink("With Tx",  { has_tx: "true"    })}
      </div>

      {/* Stats */}
      <div className="grid grid-cols-2 md:grid-cols-4 gap-4">
        <StatCard label="Settled"         value={byStatus.settled ?? 0} accent="green"  />
        <StatCard label="Proving"         value={byStatus.proving  ?? 0} accent="violet" />
        <StatCard label="Failed"          value={byStatus.failed   ?? 0} accent="red"    />
        <StatCard
          label="Avg Settle Time"
          value={avgSettleMs !== null ? `${avgSettleMs}s` : "—"}
          accent="blue"
        />
      </div>

      {/* Table */}
      <div className="bg-[#181c2a] border border-white/5 rounded-xl overflow-hidden">
        <div className="px-4 py-3 border-b border-white/5 flex items-center justify-between">
          <h2 className="text-sm font-semibold">Proof Jobs</h2>
          <span className="text-xs text-zinc-500">
            {filtered.length} total · page {page} of {Math.max(1, totalPages)}
          </span>
        </div>

        <div className="overflow-x-auto">
          <table className="w-full text-sm">
            <thead>
              <tr className="border-b border-white/5 text-zinc-500 text-xs uppercase tracking-wider">
                <th className="text-left px-4 py-3">Job ID</th>
                <th className="text-left px-4 py-3">Status</th>
                <th className="text-left px-4 py-3">Input Hash</th>
                <th className="text-left px-4 py-3">Attestation</th>
                <th className="text-left px-4 py-3">Tx Hash</th>
                <th className="text-left px-4 py-3">Submitted</th>
                <th className="text-left px-4 py-3">Settled</th>
              </tr>
            </thead>
            <tbody>
              {jobs.map((j) => (
                <tr
                  key={j.id}
                  className="border-b border-white/5 hover:bg-white/[0.02] transition-colors"
                >
                  <td className="px-4 py-3 font-mono text-xs text-zinc-400" title={j.id}>
                    {j.id.slice(0, 8)}…
                  </td>
                  <td className="px-4 py-3">
                    <StatusBadge status={j.status} />
                  </td>
                  <td className="px-4 py-3">
                    <HashCell hash={j.input_hash} />
                  </td>
                  <td className="px-4 py-3">
                    <HashCell hash={j.attestation_hash} />
                  </td>
                  <td className="px-4 py-3">
                    <HashCell
                      hash={j.tx_hash}
                      href={j.tx_hash
                        ? `https://testnet-explorer.hsk.xyz/tx/${j.tx_hash}`
                        : undefined}
                    />
                  </td>
                  <td className="px-4 py-3 text-xs text-zinc-500">
                    <span title={ts(j.submitted_at)}>{ago(j.submitted_at)}</span>
                  </td>
                  <td className="px-4 py-3 text-xs text-zinc-500">
                    {j.settled_at
                      ? <span title={ts(j.settled_at)}>{ago(j.settled_at)}</span>
                      : <span className="text-zinc-700">—</span>}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>

          {jobs.length === 0 && (
            <p className="text-center text-zinc-600 py-12">No proof jobs found</p>
          )}
        </div>

        {/* Pagination */}
        {totalPages > 1 && (
          <div className="px-4 py-3 border-t border-white/5 flex items-center justify-between">
            <Link
              href={page > 1 ? buildHref({ page: page - 1 }) : "#"}
              className={`px-3 py-1.5 rounded text-xs font-mono transition-colors ${
                page > 1
                  ? "bg-white/5 text-zinc-300 hover:bg-white/10"
                  : "text-zinc-700 pointer-events-none"
              }`}
            >
              ← Previous
            </Link>

            <div className="flex items-center gap-1">
              {Array.from({ length: Math.min(totalPages, 7) }, (_, i) => {
                const p = i + 1;
                return (
                  <Link
                    key={p}
                    href={buildHref({ page: p })}
                    className={`w-7 h-7 flex items-center justify-center rounded text-xs font-mono ${
                      p === page
                        ? "bg-[#4f6ef7] text-white"
                        : "text-zinc-500 hover:bg-white/5"
                    }`}
                  >
                    {p}
                  </Link>
                );
              })}
              {totalPages > 7 && (
                <span className="text-zinc-700 text-xs px-2">… {totalPages}</span>
              )}
            </div>

            <Link
              href={page < totalPages ? buildHref({ page: page + 1 }) : "#"}
              className={`px-3 py-1.5 rounded text-xs font-mono transition-colors ${
                page < totalPages
                  ? "bg-white/5 text-zinc-300 hover:bg-white/10"
                  : "text-zinc-700 pointer-events-none"
              }`}
            >
              Next →
            </Link>
          </div>
        )}
      </div>
    </div>
  );
}