// xterm color palettes for both themes. Kept close to the UI tokens so a
// session feels like part of the app.

export interface TerminalTheme {
  background: string;
  foreground: string;
  cursor: string;
  cursorAccent: string;
  selectionBackground: string;
  black: string;
  red: string;
  green: string;
  yellow: string;
  blue: string;
  magenta: string;
  cyan: string;
  white: string;
  brightBlack: string;
  brightRed: string;
  brightGreen: string;
  brightYellow: string;
  brightBlue: string;
  brightMagenta: string;
  brightCyan: string;
  brightWhite: string;
}

export const DARK_THEME: TerminalTheme = {
  background: "#0f1115",
  foreground: "#e7eaef",
  cursor: "#14b8a6",
  cursorAccent: "#0f1115",
  selectionBackground: "rgba(20, 184, 166, 0.32)",
  black: "#1b2028",
  red: "#e5484d",
  green: "#46b450",
  yellow: "#f5a524",
  blue: "#4c8dff",
  magenta: "#c069d6",
  cyan: "#2dd4bf",
  white: "#c9d1d9",
  brightBlack: "#5f6774",
  brightRed: "#ff6b6e",
  brightGreen: "#5fd167",
  brightYellow: "#ffbe3d",
  brightBlue: "#71a4ff",
  brightMagenta: "#d98ae3",
  brightCyan: "#5eead4",
  brightWhite: "#eef1f5",
};

export const LIGHT_THEME: TerminalTheme = {
  background: "#ffffff",
  foreground: "#22262d",
  cursor: "#0d9488",
  cursorAccent: "#ffffff",
  selectionBackground: "rgba(13, 148, 136, 0.25)",
  black: "#2b3038",
  red: "#d33b40",
  green: "#3a9c42",
  yellow: "#b7791f",
  blue: "#3465c0",
  magenta: "#a34bb0",
  cyan: "#0d9488",
  white: "#a8b0b8",
  brightBlack: "#6b7280",
  brightRed: "#e5484d",
  brightGreen: "#46b450",
  brightYellow: "#f5a524",
  brightBlue: "#4c8dff",
  brightMagenta: "#c069d6",
  brightCyan: "#14b8a6",
  brightWhite: "#3a4048",
};
