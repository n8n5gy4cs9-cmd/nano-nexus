mod Helpers;
mod pricing;

use crate::pricing::{calc_total, CartItem, TAX_RATE};
use Helpers::fmt_money;

fn main() {
    let items = vec![CartItem { name: "pen".into(), price: 2.5 }];
    let total = calc_total(&items);
    println!("{}", fmt_money(total * (1.0 + TAX_RATE)));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn total_works() {
        assert_eq!(calc_total(&[]), 0.0);
    }
}
