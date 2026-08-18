/** @type {import('tailwindcss').Config} */
export default {
  content: ["./index.html", "./src/**/*.{js,ts,jsx,tsx}"],
  darkMode: "class",
  theme: {
    extend: {
      colors: {
        // Graphite cool-gray dark surface scale（冷蓝灰主题）
        graphite: {
          50: "#f4f4f8",
          100: "#e8e8ed",
          200: "#c6c6ce",
          300: "#a8a8b3",
          400: "#6e6e7a",
          500: "#2b2b35", // border
          600: "#1f1f26", // input / hover
          700: "#18181d", // card / sidebar
          800: "#101014", // page bg
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
