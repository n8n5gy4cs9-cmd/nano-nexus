import { create } from "zustand";

import { readStored, writeStored } from "@/lib/storage";
import { parseZoom } from "@/lib/zoom";
import { isAccent, isThemePreference, type Accent, type ThemePreference } from "@/lib/theme";

const THEME_KEY = "nn.theme";
const ACCENT_KEY = "nn.accent";
const ZOOM_KEY = "nn.zoom";

interface UiState {
  theme: ThemePreference;
  accent: Accent;
  /** UI scale factor (1 = 100%). */
  zoom: number;
  sidebarOpen: boolean;
  inspectorOpen: boolean;
  paletteOpen: boolean;
  /** The welcome tour is showing. */
  onboardingOpen: boolean;
  setTheme: (t: ThemePreference) => void;
  setAccent: (a: Accent) => void;
  setZoom: (z: number) => void;
  toggleSidebar: () => void;
  toggleInspector: () => void;
  setPaletteOpen: (open: boolean) => void;
  setOnboardingOpen: (open: boolean) => void;
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
  zoom: parseZoom(readStored(ZOOM_KEY)),
  sidebarOpen: typeof window === "undefined" || window.innerWidth >= 768,
  inspectorOpen: false,
  paletteOpen: false,
  onboardingOpen: false,
  setTheme: (theme) => {
    writeStored(THEME_KEY, theme);
    set({ theme });
  },
  setAccent: (accent) => {
    writeStored(ACCENT_KEY, accent);
    set({ accent });
  },
  setZoom: (zoom) => {
    const z = parseZoom(zoom);
    writeStored(ZOOM_KEY, String(z));
    set({ zoom: z });
  },
  toggleSidebar: () => set((s) => ({ sidebarOpen: !s.sidebarOpen })),
  toggleInspector: () => set((s) => ({ inspectorOpen: !s.inspectorOpen })),
  setPaletteOpen: (paletteOpen) => set({ paletteOpen }),
  setOnboardingOpen: (onboardingOpen) => set({ onboardingOpen }),
}));
