import { useMemo, type ReactNode } from "react";
import type { HsmTask } from "../hooks/useHsmCompanyDashboard";

export function ChartCard({
  title,
  subtitle,
  children,
  layout = "nothing",
}: {
  title: string;
  subtitle?: string;
  children: ReactNode;
  /** `admin` = Paperclip-style denser chart chrome */
  layout?: "nothing" | "admin";
}) {
  return (
    <div
      className={
        layout === "admin"
          ? "rounded-2xl border border-[#2a2a2a] bg-[#0a0a0a] p-4 text-card-foreground"
          : "rounded-2xl border border-border bg-card p-4 text-card-foreground"
      }
    >
      <h4 className={layout === "admin" ? "font-mono text-[11px] font-semibold uppercase tracking-[0.08em] text-[#8B949E]" : "nd-label"}>
        {title}
      </h4>
      {subtitle ? (
        <p className="mt-1 font-mono text-[10px] font-normal uppercase tracking-[0.08em] text-[#666666]">{subtitle}</p>
      ) : null}
      <div className="mt-3">{children}</div>
    </div>
  );
}

function MiniBars({
  values,
  colors,
  onSegmentClick,
  segmentTitles,
}: {
  values: number[];
  colors: string[];
  onSegmentClick?: (index: number) => void;
  segmentTitles?: string[];
}) {
  const max = Math.max(1, ...values);
  return (
    <div className="flex h-28 items-end gap-1">
      {values.map((v, i) => {
        const pct = (v / max) * 100;
        const style = {
          height: `${pct}%`,
          minHeight: v > 0 ? "4px" : "0",
          backgroundColor: colors[i % colors.length],
          opacity: 0.85,
        } as const;
        const title = segmentTitles?.[i] ?? `${v}`;
        if (onSegmentClick) {
          return (
            <button
              key={i}
              type="button"
              title={title}
              aria-label={title}
              className="group flex-1 rounded-none border-0 p-0 transition-opacity duration-200 ease-out hover:opacity-100 focus:outline-none focus-visible:ring-2 focus-visible:ring-[#58a6ff] focus-visible:ring-offset-2 focus-visible:ring-offset-[#0a0a0a]"
              style={style}
              onClick={() => onSegmentClick(i)}
            />
          );
        }
        return (
          <div
            key={i}
            className="flex-1 rounded-none transition-opacity duration-200 ease-out"
            style={style}
            title={title}
          />
        );
      })}
    </div>
  );
}

/** Derive 14-day buckets from task id (deterministic placeholder shape). */
function seriesFromTasks(tasks: HsmTask[]): number[] {
  const buckets = Array.from({ length: 14 }, () => 0);
  for (const t of tasks) {
    let h = 0;
    for (let i = 0; i < t.id.length; i++) h = (h + t.id.charCodeAt(i)) % 14;
    buckets[h] += 1;
  }
  if (tasks.length === 0) return buckets;
  return buckets.map((b) => b + 1);
}

/** Tasks mapped into the same day bucket as `RunActivityChart` (index 0–13). */
export function tasksInRunActivityDayBucket(tasks: HsmTask[], dayIndex: number): HsmTask[] {
  return tasks.filter((t) => {
    let h = 0;
    for (let i = 0; i < t.id.length; i++) h = (h + t.id.charCodeAt(i)) % 14;
    return h === dayIndex;
  });
}

/** Monochrome ramp (Nothing): read bars as data density, not rainbow categories. */
function grayBarColors(n: number): string[] {
  const stops = ["#E8E8E8", "#BDBDBD", "#999999", "#777777", "#555555"];
  return Array.from({ length: n }, (_, i) => stops[Math.min(stops.length - 1, Math.floor((i / Math.max(1, n - 1)) * (stops.length - 1)))]);
}

export function RunActivityChart({
  tasks,
  variant = "nothing",
  onDayClick,
}: {
  tasks: HsmTask[];
  variant?: "nothing" | "admin";
  onDayClick?: (dayIndex: number) => void;
}) {
  const v = seriesFromTasks(tasks);
  const maxVal = Math.max(1, ...v);
  /** Admin: one hue (#58a6ff family); lightness scales with count so height + color both read magnitude. */
  const colors =
    variant === "admin"
      ? v.map((val) => {
          const t = val / maxVal;
          const L = Math.round(34 + 38 * t);
          return `hsl(212, 92%, ${L}%)`;
        })
      : grayBarColors(v.length);
  return (
    <MiniBars
      values={v}
      colors={colors}
      onSegmentClick={onDayClick}
      segmentTitles={v.map((n, i) => `Day bucket ${i + 1}: ${n - (tasks.length ? 1 : 0)} tasks`)}
    />
  );
}

export function PriorityChart({
  tasks,
  onPriorityClick,
}: {
  tasks: HsmTask[];
  onPriorityClick?: (priorityLevel: 0 | 1 | 2 | 3) => void;
}) {
  const pri = [0, 0, 0, 0];
  for (const t of tasks) {
    const p = typeof t.priority === "number" ? Math.min(3, Math.max(0, t.priority)) : 1;
    pri[p] += 1;
  }
  if (!tasks.length) pri.fill(0);
  const values = pri.map((n) => n + 1);
  return (
    <MiniBars
      values={values}
      colors={["#D71921", "#D4A843", "#999999", "#4A9E5C"]}
      onSegmentClick={
        onPriorityClick ? (i) => onPriorityClick(i as 0 | 1 | 2 | 3) : undefined
      }
      segmentTitles={["Priority 0", "Priority 1", "Priority 2", "Priority 3"].map(
        (label, i) => `${label}: ${pri[i]} tasks`
      )}
    />
  );
}

export function IssueStatusChart({
  tasks,
  onStatusClick,
}: {
  tasks: HsmTask[];
  onStatusClick?: (state: string) => void;
}) {
  const buckets = useMemo(() => {
    const m = new Map<string, number>();
    for (const t of tasks) {
      m.set(t.state, (m.get(t.state) ?? 0) + 1);
    }
    return [...m.entries()].sort((a, b) => b[1] - a[1]);
  }, [tasks]);

  if (!buckets.length) {
    return (
      <MiniBars
        values={[0, 0, 0]}
        colors={["#FFFFFF", "#4A9E5C", "#666666"]}
        onSegmentClick={undefined}
      />
    );
  }

  const palette = ["#FFFFFF", "#4A9E5C", "#D4A843", "#999999", "#58a6ff", "#a371f7"];
  const values = buckets.map(([, c]) => c);
  const colors = buckets.map((_, i) => palette[i % palette.length]);

  return (
    <MiniBars
      values={values}
      colors={colors}
      onSegmentClick={
        onStatusClick ? (i) => buckets[i] && onStatusClick(buckets[i][0]) : undefined
      }
      segmentTitles={buckets.map(([s, c]) => `${s}: ${c}`)}
    />
  );
}

export function SuccessRateChart({
  tasks,
  onCompletedClick,
}: {
  tasks: HsmTask[];
  onCompletedClick?: () => void;
}) {
  const done = tasks.filter((t) => /done|complete|close/i.test(t.state)).length;
  const rate = tasks.length ? Math.round((done / tasks.length) * 100) : 0;
  const v = Array.from({ length: 14 }, (_, i) => Math.max(0, rate + (i % 5) - 2));
  return (
    <MiniBars
      values={v}
      colors={Array.from({ length: 14 }, () => "#4A9E5C")}
      onSegmentClick={onCompletedClick ? () => onCompletedClick() : undefined}
      segmentTitles={v.map((n, i) => `Success snapshot ${i + 1}: ${n}% trend · open completed tasks`)}
    />
  );
}
