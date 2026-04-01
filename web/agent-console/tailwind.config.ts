import type { Config } from "tailwindcss";

export default {
  content: ["./app/**/*.{js,ts,jsx,tsx}"],
  theme: {
    extend: {
      colors: {
        panel: "#161b22",
        ink: "#0d1117",
        line: "#30363d",
        accent: "#58a6ff",
        ok: "#3fb950",
        warn: "#d29922",
      },
    },
  },
  plugins: [],
} satisfies Config;
