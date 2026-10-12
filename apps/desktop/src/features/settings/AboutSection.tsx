import { useQuery } from "@tanstack/react-query";

import { api } from "@/lib/api";

import { Row, Section } from "./fields";

/** "2026-10-20T12:00:00Z" → "20 Oct 2026"; anything else is shown as is. */
export function formatBuildDate(iso: string): string {
  const d = new Date(iso);
  return Number.isNaN(d.getTime()) ? iso : d.toLocaleDateString(undefined, { day: "numeric", month: "short", year: "numeric" });
}

/** Settings → About: app version, platform, and (in a built app) the bundle profile, commit and build date. */
export function AboutSection() {
  const info = useQuery({ queryKey: ["app-info"], queryFn: api.app.info, staleTime: Infinity, retry: false });
  if (!info.data) return null;
  const { version, os, arch, build, data_dir } = info.data;
  const models = build?.models ?? [];
  return (
    <Section title="About">
      <Row label="NanoNexus">
        <span className="text-body">{version}</span>
      </Row>
      <Row label="Platform">
        <span className="text-body">
          {os} · {arch}
        </span>
      </Row>
      {build ? (
        <>
          <Row label="Build" hint={models.length ? `${models.length} bundled model${models.length === 1 ? "" : "s"}` : undefined}>
            <span className="text-body">{build.profile}</span>
          </Row>
          {build.git_commit ? (
            <Row label="Commit">
              <code className="text-small">{build.git_commit}</code>
            </Row>
          ) : null}
          {build.built_at ? (
            <Row label="Built on">
              <span className="text-body">{formatBuildDate(build.built_at)}</span>
            </Row>
          ) : null}
        </>
      ) : (
        <Row label="Build" hint="A development build has no bundle information.">
          <span className="text-body text-muted">dev</span>
        </Row>
      )}
      {data_dir ? (
        <Row label="Data folder">
          <span className="max-w-72 truncate text-small text-muted" title={data_dir}>
            {data_dir}
          </span>
        </Row>
      ) : null}
    </Section>
  );
}
