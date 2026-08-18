/** @type {import('tailwindcss').Config} */
export default {
  content: ["./index.html", "./src/**/*.{js,ts,jsx,tsx}"],
  darkMode: "class",
  theme: {
    extend: {
      colors: {
        // Graphite pure-neutral dark surface scale（纯净中性灰主题）
        graphite: {
          50: "#f5f5f7",
          100: "#ededee",
          200: "#c8c8cc",
          300: "#a6a6aa",
          400: "#66666b",
          500: "#2c2c2f", // border
          600: "#1f1f21", // input / hover
          700: "#171718", // card / sidebar
          800: "#0c0c0d", // page bg
        },
        // 夜间主题强调色——珊瑚红（亮色主题仍用 red-*）
        accent: {
          300: "#ff8a80",
          400: "#f47f75",
          500: "#f0655a",
          600: "#e05548",
        },
        // Status colors
        success: { 400: "#34d399", 500: "#10b981", 600: "#059669" },
        danger: { 400: "#f87171", 500: "#ef4444", 600: "#dc2626" },
        warning: { 400: "#fbbf24", 500: "#f59e0b", 600: "#d97706" },
      },
      fontFamily: {
        mono: ['"JetBrains Mono"', 'ui-monospace', 'SF Mono', 'Menlo', 'Consolas', 'monospace'],
      },
    },
  },
  plugins: [],
};
