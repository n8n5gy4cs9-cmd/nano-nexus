import { calcTotal, cart_item, TAX_RATE } from "@lib/pricing";
import { fmtMoney } from "./lib/Helper_Funcs";
import { Button } from "./ui/Button";

const registry: Record<string, unknown> = { calcTotal };

export function main(): string {
  const items = [new cart_item("pen", 2.5)];
  const fn = registry["calcTotal"] as typeof calcTotal;
  return fmtMoney(fn(items) * (1 + TAX_RATE)) + Button({ label: "ok" });
}
