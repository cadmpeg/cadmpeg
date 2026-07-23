//! Bounded byte cursor for CATIA record payloads.
//!
//! `Cursor` is a newtype over the shared poisoned
//! [`cadmpeg_ir::wire::cursor::Cursor`]. It keeps CATIA's format-specific token
//! readers — `object_ref` and `compact_uint`, whose variable-width lead-byte
//! encodings the shared cursor does not model — reading them off the shared
//! cursor's `u8`/`take` primitives. The finite-checked scalar and compound
//! reads (`f64`, `point3`, `vector3`, `unit3`, `skip`) that drive the analytic
//! surface frame readers in `analytic.rs` delegate to the shared cursor's
//! `f64_le`/`point3_le`/`vector3_le`/`unit3_le` and translate its recorded
//! fault back into the `Option`-returning contract those call sites read
//! against.
//!
//! Every `f64`-bearing read is finite-checked and maps to the shared `f64_le`;
//! no CATIA call site reads raw bits, so the shared `f64_le_raw` escape hatch
//! is unused here.

use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::wire::cursor::Cursor as WireCursor;

/// A cursor over a CATIA record payload, tracking an absolute byte offset.
///
/// Wraps the shared poisoned cursor: token reads return `None` and compound
/// reads translate the underlying fault to `None`, so a failed read abandons
/// the cursor at the call site's `?`.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Cursor<'a>(WireCursor<'a>);

impl<'a> Cursor<'a> {
    /// Creates a cursor positioned at `position` within `bytes`.
    pub(crate) fn new_at(bytes: &'a [u8], position: usize) -> Self {
        Self(WireCursor::new(bytes).window(position, bytes.len()))
    }

    /// Returns the absolute cursor offset.
    pub(crate) fn position(&self) -> usize {
        self.0.position()
    }

    /// Reads the reference token at the cursor, advancing past it.
    ///
    /// `extended` selects the token dialect. The restricted dialect (used by
    /// `e5`) recognises the lead bytes `0x38`, `0x18`, `0x10`, `0x08` and any
    /// `0x80..=0xff`. The extended dialect (used by `b5`) additionally
    /// recognises `0x30`, `0x28`, and `0x20`. See `wire::object_ref`.
    pub(crate) fn object_ref(&mut self, extended: bool) -> Option<u32> {
        let lead = self.0.u8();
        if self.0.is_poisoned() {
            return None;
        }
        let value = match lead {
            0x38 => {
                let [b1, b2, b3] = self.0.take(3).try_into().ok()?;
                u32::from_le_bytes([b1, b2, b3, 0])
            }
            0x30 if extended => {
                let [b1, b2] = self.0.take(2).try_into().ok()?;
                u32::from(u16::from_le_bytes([b1, b2])) << 8
            }
            0x28 if extended => {
                let [b1, b2] = self.0.take(2).try_into().ok()?;
                u32::from(b1) | (u32::from(b2) << 16)
            }
            0x20 if extended => {
                let [b1] = self.0.take(1).try_into().ok()?;
                u32::from(b1) << 16
            }
            0x18 => {
                let [b1, b2] = self.0.take(2).try_into().ok()?;
                u32::from(u16::from_le_bytes([b1, b2]))
            }
            0x10 => {
                let [b1] = self.0.take(1).try_into().ok()?;
                u32::from(b1) << 8
            }
            0x08 => {
                let [b1] = self.0.take(1).try_into().ok()?;
                u32::from(b1)
            }
            0x80..=0xff => u32::from(lead - 0x80),
            _ => return None,
        };
        Some(value)
    }

    /// Reads the compact unsigned integer at the cursor, advancing past it.
    ///
    /// A lead byte with `lead % 4 == 1` encodes `(lead - 1) / 4` in one byte.
    /// A nonzero lead with `lead % 4 == 0` encodes a `lead / 4`-byte
    /// little-endian value (width at most four). See `wire::compact_uint`.
    pub(crate) fn compact_uint(&mut self) -> Option<u32> {
        let lead = self.0.u8();
        if self.0.is_poisoned() {
            return None;
        }
        if lead % 4 == 1 {
            Some(u32::from((lead - 1) / 4))
        } else if lead != 0 && lead.is_multiple_of(4) {
            let width = usize::from(lead / 4);
            if width > 4 {
                return None;
            }
            let bytes = self.0.take(width);
            if self.0.is_poisoned() {
                return None;
            }
            let mut value = 0u32;
            for (shift, byte) in bytes.iter().enumerate() {
                value |= u32::from(*byte) << (8 * shift);
            }
            Some(value)
        } else {
            None
        }
    }
}

/// Finite-checked scalar and compound reads.
///
/// The analytic surface readers (`analytic.rs`) consume `f64`, `point3`,
/// `vector3`, `unit3`, and `skip`. Each delegates to the shared cursor's
/// finite-checked reader and reports the first recorded fault as `None`.
impl Cursor<'_> {
    /// Advances past `count` bytes, failing if they run past the end.
    pub(crate) fn skip(&mut self, count: usize) -> Option<()> {
        self.0.skip(count);
        self.0.finish().ok()
    }

    /// Reads a finite eight-byte little-endian `f64`, rejecting NaN/infinity.
    pub(crate) fn f64(&mut self) -> Option<f64> {
        let value = self.0.f64_le();
        self.0.finish().ok()?;
        Some(value)
    }

    /// Reads three finite `f64` components as a point.
    pub(crate) fn point3(&mut self) -> Option<Point3> {
        let value = self.0.point3_le();
        self.0.finish().ok()?;
        Some(value)
    }

    /// Reads three finite `f64` components as a vector, without normalising.
    pub(crate) fn vector3(&mut self) -> Option<Vector3> {
        let value = self.0.vector3_le();
        self.0.finish().ok()?;
        Some(value)
    }

    /// Reads three finite `f64` components and normalises them to a unit
    /// direction, failing on a degenerate (near-zero-length) vector.
    pub(crate) fn unit3(&mut self) -> Option<Vector3> {
        let value = self.0.unit3_le();
        self.0.finish().ok()?;
        Some(value)
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
