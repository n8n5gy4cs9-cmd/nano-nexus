import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Trash2 } from "lucide-react";
import { useState } from "react";

import {
  Button,
  Input,
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
  Switch,
} from "@/components/ui";
import { SHARE_DEVICES_KEY, SHARE_KEY, useShareStatus } from "@/hooks/useShare";
import { api, type SettingValue } from "@/lib/api";
import { toIpcException } from "@/lib/ipc";

import type { SharePolicy } from "./types";

const POLICIES: { value: SharePolicy; label: string }[] = [
  { value: "chat_only", label: "Chat only" },
  { value: "chat_read", label: "Chat + read granted folders" },
  { value: "full", label: "Full (can also switch models)" },
];

function Row({ label, hint, children }: { label: string; hint?: string; children: React.ReactNode }) {
  return (
    <div className="flex items-center justify-between gap-4 border-b border-border py-3">
      <div className="min-w-0">
        <div className="text-body">{label}</div>
        {hint ? <div className="text-small text-muted">{hint}</div> : null}
      </div>
      <div className="shrink-0">{children}</div>
    </div>
  );
}

/** Settings → Sharing (LAN-SHARE §2). */
export function SharingSettings() {
  const qc = useQueryClient();
  const status = useShareStatus();
  const settings = useQuery({ queryKey: ["settings"], queryFn: api.settings.get });
  const devices = useQuery({ queryKey: SHARE_DEVICES_KEY, queryFn: api.share.devices });
  const s = settings.data;
  const [port, setPort] = useState<string | null>(null);

  const setSetting = useMutation({
    mutationFn: ({ key, value }: { key: string; value: SettingValue }) => api.share.setSetting(key, value),
    onSuccess: (st) => {
      qc.setQueryData(SHARE_KEY, st);
      void qc.invalidateQueries({ queryKey: ["settings"] });
    },
  });
  const enable = useMutation({
    mutationFn: (on: boolean) => api.share.setEnabled(on),
    onSuccess: (st) => {
      qc.setQueryData(SHARE_KEY, st);
      void qc.invalidateQueries({ queryKey: ["settings"] });
    },
  });
  const revoke = useMutation({
    mutationFn: (id: string) => api.share.revokeDevice(id),
    onSuccess: () => void qc.invalidateQueries({ queryKey: SHARE_DEVICES_KEY }),
  });
  const update = useMutation({
    mutationFn: (v: { id: string; name: string | null; policy: SharePolicy | null }) =>
      api.share.updateDevice(v.id, v.name, v.policy),
    onSuccess: () => void qc.invalidateQueries({ queryKey: SHARE_DEVICES_KEY }),
  });

  const set = (key: string, value: SettingValue) => setSetting.mutate({ key, value });
  const failure = [setSetting, enable].find((m) => m.isError)?.error;

  if (!s) return <div className="p-6 text-muted">Loading…</div>;
  const portText = port ?? String(s.share_port);
  const portValid = /^\d+$/.test(portText) && Number(portText) >= 1024 && Number(portText) <= 65535;

  return (
    <div className="mx-auto max-w-2xl p-6">
      <h1 className="text-lead font-semibold">Sharing</h1>
      <p className="text-small text-muted">Use your assistants from a phone or another computer on this network.</p>
      {failure ? (
        <p role="alert" className="mt-2 text-small text-danger">
          {toIpcException(failure).message}
          {status?.suggested_port ? ` Try port ${status.suggested_port}.` : ""}
        </p>
      ) : null}

      <Row label="Sharing enabled" hint={status?.running ? status.urls.join("  ·  ") : "Off by default"}>
        <Switch checked={status?.running ?? false} onCheckedChange={(on) => enable.mutate(on)} aria-label="Sharing enabled" />
      </Row>
      <Row label="Start sharing when NanoNexus launches">
        <Switch checked={s.share_autostart} onCheckedChange={(v) => set("share.autostart", v)} aria-label="Autostart" />
      </Row>
      <Row label="Port" hint="1024-65535. Restarts sharing if it is on.">
        <div className="flex items-center gap-2">
          <Input
            value={portText}
            inputMode="numeric"
            className="w-24"
            aria-label="Port"
            onChange={(e) => setPort(e.target.value)}
          />
          <Button
            size="sm"
            disabled={!portValid || Number(portText) === s.share_port}
            onClick={() => {
              set("share.port", Number(portText));
              setPort(null);
            }}
          >
            Apply
          </Button>
        </div>
      </Row>
      <Row label="Listen on" hint="Pick “This computer only” to test without exposing anything.">
        <Select value={s.share_bind === "127.0.0.1" ? "local" : "all"} onValueChange={(v) => set("share.bind", v === "local" ? "127.0.0.1" : "0.0.0.0")}>
          <SelectTrigger className="w-52" aria-label="Listen on">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="all">All local networks</SelectItem>
            <SelectItem value="local">This computer only</SelectItem>
          </SelectContent>
        </Select>
      </Row>
      <Row label="New devices can…">
        <Select value={s.share_default_policy} onValueChange={(v) => set("share.default_policy", v)}>
          <SelectTrigger className="w-64" aria-label="Default permissions">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {POLICIES.map((p) => (
              <SelectItem key={p.value} value={p.value}>
                {p.label}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </Row>
      <Row label="Session lifetime (days)">
        <Input
          type="number"
          min={1}
          max={365}
          className="w-24"
          aria-label="Session lifetime in days"
          defaultValue={s.share_session_days}
          onBlur={(e) => {
            const n = Number(e.target.value);
            if (n >= 1 && n <= 365 && n !== s.share_session_days) set("share.session_days", n);
          }}
        />
      </Row>
      <Row label="Max devices">
        <Input
          type="number"
          min={1}
          max={20}
          className="w-24"
          aria-label="Maximum devices"
          defaultValue={s.share_max_devices}
          onBlur={(e) => {
            const n = Number(e.target.value);
            if (n >= 1 && n <= 20 && n !== s.share_max_devices) set("share.max_devices", n);
          }}
        />
      </Row>
      <Row label="Notify me when a new device pairs">
        <Switch checked={s.share_notify_pairing} onCheckedChange={(v) => set("share.notify_pairing", v)} aria-label="Notify on pairing" />
      </Row>

      <h2 className="mt-6 text-body font-semibold">Paired devices</h2>
      {devices.data?.length ? (
        <ul className="mt-2 divide-y divide-border rounded-card border border-border">
          {devices.data.map((d) => (
            <li key={d.id} className="flex flex-wrap items-center gap-3 p-3">
              <div className="min-w-0 flex-1">
                <Input
                  defaultValue={d.name}
                  aria-label={`Name of ${d.name}`}
                  className="h-7"
                  onBlur={(e) => {
                    const name = e.target.value.trim();
                    if (name && name !== d.name) update.mutate({ id: d.id, name, policy: null });
                  }}
                />
                <div className="mt-1 truncate text-caption text-faint">
                  {d.last_ip ?? "unknown IP"} ·{" "}
                  {d.last_seen_at ? `seen ${new Date(d.last_seen_at).toLocaleString()}` : "never seen"}
                </div>
              </div>
              <Select value={d.policy} onValueChange={(v) => update.mutate({ id: d.id, name: null, policy: v as SharePolicy })}>
                <SelectTrigger className="w-56" aria-label={`Permissions of ${d.name}`}>
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {POLICIES.map((p) => (
                    <SelectItem key={p.value} value={p.value}>
                      {p.label}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
              <Button size="sm" variant="danger" onClick={() => revoke.mutate(d.id)}>
                <Trash2 />
                Revoke
              </Button>
            </li>
          ))}
        </ul>
      ) : (
        <p className="mt-2 text-small text-muted">No devices yet. Turn sharing on and scan the QR code.</p>
      )}
      <p className="mt-6 text-small text-muted">
        If your phone cannot connect: allow NanoNexus on <strong>Private</strong> networks in the firewall prompt, and check
        that your router does not isolate Wi-Fi clients. See the guide's Sharing section.
      </p>
    </div>
  );
}
