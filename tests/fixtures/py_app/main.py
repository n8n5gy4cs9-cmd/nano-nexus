"""Entry point of the shop demo."""
from util.pricing import calc_total, cart_item, TAX_RATE
from util import pricing
from Helper_Funcs import fmt_money


def main():
    items = [cart_item("pen", 2.5), cart_item("book", 12)]
    total = calc_total(items)
    fn = getattr(pricing, "calc_total")
    print(fmt_money(fn(items) * (1 + TAX_RATE)), total)


if __name__ == "__main__":
    main()
