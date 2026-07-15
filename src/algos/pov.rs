use crate::algos::AlgoAction;
use crate::types::{Decimal, Exchange, ExecutionReport, OrderStatus, Side, Symbol};

#[derive(Debug, Clone)]
pub struct PovAlgo {
    pub symbol: Symbol,
    pub side: Side,
    pub total_qty: Decimal,
    pub executed_qty: Decimal,
    pub participation_rate: Decimal, // e.g., 0.10 for 10%

    // Internal tracking
    pub accumulated_target_qty: Decimal,
    pub active_order_id: Option<u64>,
    pub active_order_qty: Decimal,
    pub active_order_filled: Decimal,
    pub client_order_id_counter: u64,
    pub is_finished: bool,
}

impl PovAlgo {
    pub fn new(
        symbol: Symbol,
        side: Side,
        total_qty: Decimal,
        participation_rate: Decimal,
        base_client_id: u64,
    ) -> Self {
        Self {
            symbol,
            side,
            total_qty,
            executed_qty: Decimal::ZERO,
            participation_rate,
            accumulated_target_qty: Decimal::ZERO,
            active_order_id: None,
            active_order_qty: Decimal::ZERO,
            active_order_filled: Decimal::ZERO,
            client_order_id_counter: base_client_id,
            is_finished: false,
        }
    }

    /// Handles market trades to calculate trade volume and accumulate participation target.
    pub fn on_market_trade(
        &mut self,
        trade_qty: Decimal,
        best_price: Option<Decimal>,
    ) -> Option<AlgoAction> {
        if self.is_finished {
            return None;
        }

        // Calculate: target_delta = trade_qty * P / (1 - P)
        let p = self.participation_rate;
        let one_minus_p = Decimal::from_f64(1.0) - p;
        if one_minus_p.is_zero() {
            return None;
        }
        let target_delta = (trade_qty * p) / one_minus_p;
        self.accumulated_target_qty += target_delta;

        // Clip accumulated target to total order size
        if self.accumulated_target_qty.0 > self.total_qty.0 {
            self.accumulated_target_qty = self.total_qty;
        }

        // If we don't have an active order and need to catch up, place an order
        if self.active_order_id.is_none() && self.executed_qty.0 < self.accumulated_target_qty.0 {
            let to_fill = self.accumulated_target_qty - self.executed_qty;
            let order_qty = to_fill.min(self.total_qty - self.executed_qty);

            if order_qty.0 > 0 {
                self.client_order_id_counter += 1;
                let order_id = self.client_order_id_counter;

                self.active_order_id = Some(order_id);
                self.active_order_qty = order_qty;
                self.active_order_filled = Decimal::ZERO;

                let price = best_price.unwrap_or(Decimal::from_f64(100.0));
                return Some(AlgoAction::PlaceOrder {
                    client_order_id: order_id,
                    exchange: Exchange::Binance,
                    side: self.side,
                    price,
                    quantity: order_qty,
                });
            }
        }

        None
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
                    self.active_order_id = None;
                }
                _ => {}
            }

            if self.executed_qty.0 >= self.total_qty.0 {
                self.is_finished = true;
            }
        }
        None
    }
}
