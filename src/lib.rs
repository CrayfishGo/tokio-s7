pub mod client;
pub mod packet;
pub mod error;
pub mod tpkt;
pub mod cotp;
pub mod types;
pub mod header;
pub mod paramter;
pub mod datum;
pub mod item;


pub fn bytes_to_hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|b| format!("{:02X}", b))
        .collect::<Vec<_>>()
        .join(" ")
}
