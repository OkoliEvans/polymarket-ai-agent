import type { Metadata } from "next";
import "./globals.css";
import Link from "next/link";

export const metadata: Metadata = {
  title: "Veil Agent",
  description: "Verifiable inference trading on Polymarket",
};

const NAV = [
  { href: "/",       label: "Dashboard" },
  { href: "/bets",   label: "Bets"      },
  { href: "/proofs", label: "Proofs"    },
];

export default function RootLayout({ children }: { children: React.ReactNode }) {
  return (
    <html lang="en">
      <head>
        <link
          href="https://fonts.googleapis.com/css2?family=JetBrains+Mono:wght@400;500;700&display=swap"
          rel="stylesheet"
        />
      </head>
      <body className="bg-[#0d0f1a] text-white min-h-screen antialiased">
        <header className="border-b border-white/5 bg-[#12151f] sticky top-0 z-40">
          <div className="max-w-7xl mx-auto px-6 flex items-center gap-8 h-14">
            <span className="font-mono font-bold text-[#4f6ef7] tracking-tight text-lg">
              ⬡ VEIL
            </span>
            <nav className="flex items-center gap-1">
              {NAV.map((n) => (
                <Link
                  key={n.href}
                  href={n.href}
                  className="px-3 py-1.5 rounded-md text-sm text-zinc-400 hover:text-white hover:bg-white/5 transition-colors"
                >
                  {n.label}
                </Link>
              ))}
            </nav>
            <div className="ml-auto flex items-center gap-2">
              <span className="w-2 h-2 rounded-full bg-emerald-400 animate-pulse" />
              <span className="text-xs text-zinc-500 font-mono">PAPER TRADING</span>
            </div>
          </div>
        </header>
        <main className="max-w-7xl mx-auto px-6 py-8">
          {children}
        </main>
      </body>
    </html>
  );
}