import { memo, useMemo, type ComponentPropsWithoutRef } from "react";
import ReactMarkdown, { type Components } from "react-markdown";
import remarkGfm from "remark-gfm";

import { cn } from "@/lib/cn";
import { splitMarkdownBlocks } from "@/lib/markdown-blocks";

import { CodeBlock } from "./CodeBlock";

/** Only web links may be opened, and never inside the webview. */
export function isSafeLink(href: string | undefined): href is string {
  return !!href && /^https?:\/\//i.test(href);
}

function textOf(children: React.ReactNode): string {
  if (typeof children === "string") return children;
  if (Array.isArray(children)) return children.map(textOf).join("");
  return "";
}

export interface MarkdownProps {
  source: string;
  /** Called when a link is clicked (the app opens it with the OS browser). */
  onOpenLink?: (url: string) => void;
  className?: string;
}

function makeComponents(onOpenLink: ((url: string) => void) | undefined): Components {
  return {
    a: ({ href, children }) =>
      isSafeLink(href) ? (
        <a
          href={href}
          className="text-accent underline-offset-2 hover:underline"
          onClick={(e) => {
            e.preventDefault();
            onOpenLink?.(href);
          }}
        >
          {children}
        </a>
      ) : (
        <span>{children}</span>
      ),
    // Block code goes through <pre>; inline code keeps a light chip.
    pre: ({ children }) => <>{children}</>,
    code: ({ className, children }: ComponentPropsWithoutRef<"code">) => {
      const text = textOf(children).replace(/\n$/, "");
      const lang = /language-([\w+#-]+)/.exec(className ?? "")?.[1];
      const isBlock = lang !== undefined || text.includes("\n");
      return isBlock ? (
        <CodeBlock code={text} lang={lang} />
      ) : (
        <code className="rounded-chip bg-surface-2 px-1 py-0.5 text-[0.9em]">{children}</code>
      );
    },
    table: ({ children }) => (
      <div className="my-3 overflow-x-auto rounded-card border border-border">
        <table className="w-full border-collapse text-small">{children}</table>
      </div>
    ),
    th: ({ children }) => <th className="border-b border-border bg-surface-2 px-3 py-1.5 text-left font-semibold">{children}</th>,
    td: ({ children }) => <td className="border-b border-border px-3 py-1.5 even:bg-surface">{children}</td>,
    h1: ({ children }) => <h1 className="mt-4 mb-2 text-[20px] leading-7 font-semibold">{children}</h1>,
    h2: ({ children }) => <h2 className="mt-4 mb-2 text-lead font-semibold">{children}</h2>,
    h3: ({ children }) => <h3 className="mt-3 mb-1 text-chat font-semibold">{children}</h3>,
    ul: ({ children }) => <ul className="my-2 list-disc pl-5">{children}</ul>,
    ol: ({ children }) => <ol className="my-2 list-decimal pl-5">{children}</ol>,
    p: ({ children }) => <p className="my-2">{children}</p>,
    blockquote: ({ children }) => <blockquote className="my-2 border-l-2 border-border-strong pl-3 text-muted">{children}</blockquote>,
  };
}

const Block = memo(function Block({ text, components }: { text: string; components: Components }) {
  // react-markdown skips raw HTML by default: model output can never inject markup.
  return <ReactMarkdown remarkPlugins={[remarkGfm]} components={components}>{text}</ReactMarkdown>;
});

/** Markdown for chat: GFM, highlighted code, safe links; finished blocks are not re-rendered while streaming. */
export function Markdown({ source, onOpenLink, className }: MarkdownProps) {
  const components = useMemo(() => makeComponents(onOpenLink), [onOpenLink]);
  const blocks = useMemo(() => splitMarkdownBlocks(source), [source]);
  return (
    <div className={cn("text-chat leading-[26px] break-words", className)}>
      {blocks.map((b, i) => (
        <Block key={i} text={b} components={components} />
      ))}
    </div>
  );
}
