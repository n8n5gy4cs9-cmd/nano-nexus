export function Inspector() {
  return (
    <aside className="flex w-[300px] shrink-0 flex-col border-l border-border bg-surface" aria-label="Inspector">
      <div className="flex h-11 items-center border-b border-border px-4 text-body font-semibold">Inspector</div>
      <div className="p-4 text-small text-muted">Details about the current item appear here.</div>
    </aside>
  );
}
