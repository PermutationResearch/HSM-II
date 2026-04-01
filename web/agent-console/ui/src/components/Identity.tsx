export function Identity({ name, size = "sm" }: { name: string; size?: "sm" | "md" }) {
  const initials = name
    .split(/\s+/)
    .map((w) => w[0])
    .join("")
    .slice(0, 2)
    .toUpperCase();
  const cls = size === "sm" ? "h-6 w-6 text-[10px]" : "h-8 w-8 text-xs";
  return (
    <span
      className={`inline-flex ${cls} shrink-0 items-center justify-center rounded-full bg-accent/20 font-medium text-accent`}
      title={name}
    >
      {initials || "?"}
    </span>
  );
}
