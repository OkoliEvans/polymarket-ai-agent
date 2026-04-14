import { Suspense } from "react";

export default function ProofsLayout({ children }: { children: React.ReactNode }) {
  return (
    <Suspense fallback={
      <div className="flex items-center justify-center h-64">
        <div className="text-zinc-500 text-sm font-mono animate-pulse">Loading…</div>
      </div>
    }>
      {children}
    </Suspense>
  );
}