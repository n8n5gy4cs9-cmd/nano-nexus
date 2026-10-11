export const TAX_RATE = 0.2;

export class cart_item {
  constructor(public name: string, public price: number) {}
  doStuff2(p: number): number {
    const tmp = this.price * p;
    return tmp;
  }
}

export function calcTotal(items: cart_item[]): number {
  let total = 0;
  for (const it of items) total += it.price;
  return total;
}
