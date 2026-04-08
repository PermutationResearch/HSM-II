import type { Metadata } from "next";
import { Providers } from "./providers";
import "./globals.css";

export const metadata: Metadata = {
  title: "HSM Agent Console",
  description: "Trail, memory, and agent KPIs",
};

/**
 * Nothing-inspired typography (see `.cursor/skills/nothing-design/`):
 * - **Space Grotesk / close local fallback** — body, UI, headings via CSS var.
 * - **Space Mono / close local fallback** — ALL CAPS labels, data, IDs via CSS var.
 * Optional display: Doto only for rare hero moments (not loaded by default).
 * Company OS + console: `/api/company` & `/api/console` → proxies to `HSM_CONSOLE_URL`.
 */
export default function RootLayout({ children }: { children: React.ReactNode }) {
  return (
    <html lang="en">
      <body className="min-h-screen font-sans antialiased">
        <Providers>{children}</Providers>
      </body>
    </html>
  );
}
