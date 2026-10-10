//! RealmShark-compatible RC4 state (see NOTICE.md).
use crate::ProtocolError;

/// Cloning forks the exact current cipher position; keep a fresh clone for reset.
/// Cipher state deliberately implements neither Debug nor Serialize.
#[derive(Clone)]
pub struct Rc4 {
    state: [u8; 256],
    i: u8,
    j: u8,
}

impl Rc4 {
    pub fn new(key: &[u8]) -> Result<Self, ProtocolError> {
        if key.is_empty() {
            return Err(ProtocolError::Definition);
        }
        let mut state = [0; 256];
        for (i, slot) in state.iter_mut().enumerate() {
            *slot = i as u8;
        }
        let mut j = 0u8;
        for i in 0..256 {
            j = j.wrapping_add(state[i]).wrapping_add(key[i % key.len()]);
            state.swap(i, usize::from(j));
        }
        Ok(Self { state, i: 0, j: 0 })
    }

    fn next(&mut self) -> u8 {
        self.i = self.i.wrapping_add(1);
        self.j = self.j.wrapping_add(self.state[usize::from(self.i)]);
        self.state.swap(usize::from(self.i), usize::from(self.j));
        let index = self.state[usize::from(self.i)].wrapping_add(self.state[usize::from(self.j)]);
        self.state[usize::from(index)]
    }

    pub fn apply(&mut self, bytes: &mut [u8]) {
        for byte in bytes {
            *byte ^= self.next();
        }
    }

    pub fn skip(&mut self, len: usize) {
        for _ in 0..len {
            self.next();
        }
    }
}
