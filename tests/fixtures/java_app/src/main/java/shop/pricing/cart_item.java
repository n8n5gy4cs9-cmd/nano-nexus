package shop.pricing;

public class cart_item {
    public String name;
    public double price;

    public cart_item(String name, double price) {
        this.name = name;
        this.price = price;
    }

    public double doStuff2(double p) {
        double tmp = price * p;
        return tmp;
    }
}
