import { useQuery } from "@tanstack/react-query";
import { useMemo } from "react";

import type { AssistantCard } from "@/bindings";
import { api } from "@/lib/api";

export const ASSISTANTS_KEY = ["assistants"] as const;

/** The assistant list (built-ins and user ones), cached; disabled ones are included. */
export function useAssistants() {
  const q = useQuery({ queryKey: ASSISTANTS_KEY, queryFn: api.assistants.list, retry: false, staleTime: 30_000 });
  const cards = useMemo(() => q.data ?? [], [q.data]);
  const byId = useMemo(() => new Map(cards.map((c) => [c.id, c])), [cards]);
  return { cards, byId, loading: q.isLoading };
}

export const NEXUS_ID = "nexus";

/** Fallback card used before the list loads or if an id is unknown. */
export const FALLBACK_CARD: AssistantCard = {
  id: NEXUS_ID,
  name: "Nexus",
  icon: "hexagon",
  color: "violet",
  category: "General",
  tagline: "Your everyday local copilot. It can read the folders you share.",
  description: "",
  version: null,
  source: "builtin",
  enabled: true,
  sort: -1,
  needs: ["read"],
  workflow: "router",
  pipeline: null,
  starters: [
    "What's inside my Downloads folder?",
    "Explain this error message in plain words",
    "Write a short, friendly reply to this email",
    "What's 18% of 2,450 and why?",
  ],
  placeholder: "Ask anything, or mention a file or folder…",
  available: true,
  unavailable_reason: null,
};
