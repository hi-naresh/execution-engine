use std::fmt;
use std::ops::{Add, AddAssign, Div, Mul, Sub, SubAssign};

pub const SCALE: u64 = 100_000_000; // 10^8

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct Decimal(pub u64);

impl Decimal {
    pub const ZERO: Decimal = Decimal(0);

    pub fn from_f64(v: f64) -> Self {
        Decimal((v * SCALE as f64).round() as u64)
    }

    pub fn to_f64(self) -> f64 {
        self.0 as f64 / SCALE as f64
    }

    pub fn is_zero(self) -> bool {
        self.0 == 0
    }
}

impl fmt::Display for Decimal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:.8}", self.to_f64())
    }
}

impl Add for Decimal {
    type Output = Self;
    #[inline]
    fn add(self, other: Self) -> Self {
        Decimal(self.0 + other.0)
    }
}

impl AddAssign for Decimal {
    #[inline]
    fn add_assign(&mut self, other: Self) {
        self.0 += other.0;
    }
}

impl Sub for Decimal {
    type Output = Self;
    #[inline]
    fn sub(self, other: Self) -> Self {
        Decimal(self.0.saturating_sub(other.0))
    }
}

impl SubAssign for Decimal {
    #[inline]
    fn sub_assign(&mut self, other: Self) {
        self.0 = self.0.saturating_sub(other.0);
    }
}

impl Mul<u64> for Decimal {
    type Output = Self;
    #[inline]
    fn mul(self, scalar: u64) -> Self {
        Decimal(self.0 * scalar)
    }
}

impl Div<u64> for Decimal {
    type Output = Self;
    #[inline]
    fn div(self, scalar: u64) -> Self {
        Decimal(self.0 / scalar)
    }
}

impl Mul for Decimal {
    type Output = Self;
    #[inline]
    fn mul(self, other: Self) -> Self {
        Decimal(((self.0 as u128 * other.0 as u128) / SCALE as u128) as u64)
    }
}

impl Div for Decimal {
    type Output = Self;
    #[inline]
    fn div(self, other: Self) -> Self {
        if other.0 == 0 {
            Decimal(0)
        } else {
            Decimal(((self.0 as u128 * SCALE as u128) / other.0 as u128) as u64)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Side {
    #[default]
    Buy,
    Sell,
}

impl Side {
    pub fn opposite(self) -> Self {
        match self {
            Side::Buy => Side::Sell,
            Side::Sell => Side::Buy,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrderType {
    Limit,
    Market,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimeInForce {
    GTC,
    IOC,
    FOK,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrderStatus {
    PendingNew,
    New,
    PartiallyFilled,
    Filled,
    Cancelled,
    Rejected,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Exchange {
    Binance,
    Coinbase,
}

impl fmt::Display for Exchange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Exchange::Binance => write!(f, "Binance"),
            Exchange::Coinbase => write!(f, "Coinbase"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Symbol {
    #[default]
    BTCUSDT,
    ETHUSDT,
}

impl fmt::Display for Symbol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Symbol::BTCUSDT => write!(f, "BTCUSDT"),
            Symbol::ETHUSDT => write!(f, "ETHUSDT"),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct OrderRequest {
    pub client_order_id: u64,
    pub symbol: Symbol,
    pub exchange: Exchange,
    pub side: Side,
    pub order_type: OrderType,
    pub price: Decimal,
    pub quantity: Decimal,
    pub time_in_force: TimeInForce,
}

#[derive(Debug, Clone, Copy)]
pub struct ExecutionReport {
    pub client_order_id: u64,
    pub exchange_order_id: u64,
    pub symbol: Symbol,
    pub exchange: Exchange,
    pub status: OrderStatus,
    pub last_qty: Decimal,
    pub last_price: Decimal,
    pub leaves_qty: Decimal,
    pub cumulative_qty: Decimal,
    pub timestamp_ns: u64,
}

#[derive(Debug, Clone, Copy)]
pub struct MarketTrade {
    pub symbol: Symbol,
    pub exchange: Exchange,
    pub price: Decimal,
    pub quantity: Decimal,
    pub side: Side,
    pub timestamp_ns: u64,
}

#[derive(Debug, Clone, Copy)]
pub struct BookUpdateLevel {
    pub price: Decimal,
    pub quantity: Decimal,
}

#[derive(Debug, Clone, Copy)]
pub struct BookUpdate {
    pub symbol: Symbol,
    pub exchange: Exchange,
    pub bids: [BookUpdateLevel; 20],
    pub asks: [BookUpdateLevel; 20],
    pub bid_count: usize,
    pub ask_count: usize,
    pub timestamp_ns: u64,
}

#[derive(Debug, Clone, Copy)]
pub enum MarketEvent {
    BookUpdate(BookUpdate),
    Trade(MarketTrade),
    ExecutionReport(ExecutionReport),
    TimerTick { timestamp_ns: u64 },
}
