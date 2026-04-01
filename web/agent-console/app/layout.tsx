import type { Metadata } from "next";
import "./globals.css";

export const metadata: Metadata = {
  title: "HSM Agent Console",
  description: "Trail, memory, and agent KPIs",
};

export default function RootLayout({ children }: { children: React.ReactNode }) {
  return (
    <html lang="en">
      <body className="min-h-screen">{children}</body>
    </html>
  );
}
