package shop

class cart_item(val name: String, val price: Double)

fun calcTotal(items: List<cart_item>): Double {
    var tmp = 0.0
    for (item in items) {
        tmp += item.price
    }
    return tmp
}
