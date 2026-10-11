pub const TAX_RATE: f64 = 0.2;

pub struct CartItem {
    pub name: String,
    pub price: f64,
}

impl CartItem {
    pub fn doStuff2(&self, p: f64) -> f64 {
        let tmp = self.price * p;
        tmp
    }
}

pub fn calc_total(items: &[CartItem]) -> f64 {
    items.iter().map(|i| i.price).sum()
}
