import { useQueryClient } from "@tanstack/react-query";
import { Loader2 } from "lucide-react";
import { useCallback, useEffect, useState, type ReactNode } from "react";

import { Button, Input } from "@/components/ui";
import { setTransport } from "@/lib/transport";
import { createWsTransport, type Connection, type WsTransport } from "@/lib/transport/wsTransport";

interface Session {
  csrf: string;
  device: { name: string };
}

async function fetchSession(): Promise<Session | null> {
  try {
    const res = await fetch("/api/session", { credentials: "same-origin" });
    return res.ok ? ((await res.json()) as Session) : null;
  } catch {
    return null;
  }
}

async function pair(code: string, name: string): Promise<Session> {
  const res = await fetch("/api/pair", {
    method: "POST",
    credentials: "same-origin",
    headers: { "content-type": "application/json", "x-nexus-csrf": "pair" },
    body: JSON.stringify({ code, name }),
  });
  if (res.status === 429) throw new Error("Too many wrong codes. Try again in a few minutes.");
  if (res.status === 409) throw new Error("This computer already has the maximum number of devices.");
  if (!res.ok) throw new Error("That code did not work. Show a new code on your computer.");
  return (await res.json()) as Session;
}

function defaultName(): string {
  const ua = navigator.userAgent;
  if (/iPhone/.test(ua)) return "iPhone";
  if (/iPad/.test(ua)) return "iPad";
  if (/Android/.test(ua)) return "Android phone";
  return "Browser";
}

function PairScreen({ onPaired }: { onPaired: (s: Session) => void }) {
  const fromQr = location.pathname === "/pair" ? location.hash.slice(1) : "";
  const [code, setCode] = useState(fromQr);
  const [name, setName] = useState(defaultName());
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const submit = async () => {
    setBusy(true);
    setError(null);
    try {
      const session = await pair(code.trim(), name.trim());
      history.replaceState(null, "", "/"); // drop the secret from the address bar and history
      onPaired(session);
    } catch (e) {
      setError(e instanceof Error ? e.message : "Pairing failed.");
    } finally {
      setBusy(false);
    }
  };

  return (
    <main className="mx-auto flex min-h-full max-w-sm flex-col justify-center gap-4 px-4 pb-[env(safe-area-inset-bottom)] pt-[env(safe-area-inset-top)] text-text">
      <div className="flex items-center gap-2">
        <span aria-hidden className="size-5 rounded-[6px] bg-[image:var(--nexus-gradient)]" />
        <h1 className="text-lead font-semibold">Pair with NanoNexus</h1>
      </div>
      <p className="text-body text-muted">
        On your computer open the 📡 Share menu, then scan the QR code or type the 6-digit PIN here.
      </p>
      <label className="flex flex-col gap-1 text-small text-muted">
        PIN or code
        <Input
          value={code}
          onChange={(e) => setCode(e.target.value)}
          inputMode="numeric"
          autoComplete="one-time-code"
          className="h-11 text-center text-lead tracking-widest"
          aria-label="PIN or code"
        />
      </label>
      <label className="flex flex-col gap-1 text-small text-muted">
        Name this device
        <Input value={name} onChange={(e) => setName(e.target.value)} className="h-11" maxLength={60} />
      </label>
      {error ? (
        <p role="alert" className="text-small text-danger">
          {error}
        </p>
      ) : null}
      <Button className="h-11" disabled={busy || !code.trim() || !name.trim()} onClick={() => void submit()}>
        {busy ? <Loader2 className="animate-spin" /> : null}
        Pair
      </Button>
    </main>
  );
}

/** Shown while the socket is down; the transport reconnects on its own. */
function ConnectionBanner({ state }: { state: Connection }) {
  if (state === "open" || state === "connecting") return null;
  return (
    <div
      role="status"
      className="fixed inset-x-0 top-0 z-50 border-b border-warning bg-surface-2 px-4 py-1.5 pt-[calc(env(safe-area-inset-top)+0.375rem)] text-center text-small font-medium text-warning"
    >
      {state === "ended" ? "Session ended. Pair again to continue." : "Reconnecting…"}
    </div>
  );
}

/** Web build only: checks the session (or shows the pairing screen) before the app renders. */
export function WebGate({ children }: { children: ReactNode }) {
  const qc = useQueryClient();
  const [phase, setPhase] = useState<"loading" | "pair" | "ready">("loading");
  const [conn, setConn] = useState<Connection>("connecting");

  const start = useCallback(
    (session: Session) => {
      // The QR link is /pair#<secret>. The app uses hash routes, so a leftover secret would look like a page.
      if (location.pathname !== "/" || !location.hash.startsWith("#/")) history.replaceState(null, "", "/");
      const ws: WsTransport = createWsTransport({
        csrf: session.csrf,
        onSessionEnded: () => setPhase("pair"),
      });
      ws.onConnection(setConn);
      ws.onResume(() => void qc.invalidateQueries());
      setTransport(ws);
      setPhase("ready");
    },
    [qc],
  );

  useEffect(() => {
    void fetchSession().then((s) => (s ? start(s) : setPhase("pair")));
  }, [start]);

  if (phase === "loading") {
    return (
      <div className="grid h-full place-items-center text-muted">
        <Loader2 className="animate-spin" />
      </div>
    );
  }
  if (phase === "pair") return <PairScreen onPaired={start} />;
  return (
    <>
      <ConnectionBanner state={conn} />
      {children}
    </>
  );
}
