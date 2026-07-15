use crate::algos::AlgoAction;
use crate::types::{Decimal, Exchange, ExecutionReport, OrderStatus, Side, Symbol};

#[derive(Debug, Clone)]
pub struct IcebergAlgo {
    pub symbol: Symbol,
    pub side: Side,
    pub total_qty: Decimal,
    pub limit_price: Decimal,
    pub visible_qty: Decimal,
    pub executed_qty: Decimal,

    // Internal tracking
    pub active_order_id: Option<u64>,
    pub active_order_qty: Decimal,
    pub active_order_filled: Decimal,
    pub client_order_id_counter: u64,
    pub is_finished: bool,
}

impl IcebergAlgo {
    pub fn new(
        symbol: Symbol,
        side: Side,
        total_qty: Decimal,
        limit_price: Decimal,
        visible_qty: Decimal,
        base_client_id: u64,
    ) -> Self {
        Self {
            symbol,
            side,
            total_qty,
            limit_price,
            visible_qty,
            executed_qty: Decimal::ZERO,
            active_order_id: None,
            active_order_qty: Decimal::ZERO,
            active_order_filled: Decimal::ZERO,
            client_order_id_counter: base_client_id,
            is_finished: false,
        }
    }

    /// Triggers the initial visible order slice.
    pub fn start(&mut self) -> Option<AlgoAction> {
        if self.is_finished || self.active_order_id.is_some() {
            return None;
        }

        self.submit_next_slice()
    }

    fn submit_next_slice(&mut self) -> Option<AlgoAction> {
        let remaining = self.total_qty - self.executed_qty;
        if remaining.is_zero() {
            self.is_finished = true;
            return None;
        }

        let slice_size = self.visible_qty.min(remaining);
        self.client_order_id_counter += 1;
        let order_id = self.client_order_id_counter;

        self.active_order_id = Some(order_id);
        self.active_order_qty = slice_size;
        self.active_order_filled = Decimal::ZERO;

        Some(AlgoAction::PlaceOrder {
            client_order_id: order_id,
            exchange: Exchange::Binance,
            side: self.side,
            price: self.limit_price,
            quantity: slice_size,
        })
    }

    /// Processes execution reports and triggers the next slice upon full fill of the active slice.
    pub fn on_execution_report(&mut self, report: &ExecutionReport) -> Option<AlgoAction> {
        if self.is_finished {
            return None;
        }

        if Some(report.client_order_id) == self.active_order_id {
            let fill_qty = report.last_qty;
            self.active_order_filled += fill_qty;
            self.executed_qty += fill_qty;

            match report.status {
                OrderStatus::Filled => {
                    self.active_order_id = None;
                    if self.executed_qty.0 >= self.total_qty.0 {
                        self.is_finished = true;
                        return None;
                    } else {
                        return self.submit_next_slice();
                    }
                }
                OrderStatus::Cancelled | OrderStatus::Rejected => {
                    self.active_order_id = None;
                    self.is_finished = true;
                }
                _ => {}
            }
        }
        None
    }
}
