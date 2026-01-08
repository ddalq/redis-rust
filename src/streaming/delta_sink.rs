//! Delta Sink for Streaming Persistence Integration
//!
//! Provides a channel-based mechanism to send deltas to the WriteBuffer
//! without coupling the sync execution path with async persistence.

use crate::replication::state::ReplicationDelta;
use std::sync::mpsc;

/// Error type for delta sink operations
#[derive(Debug)]
pub enum DeltaSinkError {
    /// Channel is disconnected
    Disconnected,
}

impl std::fmt::Display for DeltaSinkError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DeltaSinkError::Disconnected => write!(f, "Delta sink channel disconnected"),
        }
    }
}

impl std::error::Error for DeltaSinkError {}

/// Sender end of delta sink - held by the replicated state
#[derive(Clone)]
pub struct DeltaSinkSender {
    sender: mpsc::Sender<ReplicationDelta>,
}

impl DeltaSinkSender {
    /// Send a delta to the sink
    pub fn send(&self, delta: ReplicationDelta) -> Result<(), DeltaSinkError> {
        self.sender
            .send(delta)
            .map_err(|_| DeltaSinkError::Disconnected)
    }
}

/// Receiver end of delta sink - held by the persistence worker
pub struct DeltaSinkReceiver {
    receiver: mpsc::Receiver<ReplicationDelta>,
}

impl DeltaSinkReceiver {
    /// Try to receive a delta without blocking
    pub fn try_recv(&self) -> Option<ReplicationDelta> {
        self.receiver.try_recv().ok()
    }

    /// Receive with timeout
    pub fn recv_timeout(&self, timeout: std::time::Duration) -> Option<ReplicationDelta> {
        self.receiver.recv_timeout(timeout).ok()
    }

    /// Drain all available deltas
    pub fn drain(&self) -> Vec<ReplicationDelta> {
        let mut deltas = Vec::new();
        while let Some(delta) = self.try_recv() {
            deltas.push(delta);
        }
        deltas
    }
}

/// Create a new delta sink channel pair
pub fn delta_sink_channel() -> (DeltaSinkSender, DeltaSinkReceiver) {
    let (sender, receiver) = mpsc::channel();
    (DeltaSinkSender { sender }, DeltaSinkReceiver { receiver })
}

/// Background worker that transfers deltas from the channel to the WriteBuffer
pub struct PersistenceWorker<S: crate::streaming::ObjectStore> {
    receiver: DeltaSinkReceiver,
    write_buffer: std::sync::Arc<crate::streaming::WriteBuffer<S>>,
    poll_interval: std::time::Duration,
    shutdown: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

/// Handle for controlling the persistence worker
pub struct PersistenceWorkerHandle {
    shutdown: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl PersistenceWorkerHandle {
    /// Signal the persistence worker to stop
    pub fn shutdown(&self) {
        self.shutdown
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }
}

impl<S: crate::streaming::ObjectStore> PersistenceWorker<S> {
    /// Create a new persistence worker
    pub fn new(
        receiver: DeltaSinkReceiver,
        write_buffer: std::sync::Arc<crate::streaming::WriteBuffer<S>>,
    ) -> (Self, PersistenceWorkerHandle) {
        let shutdown = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let handle = PersistenceWorkerHandle {
            shutdown: shutdown.clone(),
        };

        let worker = PersistenceWorker {
            receiver,
            write_buffer,
            poll_interval: std::time::Duration::from_millis(10),
            shutdown,
        };

        (worker, handle)
    }

    /// Run the persistence worker loop
    pub async fn run(self) {
        loop {
            if self.shutdown.load(std::sync::atomic::Ordering::SeqCst) {
                // Final drain and flush before shutdown
                for delta in self.receiver.drain() {
                    if let Err(e) = self.write_buffer.push(delta) {
                        eprintln!("Error pushing final delta: {}", e);
                    }
                }
                if let Err(e) = self.write_buffer.flush().await {
                    eprintln!("Error during final flush: {}", e);
                }
                break;
            }

            // Drain available deltas from channel
            let deltas = self.receiver.drain();
            for delta in deltas {
                if let Err(e) = self.write_buffer.push(delta) {
                    eprintln!("Error pushing delta to write buffer: {}", e);
                    // TODO: Handle backpressure more gracefully
                }
            }

            // Check if flush is needed
            if self.write_buffer.should_flush() {
                if let Err(e) = self.write_buffer.flush().await {
                    eprintln!("Error flushing write buffer: {}", e);
                }
            }

            tokio::time::sleep(self.poll_interval).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::redis::SDS;
    use crate::replication::lattice::{LamportClock, ReplicaId};
    use crate::replication::state::ReplicatedValue;

    fn make_test_delta(key: &str, value: &str) -> ReplicationDelta {
        let replica_id = ReplicaId::new(1);
        let clock = LamportClock {
            time: 1000,
            replica_id,
        };
        let replicated = ReplicatedValue::with_value(SDS::from_str(value), clock);
        ReplicationDelta::new(key.to_string(), replicated, replica_id)
    }

    #[test]
    fn test_delta_sink_channel() {
        let (sender, receiver) = delta_sink_channel();

        sender.send(make_test_delta("key1", "value1")).unwrap();
        sender.send(make_test_delta("key2", "value2")).unwrap();

        let deltas = receiver.drain();
        assert_eq!(deltas.len(), 2);
        assert_eq!(deltas[0].key, "key1");
        assert_eq!(deltas[1].key, "key2");
    }

    #[test]
    fn test_delta_sink_clone() {
        let (sender, receiver) = delta_sink_channel();
        let sender2 = sender.clone();

        sender.send(make_test_delta("key1", "value1")).unwrap();
        sender2.send(make_test_delta("key2", "value2")).unwrap();

        let deltas = receiver.drain();
        assert_eq!(deltas.len(), 2);
    }

    #[test]
    fn test_delta_sink_disconnected() {
        let (sender, receiver) = delta_sink_channel();
        drop(receiver);

        let result = sender.send(make_test_delta("key", "value"));
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_persistence_worker() {
        use crate::streaming::{InMemoryObjectStore, WriteBuffer, WriteBufferConfig};
        use std::sync::Arc;

        let (sender, receiver) = delta_sink_channel();
        let store = Arc::new(InMemoryObjectStore::new());
        let config = WriteBufferConfig::test();
        let write_buffer = Arc::new(WriteBuffer::new(store.clone(), "test".to_string(), config));

        let (worker, handle) = PersistenceWorker::new(receiver, write_buffer.clone());

        // Run worker in background
        let worker_task = tokio::spawn(worker.run());

        // Send some deltas
        for i in 0..5 {
            sender
                .send(make_test_delta(
                    &format!("key{}", i),
                    &format!("value{}", i),
                ))
                .unwrap();
        }

        // Give it time to process
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        // Shutdown
        handle.shutdown();

        tokio::time::timeout(std::time::Duration::from_secs(1), worker_task)
            .await
            .expect("Worker should complete within timeout")
            .expect("Worker task should not panic");

        // Verify data was flushed
        let stats = write_buffer.stats();
        assert!(stats.total_deltas_flushed >= 5);
    }
}
