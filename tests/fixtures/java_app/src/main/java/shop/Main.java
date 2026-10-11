package shop;

import shop.pricing.Calc;
import shop.pricing.cart_item;

public class Main {
    public static void main(String[] args) throws Exception {
        cart_item[] items = { new cart_item("pen", 2.5) };
        double total = Calc.calcTotal(items);
        var m = Calc.class.getMethod("calcTotal", cart_item[].class);
        System.out.println(total + " " + m.getName());
    }
}
