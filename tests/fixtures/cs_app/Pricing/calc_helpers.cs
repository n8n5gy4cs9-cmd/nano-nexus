namespace Shop.Pricing
{
    public class cart_item
    {
        public string Name;
        public double Price;
        public cart_item(string name, double price) { Name = name; Price = price; }
        public double doStuff2(double p) { var tmp = Price * p; return tmp; }
    }

    public static class Calc
    {
        public const double TaxRate = 0.2;

        public static double CalcTotal(cart_item[] items)
        {
            double total = 0;
            foreach (var it in items) total += it.Price;
            return total;
        }
    }
}
