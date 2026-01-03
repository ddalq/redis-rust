//! I/O Abstraction Layer for Deterministic Simulation Testing
//!
//! This module provides traits that abstract over I/O operations, allowing the same
//! business logic to run in both production (tokio) and simulation (virtual I/O) modes.
//!
//! Inspired by FoundationDB's Flow runtime and TigerBeetle's IO abstraction.

pub mod production;
pub mod simulation;

use std::fmt::Debug;
use std::future::Future;
use std::io::Result as IoResult;
use std::pin::Pin;

/// Timestamp in milliseconds since epoch
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Timestamp(pub u64);

impl Timestamp {
    pub const ZERO: Timestamp = Timestamp(0);

    pub fn from_millis(ms: u64) -> Self {
        Timestamp(ms)
    }

    pub fn as_millis(&self) -> u64 {
        self.0
    }
}

impl std::ops::Add<Duration> for Timestamp {
    type Output = Timestamp;

    fn add(self, rhs: Duration) -> Self::Output {
        Timestamp(self.0.saturating_add(rhs.0))
    }
}

impl std::ops::Sub<Timestamp> for Timestamp {
    type Output = Duration;

    fn sub(self, rhs: Timestamp) -> Self::Output {
        Duration(self.0.saturating_sub(rhs.0))
    }
}

/// Duration in milliseconds
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Duration(pub u64);

impl Duration {
    pub const ZERO: Duration = Duration(0);

    pub fn from_millis(ms: u64) -> Self {
        Duration(ms)
    }

    pub fn from_secs(secs: u64) -> Self {
        Duration(secs * 1000)
    }

    pub fn as_millis(&self) -> u64 {
        self.0
    }

    pub fn as_std(&self) -> std::time::Duration {
        std::time::Duration::from_millis(self.0)
    }
}

impl From<std::time::Duration> for Duration {
    fn from(d: std::time::Duration) -> Self {
        Duration(d.as_millis() as u64)
    }
}

/// Ticker for periodic operations (like tokio::time::Interval)
pub trait Ticker: Send {
    /// Wait for the next tick
    fn tick(&mut self) -> Pin<Box<dyn Future<Output = ()> + Send + '_>>;
}

/// Clock abstraction for time operations
pub trait Clock: Send + Sync {
    /// Get current time
    fn now(&self) -> Timestamp;

    /// Sleep for a duration
    fn sleep(&self, duration: Duration) -> Pin<Box<dyn Future<Output = ()> + Send + '_>>;

    /// Create a periodic ticker
    fn interval(&self, period: Duration) -> Box<dyn Ticker + Send>;
}

/// Network stream abstraction (like TcpStream)
pub trait NetworkStream: Send + Debug {
    /// Read bytes into buffer, returns number of bytes read
    fn read<'a>(
        &'a mut self,
        buf: &'a mut [u8],
    ) -> Pin<Box<dyn Future<Output = IoResult<usize>> + Send + 'a>>;

    /// Read exact number of bytes
    fn read_exact<'a>(
        &'a mut self,
        buf: &'a mut [u8],
    ) -> Pin<Box<dyn Future<Output = IoResult<()>> + Send + 'a>>;

    /// Write all bytes from buffer
    fn write_all<'a>(
        &'a mut self,
        buf: &'a [u8],
    ) -> Pin<Box<dyn Future<Output = IoResult<()>> + Send + 'a>>;

    /// Flush the stream
    fn flush(&mut self) -> Pin<Box<dyn Future<Output = IoResult<()>> + Send + '_>>;

    /// Get peer address as string
    fn peer_addr(&self) -> IoResult<String>;
}

/// Network listener abstraction (like TcpListener)
pub trait NetworkListener: Send {
    /// The stream type returned by accept
    type Stream: NetworkStream;

    /// Accept a new connection
    fn accept(
        &mut self,
    ) -> Pin<Box<dyn Future<Output = IoResult<(Self::Stream, String)>> + Send + '_>>;

    /// Get local address
    fn local_addr(&self) -> IoResult<String>;
}

/// Network abstraction for creating connections and listeners
pub trait Network: Send + Sync {
    /// The listener type
    type Listener: NetworkListener;
    /// The stream type
    type Stream: NetworkStream;

    /// Bind to an address and create a listener
    fn bind<'a>(
        &'a self,
        addr: &'a str,
    ) -> Pin<Box<dyn Future<Output = IoResult<Self::Listener>> + Send + 'a>>;

    /// Connect to a remote address
    fn connect<'a>(
        &'a self,
        addr: &'a str,
    ) -> Pin<Box<dyn Future<Output = IoResult<Self::Stream>> + Send + 'a>>;
}

/// Random number generator abstraction
pub trait Rng: Send {
    /// Generate a random u64
    fn next_u64(&mut self) -> u64;

    /// Generate a random boolean with given probability
    fn gen_bool(&mut self, probability: f64) -> bool;

    /// Generate a random number in range [min, max)
    fn gen_range(&mut self, min: u64, max: u64) -> u64;

    /// Shuffle a slice
    fn shuffle<T>(&mut self, slice: &mut [T]);
}

/// Combined runtime providing all I/O capabilities
///
/// This is the main abstraction that production and simulation code uses.
/// In production, this wraps tokio. In simulation, this provides virtual I/O.
pub trait Runtime: Send + Sync + 'static {
    type Clock: Clock;
    type Network: Network;

    /// Get the clock for time operations
    fn clock(&self) -> &Self::Clock;

    /// Get the network for connection operations
    fn network(&self) -> &Self::Network;

    /// Spawn a task (production: tokio::spawn, simulation: queued execution)
    fn spawn<F>(&self, future: F)
    where
        F: Future<Output = ()> + Send + 'static;
}

/// Type alias for the current runtime based on feature flags
#[cfg(not(feature = "simulation"))]
pub type CurrentRuntime = production::ProductionRuntime;

#[cfg(feature = "simulation")]
pub type CurrentRuntime = simulation::SimulatedRuntime;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_timestamp_arithmetic() {
        let t1 = Timestamp::from_millis(1000);
        let t2 = Timestamp::from_millis(500);
        let d = Duration::from_millis(200);

        assert_eq!(t1 + d, Timestamp::from_millis(1200));
        assert_eq!(t1 - t2, Duration::from_millis(500));
    }

    #[test]
    fn test_duration_conversion() {
        let d = Duration::from_secs(5);
        assert_eq!(d.as_millis(), 5000);

        let std_d = std::time::Duration::from_millis(1234);
        let d2: Duration = std_d.into();
        assert_eq!(d2.as_millis(), 1234);
    }
}
