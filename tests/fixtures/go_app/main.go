package main

import (
	"fmt"

	"example.com/shop/pricing"
	"example.com/shop/util_funcs"
)

func main() {
	items := []pricing.CartItem{{Name: "pen", Price: 2.5}}
	total := pricing.CalcTotal(items)
	fmt.Println(util_funcs.FmtMoney(total * (1 + pricing.TaxRate)))
}
