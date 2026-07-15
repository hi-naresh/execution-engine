use crate::order_book::OrderBook;
use crate::types::{Decimal, Exchange, Side};

#[derive(Debug, Clone, Copy)]
pub struct SorRoute {
    pub exchange: Exchange,
    pub price: Decimal,
    pub quantity: Decimal,
}

#[derive(Debug, Clone, Copy)]
struct RouteCandidate {
    exchange: Exchange,
    price: Decimal,
    quantity: Decimal,
    effective_price: u64, // scaled value for comparison
}

impl Default for RouteCandidate {
    fn default() -> Self {
        Self {
            exchange: Exchange::Binance,
            price: Decimal::ZERO,
            quantity: Decimal::ZERO,
            effective_price: 0,
        }
    }
}

// Fees: Binance Taker = 0.1%, Coinbase Taker = 0.4%
const BINANCE_FEE_BPS: u64 = 10;  // 0.1%
const COINBASE_FEE_BPS: u64 = 40; // 0.4%
const BPS_BASE: u64 = 10000;

#[inline]
fn get_buy_effective_price(price: Decimal, exchange: Exchange) -> u64 {
    let fee_bps = match exchange {
        Exchange::Binance => BINANCE_FEE_BPS,
        Exchange::Coinbase => COINBASE_FEE_BPS,
    };
    price.0 + (price.0 * fee_bps) / BPS_BASE
}

#[inline]
fn get_sell_effective_price(price: Decimal, exchange: Exchange) -> u64 {
    let fee_bps = match exchange {
        Exchange::Binance => BINANCE_FEE_BPS,
        Exchange::Coinbase => COINBASE_FEE_BPS,
    };
    price.0.saturating_sub((price.0 * fee_bps) / BPS_BASE)
}

/// Computes the Smart Order Routing path to buy or sell the target quantity.
/// Uses a stack-allocated array of depth levels to guarantee zero heap allocation.
pub fn route_order(
    book_binance: &OrderBook,
    book_coinbase: &OrderBook,
    side: Side,
    target_qty: Decimal,
) -> ([SorRoute; 40], usize) {
    let mut routes = [SorRoute {
        exchange: Exchange::Binance,
        price: Decimal::ZERO,
        quantity: Decimal::ZERO,
    }; 40];
    let mut route_count = 0;

    if target_qty.is_zero() {
        return (routes, 0);
    }

    let mut candidates = [RouteCandidate::default(); 40];
    let mut candidate_count = 0;

    match side {
        Side::Buy => {
            // Fetch asks from both books (sorted ascending)
            for i in 0..book_binance.ask_count {
                let level = book_binance.asks[i];
                if candidate_count < 40 {
                    candidates[candidate_count] = RouteCandidate {
                        exchange: Exchange::Binance,
                        price: level.price,
                        quantity: level.quantity,
                        effective_price: get_buy_effective_price(level.price, Exchange::Binance),
                    };
                    candidate_count += 1;
                }
            }
            for i in 0..book_coinbase.ask_count {
                let level = book_coinbase.asks[i];
                if candidate_count < 40 {
                    candidates[candidate_count] = RouteCandidate {
                        exchange: Exchange::Coinbase,
                        price: level.price,
                        quantity: level.quantity,
                        effective_price: get_buy_effective_price(level.price, Exchange::Coinbase),
                    };
                    candidate_count += 1;
                }
            }

            // Sort candidates by effective price ascending (insertion sort for stack array)
            for i in 1..candidate_count {
                let mut j = i;
                while j > 0 && candidates[j - 1].effective_price > candidates[j].effective_price {
                    candidates.swap(j - 1, j);
                    j -= 1;
                }
            }
        }
        Side::Sell => {
            // Fetch bids from both books (sorted descending)
            for i in 0..book_binance.bid_count {
                let level = book_binance.bids[i];
                if candidate_count < 40 {
                    candidates[candidate_count] = RouteCandidate {
                        exchange: Exchange::Binance,
                        price: level.price,
                        quantity: level.quantity,
                        effective_price: get_sell_effective_price(level.price, Exchange::Binance),
                    };
                    candidate_count += 1;
                }
            }
            for i in 0..book_coinbase.bid_count {
                let level = book_coinbase.bids[i];
                if candidate_count < 40 {
                    candidates[candidate_count] = RouteCandidate {
                        exchange: Exchange::Coinbase,
                        price: level.price,
                        quantity: level.quantity,
                        effective_price: get_sell_effective_price(level.price, Exchange::Coinbase),
                    };
                    candidate_count += 1;
                }
            }

            // Sort candidates by effective price descending (highest effective bid first)
            for i in 1..candidate_count {
                let mut j = i;
                while j > 0 && candidates[j - 1].effective_price < candidates[j].effective_price {
                    candidates.swap(j - 1, j);
                    j -= 1;
                }
            }
        }
    }

    let mut remaining = target_qty;
    for i in 0..candidate_count {
        if remaining.is_zero() {
            break;
        }

        let cand = &candidates[i];
        let fill_qty = if cand.quantity.0 >= remaining.0 {
            remaining
        } else {
            cand.quantity
        };

        // Check if we can aggregate with the last route if it's the same exchange and price
        if route_count > 0 && routes[route_count - 1].exchange == cand.exchange && routes[route_count - 1].price == cand.price {
            routes[route_count - 1].quantity += fill_qty;
        } else if route_count < 40 {
            routes[route_count] = SorRoute {
                exchange: cand.exchange,
                price: cand.price,
                quantity: fill_qty,
            };
            route_count += 1;
        }

        remaining -= fill_qty;
    }

    (routes, route_count)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sor_buying_split() {
        let mut binance = OrderBook::new();
        binance.update_ask(Decimal::from_f64(100.0), Decimal::from_f64(1.0));
        binance.update_ask(Decimal::from_f64(101.0), Decimal::from_f64(2.0));

        let mut coinbase = OrderBook::new();
        coinbase.update_ask(Decimal::from_f64(100.1), Decimal::from_f64(1.0));
        coinbase.update_ask(Decimal::from_f64(100.5), Decimal::from_f64(2.0));

        let (routes, route_count) = route_order(
            &binance,
            &coinbase,
            Side::Buy,
            Decimal::from_f64(2.5),
        );

        assert_eq!(route_count, 3);

        // First route should be Binance at 100.0 (effective price is cheapest = 100.10)
        assert_eq!(routes[0].exchange, Exchange::Binance);
        assert_eq!(routes[0].price, Decimal::from_f64(100.0));
        assert_eq!(routes[0].quantity, Decimal::from_f64(1.0));

        // Second route should be Coinbase at 100.1 (effective price = 100.5004)
        assert_eq!(routes[1].exchange, Exchange::Coinbase);
        assert_eq!(routes[1].price, Decimal::from_f64(100.1));
        assert_eq!(routes[1].quantity, Decimal::from_f64(1.0));

        // Third route should be Coinbase at 100.5 (effective price = 100.902)
        assert_eq!(routes[2].exchange, Exchange::Coinbase);
        assert_eq!(routes[2].price, Decimal::from_f64(100.5));
        assert_eq!(routes[2].quantity, Decimal::from_f64(0.5));
    }
}
