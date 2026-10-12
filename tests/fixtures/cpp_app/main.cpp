#include <iostream>
#include "util/pricing.hpp"

int main() {
    shop::Calc calc;
    std::vector<shop::cart_item> items = {{2, 5}};
    std::cout << calc.calcTotal(items) << std::endl;
    return 0;
}
