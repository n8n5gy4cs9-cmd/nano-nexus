#include <stdio.h>
#include "util/pricing.h"

int main(void) {
    cart_item items[1] = {{2, 5}};
    printf("%d\n", calc_total(items, 1));
    return 0;
}
