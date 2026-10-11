import { create } from "zustand";

import { readStored, writeStored } from "@/lib/storage";

const KEY = "nn.assistant";

interface AssistantChoice {
  /** The assistant a new chat starts with. */
  selectedId: string;
  select: (id: string) => void;
  /** Open the ⌘/ switcher. */
  /** TOML (for example from Prompt Architect) waiting to be opened in the builder. */
  builderDraft: string | null;
  setBuilderDraft: (toml: string | null) => void;
  switcherOpen: boolean;
  setSwitcherOpen: (open: boolean) => void;
}

export const useAssistantChoice = create<AssistantChoice>((set) => ({
  selectedId: readStored(KEY) ?? "nexus",
  select: (id) => {
    writeStored(KEY, id);
    set({ selectedId: id });
  },
  builderDraft: null,
  setBuilderDraft: (builderDraft) => set({ builderDraft }),
  switcherOpen: false,
  setSwitcherOpen: (switcherOpen) => set({ switcherOpen }),
}));
