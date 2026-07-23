// SPDX-License-Identifier: Apache-2.0
//! Bounded navigation over one address space.

use crate::wire::cursor::bounded_len;

use super::error::SourceLocation;
use super::probe::{ParseError, ParseErrorKind};
use super::space::SpaceId;

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

    /// Returns this view's current source location.
    pub fn location(self) -> SourceLocation {
        SourceLocation {
            space: self.space,
            offset: self.position as u64,
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

    /// Builds an unexpected-eof error from the view's current state.
    fn eof(self, needed: u64) -> ParseError {
        ParseError {
            location: self.location(),
            kind: ParseErrorKind::UnexpectedEof {
                needed,
                remaining: self.remaining() as u64,
            },
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
    ($(($probe:ident, $req:ident, $ty:ty, $conv:ident, $size:literal)),* $(,)?) => {
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
            )*
        }
    };
}

view_readers!(
    (u16_le, req_u16_le, u16, from_le_bytes, 2),
    (i16_le, req_i16_le, i16, from_le_bytes, 2),
    (u32_le, req_u32_le, u32, from_le_bytes, 4),
    (i32_le, req_i32_le, i32, from_le_bytes, 4),
    (u64_le, req_u64_le, u64, from_le_bytes, 8),
    (i64_le, req_i64_le, i64, from_le_bytes, 8),
    (f32_le, req_f32_le, f32, from_le_bytes, 4),
    (f64_le, req_f64_le, f64, from_le_bytes, 8),
    (u16_be, req_u16_be, u16, from_be_bytes, 2),
    (u32_be, req_u32_be, u32, from_be_bytes, 4),
    (u64_be, req_u64_be, u64, from_be_bytes, 8),
    (f64_be, req_f64_be, f64, from_be_bytes, 8),
);
