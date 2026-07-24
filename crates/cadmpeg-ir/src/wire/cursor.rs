// SPDX-License-Identifier: Apache-2.0
//! Poisoned byte cursor shared by the binary codecs.
//!
//! [`Cursor`] reads little- and big-endian primitives, geometric compounds,
//! and byte-floored element counts out of one untrusted, in-memory window.
//! It replaces the per-codec `Option`-returning clones with a single value
//! that carries its own failure state, so decode paths read a whole frame of
//! infallible-looking calls and check for failure exactly once at the end.
//!
//! # Poisoning contract
//!
//! Every read is total: it returns a value even when it cannot be satisfied.
//! The first read that fails records a [`Fault`] and *poisons* the cursor.
//! The contract is the API, and callers may rely on all of it:
//!
//! - A failed read **never advances** [`position`](Cursor::position). The
//!   recorded [`Fault::offset`] is the position at the **first** failure and
//!   is deterministic: later failures never overwrite it.
//! - Once poisoned, every read returns a zero value (`0`, `0.0`, an empty
//!   slice, or an all-zero compound), [`remaining`](Cursor::remaining)
//!   reports `0`, and [`position`](Cursor::position) stops moving.
//! - `f64` reads through [`f64_le`](Cursor::f64_le) reject non-finite bit
//!   patterns (`NaN`, `±inf`) with [`FaultKind::NonFinite`].
//!   [`f64_le_raw`](Cursor::f64_le_raw) is the bit-preserving escape hatch
//!   for callers that must round-trip the exact encoding.
//!
//! # Adoption pattern: finish before interpret
//!
//! Read the frame, then call [`finish`](Cursor::finish) **before** acting on
//! any decoded value:
//!
//! ```ignore
//! let count = cursor.u32_le();
//! let first = cursor.f64_le();
//! cursor.finish()?;            // the single terminal check
//! // ... only now interpret `count` and `first`
//! ```
//!
//! [`finish`](Cursor::finish) is the one terminal check because the zero
//! values a poisoned cursor returns are otherwise indistinguishable from
//! genuine data. A truncated length field reads back as `0`, and a bare
//! `for _ in 0..count` loop then interprets "zero items" as success while
//! silently dropping the rest of the record. Checking [`finish`](Cursor::finish)
//! before interpreting the count closes that poison-masking hazard.
//!
//! # No allocation helpers
//!
//! This module exposes no `Vec`-returning readers. Callers size a collection
//! with [`counted`](Cursor::counted) (or the free [`bounded_len`]) and fill
//! it with a plain loop, so the `clippy::disallowed_methods` hardening that
//! bans unfloored growth calls stays at the codec call site rather than being
//! centralized and bypassed here.
//!
//! Big-endian `f64`/compound readers and length-prefixed string readers are
//! deliberately absent; adoption adds them on demand rather than growing a
//! speculative surface here.
#![deny(clippy::disallowed_methods)]
#![warn(clippy::indexing_slicing, clippy::arithmetic_side_effects)]

use crate::math::{Point3, Vector3};

/// Converts a declared element count into a safe `Vec` capacity.
///
/// Returns `None` unless `count * element_size <= remaining`, i.e. unless
/// the declared elements could actually be present in the unread input.
/// `element_size` must be the minimum encoded size of one element and must
/// be nonzero.
pub fn bounded_len(count: u64, element_size: usize, remaining: usize) -> Option<usize> {
    if element_size == 0 {
        return None;
    }
    let count = usize::try_from(count).ok()?;
    let bytes = count.checked_mul(element_size)?;
    (bytes <= remaining).then_some(count)
}

/// A recorded read failure: where the cursor first failed and why.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fault {
    /// Absolute byte offset the cursor held when the failing read began.
    pub offset: usize,
    /// The category of failure.
    pub kind: FaultKind,
}

/// The category of a [`Fault`].
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FaultKind {
    /// A read ran past the end of the readable window.
    Truncated,
    /// A finite-checked `f64` decoded to `NaN` or `±inf`.
    NonFinite,
    /// A declared element count could not fit in the unread bytes.
    ImplausibleCount,
    /// A decoded value violated a domain rule (e.g. a degenerate direction).
    Domain,
}

/// A poisoned cursor over a bounded window of an in-memory payload.
///
/// See the [module docs](self) for the poisoning contract.
#[derive(Debug, Clone, Copy)]
pub struct Cursor<'a> {
    bytes: &'a [u8],
    position: usize,
    end: usize,
    fault: Option<Fault>,
}

impl<'a> Cursor<'a> {
    /// Creates a cursor over the whole slice.
    pub fn new(bytes: &'a [u8]) -> Self {
        Self {
            bytes,
            position: 0,
            end: bytes.len(),
            fault: None,
        }
    }

    /// Creates a sub-cursor over the absolute byte range `start..end` of the
    /// underlying buffer.
    ///
    /// An out-of-bounds range (`start > end`, or `end` past the buffer) yields
    /// a cursor already poisoned with [`FaultKind::Truncated`] at `start`. The
    /// child also inherits any fault already on the parent: a window can add a
    /// fault but never clear one.
    #[must_use]
    pub fn window(&self, start: usize, end: usize) -> Cursor<'a> {
        if start <= end && end <= self.bytes.len() {
            Cursor {
                bytes: self.bytes,
                position: start,
                end,
                fault: self.fault,
            }
        } else {
            let mut child = Cursor {
                bytes: self.bytes,
                position: start,
                end: start,
                fault: self.fault,
            };
            child.poison(FaultKind::Truncated);
            child
        }
    }

    /// Returns the absolute cursor offset.
    pub fn position(&self) -> usize {
        self.position
    }

    /// Returns the unread byte count, or `0` once poisoned.
    pub fn remaining(&self) -> usize {
        if self.fault.is_some() {
            0
        } else {
            self.end.saturating_sub(self.position)
        }
    }

    /// Returns whether a read has failed.
    pub fn is_poisoned(&self) -> bool {
        self.fault.is_some()
    }

    /// Returns the recorded fault, if any.
    pub fn fault(&self) -> Option<Fault> {
        self.fault
    }

    /// The single terminal check: `Ok` unless a read has failed.
    ///
    /// Reports the **first** recorded [`Fault`]. Call this before interpreting
    /// any decoded value; see the [module docs](self).
    pub fn finish(&self) -> Result<(), Fault> {
        self.fault.map_or(Ok(()), Err)
    }

    /// Records a codec-raised fault at the current position.
    ///
    /// A no-op if the cursor is already poisoned, so the first fault wins.
    pub fn poison(&mut self, kind: FaultKind) {
        self.fault_at(self.position, kind);
    }

    /// Records a fault at `offset`, keeping any earlier fault.
    fn fault_at(&mut self, offset: usize, kind: FaultKind) {
        if self.fault.is_none() {
            self.fault = Some(Fault { offset, kind });
        }
    }

    /// Takes `count` bytes, advancing only on success.
    ///
    /// Returns an empty slice and poisons [`FaultKind::Truncated`] when the
    /// bytes are not available; the position does not move on failure.
    pub fn take(&mut self, count: usize) -> &'a [u8] {
        if self.fault.is_some() {
            return &[];
        }
        let Some(end) = self.position.checked_add(count) else {
            self.poison(FaultKind::Truncated);
            return &[];
        };
        if end > self.end {
            self.poison(FaultKind::Truncated);
            return &[];
        }
        match self.bytes.get(self.position..end) {
            Some(bytes) => {
                self.position = end;
                bytes
            }
            None => {
                self.poison(FaultKind::Truncated);
                &[]
            }
        }
    }

    /// Reads a fixed-size byte array, zero-filled on failure.
    fn array<const N: usize>(&mut self) -> [u8; N] {
        <[u8; N]>::try_from(self.take(N)).unwrap_or([0; N])
    }

    /// Skips exactly `count` bytes, poisoning [`FaultKind::Truncated`] when
    /// they are not available.
    pub fn skip(&mut self, count: usize) {
        let _ = self.take(count);
    }

    /// Reads a byte, or `0` on failure.
    pub fn u8(&mut self) -> u8 {
        let [value] = self.array::<1>();
        value
    }

    /// Reads a finite little-endian `f64`.
    ///
    /// A `NaN` or `±inf` bit pattern poisons [`FaultKind::NonFinite`] at the
    /// field's start offset and returns `0.0`. Use [`f64_le_raw`](Self::f64_le_raw)
    /// to preserve non-finite bits.
    pub fn f64_le(&mut self) -> f64 {
        let start = self.position;
        let value = f64::from_le_bytes(self.array());
        if self.fault.is_some() {
            return 0.0;
        }
        if value.is_finite() {
            value
        } else {
            self.fault_at(start, FaultKind::NonFinite);
            0.0
        }
    }

    /// Reads a little-endian `f64`, preserving all bits including `NaN`/`±inf`.
    ///
    /// Zero-filled (and so `0.0`) only on a truncated read.
    pub fn f64_le_raw(&mut self) -> f64 {
        f64::from_le_bytes(self.array())
    }

    /// Reads a [`Point3`] as three finite little-endian `f64` values.
    pub fn point3_le(&mut self) -> Point3 {
        let x = self.f64_le();
        let y = self.f64_le();
        let z = self.f64_le();
        Point3::new(x, y, z)
    }

    /// Reads a [`Vector3`] as three finite little-endian `f64` values.
    pub fn vector3_le(&mut self) -> Vector3 {
        let x = self.f64_le();
        let y = self.f64_le();
        let z = self.f64_le();
        Vector3::new(x, y, z)
    }

    /// Reads a unit direction as three finite little-endian `f64` values and
    /// normalizes it to unit length.
    ///
    /// A direction whose length is within [`f64::EPSILON`] of zero is
    /// degenerate; it poisons [`FaultKind::Domain`] and returns a zero vector.
    pub fn unit3_le(&mut self) -> Vector3 {
        let start = self.position;
        let vector = self.vector3_le();
        if self.fault.is_some() {
            return Vector3::new(0.0, 0.0, 0.0);
        }
        match vector.unit() {
            Some(unit) => unit,
            None => {
                self.fault_at(start, FaultKind::Domain);
                Vector3::new(0.0, 0.0, 0.0)
            }
        }
    }

    /// Floors a declared element count against the unread bytes.
    ///
    /// Returns the proven count when `count * element_size` fits in the
    /// remaining bytes; otherwise poisons [`FaultKind::ImplausibleCount`] and
    /// returns `0`. See [`bounded_len`].
    pub fn counted(&mut self, count: u64, element_size: usize) -> usize {
        if self.fault.is_some() {
            return 0;
        }
        match bounded_len(count, element_size, self.remaining()) {
            Some(len) => len,
            None => {
                self.poison(FaultKind::ImplausibleCount);
                0
            }
        }
    }
}

macro_rules! cursor_readers {
    ($($name:ident, $ty:ty, $conversion:ident);* $(;)?) => {
        impl Cursor<'_> {
            $(
                #[doc = concat!("Reads a little- or big-endian `", stringify!($ty), "` via `", stringify!($conversion), "`, or `0` on failure.")]
                pub fn $name(&mut self) -> $ty {
                    <$ty>::$conversion(self.array())
                }
            )*
        }
    };
}

cursor_readers!(
    u16_le, u16, from_le_bytes;
    i16_le, i16, from_le_bytes;
    u32_le, u32, from_le_bytes;
    i32_le, i32, from_le_bytes;
    u64_le, u64, from_le_bytes;
    i64_le, i64, from_le_bytes;
    f32_le, f32, from_le_bytes;
    u16_be, u16, from_be_bytes;
    u32_be, u32, from_be_bytes;
    u64_be, u64, from_be_bytes;
);

#[cfg(test)]
mod tests {
    use super::{bounded_len, Cursor, FaultKind};

    /// A fixture packing `u8`, `u16_le`, `u32_le`, `f64_le` back to back.
    /// Field start offsets: 0, 1, 3, 7; total length 15.
    fn multi_field() -> Vec<u8> {
        let mut bytes = vec![0x11u8];
        bytes.extend_from_slice(&0x2222u16.to_le_bytes());
        bytes.extend_from_slice(&0x3333_3333u32.to_le_bytes());
        bytes.extend_from_slice(&1.5f64.to_le_bytes());
        bytes
    }

    fn read_multi_field(cursor: &mut Cursor<'_>) {
        cursor.u8();
        cursor.u16_le();
        cursor.u32_le();
        cursor.f64_le();
    }

    #[test]
    fn full_frame_reads_advance_and_finish_ok() {
        let bytes = multi_field();
        let mut cursor = Cursor::new(&bytes);
        assert_eq!(cursor.u8(), 0x11);
        assert_eq!(cursor.u16_le(), 0x2222);
        assert_eq!(cursor.u32_le(), 0x3333_3333);
        assert_eq!(cursor.f64_le(), 1.5);
        assert_eq!(cursor.position(), 15);
        assert_eq!(cursor.remaining(), 0);
        assert!(!cursor.is_poisoned());
        assert_eq!(cursor.finish(), Ok(()));
    }

    #[test]
    fn truncation_at_every_prefix_poisons_at_field_start() {
        let full = multi_field();
        // First field whose read exceeds the prefix, keyed by prefix length.
        let field_starts = [0usize, 1, 3, 7];
        let field_ends = [1usize, 3, 7, 15];

        for prefix_len in 0..full.len() {
            let bytes = &full[..prefix_len];
            let mut cursor = Cursor::new(bytes);
            read_multi_field(&mut cursor);

            // The first field whose end exceeds the available prefix.
            let failed = field_ends
                .iter()
                .position(|&end| end > prefix_len)
                .expect("a short prefix truncates some field");
            let expected_offset = field_starts[failed];

            assert!(cursor.is_poisoned(), "prefix {prefix_len} should poison");
            let fault = cursor.fault().expect("poisoned cursor has a fault");
            assert_eq!(fault.kind, FaultKind::Truncated, "prefix {prefix_len}");
            assert_eq!(
                fault.offset, expected_offset,
                "prefix {prefix_len} faults at the failed field's start"
            );
            assert_eq!(
                cursor.position(),
                expected_offset,
                "prefix {prefix_len} never advanced past the failed read"
            );
            assert_eq!(cursor.finish(), Err(fault));
        }
    }

    #[test]
    fn reads_after_poison_return_zeros_without_advancing() {
        let bytes = [9u8, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9];
        let mut cursor = Cursor::new(&bytes);
        // Poison by over-reading, then confirm the position froze.
        assert_eq!(cursor.take(64), b"");
        assert!(cursor.is_poisoned());
        let frozen = cursor.position();

        assert_eq!(cursor.u8(), 0);
        assert_eq!(cursor.u16_le(), 0);
        assert_eq!(cursor.u32_le(), 0);
        assert_eq!(cursor.u64_le(), 0);
        assert_eq!(cursor.f64_le(), 0.0);
        assert_eq!(cursor.f64_le_raw(), 0.0);
        assert_eq!(cursor.take(1), b"");
        assert_eq!(cursor.counted(1, 1), 0);
        assert_eq!(cursor.remaining(), 0);
        assert_eq!(cursor.position(), frozen);
    }

    #[test]
    fn f64_le_rejects_nonfinite_and_raw_preserves_bits() {
        let quiet_nan = f64::from_bits(0x7FF8_0000_0000_0001);
        for pattern in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, quiet_nan] {
            let bytes = pattern.to_le_bytes();

            let mut checked = Cursor::new(&bytes);
            assert_eq!(checked.f64_le(), 0.0);
            let fault = checked.fault().expect("non-finite poisons");
            assert_eq!(fault.kind, FaultKind::NonFinite);
            assert_eq!(fault.offset, 0);

            let mut raw = Cursor::new(&bytes);
            assert_eq!(raw.f64_le_raw().to_bits(), pattern.to_bits());
            assert!(!raw.is_poisoned());
        }
    }

    #[test]
    fn counted_floors_impossible_counts() {
        // Byte-floored element counts: exact fit, one-over, u32::MAX, and zero size.
        let payload = [0u8; 40];

        let mut cursor = Cursor::new(&payload);
        assert_eq!(cursor.counted(10, 4), 10, "exact fit");
        assert!(!cursor.is_poisoned());

        let mut cursor = Cursor::new(&payload);
        assert_eq!(cursor.counted(11, 4), 0);
        assert_eq!(cursor.fault().unwrap().kind, FaultKind::ImplausibleCount);

        let mut cursor = Cursor::new(&payload);
        assert_eq!(cursor.counted(u64::from(u32::MAX), 4), 0);
        assert!(cursor.is_poisoned());

        let mut cursor = Cursor::new(&payload);
        assert_eq!(cursor.counted(1, 0), 0, "zero element size");
        assert!(cursor.is_poisoned());

        assert_eq!(bounded_len(u64::MAX, 20, usize::MAX), None);
    }

    #[test]
    fn window_out_of_bounds_poisons() {
        let bytes = [0u8; 8];
        let cursor = Cursor::new(&bytes);

        let past_end = cursor.window(2, 100);
        assert!(past_end.is_poisoned());
        let fault = past_end.fault().expect("oob window poisons");
        assert_eq!(fault.kind, FaultKind::Truncated);
        assert_eq!(fault.offset, 2);

        let inverted = cursor.window(6, 5);
        assert!(inverted.is_poisoned());
        assert_eq!(inverted.fault().unwrap().offset, 6);

        let mut valid = cursor.window(2, 6);
        assert!(!valid.is_poisoned());
        assert_eq!(valid.remaining(), 4);
        assert_eq!(valid.take(4).len(), 4);
        assert_eq!(valid.finish(), Ok(()));
    }

    #[test]
    fn unit3_le_normalizes_and_degenerate_poisons_domain() {
        let mut buffer = Vec::new();
        for value in [2.0f64, 0.0, 0.0] {
            buffer.extend_from_slice(&value.to_le_bytes());
        }
        let mut cursor = Cursor::new(&buffer);
        assert_eq!(cursor.unit3_le(), crate::math::Vector3::new(1.0, 0.0, 0.0));
        assert!(!cursor.is_poisoned());

        let zero = [0u8; 24];
        let mut cursor = Cursor::new(&zero);
        let direction = cursor.unit3_le();
        assert_eq!(direction, crate::math::Vector3::new(0.0, 0.0, 0.0));
        let fault = cursor.fault().expect("degenerate direction poisons");
        assert_eq!(fault.kind, FaultKind::Domain);
        assert_eq!(fault.offset, 0);
    }

    #[test]
    fn finish_reports_the_first_fault() {
        let bytes = [1u8, 2, 3];
        let mut cursor = Cursor::new(&bytes);
        cursor.u8();
        // First fault: truncated at offset 1.
        assert_eq!(cursor.u32_le(), 0);
        // A later codec-raised poison must not overwrite the first fault.
        cursor.poison(FaultKind::Domain);

        let fault = cursor.finish().expect_err("cursor is poisoned");
        assert_eq!(fault.kind, FaultKind::Truncated);
        assert_eq!(fault.offset, 1);
    }

    #[test]
    fn poison_records_at_current_position() {
        let bytes = [0u8; 8];
        let mut cursor = Cursor::new(&bytes);
        cursor.u32_le();
        assert_eq!(cursor.position(), 4);
        cursor.poison(FaultKind::Domain);
        let fault = cursor.fault().expect("explicit poison");
        assert_eq!(fault.kind, FaultKind::Domain);
        assert_eq!(fault.offset, 4);
    }
}
