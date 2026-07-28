/** @type {import('tailwindcss').Config} */
export default {
  content: ["./index.html", "./src/**/*.{js,ts,jsx,tsx}"],
  darkMode: "class",
  theme: {
    extend: {
      colors: {
        // Graphite warm-gray dark surface scale
        graphite: {
          50: "#ecece8",
          100: "#d8d8d2",
          200: "#93938d",
          300: "#6d6d68",
          400: "#4a4a46",
          500: "#2e2e36", // border
          600: "#25252b", // input / hover
          700: "#1e1e22", // card / sidebar
          800: "#161618", // page bg
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
