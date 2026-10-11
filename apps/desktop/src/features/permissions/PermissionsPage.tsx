import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { FolderPlus, Lock, Shield, Trash2 } from "lucide-react";
import { useState } from "react";
import { toast } from "sonner";

import {
  Badge,
  Button,
  Dot,
  EmptyState,
  IconButton,
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
  Skeleton,
  Switch,
} from "@/components/ui";
import { api } from "@/lib/api";
import { relativeTime } from "@/lib/format";
import { toIpcException } from "@/lib/ipc";

import { LEVEL_LABEL, type AuditRecord, type Grant, type GrantLevel, type OsPermissionsStatus } from "./types";

const GRANTS_KEY = ["fs", "grants"] as const;
const DENY_KEY = ["fs", "deny"] as const;
const AUDIT_KEY = ["fs", "audit"] as const;
const OS_KEY = ["os", "permissions"] as const;

const fail = (e: unknown) => void toast.error(toIpcException(e).message);

function Section({ title, hint, action, children }: { title: string; hint?: string; action?: React.ReactNode; children: React.ReactNode }) {
  return (
    <section className="rounded-card border border-border bg-surface p-4 shadow-1">
      <div className="mb-3 flex items-start justify-between gap-3">
        <div>
          <h2 className="text-lead font-semibold">{title}</h2>
          {hint ? <p className="text-small text-muted">{hint}</p> : null}
        </div>
        {action}
      </div>
      {children}
    </section>
  );
}

function expiryLabel(g: Grant): string {
  if (g.scope === "session") return "This session";
  if (g.expires_at === null) return "Always";
  return `Until ${new Date(g.expires_at).toLocaleDateString()}`;
}

function GrantRow({ grant, onChanged }: { grant: Grant; onChanged: () => void }) {
  const update = useMutation({
    mutationFn: (patch: { level?: GrantLevel; recursive?: boolean }) => api.fs.grantUpdate(grant.id, patch),
    onSuccess: onChanged,
    onError: fail,
  });
  const revoke = useMutation({ mutationFn: () => api.fs.grantRevoke(grant.id), onSuccess: onChanged, onError: fail });
  return (
    <tr className="border-t border-border align-middle">
      <td className="max-w-[320px] py-2 pr-3">
        <div className="truncate text-body font-medium" title={grant.root}>
          {grant.label ?? grant.root.split(/[\\/]/).filter(Boolean).pop() ?? grant.root}
        </div>
        <div className="truncate text-caption text-faint" title={grant.root}>
          {grant.root}
        </div>
      </td>
      <td className="w-48 py-2 pr-3">
        <Select value={grant.level} onValueChange={(v) => update.mutate({ level: v as GrantLevel })}>
          <SelectTrigger aria-label={`Access level for ${grant.root}`}>
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {(Object.keys(LEVEL_LABEL) as GrantLevel[]).map((l) => (
              <SelectItem key={l} value={l}>
                {LEVEL_LABEL[l]}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </td>
      <td className="py-2 pr-3">
        <Switch
          checked={grant.recursive}
          onCheckedChange={(recursive) => update.mutate({ recursive })}
          aria-label={`Include subfolders of ${grant.root}`}
        />
      </td>
      <td className="py-2 pr-3 text-small text-muted">{expiryLabel(grant)}</td>
      <td className="py-2 text-right">
        <IconButton aria-label={`Revoke access to ${grant.root}`} size="sm" disabled={revoke.isPending} onClick={() => revoke.mutate()}>
          <Trash2 />
        </IconButton>
      </td>
    </tr>
  );
}

function GrantsSection() {
  const qc = useQueryClient();
  const grants = useQuery({ queryKey: GRANTS_KEY, queryFn: api.fs.grants });
  const refresh = () => void qc.invalidateQueries({ queryKey: GRANTS_KEY });
  const add = useMutation({
    mutationFn: async () => {
      const root = await api.fs.pickFolder();
      if (root) await api.fs.grantAdd(root, "read", true);
    },
    onSuccess: refresh,
    onError: fail,
  });
  return (
    <Section
      title="Folders NanoNexus can use"
      hint="Assistants can only see folders you list here. New folders start as read-only."
      action={
        <Button size="sm" variant="primary" disabled={add.isPending} onClick={() => add.mutate()}>
          <FolderPlus /> Add folder
        </Button>
      }
    >
      {grants.isLoading ? (
        <Skeleton className="h-16 w-full" />
      ) : (grants.data ?? []).length === 0 ? (
        <EmptyState icon={<Shield />} title="No folders yet" description="Add a folder to let assistants read it. Nothing is shared until you do." />
      ) : (
        <div className="overflow-x-auto">
          <table className="w-full text-left">
            <thead className="text-caption text-faint">
              <tr>
                <th className="pb-1 font-medium">Folder</th>
                <th className="pb-1 font-medium">Access</th>
                <th className="pb-1 font-medium">Subfolders</th>
                <th className="pb-1 font-medium">Lasts</th>
                <th className="pb-1" />
              </tr>
            </thead>
            <tbody>
              {(grants.data ?? []).map((g) => (
                <GrantRow key={g.id} grant={g} onChanged={refresh} />
              ))}
            </tbody>
          </table>
        </div>
      )}
    </Section>
  );
}

const OS_TONE: Record<string, "success" | "danger" | "warning" | "neutral"> = {
  granted: "success",
  denied: "danger",
  missing: "neutral",
  other: "warning",
};
const OS_TEXT: Record<string, string> = {
  granted: "Allowed",
  denied: "Blocked by macOS",
  missing: "Folder not found",
  other: "Unknown",
};

function OsSection({ status }: { status: OsPermissionsStatus }) {
  if (status.platform !== "macos") return null;
  const name = (p: string) => p.split("/").filter(Boolean).pop() ?? p;
  return (
    <Section
      title="macOS privacy"
      hint="macOS asks separately before any app can open Desktop, Documents, or Downloads. Both layers must allow access."
      action={
        <Button size="sm" onClick={() => void api.os.openPrivacySettings("files_and_folders").catch(fail)}>
          Open System Settings
        </Button>
      }
    >
      <ul className="grid gap-2 sm:grid-cols-2">
        {status.folders.map((f) => (
          <li key={f.path} className="flex items-center justify-between rounded-card border border-border bg-surface-2 px-3 py-2">
            <span className="text-body">{name(f.path)}</span>
            <span className="flex items-center gap-2 text-small text-muted">
              <Dot tone={OS_TONE[f.state] ?? "neutral"} /> {OS_TEXT[f.state] ?? f.state}
            </span>
          </li>
        ))}
        {status.full_disk_access ? (
          <li className="flex items-center justify-between rounded-card border border-border bg-surface-2 px-3 py-2">
            <span className="text-body">Full Disk Access</span>
            <span className="flex items-center gap-2 text-small text-muted">
              <Dot tone={status.full_disk_access === "granted" ? "success" : "neutral"} />
              {status.full_disk_access === "granted" ? "On" : "Off (optional)"}
              {status.full_disk_access !== "granted" ? (
                <Button size="sm" variant="ghost" onClick={() => void api.os.openPrivacySettings("full_disk").catch(fail)}>
                  Open
                </Button>
              ) : null}
            </span>
          </li>
        ) : null}
      </ul>
    </Section>
  );
}

function DenySection() {
  const qc = useQueryClient();
  const deny = useQuery({ queryKey: DENY_KEY, queryFn: api.fs.denyList });
  const refresh = () => void qc.invalidateQueries({ queryKey: DENY_KEY });
  const add = useMutation({
    mutationFn: async () => {
      const root = await api.fs.pickFolder();
      if (root) await api.fs.denyAdd(root);
    },
    onSuccess: refresh,
    onError: fail,
  });
  const remove = useMutation({ mutationFn: (root: string) => api.fs.denyRemove(root), onSuccess: refresh, onError: fail });
  return (
    <Section
      title="Protected locations"
      hint="Never readable or changeable, even inside a folder you allowed. Built-in entries keep your system safe."
      action={
        <Button size="sm" disabled={add.isPending} onClick={() => add.mutate()}>
          <Lock /> Protect a folder
        </Button>
      }
    >
      <ul className="divide-y divide-border">
        {(deny.data ?? []).map((r) => (
          <li key={r.root} className="flex items-center justify-between gap-3 py-2">
            <div className="min-w-0">
              <div className="truncate text-body" title={r.root}>
                {r.root}
              </div>
              <div className="text-caption text-faint">{r.label}</div>
            </div>
            {r.builtin ? (
              <Badge>Built in</Badge>
            ) : (
              <IconButton aria-label={`Stop protecting ${r.root}`} size="sm" onClick={() => remove.mutate(r.root)}>
                <Trash2 />
              </IconButton>
            )}
          </li>
        ))}
      </ul>
    </Section>
  );
}

const PAGE = 50;

function ActivitySection() {
  const [extra, setExtra] = useState<AuditRecord[]>([]);
  const [done, setDone] = useState(false);
  const audit = useQuery({ queryKey: AUDIT_KEY, queryFn: () => api.fs.audit(PAGE), refetchInterval: 15_000 });
  const first = audit.data ?? [];
  const rows = [...first, ...extra.filter((e) => !first.some((f) => f.id === e.id))];
  const more = useMutation({
    mutationFn: () => api.fs.audit(PAGE, rows[rows.length - 1]?.ts ?? null),
    onSuccess: (r) => {
      setExtra((x) => [...x, ...r]);
      if (r.length < PAGE) setDone(true);
    },
    onError: fail,
  });
  return (
    <Section title="Activity" hint="What assistants tried to open or change. Contents are never logged.">
      {rows.length === 0 ? (
        <p className="text-small text-muted">Nothing yet.</p>
      ) : (
        <ul className="divide-y divide-border">
          {rows.map((r) => (
            <li key={r.id} className="flex items-center gap-3 py-1.5 text-small">
              <Dot tone={r.allowed ? "success" : "danger"} />
              <span className="w-16 shrink-0 text-faint">{relativeTime(r.ts)}</span>
              <span className="w-24 shrink-0 truncate text-muted">{r.actor}</span>
              <span className="w-14 shrink-0 font-medium">{r.action}</span>
              <span className="min-w-0 flex-1 truncate" title={r.path}>
                {r.path}
              </span>
              {r.detail ? <span className="hidden truncate text-faint md:inline">{r.detail}</span> : null}
            </li>
          ))}
        </ul>
      )}
      {rows.length >= PAGE && !done ? (
        <Button size="sm" variant="ghost" className="mt-2" disabled={more.isPending} onClick={() => more.mutate()}>
          Load older
        </Button>
      ) : null}
    </Section>
  );
}

/** Permissions page (UI-DESIGN §6.8): who can touch what. */
export function PermissionsPage() {
  const os = useQuery({ queryKey: OS_KEY, queryFn: api.os.permissions, retry: false });
  return (
    <div className="mx-auto flex max-w-4xl flex-col gap-4 p-6">
      <header>
        <h1 className="text-lead font-semibold">Permissions</h1>
        <p className="text-body text-muted">Choose which folders NanoNexus can use. Changes always need your approval.</p>
      </header>
      <GrantsSection />
      {os.data ? <OsSection status={os.data} /> : null}
      <DenySection />
      <ActivitySection />
    </div>
  );
}
