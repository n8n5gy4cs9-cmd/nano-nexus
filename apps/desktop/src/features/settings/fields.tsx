import { type ReactNode, useEffect, useState } from "react";

import { Input, Switch } from "@/components/ui";

/** A titled group of settings rows. */
export function Section({ title, hint, children }: { title: string; hint?: string; children: ReactNode }) {
  return (
    <section aria-label={title} className="rounded-card border border-border bg-surface p-5 shadow-1">
      <h2 className="text-lead font-semibold">{title}</h2>
      {hint ? <p className="mb-1 text-small text-muted">{hint}</p> : null}
      <div className="mt-1 divide-y divide-border">{children}</div>
    </section>
  );
}

/** One label + control row. */
export function Row({ label, hint, children }: { label: string; hint?: string; children: ReactNode }) {
  return (
    <div className="flex items-center justify-between gap-4 py-3">
      <div className="min-w-0">
        <div className="text-body">{label}</div>
        {hint ? <div className="text-small text-muted">{hint}</div> : null}
      </div>
      <div className="shrink-0">{children}</div>
    </div>
  );
}

export function ToggleRow({ label, hint, checked, onChange }: { label: string; hint?: string; checked: boolean; onChange: (v: boolean) => void }) {
  return (
    <Row label={label} {...(hint ? { hint } : {})}>
      <Switch checked={checked} onCheckedChange={onChange} aria-label={label} />
    </Row>
  );
}

export function SelectRow({
  label,
  hint,
  value,
  options,
  onChange,
}: {
  label: string;
  hint?: string;
  value: string;
  options: { value: string; label: string }[];
  onChange: (v: string) => void;
}) {
  return (
    <Row label={label} {...(hint ? { hint } : {})}>
      <select aria-label={label} className="h-9 min-w-40 rounded-control border border-border bg-surface px-2 text-body" value={value} onChange={(e) => onChange(e.target.value)}>
        {options.map((o) => (
          <option key={o.value} value={o.value}>
            {o.label}
          </option>
        ))}
      </select>
    </Row>
  );
}

/** A text/number input that saves when it loses focus or Enter is pressed. */
export function TextRow({
  label,
  hint,
  value,
  onCommit,
  type = "text",
  placeholder,
  width = "w-40",
}: {
  label: string;
  hint?: string;
  value: string;
  onCommit: (v: string) => void;
  type?: "text" | "number" | "time";
  placeholder?: string;
  width?: string;
}) {
  const [draft, setDraft] = useState(value);
  useEffect(() => setDraft(value), [value]);
  const commit = () => {
    if (draft !== value) onCommit(draft);
  };
  return (
    <Row label={label} {...(hint ? { hint } : {})}>
      <Input
        aria-label={label}
        type={type}
        className={width}
        value={draft}
        placeholder={placeholder}
        onChange={(e) => setDraft(e.target.value)}
        onBlur={commit}
        onKeyDown={(e) => {
          if (e.key === "Enter") commit();
        }}
      />
    </Row>
  );
}
