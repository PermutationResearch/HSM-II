import type { Config } from "tailwindcss";

/** Nothing design system (dark) + HSM aliases — see `.cursor/skills/nothing-design/references/tokens.md` */
export default {
  content: ["./app/**/*.{js,ts,jsx,tsx}", "./ui/src/**/*.{js,ts,jsx,tsx}"],
  theme: {
    extend: {
      fontFamily: {
        sans: ["var(--font-space-grotesk)", "DM Sans", "system-ui", "sans-serif"],
        mono: ["var(--font-space-mono)", "JetBrains Mono", "ui-monospace", "monospace"],
      },
      colors: {
        /* Nothing dark palette → shadcn-style names for Dashboard components */
        border: "#222222",
        background: "#000000",
        foreground: "#E8E8E8",
        card: {
          DEFAULT: "#111111",
          foreground: "#E8E8E8",
        },
        muted: {
          DEFAULT: "#1A1A1A",
          foreground: "#999999",
        },
        destructive: {
          DEFAULT: "#D71921",
          foreground: "#FFFFFF",
        },
        /* HSM / status (Nothing success & warning) */
        panel: "#111111",
        ink: "#000000",
        line: "#222222",
        accent: "#5B9BF6",
        ok: "#4A9E5C",
        warn: "#D4A843",
        accentSignal: "#D71921",
      },
    },
  },
  plugins: [],
} satisfies Config;
