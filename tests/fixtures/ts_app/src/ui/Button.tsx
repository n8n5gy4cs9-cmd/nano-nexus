import { fmtMoney } from "../lib/Helper_Funcs";

export function Button(props: { label: string }) {
  return `<button>${props.label} ${fmtMoney(1)}</button>`;
}
