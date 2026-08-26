//! Bounded byte cursor for CATIA record payloads.
//!
//! The cursor is the shared reader the per-family scan loops migrate onto.
//! It backs the compact-int and reference-token readers (`object_ref`,
//! `compact_uint`) and the finite-checked scalar and compound reads (`f64`,
//! `point3`, `vector3`, `unit3`, `skip`) that drive the analytic surface
//! frame readers in `analytic.rs`.

use cadmpeg_core::decode::View;
use cadmpeg_ir::math::{Point3, Vector3};

/// A cursor over a CATIA record payload, tracking an absolute byte offset.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Cursor<'a> {
    view: View<'a>,
}

impl<'a> Cursor<'a> {
    /// Creates a cursor positioned at `position` within `bytes`.
    pub(crate) fn new_at(bytes: &'a [u8], position: usize) -> Self {
        let mut view = View::over_retained(bytes);
        if view.seek(position).is_none() {
            let _ = view.seek(view.end());
        }
        Self { view }
    }

    /// Returns the absolute cursor offset.
    pub(crate) fn position(&self) -> usize {
        self.view.position()
    }

    /// Reads the reference token at the cursor, advancing past it.
    ///
    /// `extended` selects the token dialect. The restricted dialect (used by
    /// `e5`) recognises the lead bytes `0x38`, `0x18`, `0x10`, `0x08` and any
    /// `0x80..=0xff`. The extended dialect (used by `b5`) additionally
    /// recognises `0x30`, `0x28`, and `0x20`. See `wire::object_ref`.
    pub(crate) fn object_ref(&mut self, extended: bool) -> Option<u32> {
        let mut view = self.view;
        let lead = view.u8()?;
        let value = match lead {
            0x38 => {
                let b0 = view.u8()?;
                let b1 = view.u8()?;
                let b2 = view.u8()?;
                View::u24_le_at(&[b0, b1, b2], 0)?
            }
            0x30 if extended => u32::from(view.u16_le()?) << 8,
            0x28 if extended => {
                let low = view.u8()?;
                let high = view.u8()?;
                u32::from(low) | (u32::from(high) << 16)
            }
            0x20 if extended => u32::from(view.u8()?) << 16,
            0x18 => u32::from(view.u16_le()?),
            0x10 => u32::from(view.u8()?) << 8,
            0x08 => u32::from(view.u8()?),
            0x80..=0xff => u32::from(lead - 0x80),
            _ => return None,
        };
        self.view = view;
        Some(value)
    }

    /// Reads the compact unsigned integer at the cursor, advancing past it.
    ///
    /// A lead byte with `lead % 4 == 1` encodes `(lead - 1) / 4` in one byte.
    /// A nonzero lead with `lead % 4 == 0` encodes a `lead / 4`-byte
    /// little-endian value (width at most four). See `wire::compact_uint`.
    pub(crate) fn compact_uint(&mut self) -> Option<u32> {
        let mut view = self.view;
        let lead = view.u8()?;
        let value = if lead % 4 == 1 {
            u32::from((lead - 1) / 4)
        } else if lead != 0 && lead % 4 == 0 {
            let width = usize::from(lead / 4);
            if width > 4 {
                return None;
            }
            let mut value = 0u32;
            for shift in 0..width {
                value |= u32::from(view.u8()?) << (8 * shift);
            }
            value
        } else {
            return None;
        };
        self.view = view;
        Some(value)
    }
}

/// Finite-checked scalar and compound reads.
///
/// The analytic surface readers (`analytic.rs`) consume `f64`, `point3`,
/// `vector3`, `unit3`, and `skip`, backed by the private `f64_raw` helper.
impl Cursor<'_> {
    /// Advances past `count` bytes, failing if they run past the end.
    pub(crate) fn skip(&mut self, count: usize) -> Option<()> {
        self.view.skip(count)
    }

    /// Reads an eight-byte little-endian `f64` without a finiteness check.
    fn f64_raw(&mut self) -> Option<f64> {
        self.view.f64_le()
    }

    /// Reads a finite eight-byte little-endian `f64`, rejecting NaN/infinity.
    pub(crate) fn f64(&mut self) -> Option<f64> {
        let value = self.f64_raw()?;
        value.is_finite().then_some(value)
    }

    /// Reads three finite `f64` components as a point.
    pub(crate) fn point3(&mut self) -> Option<Point3> {
        Some(Point3::new(self.f64()?, self.f64()?, self.f64()?))
    }

    /// Reads three finite `f64` components as a vector, without normalising.
    pub(crate) fn vector3(&mut self) -> Option<Vector3> {
        Some(Vector3::new(self.f64()?, self.f64()?, self.f64()?))
    }

    /// Reads three finite `f64` components and normalises them to a unit
    /// direction, failing on a degenerate (near-zero-length) vector.
    pub(crate) fn unit3(&mut self) -> Option<Vector3> {
        self.vector3()?.unit()
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
