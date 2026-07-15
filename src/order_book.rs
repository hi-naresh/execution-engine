use crate::types::{BookUpdate, BookUpdateLevel, Decimal};

pub const MAX_DEPTH: usize = 20;

#[derive(Debug, Clone, Copy)]
pub struct OrderBook {
    pub bids: [BookUpdateLevel; MAX_DEPTH],
    pub asks: [BookUpdateLevel; MAX_DEPTH],
    pub bid_count: usize,
    pub ask_count: usize,
}

impl Default for OrderBook {
    fn default() -> Self {
        OrderBook {
            bids: [BookUpdateLevel {
                price: Decimal::ZERO,
                quantity: Decimal::ZERO,
            }; MAX_DEPTH],
            asks: [BookUpdateLevel {
                price: Decimal::ZERO,
                quantity: Decimal::ZERO,
            }; MAX_DEPTH],
            bid_count: 0,
            ask_count: 0,
        }
    }
}

impl OrderBook {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn apply_update(&mut self, update: &BookUpdate) {
        self.bid_count = update.bid_count.min(MAX_DEPTH);
        for i in 0..self.bid_count {
            self.bids[i] = update.bids[i];
        }
        self.ask_count = update.ask_count.min(MAX_DEPTH);
        for i in 0..self.ask_count {
            self.asks[i] = update.asks[i];
        }
    }

    pub fn update_bid(&mut self, price: Decimal, quantity: Decimal) {
        // Bids sorted descending (highest price first)
        let mut found_idx = None;
        for i in 0..self.bid_count {
            if self.bids[i].price == price {
                found_idx = Some(i);
                break;
            }
        }

        if let Some(idx) = found_idx {
            if quantity.is_zero() {
                // Delete
                if idx < self.bid_count - 1 {
                    self.bids.copy_within(idx + 1..self.bid_count, idx);
                }
                self.bid_count -= 1;
                self.bids[self.bid_count] = BookUpdateLevel {
                    price: Decimal::ZERO,
                    quantity: Decimal::ZERO,
                };
            } else {
                // Update
                self.bids[idx].quantity = quantity;
            }
        } else if !quantity.is_zero() {
            // Insert
            let mut insert_idx = self.bid_count;
            for i in 0..self.bid_count {
                if self.bids[i].price < price {
                    insert_idx = i;
                    break;
                }
            }

            if insert_idx < MAX_DEPTH {
                let end = (self.bid_count + 1).min(MAX_DEPTH);
                if insert_idx < self.bid_count {
                    self.bids.copy_within(insert_idx..end - 1, insert_idx + 1);
                }
                self.bids[insert_idx] = BookUpdateLevel { price, quantity };
                if self.bid_count < MAX_DEPTH {
                    self.bid_count += 1;
                }
            }
        }
    }

    pub fn update_ask(&mut self, price: Decimal, quantity: Decimal) {
        // Asks sorted ascending (lowest price first)
        let mut found_idx = None;
        for i in 0..self.ask_count {
            if self.asks[i].price == price {
                found_idx = Some(i);
                break;
            }
        }

        if let Some(idx) = found_idx {
            if quantity.is_zero() {
                // Delete
                if idx < self.ask_count - 1 {
                    self.asks.copy_within(idx + 1..self.ask_count, idx);
                }
                self.ask_count -= 1;
                self.asks[self.ask_count] = BookUpdateLevel {
                    price: Decimal::ZERO,
                    quantity: Decimal::ZERO,
                };
            } else {
                // Update
                self.asks[idx].quantity = quantity;
            }
        } else if !quantity.is_zero() {
            // Insert
            let mut insert_idx = self.ask_count;
            for i in 0..self.ask_count {
                if self.asks[i].price > price {
                    insert_idx = i;
                    break;
                }
            }

            if insert_idx < MAX_DEPTH {
                let end = (self.ask_count + 1).min(MAX_DEPTH);
                if insert_idx < self.ask_count {
                    self.asks.copy_within(insert_idx..end - 1, insert_idx + 1);
                }
                self.asks[insert_idx] = BookUpdateLevel { price, quantity };
                if self.ask_count < MAX_DEPTH {
                    self.ask_count += 1;
                }
            }
        }
    }

    pub fn get_best_bid(&self) -> Option<BookUpdateLevel> {
        if self.bid_count > 0 {
            Some(self.bids[0])
        } else {
            None
        }
    }

    pub fn get_best_ask(&self) -> Option<BookUpdateLevel> {
        if self.ask_count > 0 {
            Some(self.asks[0])
        } else {
            None
        }
    }

    pub fn get_mid_price(&self) -> Option<Decimal> {
        match (self.get_best_bid(), self.get_best_ask()) {
            (Some(bid), Some(ask)) => Some((bid.price + ask.price) / 2),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_order_book_bids() {
        let mut book = OrderBook::new();
        assert_eq!(book.bid_count, 0);

        // Insert first bid
        book.update_bid(Decimal::from_f64(100.0), Decimal::from_f64(1.0));
        assert_eq!(book.bid_count, 1);
        assert_eq!(book.get_best_bid().unwrap().price, Decimal::from_f64(100.0));

        // Insert higher bid (should sort to index 0)
        book.update_bid(Decimal::from_f64(101.0), Decimal::from_f64(2.0));
        assert_eq!(book.bid_count, 2);
        assert_eq!(book.bids[0].price, Decimal::from_f64(101.0));
        assert_eq!(book.bids[1].price, Decimal::from_f64(100.0));

        // Insert lower bid (should sort to index 2)
        book.update_bid(Decimal::from_f64(99.0), Decimal::from_f64(3.0));
        assert_eq!(book.bid_count, 3);
        assert_eq!(book.bids[0].price, Decimal::from_f64(101.0));
        assert_eq!(book.bids[1].price, Decimal::from_f64(100.0));
        assert_eq!(book.bids[2].price, Decimal::from_f64(99.0));

        // Update quantity
        book.update_bid(Decimal::from_f64(100.0), Decimal::from_f64(1.5));
        assert_eq!(book.bid_count, 3);
        assert_eq!(book.bids[1].quantity, Decimal::from_f64(1.5));

        // Delete bid
        book.update_bid(Decimal::from_f64(100.0), Decimal::ZERO);
        assert_eq!(book.bid_count, 2);
        assert_eq!(book.bids[0].price, Decimal::from_f64(101.0));
        assert_eq!(book.bids[1].price, Decimal::from_f64(99.0));
    }

    #[test]
    fn test_order_book_asks() {
        let mut book = OrderBook::new();
        assert_eq!(book.ask_count, 0);

        // Insert asks
        book.update_ask(Decimal::from_f64(100.0), Decimal::from_f64(1.0));
        book.update_ask(Decimal::from_f64(99.0), Decimal::from_f64(2.0)); // lower ask (better price)
        book.update_ask(Decimal::from_f64(101.0), Decimal::from_f64(3.0)); // higher ask

        assert_eq!(book.ask_count, 3);
        // Asks sort ascending (lowest first)
        assert_eq!(book.asks[0].price, Decimal::from_f64(99.0));
        assert_eq!(book.asks[1].price, Decimal::from_f64(100.0));
        assert_eq!(book.asks[2].price, Decimal::from_f64(101.0));
    }
}
