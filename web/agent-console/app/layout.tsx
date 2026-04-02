import type { Metadata } from "next";
import { Space_Grotesk, Space_Mono } from "next/font/google";
import { Providers } from "./providers";
import "./globals.css";

const spaceGrotesk = Space_Grotesk({
  subsets: ["latin"],
  weight: ["300", "400", "500", "700"],
  variable: "--font-space-grotesk",
  display: "swap",
});

const spaceMono = Space_Mono({
  subsets: ["latin"],
  weight: ["400", "700"],
  variable: "--font-space-mono",
  display: "swap",
});

export const metadata: Metadata = {
  title: "HSM Agent Console",
  description: "Trail, memory, and agent KPIs",
};

/**
 * Nothing-style dark shell: Space Grotesk (UI) + Space Mono (labels / data).
 * Fonts: https://fonts.google.com — Space Grotesk, Space Mono
 * Company OS + console API: same-origin `/api/company` & `/api/console` → App Router proxies to `HSM_CONSOLE_URL` (see `app/api/company/[[...path]]/route.ts` and `app/api/console/[[...path]]/route.ts`).
 */
export default function RootLayout({ children }: { children: React.ReactNode }) {
  return (
    <html lang="en" className={`${spaceGrotesk.variable} ${spaceMono.variable}`}>
      <body className="min-h-screen font-sans antialiased">
        <Providers>{children}</Providers>
      </body>
    </html>
  );
}
