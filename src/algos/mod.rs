pub mod iceberg;
pub mod pov;
pub mod twap;

use crate::types::{Decimal, Exchange, Side};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlgoAction {
    PlaceOrder {
        client_order_id: u64,
        exchange: Exchange,
        side: Side,
        price: Decimal,
        quantity: Decimal,
    },
    CancelOrder {
        client_order_id: u64,
        exchange: Exchange,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlgoType {
    Twap,
    Pov,
    Iceberg,
}
