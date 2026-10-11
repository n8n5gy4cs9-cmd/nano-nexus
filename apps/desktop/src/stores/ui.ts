import { create } from "zustand";

import { readStored, writeStored } from "@/lib/storage";
import { isAccent, isThemePreference, type Accent, type ThemePreference } from "@/lib/theme";

const THEME_KEY = "nn.theme";
const ACCENT_KEY = "nn.accent";

interface UiState {
  theme: ThemePreference;
  accent: Accent;
  sidebarOpen: boolean;
  inspectorOpen: boolean;
  paletteOpen: boolean;
  setTheme: (t: ThemePreference) => void;
  setAccent: (a: Accent) => void;
  toggleSidebar: () => void;
  toggleInspector: () => void;
  setPaletteOpen: (open: boolean) => void;
}

function initialTheme(): ThemePreference {
  const v = readStored(THEME_KEY);
  return isThemePreference(v) ? v : "dark";
}

function initialAccent(): Accent {
  const v = readStored(ACCENT_KEY);
  return isAccent(v) ? v : "violet";
}

export const useUi = create<UiState>((set) => ({
  theme: initialTheme(),
  accent: initialAccent(),
  sidebarOpen: typeof window === "undefined" || window.innerWidth >= 768,
  inspectorOpen: false,
  paletteOpen: false,
  setTheme: (theme) => {
    writeStored(THEME_KEY, theme);
    set({ theme });
  },
  setAccent: (accent) => {
    writeStored(ACCENT_KEY, accent);
    set({ accent });
  },
  toggleSidebar: () => set((s) => ({ sidebarOpen: !s.sidebarOpen })),
  toggleInspector: () => set((s) => ({ inspectorOpen: !s.inspectorOpen })),
  setPaletteOpen: (paletteOpen) => set({ paletteOpen }),
}));
