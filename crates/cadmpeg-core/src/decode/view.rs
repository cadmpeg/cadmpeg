// SPDX-License-Identifier: Apache-2.0
//! Bounded navigation over one address space.
#![warn(clippy::indexing_slicing, clippy::arithmetic_side_effects)]

use super::error::SourceLocation;
use super::probe::{ParseError, ParseErrorKind};
use super::space::SpaceId;

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

/// A count proven to fit in the unread input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BoundedCount(usize);

impl BoundedCount {
    /// Returns the proven element count.
    pub fn get(self) -> usize {
        self.0
    }
}

/// A bounded window over one address space.
#[derive(Debug, Clone, Copy)]
pub struct View<'a> {
    bytes: &'a [u8],
    space: SpaceId,
    start: usize,
    end: usize,
    position: usize,
}

impl<'a> View<'a> {
    /// Creates a full-window view over an entire space buffer.
    pub(crate) fn over_space(bytes: &'a [u8], space: SpaceId) -> View<'a> {
        View {
            bytes,
            space,
            start: 0,
            end: bytes.len(),
            position: 0,
        }
    }

    /// Bounded window over retained bytes that left the live space chain.
    ///
    /// Prefer views returned by [`DecodeContext`](super::DecodeContext). This
    /// constructor exists for opportunistic scanners over owned payloads that
    /// were copied out of the arena; locations report [`SpaceId::ROOT`].
    pub fn over_retained(bytes: &'a [u8]) -> View<'a> {
        Self::over_space(bytes, SpaceId::ROOT)
    }

    /// Returns the space this view navigates.
    pub(crate) fn space(self) -> SpaceId {
        self.space
    }

    /// Returns the absolute position within the space.
    pub fn position(self) -> usize {
        self.position
    }

    /// Returns the window's inclusive lower bound.
    pub fn start(self) -> usize {
        self.start
    }

    /// Returns the window's exclusive upper bound.
    pub fn end(self) -> usize {
        self.end
    }

    /// Returns the number of unread bytes before the window's end.
    pub fn remaining(self) -> usize {
        self.end.saturating_sub(self.position)
    }

    /// Returns whether all bounded bytes have been read.
    pub fn is_empty(self) -> bool {
        self.remaining() == 0
    }

    /// Returns this view's current source location.
    pub fn location(self) -> SourceLocation {
        SourceLocation {
            space: self.space,
            offset: self.position as u64,
        }
    }

    /// Returns a source location at an absolute offset in this view's space.
    pub fn location_at(self, offset: usize) -> SourceLocation {
        SourceLocation {
            space: self.space,
            offset: offset as u64,
        }
    }

    /// Returns the readable window `start..end` as a slice.
    pub fn window(self) -> &'a [u8] {
        self.bytes.get(self.start..self.end).unwrap_or_default()
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

    /// Moves to an absolute offset, honoring the stored lower bound.
    pub fn seek(&mut self, position: usize) -> Option<()> {
        (self.start <= position && position <= self.end).then(|| self.position = position)
    }

    /// Reads a single byte.
    pub fn u8(&mut self) -> Option<u8> {
        self.array::<1>().map(|[value]| value)
    }

    /// Returns an exactly contained child window.
    pub fn child(self, start: usize, end: usize) -> Option<View<'a>> {
        if self.start <= start && start <= end && end <= self.end {
            Some(View {
                bytes: self.bytes,
                space: self.space,
                start,
                end,
                position: start,
            })
        } else {
            None
        }
    }

    /// Proves a declared element count could fit in the unread bytes.
    pub fn counted(self, count: u64, min_element_size: usize) -> Option<BoundedCount> {
        bounded_len(count, min_element_size, self.remaining()).map(BoundedCount)
    }

    /// Reads `count` elements of at least `element_size` encoded bytes each.
    ///
    /// The count is validated with [`View::counted`] before any allocation, and
    /// the reader closure's first failure aborts the read.
    pub fn read_counted<T>(
        &mut self,
        count: u64,
        element_size: usize,
        mut read: impl FnMut(&mut Self) -> Option<T>,
    ) -> Option<Vec<T>> {
        let count = self.counted(count, element_size)?.get();
        let mut values = Vec::new();
        values.try_reserve_exact(count).ok()?;
        for _ in 0..count {
            values.push(read(self)?);
        }
        Some(values)
    }

    /// Decodes `count` UTF-16LE code units from the current position.
    ///
    /// Strict `from_utf16`; unpaired surrogates and a truncated window yield
    /// `None`. Does not advance on failure.
    pub fn utf16_le(&mut self, count: usize) -> Option<String> {
        let units = self.read_counted(u64::try_from(count).ok()?, 2, View::u16_le)?;
        String::from_utf16(&units).ok()
    }

    /// Decodes `count` UTF-16LE code units at `offset` and returns the string
    /// and end offset.
    ///
    /// Sequential readers should keep a live `View` and call [`View::utf16_le`]
    /// instead.
    pub fn utf16le_at(bytes: &[u8], offset: usize, count: usize) -> Option<(String, usize)> {
        let mut view = View::over_retained(bytes);
        view.seek(offset)?;
        let units = view.read_counted(u64::try_from(count).ok()?, 2, View::u16_le)?;
        Some((String::from_utf16(&units).ok()?, view.position()))
    }

    /// Builds an unexpected-eof error from the view's current state.
    fn eof(self, needed: u64) -> ParseError {
        ParseError {
            location: self.location(),
            kind: ParseErrorKind::UnexpectedEof {
                needed,
                remaining: self.remaining() as u64,
            },
            operation: "required_read",
        }
    }

    /// Required-read mirror of [`View::take`].
    pub fn req_take(&mut self, count: usize) -> Result<&'a [u8], ParseError> {
        match self.take(count) {
            Some(bytes) => Ok(bytes),
            None => Err(self.eof(count as u64)),
        }
    }

    /// Required-read mirror of [`View::u8`].
    pub fn req_u8(&mut self) -> Result<u8, ParseError> {
        match self.u8() {
            Some(value) => Ok(value),
            None => Err(self.eof(1)),
        }
    }
}

macro_rules! view_readers {
    ($(($probe:ident, $req:ident, $at:ident, $ty:ty, $conv:ident, $size:literal)),* $(,)?) => {
        impl View<'_> {
            $(
                #[doc = concat!("Probe read of a `", stringify!($ty), "` via `", stringify!($conv), "`.")]
                pub fn $probe(&mut self) -> Option<$ty> {
                    self.array::<$size>().map(<$ty>::$conv)
                }

                #[doc = concat!("Required-read mirror of [`View::", stringify!($probe), "`].")]
                pub fn $req(&mut self) -> Result<$ty, ParseError> {
                    match self.array::<$size>() {
                        Some(bytes) => Ok(<$ty>::$conv(bytes)),
                        None => Err(self.eof($size)),
                    }
                }

                #[doc = concat!(
                    "Offset probe of a `",
                    stringify!($ty),
                    "` over retained bytes. Sequential readers should keep a live `View` and call [`View::",
                    stringify!($probe),
                    "`] instead."
                )]
                pub fn $at(bytes: &[u8], offset: usize) -> Option<$ty> {
                    let mut view = View::over_retained(bytes);
                    view.seek(offset)?;
                    view.$probe()
                }
            )*
        }
    };
}

view_readers!(
    (u16_le, req_u16_le, u16_le_at, u16, from_le_bytes, 2),
    (i16_le, req_i16_le, i16_le_at, i16, from_le_bytes, 2),
    (u32_le, req_u32_le, u32_le_at, u32, from_le_bytes, 4),
    (i32_le, req_i32_le, i32_le_at, i32, from_le_bytes, 4),
    (u64_le, req_u64_le, u64_le_at, u64, from_le_bytes, 8),
    (i64_le, req_i64_le, i64_le_at, i64, from_le_bytes, 8),
    (f32_le, req_f32_le, f32_le_at, f32, from_le_bytes, 4),
    (f64_le, req_f64_le, f64_le_at, f64, from_le_bytes, 8),
    (u16_be, req_u16_be, u16_be_at, u16, from_be_bytes, 2),
    (i16_be, req_i16_be, i16_be_at, i16, from_be_bytes, 2),
    (u32_be, req_u32_be, u32_be_at, u32, from_be_bytes, 4),
    (i32_be, req_i32_be, i32_be_at, i32, from_be_bytes, 4),
    (u64_be, req_u64_be, u64_be_at, u64, from_be_bytes, 8),
    (i64_be, req_i64_be, i64_be_at, i64, from_be_bytes, 8),
    (f32_be, req_f32_be, f32_be_at, f32, from_be_bytes, 4),
    (f64_be, req_f64_be, f64_be_at, f64, from_be_bytes, 8),
);

#[cfg(test)]
mod tests {
    use super::{bounded_len, BoundedCount, View};
    use crate::decode::space::SpaceId;

    #[test]
    fn read_counted_allocates_only_plausible_lengths() {
        let payload = [1u8, 0, 0, 0, 2, 0, 0, 0];
        let mut view = View::over_space(&payload, SpaceId::ROOT);
        let values = view
            .read_counted(2, 4, View::u32_le)
            .expect("two fixture values");
        assert_eq!(values, [1, 2]);
        let mut view = View::over_space(&payload, SpaceId::ROOT);
        assert_eq!(
            view.read_counted(u64::from(u32::MAX), 4, View::u32_le),
            None
        );
    }

    #[test]
    fn counted_rejects_impossible_counts() {
        let payload = [0u8; 40];
        let view = View::over_space(&payload, SpaceId::ROOT);
        assert_eq!(view.counted(10, 4).map(BoundedCount::get), Some(10));
        assert_eq!(view.counted(11, 4), None);
        assert_eq!(bounded_len(u64::MAX, 20, usize::MAX), None);
    }

    #[test]
    fn seek_honors_lower_bound() {
        let payload = [0u8; 8];
        let mut view = View::over_space(&payload, SpaceId::ROOT)
            .child(2, 6)
            .expect("valid child");
        assert_eq!(view.seek(0), None);
        assert_eq!(view.seek(2), Some(()));
        assert_eq!(view.seek(6), Some(()));
        assert_eq!(view.seek(7), None);
    }

    #[test]
    fn offset_helpers_match_live_reads() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&0x0102_u16.to_le_bytes());
        bytes.extend_from_slice(&0x0304_u16.to_be_bytes());
        bytes.extend_from_slice(&(-5_i16).to_be_bytes());
        bytes.extend_from_slice(&1.5_f32.to_le_bytes());
        assert_eq!(View::u16_le_at(&bytes, 0), Some(0x0102));
        assert_eq!(View::u16_be_at(&bytes, 2), Some(0x0304));
        assert_eq!(View::i16_be_at(&bytes, 4), Some(-5));
        assert_eq!(View::f32_le_at(&bytes, 6), Some(1.5));
        assert_eq!(View::u16_le_at(&bytes, bytes.len()), None);
        let mut view = View::over_space(&bytes, SpaceId::ROOT);
        assert_eq!(view.u16_le(), Some(0x0102));
        assert_eq!(view.u16_be(), Some(0x0304));
        assert_eq!(view.i16_be(), Some(-5));
        assert_eq!(view.f32_le(), Some(1.5));
    }

    #[test]
    fn utf16le_matches_le_helper() {
        assert_eq!(
            View::utf16le_at(b"A\0B\0", 0, 2),
            Some(("AB".to_string(), 4))
        );
        assert_eq!(View::utf16le_at(b"A\0B\0", 0, 3), None);
        let mut view = View::over_space(b"A\0B\0", SpaceId::ROOT);
        assert_eq!(view.utf16_le(2), Some("AB".to_string()));
        assert_eq!(view.position(), 4);
    }
}
