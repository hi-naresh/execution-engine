use crate::algos::AlgoAction;
use crate::types::{Decimal, Exchange, ExecutionReport, OrderStatus, Side, Symbol};

#[derive(Debug, Clone)]
pub struct TwapAlgo {
    pub symbol: Symbol,
    pub side: Side,
    pub total_qty: Decimal,
    pub executed_qty: Decimal,
    pub start_time_ns: u64,
    pub end_time_ns: u64,
    pub interval_ns: u64,
    pub slice_qty: Decimal,

    // Internal state
    pub current_slice_idx: u64,
    pub total_slices: u64,
    pub active_order_id: Option<u64>,
    pub active_order_qty: Decimal,
    pub active_order_filled: Decimal,
    pub last_interval_start_ns: u64,
    pub is_finished: bool,
    pub client_order_id_counter: u64,
}

impl TwapAlgo {
    pub fn new(
        symbol: Symbol,
        side: Side,
        total_qty: Decimal,
        start_time_ns: u64,
        duration_ns: u64,
        interval_ns: u64,
        base_client_id: u64,
    ) -> Self {
        let total_slices = (duration_ns / interval_ns).max(1);
        let slice_qty = total_qty / total_slices;

        Self {
            symbol,
            side,
            total_qty,
            executed_qty: Decimal::ZERO,
            start_time_ns,
            end_time_ns: start_time_ns + duration_ns,
            interval_ns,
            slice_qty,
            current_slice_idx: 0,
            total_slices,
            active_order_id: None,
            active_order_qty: Decimal::ZERO,
            active_order_filled: Decimal::ZERO,
            last_interval_start_ns: 0,
            is_finished: false,
            client_order_id_counter: base_client_id,
        }
    }

    /// Handles timer ticks to decide when to submit new child orders or cancel existing ones.
    pub fn on_tick(&mut self, timestamp_ns: u64, best_price: Option<Decimal>) -> Option<AlgoAction> {
        if self.is_finished || timestamp_ns < self.start_time_ns {
            return None;
        }

        if timestamp_ns >= self.end_time_ns {
            // Check if we need to clean up the last order
            if self.active_order_id.is_some() {
                let order_id = self.active_order_id.take().unwrap();
                return Some(AlgoAction::CancelOrder {
                    client_order_id: order_id,
                    exchange: Exchange::Binance, // Mock/default venue
                });
            }
            self.is_finished = true;
            return None;
        }

        // Initialize first interval if not done
        if self.last_interval_start_ns == 0 {
            self.last_interval_start_ns = timestamp_ns;
            return self.submit_next_slice(best_price);
        }

        // Check if interval has elapsed
        if timestamp_ns - self.last_interval_start_ns >= self.interval_ns {
            self.last_interval_start_ns = timestamp_ns;
            self.current_slice_idx += 1;

            if self.current_slice_idx >= self.total_slices {
                self.is_finished = true;
                return None;
            }

            // Cancel any active order from previous interval
            if let Some(order_id) = self.active_order_id.take() {
                // Return cancel action first. The execution report will confirm cancellation,
                // and we will place the next slice.
                return Some(AlgoAction::CancelOrder {
                    client_order_id: order_id,
                    exchange: Exchange::Binance,
                });
            }

            return self.submit_next_slice(best_price);
        }

        None
    }

    fn submit_next_slice(&mut self, best_price: Option<Decimal>) -> Option<AlgoAction> {
        let price = best_price.unwrap_or(Decimal::from_f64(100.0)); // fallback
        self.client_order_id_counter += 1;
        let order_id = self.client_order_id_counter;

        self.active_order_id = Some(order_id);
        self.active_order_qty = self.slice_qty;
        self.active_order_filled = Decimal::ZERO;

        Some(AlgoAction::PlaceOrder {
            client_order_id: order_id,
            exchange: Exchange::Binance,
            side: self.side,
            price,
            quantity: self.slice_qty,
        })
    }

    /// Processes execution reports to track filled quantities.
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
                }
                OrderStatus::Cancelled | OrderStatus::Rejected => {
                    // Calculate remainder of this slice and place a market order or just let it go
                    self.active_order_id = None;
                }
                _ => {}
            }
        }
        None
    }
}
