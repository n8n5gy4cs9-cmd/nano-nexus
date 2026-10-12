package shop

import shop.calcTotal

fun main() {
    val items = listOf(cart_item("pen", 2.5))
    println(calcTotal(items))
}
