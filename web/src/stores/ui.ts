import { create } from "zustand";

export type Theme = "light" | "dark";
export type Density = "normal" | "compact";

interface UiState {
  drawerOpen: boolean;
  theme: Theme;
  density: Density;
  /** Add-feed modal visibility — shared by the top bar and Manage toolbar (§9.0, §9.3). */
  addFeedOpen: boolean;
  setDrawerOpen: (open: boolean) => void;
  toggleDrawer: () => void;
  setTheme: (theme: Theme) => void;
  setDensity: (density: Density) => void;
  setAddFeedOpen: (open: boolean) => void;
}

const THEME_KEY = "hf_theme";
const DENSITY_KEY = "hf_density";

function initialTheme(): Theme {
  if (typeof localStorage !== "undefined") {
    const saved = localStorage.getItem(THEME_KEY);
    if (saved === "light" || saved === "dark") return saved;
  }
  return "dark"; // mockup is dark-first
}

function initialDensity(): Density {
  if (typeof localStorage !== "undefined") {
    const saved = localStorage.getItem(DENSITY_KEY);
    if (saved === "normal" || saved === "compact") return saved;
  }
  return "normal";
}

/** Ephemeral UI state only (prompt.md frontend rules). URL owns feed filters, not this store. */
export const useUiStore = create<UiState>((set) => ({
  drawerOpen: false,
  theme: initialTheme(),
  density: initialDensity(),
  addFeedOpen: false,
  setDrawerOpen: (drawerOpen) => set({ drawerOpen }),
  toggleDrawer: () => set((s) => ({ drawerOpen: !s.drawerOpen })),
  setTheme: (theme) => {
    if (typeof localStorage !== "undefined") localStorage.setItem(THEME_KEY, theme);
    applyTheme(theme);
    set({ theme });
  },
  setDensity: (density) => {
    if (typeof localStorage !== "undefined") localStorage.setItem(DENSITY_KEY, density);
    applyDensity(density);
    set({ density });
  },
  setAddFeedOpen: (addFeedOpen) => set({ addFeedOpen }),
}));

/** Reflect the theme onto <html> so token variables switch. */
export function applyTheme(theme: Theme) {
  document.documentElement.classList.toggle("dark", theme === "dark");
}

/** Reflect density onto <html> (drives the compact spacing token overrides). */
export function applyDensity(density: Density) {
  document.documentElement.classList.toggle("density-compact", density === "compact");
}
