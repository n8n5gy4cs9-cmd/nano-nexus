import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Check, Loader2 } from "lucide-react";
import { useEffect, useState } from "react";
import { useNavigate } from "react-router-dom";
import { toast } from "sonner";

import { AssistantIcon, Button, Dialog, DialogContent, DialogDescription, DialogTitle } from "@/components/ui";
import { useAssistants } from "@/hooks/useAssistants";
import { useRuntime } from "@/hooks/useRuntime";
import { api } from "@/lib/api";
import { toIpcException } from "@/lib/ipc";
import { baseName } from "@/lib/runtime-view";
import { useAssistantChoice } from "@/stores/assistant";
import { useUi } from "@/stores/ui";

const STEPS = 4;
/** The assistants the last step offers first, when they are installed and usable. */
const FEATURED = ["nexus", "duo", "folder-scout", "code-mentor", "doc-writer", "tldr", "polyglot", "librarian"];

/** The ring that draws itself, then the dot (UI-DESIGN §4.11). */
export function HexagonHero({ size = 96 }: { size?: number }) {
  return (
    <svg viewBox="0 0 32 32" width={size} height={size} aria-hidden="true" className="mx-auto">
      <defs>
        <linearGradient id="nn-hero" x1="0" y1="0" x2="1" y2="1">
          <stop offset="0" stopColor="oklch(0.70 0.18 288)" />
          <stop offset="1" stopColor="oklch(0.82 0.12 205)" />
        </linearGradient>
      </defs>
      <path className="nn-hex-ring" pathLength={100} d="M16 2.5 27.7 9.25v13.5L16 29.5 4.3 22.75V9.25z" fill="none" stroke="url(#nn-hero)" strokeWidth="2.2" strokeLinejoin="round" />
      <circle className="nn-hex-dot" cx="16" cy="16" r="3.2" fill="url(#nn-hero)" />
    </svg>
  );
}

function Welcome() {
  return (
    <div className="flex flex-col gap-3 text-center">
      <HexagonHero />
      <h2 className="text-title font-semibold">Private AI on your computer</h2>
      <p className="text-body text-muted">NanoNexus runs a small AI model right here. Your chats and files never leave this computer, and it works without internet.</p>
    </div>
  );
}

function ModelCheck({ onModels }: { onModels: () => void }) {
  const state = useRuntime();
  const installed = useQuery({ queryKey: ["models", "installed"], queryFn: api.models.list, retry: false });
  const ready = state?.state === "ready" ? state : undefined;
  const none = installed.isSuccess && installed.data.length === 0;
  return (
    <div className="flex flex-col gap-3">
      <h2 className="text-title font-semibold">Your model</h2>
      {ready ? (
        <p className="flex items-center gap-2 text-body" role="status">
          <Check className="size-5 text-success" /> {baseName(ready.model_path).replace(/\.gguf$/i, "")} is ready.
        </p>
      ) : none ? (
        <>
          <p className="text-body text-muted">There is no model on this computer yet. Download a small one (about 1 GB) to start chatting.</p>
          <Button variant="primary" onClick={onModels}>
            Download a model
          </Button>
        </>
      ) : state?.state === "failed" ? (
        <p className="text-body text-danger" role="status">
          The model could not start. You can fix this later from the pill at the top right.
        </p>
      ) : (
        <p className="flex items-center gap-2 text-body text-muted" role="status">
          <Loader2 className="size-5 animate-spin" /> Starting the model…
        </p>
      )}
      <p className="text-small text-muted">You can add or switch models any time in Models.</p>
    </div>
  );
}

function GiveFolder() {
  const [added, setAdded] = useState<string | null>(null);
  const pick = async () => {
    try {
      const dir = await api.fs.pickFolder();
      if (!dir) return;
      await api.fs.grantAdd(dir, "read", true);
      setAdded(dir);
    } catch (e) {
      toast.error(toIpcException(e).message);
    }
  };
  return (
    <div className="flex flex-col gap-3">
      <h2 className="text-title font-semibold">Give it a folder (optional)</h2>
      <p className="text-body text-muted">Assistants can only see folders you allow. Choose one, for example Downloads, and they can read it, but never change it without showing you a preview first.</p>
      {added ? (
        <p className="flex items-center gap-2 text-body break-all" role="status">
          <Check className="size-5 shrink-0 text-success" /> Allowed to read {added}
        </p>
      ) : (
        <Button onClick={() => void pick()}>Choose a folder…</Button>
      )}
      <p className="text-small text-muted">Manage this later in Permissions.</p>
    </div>
  );
}

function PickAssistant({ onPick }: { onPick: (id: string) => void }) {
  const { cards } = useAssistants();
  const usable = cards.filter((c) => c.enabled && c.available);
  const featured = [...FEATURED.map((id) => usable.find((c) => c.id === id)).filter((c) => c !== undefined), ...usable].filter((c, i, all) => all.indexOf(c) === i).slice(0, 6);
  return (
    <div className="flex flex-col gap-3">
      <h2 className="text-title font-semibold">Pick your first assistant</h2>
      <ul className="grid gap-2 sm:grid-cols-2">
        {featured.map((c) => (
          <li key={c.id}>
            <button type="button" className="flex h-full w-full items-start gap-3 rounded-card border border-border bg-surface p-3 text-left hover:bg-surface-2" onClick={() => onPick(c.id)}>
              <AssistantIcon icon={c.icon} color={c.color} size={32} />
              <span className="min-w-0">
                <span className="block text-body font-medium">{c.name}</span>
                <span className="block text-small text-muted">{c.tagline}</span>
              </span>
            </button>
          </li>
        ))}
      </ul>
    </div>
  );
}

/** First-run tour (4 steps, skippable). Re-run it from Settings or the command palette. */
export function Onboarding() {
  const open = useUi((s) => s.onboardingOpen);
  const setOpen = useUi((s) => s.setOnboardingOpen);
  const [step, setStep] = useState(0);
  const navigate = useNavigate();
  const qc = useQueryClient();

  const finish = (then?: () => void) => {
    setOpen(false);
    setStep(0);
    void api.settings
      .set("ui.onboarded", true)
      .then((s) => qc.setQueryData(["settings"], s))
      .catch(() => undefined);
    then?.();
  };

  return (
    <Dialog open={open} onOpenChange={(o) => !o && finish()}>
      <DialogContent className="w-[min(560px,calc(100vw-32px))]" aria-describedby="onboarding-desc">
        <DialogTitle className="sr-only">Welcome to NanoNexus</DialogTitle>
        <DialogDescription id="onboarding-desc" className="sr-only">
          Step {step + 1} of {STEPS}
        </DialogDescription>
        <div className="min-h-64 py-2">
          {step === 0 ? <Welcome /> : null}
          {step === 1 ? <ModelCheck onModels={() => finish(() => void navigate("/models"))} /> : null}
          {step === 2 ? <GiveFolder /> : null}
          {step === 3 ? (
            <PickAssistant
              onPick={(id) =>
                finish(() => {
                  useAssistantChoice.getState().select(id);
                  void navigate("/");
                })
              }
            />
          ) : null}
        </div>
        <div className="flex items-center justify-between gap-2">
          <div className="flex items-center gap-1.5" aria-hidden="true">
            {Array.from({ length: STEPS }, (_, i) => (
              <span key={i} className={i === step ? "h-1.5 w-5 rounded-full bg-accent" : "size-1.5 rounded-full bg-surface-3"} />
            ))}
          </div>
          <div className="flex gap-2">
            <Button variant="ghost" onClick={() => finish()}>
              Skip
            </Button>
            {step > 0 ? <Button onClick={() => setStep(step - 1)}>Back</Button> : null}
            {step < STEPS - 1 ? (
              <Button variant="primary" onClick={() => setStep(step + 1)}>
                Next
              </Button>
            ) : null}
          </div>
        </div>
      </DialogContent>
    </Dialog>
  );
}

/**
 * Opens the tour once on a first run: the setting says it was never shown and there are no chats yet
 * (so people who used the app before the tour existed are not interrupted).
 */
export function OnboardingGate() {
  const settings = useQuery({ queryKey: ["settings"], queryFn: api.settings.get, retry: false });
  const chats = useQuery({ queryKey: ["onboarding", "chats"], queryFn: () => api.conversations.list(null, 1), retry: false, staleTime: Infinity });
  const fresh = settings.data !== undefined && !(settings.data as unknown as Record<string, unknown>)["ui.onboarded"] && chats.data !== undefined && chats.data.length === 0;
  useEffect(() => {
    if (fresh) useUi.getState().setOnboardingOpen(true);
  }, [fresh]);
  return <Onboarding />;
}
