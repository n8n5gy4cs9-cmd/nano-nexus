import { Check, Copy } from "lucide-react";
import { useEffect, useState } from "react";

import { IconButton } from "@/components/ui";
import { highlight } from "@/lib/highlight";
import { normalizeLanguage } from "@/lib/markdown-blocks";
import { useUi } from "@/stores/ui";

/** Fenced code: language label, Copy, and lazy syntax highlighting (plain monospace until ready). */
export function CodeBlock({ code, lang }: { code: string; lang?: string | undefined }) {
  const theme = useUi((s) => s.theme);
  const resolved = theme === "system" ? (window.matchMedia("(prefers-color-scheme: light)").matches ? "light" : "dark") : theme;
  const [html, setHtml] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);

  useEffect(() => {
    let alive = true;
    void highlight(code, lang, resolved).then((h) => alive && setHtml(h));
    return () => {
      alive = false;
    };
  }, [code, lang, resolved]);

  const copy = async () => {
    try {
      await navigator.clipboard.writeText(code);
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    } catch {
      /* clipboard unavailable */
    }
  };

  return (
    <figure className="my-3 overflow-hidden rounded-card border border-border bg-code">
      <figcaption className="flex items-center justify-between border-b border-border px-3 py-1 text-caption text-muted">
        <span>{normalizeLanguage(lang)}</span>
        <IconButton aria-label={copied ? "Copied" : "Copy code"} size="sm" onClick={() => void copy()}>
          {copied ? <Check /> : <Copy />}
        </IconButton>
      </figcaption>
      {html ? (
        <div className="overflow-x-auto p-3 text-small leading-5 [&_pre]:!bg-transparent [&_pre]:m-0" dangerouslySetInnerHTML={{ __html: html }} />
      ) : (
        <pre className="m-0 overflow-x-auto p-3 text-small leading-5">
          <code>{code}</code>
        </pre>
      )}
    </figure>
  );
}
