using System;
using Shop.Pricing;

namespace Shop
{
    public class Program
    {
        public static void Main(string[] args)
        {
            var items = new[] { new cart_item("pen", 2.5) };
            var total = Calc.CalcTotal(items);
            var name = nameof(Calc.CalcTotal);
            Console.WriteLine(total + name);
        }
    }
}
