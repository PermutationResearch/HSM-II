import type { Metadata } from "next";
import "./globals.css";

export const metadata: Metadata = {
  title: "HSM-II Dashboard",
  description: "Hyper-Stigmergic Morphogenesis II — live memory & peer visualization",
};

export default function RootLayout({ children }: { children: React.ReactNode }) {
  return (
    <html lang="en">
      <body className="bg-zinc-950 text-zinc-100 min-h-screen font-mono antialiased">
        <header className="border-b border-zinc-800 px-6 py-3 flex items-center gap-4">
          <span className="text-emerald-400 font-bold tracking-tight">HSM-II</span>
          <nav className="flex gap-4 text-sm text-zinc-400">
            <a href="/" className="hover:text-zinc-100 transition-colors">Overview</a>
            <a href="/peers" className="hover:text-zinc-100 transition-colors">Peers</a>
            <a href="/memory" className="hover:text-zinc-100 transition-colors">Memory</a>
            <a href="/council" className="hover:text-zinc-100 transition-colors">Council</a>
            <a href="/llm-chat" className="hover:text-zinc-100 transition-colors">LLM stream</a>
            <a href="/gen-ui" className="hover:text-zinc-100 transition-colors">Gen UI</a>
            <a href="/architecture" className="hover:text-zinc-100 transition-colors">Architecture</a>
          </nav>
        </header>
        <main className="px-6 py-6">{children}</main>
      </body>
    </html>
  );
}
