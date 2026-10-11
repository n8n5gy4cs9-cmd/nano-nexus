package pricing

const TaxRate = 0.2

type CartItem struct {
	Name  string
	Price float64
}

func (c CartItem) doStuff2(p float64) float64 {
	tmp := c.Price * p
	return tmp
}

func CalcTotal(items []CartItem) float64 {
	total := 0.0
	for _, it := range items {
		total += it.Price
	}
	return total
}
