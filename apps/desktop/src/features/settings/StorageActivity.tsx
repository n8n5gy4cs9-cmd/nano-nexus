import { useQuery } from "@tanstack/react-query";
import { useMemo, useState } from "react";

import { Button, Skeleton } from "@/components/ui";
import { api } from "@/lib/api";
import { toIpcException } from "@/lib/ipc";

import { Section } from "./fields";

const bytesLabel = (n: number | null) => (n === null ? "size unknown" : n < 1024 ? `${n} B` : n < 1024 * 1024 ? `${(n / 1024).toFixed(1)} KiB` : `${(n / 1024 / 1024).toFixed(1)} MiB`);

/** Settings → Storage activity: operations the app recorded, where workers are told to write, and what is not observed. */
export function StorageActivity() {
  const report = useQuery({ queryKey: ["write-report"], queryFn: api.app.writeReport, retry: false });
  const [subsystem, setSubsystem] = useState("all");
  const [outsideOnly, setOutsideOnly] = useState(false);
  const subsystems = useMemo(() => [...new Set((report.data?.recorded ?? []).map((e) => e.subsystem))].sort(), [report.data]);
  if (report.isPending) return <Section title="Storage activity"><Skeleton className="h-16" /></Section>;
  if (report.isError) {
    return (
      <Section title="Storage activity">
        <div role="alert" className="py-3">
          <p className="text-body">Storage activity is unavailable.</p>
          <p className="text-small text-muted">{toIpcException(report.error).message}</p>
          <Button variant="secondary" onClick={() => void report.refetch()}>Retry</Button>
        </div>
      </Section>
    );
  }
  const { recorded, configured, gaps, log_path } = report.data;
  const shown = recorded.filter((e) => (subsystem === "all" || e.subsystem === subsystem) && (!outsideOnly || e.outside_root));
  const outside = configured.filter((c) => c.outside_root);
  return (
    <Section title="Storage activity" hint="What the app recorded writing, and where it is configured to write. This is not a system-wide monitor.">
      {outside.length ? (
        <div role="alert" className="py-3">
          <h3 className="text-body font-medium">Configured outside the app drive</h3>
          {outside.map((c) => <code key={`${c.subsystem}-${c.path}`} className="block break-all text-small">{c.subsystem}: {c.path}</code>)}
        </div>
      ) : null}
      <div className="py-3">
        <div className="flex flex-wrap items-center gap-2">
          <h3 className="text-body font-medium">Recorded operations</h3>
          <select aria-label="Filter by area" className="rounded-control border border-border bg-surface px-2 py-1 text-small" value={subsystem} onChange={(e) => setSubsystem(e.target.value)}>
            <option value="all">All areas</option>
            {subsystems.map((s) => <option key={s} value={s}>{s}</option>)}
          </select>
          <label className="flex items-center gap-1 text-small"><input type="checkbox" checked={outsideOnly} onChange={(e) => setOutsideOnly(e.target.checked)} />Outside the app drive only</label>
          <Button variant="ghost" onClick={() => void report.refetch()}>Refresh</Button>
        </div>
        {shown.length === 0 ? <p className="text-small text-muted">{recorded.length === 0 ? "Nothing recorded yet this session." : "No recorded operations match this filter."}</p> : (
          <ul className="mt-2 space-y-2">
            {shown.map((e, i) => (
              <li key={`${e.ts_ms}-${e.destination}-${i}`} className="min-w-0 text-small">
                <span className="font-medium">{e.subsystem} · {e.operation}</span>{e.count > 1 ? ` ×${e.count}` : ""} · {e.ok ? bytesLabel(e.bytes) : "failed"}{e.outside_root ? " · outside the app drive" : ""}
                <code className="block break-all text-muted">{e.destination}</code>
                {e.error ? <span className="text-muted">{e.error}</span> : null}
              </li>
            ))}
          </ul>
        )}
        {log_path ? <p className="mt-2 break-all text-small text-muted">Kept in {log_path}, bounded and rotated.</p> : <p className="mt-2 text-small text-muted">No log file is active in this session.</p>}
      </div>
      <div className="py-3">
        <h3 className="text-body font-medium">Configured destinations</h3>
        <p className="text-small text-muted">Workers and caches are told to use these; individual writes inside them are not itemised.</p>
        <ul className="mt-2 space-y-1">{configured.map((c) => <li key={`${c.subsystem}-${c.path}`} className="min-w-0 text-small">{c.subsystem}<code className="block break-all text-muted">{c.path}{c.outside_root ? " (outside the app drive)" : ""}</code></li>)}</ul>
      </div>
      <div className="py-3">
        <h3 className="text-body font-medium">Not observed</h3>
        <ul className="list-disc pl-5 text-small text-muted">{gaps.map((g) => <li key={g}>{g}</li>)}</ul>
      </div>
    </Section>
  );
}
