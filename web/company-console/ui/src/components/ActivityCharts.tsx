import { useMemo, type ReactNode } from "react";
import type { HsmTask } from "../hooks/useHsmCompanyDashboard";

const DAY_BUCKET_COUNT = 14;

type LegendItem = {
  color: string;
  label: string;
};

type DayBucket = {
  key: string;
  shortLabel: string;
  longLabel: string;
  startMs: number;
  endMs: number;
};

function startOfLocalDay(input: Date): Date {
  return new Date(input.getFullYear(), input.getMonth(), input.getDate());
}

function buildRecentDayBuckets(count: number = DAY_BUCKET_COUNT): DayBucket[] {
  const today = startOfLocalDay(new Date());
  const buckets: DayBucket[] = [];
  for (let offset = count - 1; offset >= 0; offset -= 1) {
    const start = new Date(today);
    start.setDate(today.getDate() - offset);
    const end = new Date(start);
    end.setDate(start.getDate() + 1);
    buckets.push({
      key: start.toISOString().slice(0, 10),
      shortLabel: start.toLocaleDateString(undefined, { month: "short", day: "numeric" }),
      longLabel: start.toLocaleDateString(undefined, { month: "short", day: "numeric", year: "numeric" }),
      startMs: start.getTime(),
      endMs: end.getTime(),
    });
  }
  return buckets;
}

function bucketIndexFromTimestamp(timestampMs: number, buckets: DayBucket[]): number {
  if (!Number.isFinite(timestampMs)) return -1;
  return buckets.findIndex((bucket) => timestampMs >= bucket.startMs && timestampMs < bucket.endMs);
}

function parseTimestamp(value?: string | null): number {
  if (!value) return Number.NaN;
  const parsed = Date.parse(value);
  return Number.isFinite(parsed) ? parsed : Number.NaN;
}

function taskActivityTimestamp(task: HsmTask): number {
  return (
    parseTimestamp(task.run?.finished_at) ||
    parseTimestamp(task.run?.updated_at) ||
    parseTimestamp(task.due_at)
  );
}

function taskRunTerminalTimestamp(task: HsmTask): number {
  if (task.run?.status !== "success" && task.run?.status !== "error") return Number.NaN;
  return parseTimestamp(task.run?.finished_at) || parseTimestamp(task.run?.updated_at);
}

function ChartLegend({ items }: { items: LegendItem[] }) {
  return (
    <div className="mt-3 flex flex-wrap items-center gap-x-3 gap-y-2">
      {items.map((item) => (
        <span key={`${item.label}-${item.color}`} className="inline-flex items-center gap-1.5 font-mono text-[10px] uppercase tracking-[0.08em] text-[#6E7681]">
          <span className="h-2 w-2 rounded-full" style={{ backgroundColor: item.color }} />
          {item.label}
        </span>
      ))}
    </div>
  );
}

function ChartDateRange({ buckets }: { buckets: DayBucket[] }) {
  if (buckets.length === 0) return null;
  return (
    <div className="mt-3 flex items-center justify-between font-mono text-[10px] uppercase tracking-[0.08em] text-[#666666]">
      <span>{buckets[0]?.shortLabel}</span>
      <span>{buckets[buckets.length - 1]?.shortLabel}</span>
    </div>
  );
}

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
          ? "rounded-2xl bg-[#0a0a0a] p-4 text-card-foreground"
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

export function tasksInRunActivityDayBucket(tasks: HsmTask[], dayIndex: number): HsmTask[] {
  const buckets = buildRecentDayBuckets();
  const target = buckets[dayIndex];
  if (!target) return [];
  return tasks.filter((t) => {
    const timestampMs = taskActivityTimestamp(t);
    return timestampMs >= target.startMs && timestampMs < target.endMs;
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
  const buckets = useMemo(() => buildRecentDayBuckets(), []);
  const v = useMemo(() => {
    const values = Array.from({ length: buckets.length }, () => 0);
    for (const task of tasks) {
      const bucketIndex = bucketIndexFromTimestamp(taskActivityTimestamp(task), buckets);
      if (bucketIndex >= 0) values[bucketIndex] += 1;
    }
    return values;
  }, [buckets, tasks]);
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
    <>
      <MiniBars
        values={v}
        colors={colors}
        onSegmentClick={onDayClick}
        segmentTitles={v.map((n, i) => `${buckets[i]?.longLabel}: ${n} task updates`)}
      />
      <ChartLegend items={[{ color: "#58a6ff", label: "Task updates by day" }]} />
      <ChartDateRange buckets={buckets} />
    </>
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
  const values = pri;
  return (
    <>
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
      <ChartLegend
        items={[
          { color: "#D71921", label: "P0 critical" },
          { color: "#D4A843", label: "P1 high" },
          { color: "#999999", label: "P2 normal" },
          { color: "#4A9E5C", label: "P3 low" },
        ]}
      />
    </>
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
    <>
      <MiniBars
        values={values}
        colors={colors}
        onSegmentClick={
          onStatusClick ? (i) => buckets[i] && onStatusClick(buckets[i][0]) : undefined
        }
        segmentTitles={buckets.map(([s, c]) => `${s}: ${c}`)}
      />
      <ChartLegend items={buckets.slice(0, palette.length).map(([status], index) => ({ color: colors[index], label: status }))} />
    </>
  );
}

export function SuccessRateChart({
  tasks,
  onCompletedClick,
}: {
  tasks: HsmTask[];
  onCompletedClick?: () => void;
}) {
  const buckets = useMemo(() => buildRecentDayBuckets(), []);
  const daily = useMemo(() => {
    const rows = buckets.map((bucket) => ({
      bucket,
      success: 0,
      failure: 0,
      total: 0,
      rate: 0,
    }));
    for (const task of tasks) {
      const bucketIndex = bucketIndexFromTimestamp(taskRunTerminalTimestamp(task), buckets);
      if (bucketIndex < 0) continue;
      const row = rows[bucketIndex];
      row.total += 1;
      if (task.run?.status === "success") row.success += 1;
      if (task.run?.status === "error") row.failure += 1;
    }
    for (const row of rows) {
      row.rate = row.total > 0 ? Math.round((row.success / row.total) * 100) : 0;
    }
    return rows;
  }, [buckets, tasks]);
  const values = daily.map((row) => row.rate);
  const colors = daily.map((row) => {
    if (row.total === 0) return "#30363D";
    if (row.rate >= 80) return "#4A9E5C";
    if (row.rate >= 50) return "#D4A843";
    return "#D71921";
  });
  return (
    <>
      <MiniBars
        values={values}
        colors={colors}
        onSegmentClick={onCompletedClick ? () => onCompletedClick() : undefined}
        segmentTitles={daily.map(
          (row) =>
            `${row.bucket.longLabel}: ${row.rate}% success (${row.success}/${row.total} successful${row.failure ? `, ${row.failure} failed` : ""})`
        )}
      />
      <ChartLegend
        items={[
          { color: "#4A9E5C", label: "80-100% success" },
          { color: "#D4A843", label: "50-79% success" },
          { color: "#D71921", label: "0-49% success" },
          { color: "#30363D", label: "No terminal runs" },
        ]}
      />
      <ChartDateRange buckets={buckets} />
    </>
  );
}
