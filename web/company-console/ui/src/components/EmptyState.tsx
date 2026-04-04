import type { LucideIcon } from "lucide-react";

/**
 * Paperclip EmptyState — https://github.com/paperclipai/paperclip/blob/master/ui/src/components/EmptyState.tsx
 * Button replaced with native styled button (no shadcn Button).
 */
interface EmptyStateProps {
  icon: LucideIcon;
  message: string;
  action?: string;
  onAction?: () => void;
}

export function EmptyState({ icon: Icon, message, action, onAction }: EmptyStateProps) {
  return (
    <div className="flex flex-col items-center justify-center gap-4 rounded-2xl border border-dashed border-[#333333] bg-card px-8 py-16 text-center">
      <div className="rounded-full border border-[#222222] bg-[#111111] p-4">
        <Icon className="h-8 w-8 text-[#666666]" strokeWidth={1.5} />
      </div>
      <p className="max-w-sm text-sm text-[#999999]">{message}</p>
      {action && onAction ? (
        <button
          type="button"
          onClick={onAction}
          className="inline-flex h-11 min-w-[44px] items-center justify-center gap-2 rounded-full border border-[#333333] bg-white px-6 font-mono text-[13px] font-normal uppercase tracking-[0.06em] text-black transition-colors duration-200 ease-out hover:border-white"
        >
          {action}
        </button>
      ) : null}
    </div>
  );
}
