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
  const info = useQuery({ queryKey: ["app-info"], queryFn: api.app.info, retry: false });
  if (!info.data) return null;
  const { version, os, arch, build, locations } = info.data;
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
      {locations.length ? (
        <div className="py-3">
          <h3 className="text-body font-medium">Where this version stores files</h3>
          <p className="text-small text-muted">These are the paths used by the app running now. Missing sources are marked below.</p>
          <div className="mt-2 space-y-3">
            {locations.map((location, index) => (
              <div key={`${location.label}-${location.path}-${index}`} className="min-w-0">
                <div className="text-body font-medium">{location.label}{location.exists ? "" : " · Missing"}</div>
                <p className="text-small text-muted">{location.purpose}</p>
                <code className="block select-text break-all text-small" title={location.path}>{location.path}</code>
              </div>
            ))}
          </div>
        </div>
      ) : null}
    </Section>
  );
}
