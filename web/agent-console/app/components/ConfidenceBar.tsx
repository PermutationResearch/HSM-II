"use client";

export function ConfidenceBar({ label, value }: { label: string; value: number }) {
  const pct = Math.max(0, Math.min(100, Math.round(value * 100)));
  const tone =
    pct >= 80 ? "bg-emerald-500/70" : pct >= 60 ? "bg-amber-500/70" : "bg-red-500/70";
  return (
    <div className="space-y-1">
      <div className="flex justify-between text-[11px] text-gray-400">
        <span>{label}</span>
        <span>{pct}%</span>
      </div>
      <div className="h-1.5 rounded bg-white/10">
        <div className={`h-1.5 rounded ${tone}`} style={{ width: `${pct}%` }} />
      </div>
    </div>
  );
}

