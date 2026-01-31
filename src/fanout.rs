//! Generic fanout module for broadcasting messages to multiple receivers.

use std::thread;

use crossbeam_channel as channel;
use channel::Receiver;

// Duplicate the receiver
pub fn duplicate<T: Clone + Send + 'static>(rx: Receiver<T>) -> (Receiver<T>, Receiver<T>) {
    let (tx1, rx1) = channel::unbounded::<T>();
    let (tx2, rx2) = channel::unbounded::<T>();
    thread::spawn(move || {
        rx.iter().for_each(|v| {
            let _ = tx1.send(v.clone());
            let _ = tx2.send(v);
            ()
        })
    });
    (rx1, rx2)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossbeam_channel as channel;

    #[test]
    fn test_fanout_delivers_to_all_receivers() {
        let (input_tx, input_rx) = channel::unbounded::<i32>();
        let (out1_rx, out2_rx) = duplicate(input_rx);

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
    }

    #[test]
    fn test_fanout_closes_on_input_close() {
        let (input_tx, input_rx) = channel::unbounded::<i32>();
        let (out_rx, _) = duplicate(input_rx);

        input_tx.send(42).unwrap();
        drop(input_tx); // Close input channel

        // Thread should exit
        // Should receive the message then get disconnect
        assert_eq!(out_rx.recv().unwrap(), 42);
        assert!(out_rx.recv().is_err());
    }

    #[test]
    fn test_fanout_closes_downstream_on_exit() {
        let (input_tx, input_rx) = channel::unbounded::<i32>();
        let (out1_rx, out2_rx) = duplicate(input_rx);

        drop(input_tx); // Close input immediately

        // Both downstream channels should be closed
        assert!(out1_rx.recv().is_err());
        assert!(out2_rx.recv().is_err());
    }
}
