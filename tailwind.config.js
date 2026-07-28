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
        // Coral/amber brand accent
        ember: {
          50: "#fff7ed",
          100: "#ffedd5",
          200: "#ffedd5",
          400: "#fdba74",
          500: "#fb923c",
          600: "#ea580c",
          700: "#c2410c",
        },
        brand: {
          50: "#eff6ff",
          100: "#dbeafe",
          200: "#bfdbfe",
          300: "#93c5fd",
          400: "#60a5fa",
          500: "#3b82f6",
          600: "#2563eb",
          700: "#1d4ed8",
          800: "#1e40af",
          900: "#1e3a8a",
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
