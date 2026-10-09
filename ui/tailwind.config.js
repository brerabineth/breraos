/** @type {import('tailwindcss').Config} */
export default {
  content: ["./index.html", "./src/**/*.{ts,css}"],
  theme: {
    extend: {
      colors: {
        void: "#05060b",
        panel: "rgba(255,255,255,0.045)",
        anchor: {
          secure: "#2dd4bf",
          warn: "#fbbf24",
          alert: "#f87171",
          locked: "#64748b",
        },
      },
      fontFamily: {
        mono: [
          "JetBrains Mono",
          "SFMono-Regular",
          "Consolas",
          "Liberation Mono",
          "monospace",
        ],
      },
    },
  },
  plugins: [],
};
