#ifndef PRICING_H
#define PRICING_H

typedef struct cart_item {
    int qty;
    int price;
} cart_item;

int calc_total(const cart_item *items, int n);

#endif
