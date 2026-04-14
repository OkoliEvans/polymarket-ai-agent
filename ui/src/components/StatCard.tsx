export function StatCard({
  label, value, sub, accent,
}: {
  label: string;
  value: string | number;
  sub?: string;
  accent?: "green" | "red" | "blue" | "violet";
}) {
  const colors: Record<string, string> = {
    green:  "text-emerald-400",
    red:    "text-red-400",
    blue:   "text-[#4f6ef7]",
    violet: "text-violet-400",
  };

  return (
    <div className="bg-[#181c2a] border border-white/5 rounded-xl p-5">
      <p className="text-xs text-zinc-500 uppercase tracking-widest mb-1">{label}</p>
      <p className={`text-2xl font-bold font-mono ${accent ? colors[accent] : "text-white"}`}>
        {value}
      </p>
      {sub && <p className="text-xs text-zinc-600 mt-1">{sub}</p>}
    </div>
  );
}