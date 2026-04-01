/**
 * Nothing-style loading: bracket label, flat blocks (no pulse/skeleton shimmer).
 */
interface PageSkeletonProps {
  variant?: "list" | "dashboard";
}

export function PageSkeleton({ variant = "list" }: PageSkeletonProps) {
  if (variant === "dashboard") {
    return (
      <div className="space-y-6">
        <p className="font-mono text-[11px] uppercase tracking-[0.08em] text-[#666666]">[LOADING…]</p>
        <div className="grid grid-cols-1 gap-2 sm:grid-cols-2 xl:grid-cols-4">
          {Array.from({ length: 4 }).map((_, i) => (
            <div key={i} className="h-28 rounded-2xl border border-[#222222] bg-[#111111]" />
          ))}
        </div>
        <div className="grid grid-cols-2 gap-2 xl:grid-cols-4">
          {Array.from({ length: 4 }).map((_, i) => (
            <div key={i} className="h-32 rounded-2xl border border-[#222222] bg-[#111111]" />
          ))}
        </div>
        <div className="grid grid-cols-2 gap-4 lg:grid-cols-4">
          {Array.from({ length: 4 }).map((_, i) => (
            <div key={i} className="h-44 rounded-2xl border border-[#222222] bg-[#111111]" />
          ))}
        </div>
      </div>
    );
  }

  return (
    <div className="space-y-3">
      <p className="font-mono text-[11px] uppercase tracking-[0.08em] text-[#666666]">[LOADING…]</p>
      {Array.from({ length: 7 }).map((_, i) => (
        <div key={i} className="h-12 rounded-lg border border-[#222222] bg-[#111111]" />
      ))}
    </div>
  );
}
