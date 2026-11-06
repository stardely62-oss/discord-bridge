/** @type {import('tailwindcss').Config} */
// Colors are CSS variables (RGB triplets) so the dark and light themes share
// one set of utility classes. See src/styles/index.css.
const c = (v) => `rgb(var(--c-${v}) / <alpha-value>)`;
const soft = (v) => `rgb(var(--c-${v}) / 0.1)`;
export default {
  content: ["./index.html", "./src/**/*.{ts,tsx}"],
  theme: {
    extend: {
      colors: {
        bg: c("bg"),
        rail: c("rail"),
        sunken: c("sunken"),
        surface: { DEFAULT: c("surface"), 2: c("surface-2"), 3: c("surface-3") },
        line: { DEFAULT: c("line"), strong: c("line-strong") },
        ink: { DEFAULT: c("ink"), muted: c("ink-muted"), faint: c("ink-faint") },
        accent: { DEFAULT: c("accent"), strong: c("accent-strong"), soft: soft("accent") },
        ok: { DEFAULT: c("ok"), soft: soft("ok") },
        warn: { DEFAULT: c("warn"), soft: soft("warn") },
        bad: { DEFAULT: c("bad"), soft: soft("bad") },
      },
      fontFamily: {
        sans: ['"Segoe UI Variable Text"', '"Segoe UI"', "system-ui", "sans-serif"],
        mono: ['"Cascadia Mono"', "Consolas", "ui-monospace", "monospace"],
      },
      screens: { narrow: { max: "1060px" } },
      boxShadow: { pop: "0 8px 30px -6px rgb(0 0 0 / 0.35)" },
    },
  },
  plugins: [],
};
