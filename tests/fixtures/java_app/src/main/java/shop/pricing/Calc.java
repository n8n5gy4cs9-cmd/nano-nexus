package shop.pricing;

public class Calc {
    public static final double TAX_RATE = 0.2;

    public static double calcTotal(cart_item[] items) {
        double total = 0;
        for (cart_item it : items) total += it.price;
        return total;
    }
}
