"use client";

type Tone = "green" | "amber" | "red" | "gray";

function toneForStatus(status: string): Tone {
  const s = status.trim().toUpperCase();
  if (s === "AUTO" || s === "IN_PROGRESS" || s === "OPEN") return "green";
  if (s === "ADMIN_REQUIRED" || s === "WAITING_ADMIN" || s === "AT_RISK") return "amber";
  if (s === "BLOCKED" || s === "OVERDUE") return "red";
  return "gray";
}

export function StatusChip({ label, tone }: { label: string; tone?: Tone }) {
  const t = tone ?? toneForStatus(label);
  const cls =
    t === "green"
      ? "bg-emerald-900/40 text-emerald-300"
      : t === "amber"
      ? "bg-amber-900/40 text-amber-300"
      : t === "red"
      ? "bg-red-900/40 text-red-300"
      : "bg-white/10 text-gray-300";
  return <span className={`inline-block rounded px-1.5 py-0.5 text-[10px] ${cls}`}>{label}</span>;
}

