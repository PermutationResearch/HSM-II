"use client";

export function EmptyState({ message }: { message: string }) {
  return (
    <div className="rounded border border-line bg-ink/40 px-3 py-2 text-sm text-gray-600">
      {message}
    </div>
  );
}

