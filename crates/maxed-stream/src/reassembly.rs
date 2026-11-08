pub fn serial_distance(start: u32, expected: u32) -> i32 {
    start.wrapping_sub(expected) as i32
}

use maxed_types::*;
use std::collections::VecDeque;

const MAX_PENDING: usize = 256;
const HOLE_SEGMENTS: usize = 32;
const HOLE_AGE_MICROS: u64 = 1_000_000;
const MAX_BYTES: usize = 2_097_152;
const HISTORY_BYTES: usize = 65_535;
const DIRECTIONS: [Direction; 2] = [Direction::ClientToServer, Direction::ServerToClient];

#[derive(Default)]
struct Half {
    syn: Option<u32>,
    expected: Option<u32>,
    opened: bool,
    closed: bool,
    pending: Vec<TcpSegment>,
    // Bounded recent delivered bytes permit contradictory retransmission detection.
    history: VecDeque<(u32, Vec<u8>)>,
    history_bytes: usize,
}
impl Half {
    fn invalidate(&mut self) {
        self.expected = None;
        self.pending.clear();
        self.history.clear();
        self.history_bytes = 0;
    }
    fn drain(
        &mut self,
        conn: ConnId,
        dir: Direction,
        at: Timestamp,
        force: bool,
        events: &mut Vec<StreamEvent>,
    ) {
        while let Some(expected) = self.expected {
            let Some(position) = self
                .pending
                .iter()
                .position(|p| serial_distance(p.seq.wrapping_add(u32::from(p.syn)), expected) <= 0)
            else {
                if !force || self.pending.is_empty() {
                    break;
                }
                let next = self
                    .pending
                    .iter()
                    .map(|p| p.seq.wrapping_add(u32::from(p.syn)))
                    .min_by_key(|seq| serial_distance(*seq, expected))
                    .unwrap();
                if serial_distance(next, expected) as usize > MAX_BYTES {
                    // A stray far-ahead segment must not pin the sequence cursor
                    // ahead of all subsequent legitimate traffic.
                    self.invalidate();
                    events.push(StreamEvent::Gap {
                        conn,
                        dir,
                        at,
                        reason: GapReason::ReorderLimit,
                    });
                    break;
                }
                self.expected = Some(next);
                events.push(StreamEvent::Gap {
                    conn,
                    dir,
                    at,
                    reason: GapReason::ReorderLimit,
                });
                continue;
            };
            let mut part = self.pending.remove(position);
            let part_len = part.bytes.len();
            let start = part.seq.wrapping_add(u32::from(part.syn));
            let consumed = (-i64::from(serial_distance(start, expected))) as usize;
            if consumed < part.bytes.len() {
                let Some(offset) = part.origin.offset.checked_add(consumed) else {
                    self.invalidate();
                    events.push(StreamEvent::Gap {
                        conn,
                        dir: part.dir,
                        at,
                        reason: GapReason::InvalidFrame,
                    });
                    break;
                };
                if part.origin.len != part.bytes.len()
                    || part.origin.offset.checked_add(part.bytes.len()).is_none()
                {
                    self.invalidate();
                    events.push(StreamEvent::Gap {
                        conn,
                        dir: part.dir,
                        at,
                        reason: GapReason::InvalidFrame,
                    });
                    break;
                }
                // Reuse the segment allocation, including for a partial overlap.
                if consumed != 0 {
                    part.bytes.drain(..consumed);
                }
                let fresh = part.bytes;
                self.expected = Some(expected.wrapping_add(fresh.len() as u32));
                self.remember(expected, fresh.clone());
                events.push(StreamEvent::Data(StreamChunk {
                    conn,
                    dir: part.dir,
                    at,
                    origins: vec![ByteOrigin {
                        frame_index: part.origin.frame_index,
                        offset,
                        len: fresh.len(),
                    }],
                    bytes: fresh,
                }));
            }
            if part.fin && self.expected == Some(start.wrapping_add(part_len as u32)) {
                self.expected = self.expected.map(|seq| seq.wrapping_add(1));
                self.closed = true;
                self.pending.clear();
                events.push(StreamEvent::Closed {
                    conn,
                    dir: part.dir,
                    at,
                    reason: CloseReason::Fin,
                });
                break;
            }
        }
    }
    fn remember(&mut self, seq: u32, bytes: Vec<u8>) {
        self.history_bytes += bytes.len();
        self.history.push_back((seq, bytes));
        while self.history.len() > MAX_PENDING || self.history_bytes > HISTORY_BYTES {
            if let Some((_, bytes)) = self.history.pop_front() {
                self.history_bytes -= bytes.len();
            }
        }
    }
}
struct Generation {
    flow: FlowKey,
    conn: ConnId,
    halves: [Half; 2],
    latest_at: Timestamp,
    // Keep one clock step reversible until capture time advances again.
    clock_step: Option<(Timestamp, Timestamp)>,
}
struct Retired {
    flow: FlowKey,
    ranges: [Vec<(u32, usize)>; 2],
}

#[derive(Default)]
pub struct Reassembler {
    next_id: u64,
    active: VecDeque<Generation>,
    retired: VecDeque<Retired>,
    // Java's directional constructor remains reset after an RST, even when
    // that RST was the first observed frame of an otherwise stale connection.
    reset_pending: [bool; 2],
    // Java selects connections independently in each direction and resets the
    // constructor on replacement, including when the client socket changes.
    current_flow: [Option<FlowKey>; 2],
}

fn direction_index(dir: Direction) -> usize {
    usize::from(dir == Direction::ServerToClient)
}

fn contradicts(a: u32, bytes: &[u8], b: u32, other: &[u8]) -> bool {
    let delta = i64::from(serial_distance(b, a));
    let start = delta.max(0) as usize;
    let other_start = (-delta).max(0) as usize;
    let count = bytes
        .len()
        .saturating_sub(start)
        .min(other.len().saturating_sub(other_start));
    count > 0 && bytes[start..start + count] != other[other_start..other_start + count]
}

impl Reassembler {
    fn retire(
        &mut self,
        index: usize,
        at: Timestamp,
        reason: CloseReason,
        events: &mut Vec<StreamEvent>,
    ) {
        if let Some(generation) = self.active.remove(index) {
            let mut ranges: [Vec<(u32, usize)>; 2] = Default::default();
            for (i, mut half) in generation.halves.into_iter().enumerate() {
                let was_closed = half.closed;
                if reason != CloseReason::Stopped {
                    half.drain(generation.conn, DIRECTIONS[i], at, true, events);
                }
                if was_closed || !half.closed {
                    events.push(StreamEvent::Closed {
                        conn: generation.conn,
                        dir: DIRECTIONS[i],
                        at,
                        reason,
                    });
                }
                ranges[i].extend(
                    half.history
                        .into_iter()
                        .map(|(seq, bytes)| (seq, bytes.len())),
                );
                if let Some(syn) = half.syn {
                    ranges[i].push((syn, 1));
                }
            }
            self.retired.push_back(Retired {
                flow: generation.flow,
                ranges,
            });
            while self.retired.len() > 256 {
                self.retired.pop_front();
            }
        }
    }

    pub fn push(&mut self, segment: TcpSegment) -> Vec<StreamEvent> {
        let mut events = self.expire(segment.at);
        if !segment.syn && !segment.fin && !segment.rst && segment.bytes.is_empty() {
            return events;
        }
        let at = segment.at;
        let di = direction_index(segment.dir);
        let existing = self.active.iter().position(|g| g.flow == segment.flow);
        if existing.is_none() && !segment.syn && self.retired.iter().any(|g| g.flow == segment.flow)
        {
            return events;
        }
        if let Some(index) = existing {
            let half = &self.active[index].halves[di];
            if segment.syn
                && (half.syn.is_some_and(|seq| seq != segment.seq)
                    || (half.opened && half.syn.is_none()))
            {
                self.retire(index, at, CloseReason::Replaced, &mut events);
            } else if !segment.syn
                && self
                    .retired
                    .iter()
                    .filter(|g| g.flow == segment.flow)
                    .any(|g| {
                        g.ranges[di].iter().any(|(seq, len)| {
                            let distance = i64::from(serial_distance(segment.seq, *seq));
                            distance >= 0 && distance < *len as i64
                        })
                    })
            {
                return events;
            }
        }
        if !self.active.iter().any(|g| g.flow == segment.flow) {
            // A new server on the same client socket replaces the old connection.
            if let Some(index) = self
                .active
                .iter()
                .position(|g| g.flow.client == segment.flow.client)
            {
                self.retire(index, at, CloseReason::Replaced, &mut events);
            }
            if self.active.len() >= 64 {
                self.retire(0, at, CloseReason::Replaced, &mut events);
            }
            self.next_id = self.next_id.saturating_add(1);
            self.active.push_back(Generation {
                flow: segment.flow,
                conn: ConnId(self.next_id),
                halves: Default::default(),
                latest_at: at,
                clock_step: None,
            });
        }
        let Some(index) = self.active.iter().position(|g| g.flow == segment.flow) else {
            return events;
        };
        if segment.rst {
            self.reset_pending[di] = true;
            self.current_flow[di] = None;
            self.retire(index, at, CloseReason::Reset, &mut events);
            return events;
        }
        let generation = &mut self.active[index];
        let conn = generation.conn;
        let half = &mut generation.halves[di];
        if half.closed {
            return events;
        }
        let start = segment.seq.wrapping_add(u32::from(segment.syn));
        if !half.opened {
            let replaced = self.current_flow[di]
                .replace(segment.flow)
                .is_some_and(|previous| previous != segment.flow);
            half.opened = true;
            half.syn = segment.syn.then_some(segment.seq);
            half.expected = Some(start);
            events.push(StreamEvent::Opened {
                conn,
                dir: segment.dir,
                at,
                fresh: segment.syn | replaced | std::mem::take(&mut self.reset_pending[di]),
                syn_proven: segment.syn,
            });
        }
        let expected = *half.expected.get_or_insert(start);
        if segment.bytes.is_empty() && !segment.fin {
            return events;
        }
        if segment.bytes.len() > MAX_BYTES
            || half
                .history
                .iter()
                .any(|(seq, bytes)| contradicts(start, &segment.bytes, *seq, bytes))
            || half.pending.iter().any(|p| {
                contradicts(
                    start,
                    &segment.bytes,
                    p.seq.wrapping_add(u32::from(p.syn)),
                    &p.bytes,
                )
            })
        {
            let reason = if segment.bytes.len() > MAX_BYTES {
                GapReason::ReorderLimit
            } else {
                GapReason::ConflictingOverlap
            };
            half.invalidate();
            events.push(StreamEvent::Gap {
                conn,
                dir: segment.dir,
                at,
                reason,
            });
            return events;
        }
        if half.pending.iter().any(|p| {
            p.seq == segment.seq
                && p.syn == segment.syn
                && p.fin == segment.fin
                && p.bytes == segment.bytes
        }) {
            return events;
        }
        if serial_distance(start, expected) > 0
            && half.pending.iter().map(|p| p.bytes.len()).sum::<usize>() + segment.bytes.len()
                > MAX_BYTES
        {
            half.drain(conn, segment.dir, at, true, &mut events);
        }
        let segment_dir = segment.dir;
        half.expected.get_or_insert(start);
        half.pending.push(segment);
        half.drain(
            conn,
            segment_dir,
            at,
            half.pending.len() >= HOLE_SEGMENTS
                || half
                    .pending
                    .iter()
                    .any(|p| generation.latest_at.0.saturating_sub(p.at.0) >= HOLE_AGE_MICROS),
            &mut events,
        );
        if generation.halves.iter().all(|h| h.closed) {
            // Preserve the retired key without emitting duplicate FIN closes.
            let mut discarded = Vec::new();
            self.retire(index, at, CloseReason::Fin, &mut discarded);
        }
        events
    }

    /// Advance aged holes when capture time advances, even on unrelated frames.
    pub fn expire(&mut self, at: Timestamp) -> Vec<StreamEvent> {
        let mut events = Vec::new();
        for generation in &mut self.active {
            if let Some((previous, stepped)) = generation.clock_step {
                if at != stepped {
                    generation.clock_step = None;
                    if previous.0.saturating_sub(at.0) <= HOLE_AGE_MICROS {
                        // A lone old timestamp was an outlier. Undo its shift
                        // before evaluating age on the restored timeline. Repeated
                        // expire calls for that same frame must not commit the step.
                        let forwards = previous.0 - stepped.0;
                        for half in &mut generation.halves {
                            for pending in &mut half.pending {
                                pending.at.0 = pending.at.0.saturating_add(forwards);
                            }
                        }
                        generation.latest_at = previous;
                    }
                }
            }
            if generation.latest_at.0.saturating_sub(at.0) > HOLE_AGE_MICROS {
                // A clock step starts a new capture-time baseline. Preserve elapsed
                // hole age where representable; small timestamp reorders still use
                // the watermark. The segment-count budget also bounds stalled clocks.
                generation.clock_step = Some((generation.latest_at, at));
                let backwards = generation.latest_at.0 - at.0;
                for half in &mut generation.halves {
                    for pending in &mut half.pending {
                        pending.at.0 = pending.at.0.saturating_sub(backwards);
                    }
                }
                generation.latest_at = at;
            } else {
                generation.latest_at = generation.latest_at.max(at);
            }
            let at = generation.latest_at;
            for (i, half) in generation.halves.iter_mut().enumerate() {
                if half
                    .pending
                    .iter()
                    .any(|p| at.0.saturating_sub(p.at.0) >= HOLE_AGE_MICROS)
                {
                    half.drain(generation.conn, DIRECTIONS[i], at, true, &mut events);
                }
            }
        }
        events
    }

    /// An attributable malformed frame invalidates only its observed direction.
    pub fn gap_flow(
        &mut self,
        flow: FlowKey,
        dir: Direction,
        at: Timestamp,
        reason: GapReason,
    ) -> Vec<StreamEvent> {
        let active = self.active.iter().position(|g| {
            g.flow == flow
                && g.halves[direction_index(dir)].opened
                && !g.halves[direction_index(dir)].closed
        });
        let Some(index) = active else {
            // No live directional recipient: retain the unattributed-loss path.
            return self.gap_all(at, reason);
        };
        let generation = &mut self.active[index];
        let half = &mut generation.halves[direction_index(dir)];
        half.invalidate();
        vec![StreamEvent::Gap {
            conn: generation.conn,
            dir,
            at,
            reason,
        }]
    }

    pub fn gap_all(&mut self, at: Timestamp, reason: GapReason) -> Vec<StreamEvent> {
        let mut events = Vec::new();
        for generation in &mut self.active {
            for (i, half) in generation.halves.iter_mut().enumerate() {
                if half.opened && !half.closed {
                    half.invalidate();
                    events.push(StreamEvent::Gap {
                        conn: generation.conn,
                        dir: DIRECTIONS[i],
                        at,
                        reason,
                    });
                }
            }
        }
        events
    }

    pub fn close_all(&mut self, at: Timestamp) -> Vec<StreamEvent> {
        let mut events = Vec::new();
        while !self.active.is_empty() {
            self.retire(0, at, CloseReason::Stopped, &mut events);
        }
        self.current_flow = [None; 2];
        self.reset_pending = [false; 2];
        events
    }
}
