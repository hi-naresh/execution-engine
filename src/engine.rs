use crate::algos::iceberg::IcebergAlgo;
use crate::algos::pov::PovAlgo;
use crate::algos::twap::TwapAlgo;
use crate::algos::AlgoAction;
use crate::order_book::OrderBook;
use crate::sor::route_order;
use crate::tca::TcaTracker;
use crate::types::{
    Decimal, Exchange, MarketEvent, OrderRequest, OrderType, Side, Symbol, TimeInForce,
};
use crossbeam_queue::ArrayQueue;
use std::sync::Arc;

pub struct ExecutionEngine {
    pub book_binance: OrderBook,
    pub book_coinbase: OrderBook,

    // Active algorithms
    pub twap: Option<TwapAlgo>,
    pub pov: Option<PovAlgo>,
    pub iceberg: Option<IcebergAlgo>,

    // Queues
    pub output_queue: Arc<ArrayQueue<OrderRequest>>,

    // TCA Tracker
    pub tca: TcaTracker,

    // Counter to map client order IDs for routing splits
    pub internal_client_order_id_counter: u64,
    // Maps routed child IDs back to the algo's client order ID: routed_id -> algo_order_id
    pub child_to_algo_id: std::collections::HashMap<u64, (u64, String)>,
}

impl ExecutionEngine {
    pub fn new(output_queue: Arc<ArrayQueue<OrderRequest>>) -> Self {
        Self {
            book_binance: OrderBook::new(),
            book_coinbase: OrderBook::new(),
            twap: None,
            pov: None,
            iceberg: None,
            output_queue,
            tca: TcaTracker::new(),
            internal_client_order_id_counter: 1_000_000,
            child_to_algo_id: std::collections::HashMap::new(),
        }
    }

    /// Entry point to process a market or execution event in the hot loop.
    pub fn on_event(&mut self, event: MarketEvent) {
        match event {
            MarketEvent::BookUpdate(update) => {
                match update.exchange {
                    Exchange::Binance => self.book_binance.apply_update(&update),
                    Exchange::Coinbase => self.book_coinbase.apply_update(&update),
                }

                // Tick algorithms that depend on prices
                let timestamp = update.timestamp_ns;
                let mid_binance = self.book_binance.get_mid_price();

                if let Some(ref mut twap) = self.twap {
                    if let Some(action) = twap.on_tick(timestamp, mid_binance) {
                        self.handle_algo_action(action, "TWAP", timestamp);
                    }
                }
            }
            MarketEvent::Trade(trade) => {
                let mid_binance = self.book_binance.get_mid_price();
                if let Some(ref mut pov) = self.pov {
                    if let Some(action) = pov.on_market_trade(trade.quantity, mid_binance) {
                        self.handle_algo_action(action, "POV", trade.timestamp_ns);
                    }
                }
            }
            MarketEvent::ExecutionReport(report) => {
                // Determine if this report belongs to a routed child order
                let mut original_report = report;
                let mut algo_name = "DIRECT".to_string();

                if let Some(&(algo_order_id, ref name)) = self.child_to_algo_id.get(&report.client_order_id) {
                    algo_name = name.clone();
                    // Map report back to what the algorithm expects
                    original_report.client_order_id = algo_order_id;
                }

                // Record execution in TCA
                self.tca.record_execution_report(&report);

                // Forward execution report to active algorithms
                if algo_name == "TWAP" {
                    if let Some(ref mut twap) = self.twap {
                        if let Some(action) = twap.on_execution_report(&original_report) {
                            self.handle_algo_action(action, "TWAP", report.timestamp_ns);
                        }
                    }
                } else if algo_name == "POV" {
                    if let Some(ref mut pov) = self.pov {
                        if let Some(action) = pov.on_execution_report(&original_report) {
                            self.handle_algo_action(action, "POV", report.timestamp_ns);
                        }
                    }
                } else if algo_name == "ICEBERG" {
                    if let Some(ref mut iceberg) = self.iceberg {
                        if let Some(action) = iceberg.on_execution_report(&original_report) {
                            self.handle_algo_action(action, "ICEBERG", report.timestamp_ns);
                        }
                    }
                }
            }
            MarketEvent::TimerTick { timestamp_ns } => {
                let mid_binance = self.book_binance.get_mid_price();
                if let Some(ref mut twap) = self.twap {
                    if let Some(action) = twap.on_tick(timestamp_ns, mid_binance) {
                        self.handle_algo_action(action, "TWAP", timestamp_ns);
                    }
                }
            }
        }
    }

    /// Processes an action requested by an execution algorithm.
    /// Intercepts order placement to perform Smart Order Routing (SOR) across venues.
    pub fn handle_algo_action(&mut self, action: AlgoAction, algo_name: &str, timestamp_ns: u64) {
        match action {
            AlgoAction::PlaceOrder {
                client_order_id,
                side,
                price,
                quantity,
                ..
            } => {
                // Run Smart Order Routing (SOR) over the Binance and Coinbase books
                let (routes, route_count) = route_order(&self.book_binance, &self.book_coinbase, side, quantity);

                if route_count == 0 {
                    // Fallback to direct routing if SOR returns nothing
                    self.internal_client_order_id_counter += 1;
                    let child_id = self.internal_client_order_id_counter;
                    self.child_to_algo_id.insert(child_id, (client_order_id, algo_name.to_string()));

                    let req = OrderRequest {
                        client_order_id: child_id,
                        symbol: Symbol::BTCUSDT,
                        exchange: Exchange::Binance,
                        side,
                        order_type: OrderType::Limit,
                        price,
                        quantity,
                        time_in_force: TimeInForce::GTC,
                    };
                    self.tca.record_order_request(&req, algo_name, timestamp_ns);
                    let _ = self.output_queue.push(req);
                } else {
                    for i in 0..route_count {
                        let route = &routes[i];
                        self.internal_client_order_id_counter += 1;
                        let child_id = self.internal_client_order_id_counter;
                        self.child_to_algo_id.insert(child_id, (client_order_id, algo_name.to_string()));

                        let req = OrderRequest {
                            client_order_id: child_id,
                            symbol: Symbol::BTCUSDT,
                            exchange: route.exchange,
                            side,
                            order_type: OrderType::Limit,
                            price: route.price, // route price is taken from order book
                            quantity: route.quantity,
                            time_in_force: TimeInForce::GTC,
                        };
                        self.tca.record_order_request(&req, algo_name, timestamp_ns);
                        let _ = self.output_queue.push(req);
                    }
                }
            }
            AlgoAction::CancelOrder { client_order_id, exchange } => {
                // Find any routed child order IDs associated with this algo order ID
                let mut children_to_cancel = Vec::new();
                for (&child_id, &(algo_id, ref name)) in &self.child_to_algo_id {
                    if algo_id == client_order_id && name == algo_name {
                        children_to_cancel.push(child_id);
                    }
                }

                for child_id in children_to_cancel {
                    // Send simulated cancel request by pushing a special 0-qty order request
                    // representing a cancellation.
                    let cancel_req = OrderRequest {
                        client_order_id: child_id,
                        symbol: Symbol::BTCUSDT,
                        exchange,
                        side: Side::Buy,             // placeholder
                        order_type: OrderType::Limit, // placeholder
                        price: Decimal::ZERO,        // 0 price indicates cancellation
                        quantity: Decimal::ZERO,     // 0 qty indicates cancellation
                        time_in_force: TimeInForce::GTC,
                    };
                    let _ = self.output_queue.push(cancel_req);
                }
            }
        }
    }
}
