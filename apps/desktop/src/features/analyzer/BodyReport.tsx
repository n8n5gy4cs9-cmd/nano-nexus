import { useQuery } from "@tanstack/react-query";

import { Badge, EmptyState, Skeleton } from "@/components/ui";
import { api } from "@/lib/api";
import { formatBytes } from "@/lib/format";
import { toIpcException } from "@/lib/ipc";

interface Fact {
  label: string;
  value: string;
}
interface Table {
  title: string;
  columns: string[];
  rows: string[][];
}
interface Flag {
  level: string;
  text: string;
}
interface FileReportBody {
  path: string;
  name: string;
  kind: string;
  bytes: number;
  facts: Fact[];
  tables: Table[];
  flags: Flag[];
}
interface SecretFinding {
  rule: string;
  kind: string;
  severity: string;
  file: string;
  line: number;
  masked: string;
  likely_fixture: boolean;
}

const isRecord = (v: unknown): v is Record<string, unknown> => typeof v === "object" && v !== null;

function parse(text: string): unknown {
  try {
    return JSON.parse(text);
  } catch {
    return null;
  }
}

function isFile(v: unknown): v is FileReportBody {
  return isRecord(v) && Array.isArray(v.facts) && Array.isArray(v.tables) && typeof v.name === "string";
}

function isSecrets(v: unknown): v is { findings: SecretFinding[] } {
  return isRecord(v) && Array.isArray(v.findings);
}

/** File, secrets, and log reports: the stored body shown as facts and tables. */
export function BodyReport({ id, kind, title }: { id: string; kind: string; title: string }) {
  const q = useQuery({ queryKey: ["analyzer", "body", id], queryFn: () => api.analyzer.body(id) });
  if (q.isPending) return <Skeleton className="h-48 w-full" />;
  if (q.isError) return <EmptyState title="Could not open the report" description={toIpcException(q.error).message} />;
  const body = parse(q.data);
  return (
    <>
      <h1 className="truncate text-title font-semibold">{title}</h1>
      {isFile(body) ? (
        <FileView r={body} />
      ) : kind === "secrets" && isSecrets(body) ? (
        <SecretsView findings={body.findings} />
      ) : (
        <pre className="overflow-auto rounded-card border border-border bg-code p-3 text-small">{JSON.stringify(body, null, 2)}</pre>
      )}
    </>
  );
}

function FileView({ r }: { r: FileReportBody }) {
  return (
    <>
      <p className="text-small text-muted">
        {r.kind} · {formatBytes(r.bytes)} · <span className="break-all">{r.path}</span>
      </p>
      {r.flags.length > 0 ? (
        <ul className="flex flex-col gap-1">
          {r.flags.map((f) => (
            <li key={f.text} className="flex items-center gap-2 text-body">
              <Badge tone={f.level === "warning" ? "warning" : "neutral"}>{f.level}</Badge> {f.text}
            </li>
          ))}
        </ul>
      ) : null}
      <dl className="grid grid-cols-[max-content_1fr] gap-x-6 gap-y-1 rounded-card border border-border bg-surface p-4 text-body">
        {r.facts.map((f) => (
          <div key={f.label} className="contents">
            <dt className="text-muted">{f.label}</dt>
            <dd className="break-words">{f.value}</dd>
          </div>
        ))}
      </dl>
      {r.tables.map((t) => (
        <section key={t.title} className="flex flex-col gap-1">
          <h2 className="text-lead font-semibold">{t.title}</h2>
          <div className="overflow-auto rounded-card border border-border">
            <table className="w-full text-small">
              <thead className="bg-surface-2 text-left text-muted">
                <tr>
                  {t.columns.map((c) => (
                    <th key={c} className="px-3 py-1.5 font-medium">
                      {c}
                    </th>
                  ))}
                </tr>
              </thead>
              <tbody>
                {t.rows.slice(0, 200).map((row, i) => (
                  <tr key={i} className="border-t border-border">
                    {row.map((cell, j) => (
                      <td key={j} className="px-3 py-1">
                        {cell}
                      </td>
                    ))}
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </section>
      ))}
    </>
  );
}

function SecretsView({ findings }: { findings: SecretFinding[] }) {
  if (findings.length === 0) return <EmptyState title="No secrets found" description="Nothing that looks like a key, token, or password." />;
  return (
    <ul className="flex flex-col divide-y divide-border rounded-card border border-border bg-surface">
      {findings.map((f, i) => (
        <li key={i} className="flex items-center gap-3 px-3 py-2 text-small">
          <Badge tone={f.severity === "high" || f.severity === "critical" ? "danger" : "warning"}>{f.severity}</Badge>
          <span className="min-w-0 flex-1 truncate" title={f.file}>
            {f.file}:{f.line}
          </span>
          <code className="text-muted">{f.masked}</code>
          <span className="text-faint">{f.rule}</span>
          {f.likely_fixture ? <Badge tone="neutral">test data?</Badge> : null}
        </li>
      ))}
    </ul>
  );
}
