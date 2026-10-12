#pragma once
#include <vector>

namespace shop {

struct cart_item {
    int qty;
    int price;
};

class Calc {
public:
    int calcTotal(const std::vector<cart_item> &items) const;
};

}  // namespace shop
