use crossbeam_channel::{Receiver, Sender, TrySendError};

/// Single-producer queue. The receiver clone is retained solely for eviction.
pub fn send_latest<T>(tx: &Sender<T>, eviction: &Receiver<T>, mut item: T) -> Result<u32, T> {
    let mut drops = 0u32;
    loop {
        match tx.try_send(item) {
            Ok(()) => return Ok(drops),
            Err(TrySendError::Disconnected(value)) => return Err(value),
            Err(TrySendError::Full(value)) => {
                item = value;
                if eviction.try_recv().is_ok() {
                    drops = drops.saturating_add(1);
                }
            }
        }
    }
}
