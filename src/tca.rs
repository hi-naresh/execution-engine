use crate::types::{Decimal, Exchange, ExecutionReport, OrderRequest, Side, Symbol};
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct ParentOrder {
    pub algo_name: String,
    pub symbol: Symbol,
    pub side: Side,
    pub total_qty: Decimal,
    pub benchmark_price: Decimal,
    pub start_time_ns: u64,
}

#[derive(Debug, Clone, Default)]
pub struct AlgoTcaReport {
    pub algo_name: String,
    pub symbol: Symbol,
    pub side: Side,
    pub total_qty: Decimal,
    pub executed_qty: Decimal,
    pub benchmark_price: Decimal,
    pub avg_execution_price: Decimal,
    pub total_slippage_bps: f64,
    pub total_fees: Decimal,
    pub avg_latency_us: f64,
    pub p95_latency_us: f64,
}

pub struct TcaTracker {
    // Tracks parent order details
    pub parent_orders: HashMap<String, ParentOrder>,
    // Tracks order send times: client_order_id -> timestamp_ns
    pub order_send_times: HashMap<u64, u64>,
    // Tracks order placement associations: client_order_id -> parent_algo_name
    pub order_to_parent: HashMap<u64, String>,
    // Accumulators per parent order: parent_algo_name -> (total_qty_filled, total_cost_raw, total_fees_raw, latencies)
    pub execution_accumulators: HashMap<String, (Decimal, u128, u64, Vec<u64>)>,
}

impl TcaTracker {
    pub fn new() -> Self {
        Self {
            parent_orders: HashMap::new(),
            order_send_times: HashMap::new(),
            order_to_parent: HashMap::new(),
            execution_accumulators: HashMap::new(),
        }
    }

    pub fn register_parent_order(
        &mut self,
        algo_name: &str,
        symbol: Symbol,
        side: Side,
        total_qty: Decimal,
        benchmark_price: Decimal,
        start_time_ns: u64,
    ) {
        self.parent_orders.insert(
            algo_name.to_string(),
            ParentOrder {
                algo_name: algo_name.to_string(),
                symbol,
                side,
                total_qty,
                benchmark_price,
                start_time_ns,
            },
        );
        self.execution_accumulators.insert(
            algo_name.to_string(),
            (Decimal::ZERO, 0, 0, Vec::new()),
        );
    }

    pub fn record_order_request(&mut self, req: &OrderRequest, parent_algo_name: &str, timestamp_ns: u64) {
        self.order_send_times.insert(req.client_order_id, timestamp_ns);
        self.order_to_parent.insert(req.client_order_id, parent_algo_name.to_string());
    }

    pub fn record_execution_report(&mut self, report: &ExecutionReport) {
        let parent_name = match self.order_to_parent.get(&report.client_order_id) {
            Some(name) => name.clone(),
            None => return, // not tracked
        };

        // If filled or partially filled, accumulate stats
        if report.last_qty.0 > 0 {
            if let Some((filled_qty, total_cost, total_fees, latencies)) =
                self.execution_accumulators.get_mut(&parent_name)
            {
                *filled_qty += report.last_qty;
                *total_cost += report.last_qty.0 as u128 * report.last_price.0 as u128;

                // Calculate fee: Binance = 0.1%, Coinbase = 0.4%
                let fee_bps = match report.exchange {
                    Exchange::Binance => 10,
                    Exchange::Coinbase => 40,
                };
                let fee_val = (report.last_qty.0 as u128 * report.last_price.0 as u128 * fee_bps as u128)
                    / (10000 * crate::types::SCALE as u128);
                *total_fees += fee_val as u64;

                // Latency calculation
                if let Some(send_time) = self.order_send_times.get(&report.client_order_id) {
                    if report.timestamp_ns >= *send_time {
                        latencies.push(report.timestamp_ns - *send_time);
                    }
                }
            }
        }
    }

    pub fn generate_reports(&self) -> Vec<AlgoTcaReport> {
        let mut reports = Vec::new();

        for (algo_name, parent) in &self.parent_orders {
            if let Some((filled_qty, total_cost, total_fees_raw, latencies)) =
                self.execution_accumulators.get(algo_name)
            {
                if filled_qty.0 == 0 {
                    continue;
                }

                let avg_price = Decimal((total_cost / filled_qty.0 as u128) as u64);

                // Calculate slippage in bps
                // Slippage Bps = (AvgPrice - BenchPrice) / BenchPrice * 10000 (for buy)
                // Slippage Bps = (BenchPrice - AvgPrice) / BenchPrice * 10000 (for sell)
                let diff = if parent.side == Side::Buy {
                    avg_price.to_f64() - parent.benchmark_price.to_f64()
                } else {
                    parent.benchmark_price.to_f64() - avg_price.to_f64()
                };
                let slippage_bps = (diff / parent.benchmark_price.to_f64()) * 10000.0;

                // Latency calculations in microseconds
                let mut latencies_us: Vec<f64> = latencies
                    .iter()
                    .map(|&ns| ns as f64 / 1000.0)
                    .collect();
                latencies_us.sort_by(|a, b| a.partial_cmp(b).unwrap());

                let avg_latency = if latencies_us.is_empty() {
                    0.0
                } else {
                    latencies_us.iter().sum::<f64>() / latencies_us.len() as f64
                };

                let p95_latency = if latencies_us.is_empty() {
                    0.0
                } else {
                    let idx = (latencies_us.len() as f64 * 0.95).floor() as usize;
                    latencies_us[idx.min(latencies_us.len() - 1)]
                };

                reports.push(AlgoTcaReport {
                    algo_name: algo_name.clone(),
                    symbol: parent.symbol,
                    side: parent.side,
                    total_qty: parent.total_qty,
                    executed_qty: *filled_qty,
                    benchmark_price: parent.benchmark_price,
                    avg_execution_price: avg_price,
                    total_slippage_bps: slippage_bps,
                    total_fees: Decimal(*total_fees_raw),
                    avg_latency_us: avg_latency,
                    p95_latency_us: p95_latency,
                });
            }
        }

        reports
    }

    pub fn print_reports(&self) {
        let reports = self.generate_reports();
        println!("\n================================== TCA REPORT ==================================");
        println!(
            "{:<10} | {:<8} | {:<4} | {:<10} | {:<10} | {:<12} | {:<10} | {:<8} | {:<8}",
            "Algo", "Symbol", "Side", "Qty", "Filled", "AvgPrice", "Slip(bps)", "AvgLat(us)", "Fees"
        );
        println!("{}", "-".repeat(95));
        for r in reports {
            println!(
                "{:<10} | {:<8} | {:<4?} | {:<10.4} | {:<10.4} | {:<12.4} | {:<10.2} | {:<10.2} | {:<8.4}",
                r.algo_name,
                r.symbol.to_string(),
                r.side,
                r.total_qty.to_f64(),
                r.executed_qty.to_f64(),
                r.avg_execution_price.to_f64(),
                r.total_slippage_bps,
                r.avg_latency_us,
                r.total_fees.to_f64()
            );
        }
        println!("================================================================================");
    }
}
