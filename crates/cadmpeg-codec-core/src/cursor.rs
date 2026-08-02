// SPDX-License-Identifier: Apache-2.0
//! Bounded byte cursor shared by binary codecs.
//!
//! The cursor makes the two recurring decoder defects unrepresentable at the
//! call site instead of relying on per-function discipline:
//!
//! - **Out-of-bounds and underflowing offset arithmetic**: every read and
//!   seek is checked and returns `Option`, so raw `payload[a - 12..b]`
//!   indexing never appears in decoder code.
//! - **Unbounded allocations from declared counts**: [`Cursor::counted`] and
//!   [`bounded_len`] refuse any element count that could not physically fit
//!   in the unread bytes, so a malformed count of `0xFFFF_FFFF` fails the
//!   read instead of reserving gigabytes.
//!
//! New decoder modules should read through this cursor (or the free
//! functions in [`crate::le`]/[`crate::be`]) rather than slicing payloads
//! directly, and should carry this module's lint attributes.
#![warn(clippy::indexing_slicing, clippy::arithmetic_side_effects)]

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

/// A checked cursor over a bounded window of an in-memory payload.
#[derive(Debug, Clone, Copy)]
pub struct Cursor<'a> {
    bytes: &'a [u8],
    position: usize,
    end: usize,
}

impl<'a> Cursor<'a> {
    /// Creates a cursor over the whole slice.
    pub fn new(bytes: &'a [u8]) -> Self {
        Self {
            bytes,
            position: 0,
            end: bytes.len(),
        }
    }

    /// Creates a cursor over `start..end` of the slice.
    pub fn with_bounds(bytes: &'a [u8], start: usize, end: usize) -> Option<Self> {
        (start <= end && end <= bytes.len()).then_some(Self {
            bytes,
            position: start,
            end,
        })
    }

    /// Returns the absolute cursor offset.
    pub fn position(&self) -> usize {
        self.position
    }

    /// Returns the unread byte count.
    pub fn remaining(&self) -> usize {
        self.end.saturating_sub(self.position)
    }

    /// Returns whether all bounded bytes have been read.
    pub fn is_empty(&self) -> bool {
        self.remaining() == 0
    }

    /// Takes `count` bytes, advancing only on success.
    pub fn take(&mut self, count: usize) -> Option<&'a [u8]> {
        let end = self.position.checked_add(count)?;
        if end > self.end {
            return None;
        }
        let bytes = self.bytes.get(self.position..end)?;
        self.position = end;
        Some(bytes)
    }

    /// Takes a fixed-size byte array, advancing only on success.
    pub fn array<const N: usize>(&mut self) -> Option<[u8; N]> {
        self.take(N)?.try_into().ok()
    }

    /// Skips exactly `count` bytes.
    pub fn skip(&mut self, count: usize) -> Option<()> {
        self.take(count).map(|_| ())
    }

    /// Moves the cursor to an absolute offset inside the bounds.
    pub fn seek(&mut self, position: usize) -> Option<()> {
        (position <= self.end).then(|| self.position = position)
    }

    /// Reads the bytes at `self.position + offset` without advancing.
    pub fn peek(&self, offset: usize, count: usize) -> Option<&'a [u8]> {
        let start = self.position.checked_add(offset)?;
        let end = start.checked_add(count)?;
        (end <= self.end).then(|| self.bytes.get(start..end))?
    }

    /// Reads a byte.
    pub fn u8(&mut self) -> Option<u8> {
        self.array::<1>().map(|[value]| value)
    }

    /// Converts a declared element count into a safe `Vec` capacity.
    ///
    /// See [`bounded_len`]; `remaining` is the cursor's unread byte count.
    pub fn counted(&self, count: u64, element_size: usize) -> Option<usize> {
        bounded_len(count, element_size, self.remaining())
    }

    /// Reads `count` elements of at least `element_size` encoded bytes each.
    ///
    /// The count is validated with [`Cursor::counted`] before any
    /// allocation, and the reader closure's first failure aborts the read.
    pub fn read_counted<T>(
        &mut self,
        count: u64,
        element_size: usize,
        mut read: impl FnMut(&mut Self) -> Option<T>,
    ) -> Option<Vec<T>> {
        let count = self.counted(count, element_size)?;
        let mut values = Vec::with_capacity(count);
        for _ in 0..count {
            values.push(read(self)?);
        }
        Some(values)
    }
}

macro_rules! cursor_readers {
    ($($name:ident, $ty:ty, $conversion:ident);* $(;)?) => {
        impl Cursor<'_> {
            $(
                #[doc = concat!("Reads a `", stringify!($ty), "` via `", stringify!($conversion), "`.")]
                pub fn $name(&mut self) -> Option<$ty> {
                    self.array().map(<$ty>::$conversion)
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
    f64_le, f64, from_le_bytes;
    u16_be, u16, from_be_bytes;
    u32_be, u32, from_be_bytes;
    u64_be, u64, from_be_bytes;
    f64_be, f64, from_be_bytes;
);

#[cfg(test)]
mod tests {
    use super::{bounded_len, Cursor};

    #[test]
    fn reads_are_bounded_and_checked() {
        let payload = [1u8, 0, 2, 0, 0, 0, 9];
        let mut cursor = Cursor::new(&payload);
        assert_eq!(cursor.u16_le(), Some(1));
        assert_eq!(cursor.u32_le(), Some(2));
        assert_eq!(cursor.remaining(), 1);
        assert_eq!(cursor.u16_le(), None);
        assert_eq!(cursor.position(), 6, "failed read does not advance");
        assert_eq!(cursor.u8(), Some(9));
        assert!(cursor.is_empty());
    }

    #[test]
    fn bounds_clamp_the_window() {
        let payload = [0u8; 8];
        assert!(Cursor::with_bounds(&payload, 6, 5).is_none());
        assert!(Cursor::with_bounds(&payload, 2, 9).is_none());
        let mut cursor = Cursor::with_bounds(&payload, 2, 6).expect("valid fixture bounds");
        assert_eq!(cursor.remaining(), 4);
        assert_eq!(cursor.take(5), None);
        assert_eq!(cursor.take(4), Some(&payload[2..6]));
    }

    #[test]
    fn counted_rejects_impossible_counts() {
        let payload = [0u8; 40];
        let cursor = Cursor::new(&payload);
        assert_eq!(cursor.counted(10, 4), Some(10));
        assert_eq!(cursor.counted(11, 4), None);
        assert_eq!(cursor.counted(u64::from(u32::MAX), 4), None);
        assert_eq!(cursor.counted(1, 0), None);
        assert_eq!(bounded_len(u64::MAX, 20, usize::MAX), None);
    }

    #[test]
    fn read_counted_allocates_only_plausible_lengths() {
        let payload = [1u8, 0, 0, 0, 2, 0, 0, 0];
        let mut cursor = Cursor::new(&payload);
        let values = cursor
            .read_counted(2, 4, Cursor::u32_le)
            .expect("two fixture values");
        assert_eq!(values, [1, 2]);
        let mut cursor = Cursor::new(&payload);
        assert_eq!(
            cursor.read_counted(u64::from(u32::MAX), 4, Cursor::u32_le),
            None
        );
    }
}
