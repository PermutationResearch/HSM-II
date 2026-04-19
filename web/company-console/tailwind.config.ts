import type { Config } from "tailwindcss";

/**
 * Nothing dark (OLED) + shadcn/Radix + Paperclip-class admin chrome.
 * Semantic colors: CSS vars in `app/globals.css`. Structural utilities: `pc-*` below + @layer components.
 */
export default {
  content: ["./app/**/*.{js,ts,jsx,tsx}", "./ui/src/**/*.{js,ts,jsx,tsx}"],
  theme: {
    extend: {
      boxShadow: {
        /** Paperclip-style inset highlight on dark panels */
        "pc-card": "inset 0 1px 0 rgb(255 255 255 / 0.04), 0 1px 2px rgb(0 0 0 / 0.45)",
        "pc-glow": "0 0 0 1px rgb(88 166 255 / 0.18)",
        /** Nothing OLED lift */
        "nd-elevate": "0 8px 32px rgb(0 0 0 / 0.55)",
      },
      fontFamily: {
        sans: ["var(--font-space-grotesk)", "DM Sans", "system-ui", "sans-serif"],
        mono: ["var(--font-space-mono)", "JetBrains Mono", "ui-monospace", "monospace"],
        serif: ["var(--font-lora)", "Lora", "Georgia", "ui-serif", "serif"],
      },
      borderRadius: {
        lg: "var(--radius)",
        md: "calc(var(--radius) - 2px)",
        sm: "calc(var(--radius) - 4px)",
      },
      colors: {
        border: "var(--border)",
        input: "var(--input)",
        ring: "var(--ring)",
        background: "var(--background)",
        foreground: "var(--foreground)",
        primary: {
          DEFAULT: "var(--primary)",
          foreground: "var(--primary-foreground)",
        },
        secondary: {
          DEFAULT: "var(--secondary)",
          foreground: "var(--secondary-foreground)",
        },
        card: {
          DEFAULT: "var(--card)",
          foreground: "var(--card-foreground)",
        },
        popover: {
          DEFAULT: "var(--popover)",
          foreground: "var(--popover-foreground)",
        },
        muted: {
          DEFAULT: "var(--muted)",
          foreground: "var(--muted-foreground)",
        },
        accent: {
          DEFAULT: "var(--accent)",
          foreground: "var(--accent-foreground)",
        },
        destructive: {
          DEFAULT: "var(--destructive)",
          foreground: "var(--destructive-foreground)",
        },
        /* HSM / Nothing aliases (legacy class names in console) */
        panel: "#111111",
        ink: "#000000",
        line: "#222222",
        ok: "#4A9E5C",
        warn: "#D4A843",
        accentSignal: "#D71921",
        /** GitHub-dark admin chrome */
        admin: {
          bg: "#010409",
          panel: "#0d1117",
          border: "#30363d",
          muted: "#8b949e",
        },
      },
      keyframes: {
        "accordion-down": { from: { height: "0" }, to: { height: "var(--radix-accordion-content-height)" } },
        "accordion-up": { from: { height: "var(--radix-accordion-content-height)" }, to: { height: "0" } },
      },
      animation: {
        "accordion-down": "accordion-down 0.2s ease-out",
        "accordion-up": "accordion-up 0.2s ease-out",
      },
    },
  },
  plugins: [],
} satisfies Config;
