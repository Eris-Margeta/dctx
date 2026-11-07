use std::io;

/// One order as the kiosk records it.
pub struct Order {
    pub id: u32,
    pub item: String,
    pub qty: u32,
}

/// Write the order to orders/<id>.tsv.
pub fn save(order: &Order) -> io::Result<()> {
    std::fs::create_dir_all("orders")?;
    std::fs::write(format!("orders/{}.tsv", order.id), encode(order))
}

/// One tab-separated line with a checksum of the item name.
fn encode(order: &Order) -> String {
    format!("{}\t{}\t{}\t{:08x}\n", order.id, order.item, order.qty, checksum(&order.item))
}

/// FNV-1a over the bytes of the text.
fn checksum(text: &str) -> u32 {
    text.bytes().fold(0x811c9dc5, |hash, byte| (hash ^ u32::from(byte)).wrapping_mul(0x0100_0193))
}
