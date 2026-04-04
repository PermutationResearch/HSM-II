/** Pretty-print arbitrary JSON for dashboards (replaces invalid `@vercel-labs/json-render` dep). */

export function PrettyJson({ value }: { value: unknown }) {
  let text: string;
  try {
    text = JSON.stringify(value, null, 2);
  } catch {
    text = String(value);
  }
  return (
    <pre className="whitespace-pre-wrap break-words font-mono text-xs leading-relaxed text-zinc-300">
      {text}
    </pre>
  );
}
