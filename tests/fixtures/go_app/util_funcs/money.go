package util_funcs

import "fmt"

func FmtMoney(v float64) string {
	return fmt.Sprintf("$%.2f", v)
}
