#include "pricing.hpp"

namespace shop {

int Calc::calcTotal(const std::vector<cart_item> &items) const {
    int tmp = 0;
    for (const auto &item : items) {
        tmp += item.qty * item.price;
    }
    return tmp;
}

}  // namespace shop
