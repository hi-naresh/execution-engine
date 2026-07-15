use crate::order_book::OrderBook;
use crate::types::{
    BookUpdate, BookUpdateLevel, Decimal, Exchange, ExecutionReport, MarketEvent, MarketTrade,
    OrderRequest, OrderStatus, Side, Symbol,
};
use crossbeam_queue::ArrayQueue;
use rand::Rng;
use std::sync::Arc;

pub struct ExchangeSim {
    pub exchange: Exchange,
    pub symbol: Symbol,
    pub current_price: Decimal,
    pub book: OrderBook,
    pub resting_orders: Vec<OrderRequest>, // Pre-allocated resting orders
    pub order_id_counter: u64,
}

impl ExchangeSim {
    pub fn new(exchange: Exchange, symbol: Symbol, initial_price: Decimal) -> Self {
        let mut sim = Self {
            exchange,
            symbol,
            current_price: initial_price,
            book: OrderBook::new(),
            resting_orders: Vec::with_capacity(100),
            order_id_counter: 0,
        };
        sim.regenerate_book();
        sim
    }

    /// Regens the cache-friendly order book levels based on the current price.
    pub fn regenerate_book(&mut self) {
        let price_raw = self.current_price.0;
        let mut rng = rand::thread_rng();

        let mut bids = [BookUpdateLevel {
            price: Decimal::ZERO,
            quantity: Decimal::ZERO,
        }; 20];
        let mut asks = [BookUpdateLevel {
            price: Decimal::ZERO,
            quantity: Decimal::ZERO,
        }; 20];

        // Let's create spread: Binance has tighter spread, Coinbase wider spread
        let spread_offset = match self.exchange {
            Exchange::Binance => 10_000, // 0.0001
            Exchange::Coinbase => 30_000, // 0.0003
        };

        for i in 0..20 {
            // Bids descend from current_price - spread_offset
            let bid_price = price_raw.saturating_sub(spread_offset + (i as u64 * 10_000));
            let bid_qty = rng.gen_range(50_000_000..500_000_000); // 0.5 to 5.0
            bids[i] = BookUpdateLevel {
                price: Decimal(bid_price),
                quantity: Decimal(bid_qty),
            };

            // Asks ascend from current_price + spread_offset
            let ask_price = price_raw + spread_offset + (i as u64 * 10_000);
            let ask_qty = rng.gen_range(50_000_000..500_000_000); // 0.5 to 5.0
            asks[i] = BookUpdateLevel {
                price: Decimal(ask_price),
                quantity: Decimal(ask_qty),
            };
        }

        self.book.apply_update(&BookUpdate {
            symbol: self.symbol,
            exchange: self.exchange,
            bids,
            asks,
            bid_count: 20,
            ask_count: 20,
            timestamp_ns: 0,
        });
    }

    /// Ticks the simulated price using a random walk and matches resting orders.
    pub fn tick(
        &mut self,
        timestamp_ns: u64,
        input_queue: &Arc<ArrayQueue<MarketEvent>>,
    ) -> Option<MarketEvent> {
        let mut rng = rand::thread_rng();
        // Price change: random walk +/- 0.02%
        let pct = rng.gen_range(-20..=20); // -0.02% to +0.02%
        let delta = ((self.current_price.0 as i64 * pct) / 100_000) as i64;
        let new_price = (self.current_price.0 as i64 + delta).max(1) as u64;
        self.current_price = Decimal(new_price);

        self.regenerate_book();

        // 1. Generate BookUpdate event
        let book_event = MarketEvent::BookUpdate(BookUpdate {
            symbol: self.symbol,
            exchange: self.exchange,
            bids: self.book.bids,
            asks: self.book.asks,
            bid_count: self.book.bid_count,
            ask_count: self.book.ask_count,
            timestamp_ns,
        });
        let _ = input_queue.push(book_event);

        // 2. Generate random public trades (from time to time)
        if rng.gen_bool(0.3) {
            let trade_qty = rng.gen_range(10_000_000..150_000_000); // 0.1 to 1.5
            let side = if rng.gen_bool(0.5) { Side::Buy } else { Side::Sell };
            let trade_price = if side == Side::Buy {
                self.book.asks[0].price
            } else {
                self.book.bids[0].price
            };

            let trade_event = MarketEvent::Trade(MarketTrade {
                symbol: self.symbol,
                exchange: self.exchange,
                price: trade_price,
                quantity: Decimal(trade_qty),
                side,
                timestamp_ns,
            });
            let _ = input_queue.push(trade_event);
        }

        // 3. Match resting orders
        let mut i = 0;
        while i < self.resting_orders.len() {
            let order = &self.resting_orders[i];
            let is_filled = match order.side {
                Side::Buy => self.current_price.0 <= order.price.0,
                Side::Sell => self.current_price.0 >= order.price.0,
            };

            if is_filled {
                self.order_id_counter += 1;
                let report = ExecutionReport {
                    client_order_id: order.client_order_id,
                    exchange_order_id: self.order_id_counter,
                    symbol: order.symbol,
                    exchange: order.exchange,
                    status: OrderStatus::Filled,
                    last_qty: order.quantity,
                    last_price: order.price,
                    leaves_qty: Decimal::ZERO,
                    cumulative_qty: order.quantity,
                    timestamp_ns,
                };
                let _ = input_queue.push(MarketEvent::ExecutionReport(report));
                self.resting_orders.swap_remove(i);
            } else {
                i += 1;
            }
        }

        None
    }

    /// Handles an incoming order request from the execution loop.
    pub fn handle_order_request(
        &mut self,
        req: OrderRequest,
        timestamp_ns: u64,
        input_queue: &Arc<ArrayQueue<MarketEvent>>,
    ) {
        self.order_id_counter += 1;
        let ex_id = self.order_id_counter;

        // Immediately send PendingNew -> New
        let report_new = ExecutionReport {
            client_order_id: req.client_order_id,
            exchange_order_id: ex_id,
            symbol: req.symbol,
            exchange: self.exchange,
            status: OrderStatus::New,
            last_qty: Decimal::ZERO,
            last_price: Decimal::ZERO,
            leaves_qty: req.quantity,
            cumulative_qty: Decimal::ZERO,
            timestamp_ns,
        };
        let _ = input_queue.push(MarketEvent::ExecutionReport(report_new));

        match req.order_type {
            crate::types::OrderType::Market => {
                // Fill immediately against the book
                let mut filled_qty = Decimal::ZERO;
                let mut total_cost = 0u128;
                let mut remaining = req.quantity;

                match req.side {
                    Side::Buy => {
                        for i in 0..self.book.ask_count {
                            if remaining.is_zero() {
                                break;
                            }
                            let ask = &self.book.asks[i];
                            let fill = ask.quantity.min(remaining);
                            filled_qty += fill;
                            total_cost += fill.0 as u128 * ask.price.0 as u128;
                            remaining -= fill;
                        }
                    }
                    Side::Sell => {
                        for i in 0..self.book.bid_count {
                            if remaining.is_zero() {
                                break;
                            }
                            let bid = &self.book.bids[i];
                            let fill = bid.quantity.min(remaining);
                            filled_qty += fill;
                            total_cost += fill.0 as u128 * bid.price.0 as u128;
                            remaining -= fill;
                        }
                    }
                }

                if filled_qty.0 > 0 {
                    let avg_price = Decimal((total_cost / filled_qty.0 as u128) as u64);
                    let report_fill = ExecutionReport {
                        client_order_id: req.client_order_id,
                        exchange_order_id: ex_id,
                        symbol: req.symbol,
                        exchange: self.exchange,
                        status: if remaining.is_zero() {
                            OrderStatus::Filled
                        } else {
                            OrderStatus::PartiallyFilled
                        },
                        last_qty: filled_qty,
                        last_price: avg_price,
                        leaves_qty: remaining,
                        cumulative_qty: filled_qty,
                        timestamp_ns,
                    };
                    let _ = input_queue.push(MarketEvent::ExecutionReport(report_fill));
                } else {
                    // No liquidity, reject order
                    let report_reject = ExecutionReport {
                        client_order_id: req.client_order_id,
                        exchange_order_id: ex_id,
                        symbol: req.symbol,
                        exchange: self.exchange,
                        status: OrderStatus::Rejected,
                        last_qty: Decimal::ZERO,
                        last_price: Decimal::ZERO,
                        leaves_qty: req.quantity,
                        cumulative_qty: Decimal::ZERO,
                        timestamp_ns,
                    };
                    let _ = input_queue.push(MarketEvent::ExecutionReport(report_reject));
                }
            }
            crate::types::OrderType::Limit => {
                // Check if it crosses current book for immediate fill
                let mut can_fill = false;
                match req.side {
                    Side::Buy => {
                        if let Some(best_ask) = self.book.get_best_ask() {
                            if req.price.0 >= best_ask.price.0 {
                                can_fill = true;
                            }
                        }
                    }
                    Side::Sell => {
                        if let Some(best_bid) = self.book.get_best_bid() {
                            if req.price.0 <= best_bid.price.0 {
                                can_fill = true;
                            }
                        }
                    }
                }

                if can_fill {
                    // Immediate fill
                    let report_fill = ExecutionReport {
                        client_order_id: req.client_order_id,
                        exchange_order_id: ex_id,
                        symbol: req.symbol,
                        exchange: self.exchange,
                        status: OrderStatus::Filled,
                        last_qty: req.quantity,
                        last_price: req.price,
                        leaves_qty: Decimal::ZERO,
                        cumulative_qty: req.quantity,
                        timestamp_ns,
                    };
                    let _ = input_queue.push(MarketEvent::ExecutionReport(report_fill));
                } else {
                    // Rest order
                    self.resting_orders.push(req);
                }
            }
        }
    }

    /// Handles cancellation request.
    pub fn handle_cancel(
        &mut self,
        client_order_id: u64,
        timestamp_ns: u64,
        input_queue: &Arc<ArrayQueue<MarketEvent>>,
    ) {
        if let Some(idx) = self
            .resting_orders
            .iter()
            .position(|o| o.client_order_id == client_order_id)
        {
            let req = self.resting_orders.swap_remove(idx);
            self.order_id_counter += 1;
            let report = ExecutionReport {
                client_order_id,
                exchange_order_id: self.order_id_counter,
                symbol: req.symbol,
                exchange: self.exchange,
                status: OrderStatus::Cancelled,
                last_qty: Decimal::ZERO,
                last_price: Decimal::ZERO,
                leaves_qty: Decimal::ZERO,
                cumulative_qty: Decimal::ZERO, // in simple mock, we assume 0 was filled while resting
                timestamp_ns,
            };
            let _ = input_queue.push(MarketEvent::ExecutionReport(report));
        }
    }
}
