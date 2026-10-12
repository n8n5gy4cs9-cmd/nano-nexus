#include "pricing.h"

int calc_total(const cart_item *items, int n) {
    int tmp = 0;
    for (int i = 0; i < n; i++) {
        tmp += items[i].qty * items[i].price;
    }
    return tmp;
}
