import type { Metadata } from "next";
import { Space_Grotesk, Space_Mono } from "next/font/google";
import { Providers } from "./providers";
import "./globals.css";

export const metadata: Metadata = {
  title: "HSM Agent Console",
  description: "Trail, memory, and agent KPIs",
};

/** Nothing design system: body / UI + instrument labels (see `tailwind.config.ts` fontFamily). */
const fontSans = Space_Grotesk({
  subsets: ["latin"],
  variable: "--font-space-grotesk",
  display: "swap",
});

const fontMono = Space_Mono({
  subsets: ["latin"],
  weight: ["400", "700"],
  variable: "--font-space-mono",
  display: "swap",
});

/**
 * Nothing-inspired typography (see `.cursor/skills/nothing-design/`):
 * - **Space Grotesk** — body, UI, headings (`next/font` → `--font-space-grotesk`).
 * - **Space Mono** — ALL CAPS labels, data, IDs (`next/font` → `--font-space-mono`).
 * Optional display: Doto only for rare hero moments (not loaded by default).
 * Company OS + console: `/api/company` & `/api/console` → proxies to `HSM_CONSOLE_URL`.
 */
export default function RootLayout({ children }: { children: React.ReactNode }) {
  return (
    <html lang="en" className={`${fontSans.variable} ${fontMono.variable}`}>
      <body className="min-h-screen font-sans antialiased">
        <Providers>{children}</Providers>
      </body>
    </html>
  );
}
