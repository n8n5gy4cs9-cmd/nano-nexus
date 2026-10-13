import { footerParts, type ChatFooterContext } from "@/plugins";

/** Tiny, quiet line under the composer, filled by the enabled plugins (model name, speed, ...). */
export function ChatStatusLine({ ctx, disabled }: { ctx: ChatFooterContext; disabled: ReadonlySet<string> }) {
  const parts = footerParts(ctx, disabled);
  if (parts.length === 0) return null;
  const text = parts.join(" · ");
  return (
    <p aria-label="Plugins" className="truncate px-5 pb-2 text-center text-caption text-faint" title={text}>
      {text}
    </p>
  );
}
