//! Bounded byte cursor for CATIA record payloads.
//!
//! The cursor is the shared reader the per-family scan loops migrate onto.
//! In this phase it backs the compact-int and reference-token readers
//! (`object_ref`, `compact_uint`); the finite-checked scalar and compound
//! reads (`f64`, `point3`, `unit3`, `range`, `positive`) carry the target API
//! shape for later phases and are not yet wired into the scan loops.

use cadmpeg_ir::math::{Point3, Vector3};

/// A cursor over a CATIA record payload, tracking an absolute byte offset.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Cursor<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> Cursor<'a> {
    /// Creates a cursor positioned at `position` within `bytes`.
    pub(crate) fn new_at(bytes: &'a [u8], position: usize) -> Self {
        Self { bytes, position }
    }

    /// Returns the absolute cursor offset.
    pub(crate) fn position(&self) -> usize {
        self.position
    }

    /// Reads the reference token at the cursor, advancing past it.
    ///
    /// `extended` selects the token dialect. The restricted dialect (used by
    /// `e5`) recognises the lead bytes `0x38`, `0x18`, `0x10`, `0x08` and any
    /// `0x80..=0xff`. The extended dialect (used by `b5`) additionally
    /// recognises `0x30`, `0x28`, and `0x20`. See `wire::object_ref`.
    pub(crate) fn object_ref(&mut self, extended: bool) -> Option<u32> {
        let lead = *self.bytes.get(self.position)?;
        let get = |offset: usize| self.bytes.get(self.position + offset).copied();
        let (value, width) = match lead {
            0x38 => (u32::from_le_bytes([get(1)?, get(2)?, get(3)?, 0]), 4),
            0x30 if extended => (u32::from(u16::from_le_bytes([get(1)?, get(2)?])) << 8, 3),
            0x28 if extended => (u32::from(get(1)?) | (u32::from(get(2)?) << 16), 3),
            0x20 if extended => (u32::from(get(1)?) << 16, 2),
            0x18 => (u32::from(u16::from_le_bytes([get(1)?, get(2)?])), 3),
            0x10 => (u32::from(get(1)?) << 8, 2),
            0x08 => (u32::from(get(1)?), 2),
            0x80..=0xff => (u32::from(lead - 0x80), 1),
            _ => return None,
        };
        self.position += width;
        Some(value)
    }

    /// Reads the compact unsigned integer at the cursor, advancing past it.
    ///
    /// A lead byte with `lead % 4 == 1` encodes `(lead - 1) / 4` in one byte.
    /// A nonzero lead with `lead % 4 == 0` encodes a `lead / 4`-byte
    /// little-endian value (width at most four). See `wire::compact_uint`.
    pub(crate) fn compact_uint(&mut self) -> Option<u32> {
        let lead = *self.bytes.get(self.position)?;
        if lead % 4 == 1 {
            self.position += 1;
            Some(u32::from((lead - 1) / 4))
        } else if lead != 0 && lead % 4 == 0 {
            let width = usize::from(lead / 4);
            if width > 4 {
                return None;
            }
            let mut value = 0u32;
            for (shift, byte) in self
                .bytes
                .get(self.position + 1..self.position + 1 + width)?
                .iter()
                .enumerate()
            {
                value |= u32::from(*byte) << (8 * shift);
            }
            self.position += width + 1;
            Some(value)
        } else {
            None
        }
    }
}

/// Compound reads carrying the target `wire::Cursor` API shape.
///
/// These are not yet wired into the family scan loops (that migration is a
/// later phase), so they have no in-crate callers during this phase.
#[allow(dead_code)]
impl Cursor<'_> {
    fn take(&mut self, count: usize) -> Option<&[u8]> {
        let end = self.position.checked_add(count)?;
        let bytes = self.bytes.get(self.position..end)?;
        self.position = end;
        Some(bytes)
    }

    /// Reads an eight-byte little-endian `f64` without a finiteness check.
    fn f64_raw(&mut self) -> Option<f64> {
        Some(f64::from_le_bytes(self.take(8)?.try_into().ok()?))
    }

    /// Reads a finite eight-byte little-endian `f64`, rejecting NaN/infinity.
    fn f64(&mut self) -> Option<f64> {
        let value = self.f64_raw()?;
        value.is_finite().then_some(value)
    }

    /// Reads three finite `f64` components as a point.
    fn point3(&mut self) -> Option<Point3> {
        Some(Point3::new(self.f64()?, self.f64()?, self.f64()?))
    }

    /// Reads three finite `f64` components and normalises them to a unit
    /// direction, failing on a degenerate (near-zero-length) vector.
    fn unit3(&mut self) -> Option<Vector3> {
        Vector3::new(self.f64()?, self.f64()?, self.f64()?).unit()
    }

    /// Reads two finite `f64` values as a `[start, end]` pair. Ordering is
    /// not enforced; callers that need an increasing range check it.
    fn range(&mut self) -> Option<[f64; 2]> {
        Some([self.f64()?, self.f64()?])
    }

    /// Reads a finite, strictly positive `f64`.
    fn positive(&mut self) -> Option<f64> {
        let value = self.f64()?;
        (value > 0.0).then_some(value)
    }
}

#[cfg(test)]
mod tests {
    use super::Cursor;

    #[test]
    fn object_ref_extended_reads_all_dialect_leads() {
        let mut position = 0;
        let mut cursor = Cursor::new_at(&[0x28, 0x34, 0x02], position);
        assert_eq!(cursor.object_ref(true), Some(0x02_0034));
        position = cursor.position();
        assert_eq!(position, 3);
    }

    #[test]
    fn object_ref_restricted_rejects_extended_only_leads() {
        // 0x30 is an extended-only lead; the restricted dialect rejects it.
        assert_eq!(
            Cursor::new_at(&[0x30, 0x07, 0x00], 0).object_ref(false),
            None
        );
        // Shared leads still decode under the restricted dialect.
        assert_eq!(Cursor::new_at(&[0x8b], 0).object_ref(false), Some(11));
    }

    #[test]
    fn compact_uint_matches_single_and_multi_byte_encodings() {
        assert_eq!(Cursor::new_at(&[0x05], 0).compact_uint(), Some(1));
        let mut cursor = Cursor::new_at(&[0x08, 0x2a, 0x00], 0);
        assert_eq!(cursor.compact_uint(), Some(42));
        assert_eq!(cursor.position(), 3);
    }
}
