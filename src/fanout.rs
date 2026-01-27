//! Generic fanout module for broadcasting messages to multiple receivers.

use std::thread::{self, JoinHandle};

use crossbeam_channel::{Receiver, Sender};

/// Spawns a thread that reads from `rx` and sends each message to all `txs`.
///
/// - Uses blocking `recv()` (no sleep/polling)
/// - Thread exits when `rx` is closed (all senders dropped)
/// - All `txs` are dropped on exit, closing downstream channels
///
/// Returns a `JoinHandle` for the spawned thread.
pub fn spawn<T: Clone + Send + 'static>(rx: Receiver<T>, txs: Vec<Sender<T>>) -> JoinHandle<()> {
    thread::spawn(move || {
        while let Ok(msg) = rx.recv() {
            txs.iter().for_each(|tx| {
                let _ = tx.send(msg.clone());
            });
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossbeam_channel as channel;

    #[test]
    fn test_fanout_delivers_to_all_receivers() {
        let (input_tx, input_rx) = channel::unbounded::<i32>();
        let (out1_tx, out1_rx) = channel::unbounded::<i32>();
        let (out2_tx, out2_rx) = channel::unbounded::<i32>();
        let (out3_tx, out3_rx) = channel::unbounded::<i32>();

        let _handle = spawn(input_rx, vec![out1_tx, out2_tx, out3_tx]);

        // Send messages
        input_tx.send(1).unwrap();
        input_tx.send(2).unwrap();
        input_tx.send(3).unwrap();

        // All receivers should get all messages
        assert_eq!(out1_rx.recv().unwrap(), 1);
        assert_eq!(out1_rx.recv().unwrap(), 2);
        assert_eq!(out1_rx.recv().unwrap(), 3);

        assert_eq!(out2_rx.recv().unwrap(), 1);
        assert_eq!(out2_rx.recv().unwrap(), 2);
        assert_eq!(out2_rx.recv().unwrap(), 3);

        assert_eq!(out3_rx.recv().unwrap(), 1);
        assert_eq!(out3_rx.recv().unwrap(), 2);
        assert_eq!(out3_rx.recv().unwrap(), 3);
    }

    #[test]
    fn test_fanout_closes_on_input_close() {
        let (input_tx, input_rx) = channel::unbounded::<i32>();
        let (out_tx, out_rx) = channel::unbounded::<i32>();

        let handle = spawn(input_rx, vec![out_tx]);

        input_tx.send(42).unwrap();
        drop(input_tx); // Close input channel

        // Thread should exit
        handle.join().expect("fanout thread panicked");

        // Should receive the message then get disconnect
        assert_eq!(out_rx.recv().unwrap(), 42);
        assert!(out_rx.recv().is_err());
    }

    #[test]
    fn test_fanout_closes_downstream_on_exit() {
        let (input_tx, input_rx) = channel::unbounded::<i32>();
        let (out1_tx, out1_rx) = channel::unbounded::<i32>();
        let (out2_tx, out2_rx) = channel::unbounded::<i32>();

        let handle = spawn(input_rx, vec![out1_tx, out2_tx]);

        drop(input_tx); // Close input immediately
        handle.join().expect("fanout thread panicked");

        // Both downstream channels should be closed
        assert!(out1_rx.recv().is_err());
        assert!(out2_rx.recv().is_err());
    }
}
