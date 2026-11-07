//! kiosk: greet a customer, take one order, save it.
mod store;
mod util;

fn main() {
    let name = std::env::args().nth(1).unwrap_or_else(|| "guest".to_string());
    println!("{}", util::greet(&name));
    let order = store::Order { id: 1, item: "coffee".to_string(), qty: 2 };
    store::save(&order).expect("save order");
}
