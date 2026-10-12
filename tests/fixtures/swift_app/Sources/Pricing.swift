struct cart_item {
    var name: String
    var price: Double
}

func calcTotal(items: [cart_item]) -> Double {
    var tmp = 0.0
    for item in items {
        tmp += item.price
    }
    return tmp
}
