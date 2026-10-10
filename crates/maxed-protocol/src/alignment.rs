//! Directional tick alignment, ported from RealmShark (NOTICE.md).
use crate::{key_bytes, ProtocolError, Rc4};
use maxed_types::Direction;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
pub const SEARCH_SIZE: usize = 10_000_000;
pub const RECOVERY_BUDGET: usize = 2_097_152;
const BATCH: usize = 4096;

pub fn find_tick_offset(base: &Rc4, a: [u8; 4], b: [u8; 4], delta: usize) -> Option<usize> {
    search(base, a, b, delta, &AtomicBool::new(false))
}
fn search(base: &Rc4, a: [u8; 4], b: [u8; 4], delta: usize, cancel: &AtomicBool) -> Option<usize> {
    if delta > RECOVERY_BUDGET {
        return None;
    }
    let mut left = base.clone();
    let mut right = base.clone();
    let mut remaining = delta;
    while remaining > 0 {
        if cancel.load(Ordering::Relaxed) {
            return None;
        }
        let n = remaining.min(BATCH);
        right.skip(n);
        remaining -= n;
    }
    let mut l = [0; 4];
    let mut r = [0; 4];
    left.apply(&mut l);
    right.apply(&mut r);
    // Sliding keystream windows avoid copying a 256-byte cipher for each candidate.
    // Java also rejects offset SEARCH_SIZE, including when that candidate matches.
    for offset in 0..SEARCH_SIZE {
        if offset.is_multiple_of(BATCH) && cancel.load(Ordering::Relaxed) {
            return None;
        }
        let x = i32::from_be_bytes(std::array::from_fn(|i| a[i] ^ l[i]));
        let y = i32::from_be_bytes(std::array::from_fn(|i| b[i] ^ r[i]));
        if x.wrapping_add(1) == y {
            return Some(offset);
        }
        l.rotate_left(1);
        r.rotate_left(1);
        l[3] = 0;
        r[3] = 0;
        left.apply(&mut l[3..]);
        right.apply(&mut r[3..]);
    }
    None
}

pub struct Alignment {
    fresh: Rc4,
    cipher: Rc4,
    synced: bool,
    current_tick: i32,
    first: Option<[u8; 4]>,
    delta: usize,
    cancel: Arc<AtomicBool>,
}
impl Alignment {
    pub fn new(dir: Direction, fresh: bool) -> Result<Self, ProtocolError> {
        Self::with_cancellation(dir, fresh, Arc::new(AtomicBool::new(false)))
    }
    pub(crate) fn with_cancellation(
        dir: Direction,
        fresh: bool,
        cancel: Arc<AtomicBool>,
    ) -> Result<Self, ProtocolError> {
        let cipher = Rc4::new(&key_bytes(dir)?)?;
        Ok(Self {
            fresh: cipher.clone(),
            cipher,
            synced: fresh,
            current_tick: -1,
            first: None,
            delta: 0,
            cancel,
        })
    }
    pub fn is_synced(&self) -> bool {
        self.synced
    }
    pub(crate) fn current_tick(&self) -> Option<i32> {
        self.synced.then_some(self.current_tick)
    }
    pub fn consume(&mut self, id: u8, body: &[u8]) -> Result<Option<Vec<u8>>, ProtocolError> {
        if self.cancel.load(Ordering::Relaxed) {
            return Ok(None);
        }
        if body.len() > crate::framing::MAX_FRAME - 5 {
            return Err(ProtocolError::Length);
        }
        let tick = matches!(id, 10 | 62) && body.len() >= 4;
        if self.synced {
            if tick {
                let mut head = [body[0], body[1], body[2], body[3]];
                self.cipher.clone().apply(&mut head);
                let n = i32::from_be_bytes(head);
                if n != self.current_tick.wrapping_add(1) {
                    self.synced = false;
                    self.first = None;
                    self.delta = 0;
                    self.cipher = self.fresh.clone();
                } else {
                    self.current_tick = n;
                }
            }
            if self.synced {
                let mut plain = body.to_vec();
                self.cipher.apply(&mut plain);
                return Ok(Some(plain));
            }
        }
        if tick {
            let head = [body[0], body[1], body[2], body[3]];
            if let Some(first) = self.first.take() {
                if let Some(offset) = search(&self.fresh, first, head, self.delta, &self.cancel) {
                    self.cipher = self.fresh.clone();
                    // Positioning is cancellable too; no long unchecked skip after search.
                    let mut remaining = offset + self.delta;
                    while remaining > 0 {
                        if self.cancel.load(Ordering::Relaxed) {
                            return Ok(None);
                        }
                        let n = remaining.min(BATCH);
                        self.cipher.skip(n);
                        remaining -= n;
                    }
                    let mut plain = head;
                    self.cipher.apply(&mut plain);
                    self.cipher.skip(body.len() - 4);
                    self.current_tick = i32::from_be_bytes(plain);
                    self.synced = true;
                }
                self.delta = 0; // B is suppressed, whether the candidate succeeded or failed.
            } else {
                self.first = Some(head);
                self.delta = body.len();
            }
        } else if self.first.is_some() {
            self.delta = self.delta.saturating_add(body.len());
            if self.delta > RECOVERY_BUDGET {
                self.first = None;
                self.delta = 0;
            }
        }
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cancellation_can_interrupt_an_exhaustive_search() {
        let cancelled = Arc::new(AtomicBool::new(false));
        let signal = cancelled.clone();
        let (ready, wait) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            let base = Rc4::new(&key_bytes(Direction::ServerToClient).unwrap()).unwrap();
            ready.send(()).unwrap();
            search(&base, [0; 4], [0; 4], 4, &signal)
        });
        wait.recv().unwrap();
        cancelled.store(true, Ordering::Relaxed);
        assert_eq!(worker.join().unwrap(), None);
    }
}
