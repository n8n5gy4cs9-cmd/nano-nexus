/** Move keyboard focus to the message box (after New chat, navigation, or sending). */
export function focusComposer(): void {
  requestAnimationFrame(() => document.querySelector<HTMLTextAreaElement>("[data-composer]")?.focus());
}
