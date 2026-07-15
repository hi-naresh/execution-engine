pub mod algos;
pub mod engine;
pub mod exchanges;
pub mod order_book;
pub mod sor;
pub mod types;
pub mod tca;

use crate::algos::iceberg::IcebergAlgo;
use crate::algos::pov::PovAlgo;
use crate::algos::twap::TwapAlgo;
use crate::engine::ExecutionEngine;
use crate::exchanges::ExchangeSim;
use crate::types::{Decimal, Exchange, MarketEvent, OrderRequest, Side, Symbol};
use crossbeam_queue::ArrayQueue;
use std::sync::Arc;

fn main() {
    println!("Starting Low-Latency Crypto Execution Engine Benchmark...");

    // 1. Initialize Thread-safe Lock-Free Queues
    let input_queue = Arc::new(ArrayQueue::<MarketEvent>::new(10000));
    let output_queue = Arc::new(ArrayQueue::<OrderRequest>::new(10000));

    // 2. Initialize Execution Engine
    let mut engine = ExecutionEngine::new(output_queue.clone());

    // 3. Initialize Exchange Simulators (Binance starts at 100.0, Coinbase at 100.2)
    let mut binance_sim = ExchangeSim::new(Exchange::Binance, Symbol::BTCUSDT, Decimal::from_f64(100.0));
    let mut coinbase_sim = ExchangeSim::new(Exchange::Coinbase, Symbol::BTCUSDT, Decimal::from_f64(100.2));

    // Setup initial consolidated books in engine
    engine.book_binance = binance_sim.book;
    engine.book_coinbase = coinbase_sim.book;

    // 4. Set up Algorithmic Orders
    let start_time_ns = 100_000_000; // 100 ms in simulation time

    // Setup TWAP: Buy 10.0 BTC over 1.0 second, in 100ms intervals
    let twap_qty = Decimal::from_f64(10.0);
    engine.twap = Some(TwapAlgo::new(
        Symbol::BTCUSDT,
        Side::Buy,
        twap_qty,
        start_time_ns,
        1_000_000_000, // 1.0 second duration
        100_000_000,   // 100 ms intervals
        10000,         // Base client order ID
    ));
    engine.tca.register_parent_order(
        "TWAP",
        Symbol::BTCUSDT,
        Side::Buy,
        twap_qty,
        Decimal::from_f64(100.1), // benchmark price (mid-price at start)
        start_time_ns,
    );

    // Setup POV: Sell 5.0 BTC, targeting 20% participation rate
    let pov_qty = Decimal::from_f64(5.0);
    engine.pov = Some(PovAlgo::new(
        Symbol::BTCUSDT,
        Side::Sell,
        pov_qty,
        Decimal::from_f64(0.20),
        20000,
    ));
    engine.tca.register_parent_order(
        "POV",
        Symbol::BTCUSDT,
        Side::Sell,
        pov_qty,
        Decimal::from_f64(100.1),
        start_time_ns,
    );

    // Setup Iceberg: Buy 8.0 BTC, limit price 100.5, visible size 1.0 BTC
    let iceberg_qty = Decimal::from_f64(8.0);
    engine.iceberg = Some(IcebergAlgo::new(
        Symbol::BTCUSDT,
        Side::Buy,
        iceberg_qty,
        Decimal::from_f64(100.5),
        Decimal::from_f64(1.0),
        30000,
    ));
    engine.tca.register_parent_order(
        "ICEBERG",
        Symbol::BTCUSDT,
        Side::Buy,
        iceberg_qty,
        Decimal::from_f64(100.1),
        start_time_ns,
    );

    // Start Iceberg slice immediately
    if let Some(ref mut iceberg) = engine.iceberg {
        if let Some(action) = iceberg.start() {
            engine.handle_algo_action(action, "ICEBERG", start_time_ns);
        }
    }

    // 5. Run Discrete-Event Simulation Loop
    // We run for 3 seconds of simulated time in steps of 10 ms
    let step_ns = 10_000_000;
    let end_time_ns = 3_000_000_000;

    println!("Running simulation loop...");
    for time_ns in (0..=end_time_ns).step_by(step_ns as usize) {
        // Trigger Iceberg start if it's the start time
        if time_ns == start_time_ns {
            if let Some(ref mut iceberg) = engine.iceberg {
                // To keep it simple, we can run Iceberg inside engine by passing it on_event.
                // Or we can call engine.handle_algo_action. We will replace handle_algo_action to be public.
                // Let's call start and process it:
                if let Some(action) = iceberg.start() {
                    // Let's invoke it using engine's new public method.
                    engine.handle_algo_action(action, "ICEBERG", time_ns);
                }
            }
        }

        // A. Tick simulated exchanges to generate market updates and public trades
        binance_sim.tick(time_ns, &input_queue);
        coinbase_sim.tick(time_ns, &input_queue);

        // B. Inject timer ticks to engine for TWAP schedules
        let _ = input_queue.push(MarketEvent::TimerTick { timestamp_ns: time_ns });

        // C. Feed order requests from engine to exchange simulators
        while let Some(req) = output_queue.pop() {
            let target_sim = match req.exchange {
                Exchange::Binance => &mut binance_sim,
                Exchange::Coinbase => &mut coinbase_sim,
            };

            // Quantity = 0 and Price = 0 represents a cancellation
            if req.quantity.is_zero() && req.price.is_zero() {
                target_sim.handle_cancel(req.client_order_id, time_ns, &input_queue);
            } else {
                target_sim.handle_order_request(req, time_ns, &input_queue);
            }
        }

        // D. Drain and process all events in the engine
        while let Some(event) = input_queue.pop() {
            engine.on_event(event);
        }
    }

    println!("Simulation finished.");

    // 6. Output TCA Reports
    engine.tca.print_reports();
}
