// SPDX-License-Identifier: Apache-2.0
//! Nonempty class-0x62 counted-owner tail bytes.

use cadmpeg_core::decode::View;
use cadmpeg_ir::native::bytes::NativeBytes;

#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct CountedOwnerBounds {
    pub(crate) lower: [f64; 2],
    pub(crate) upper: [f64; 2],
    pub(crate) bounds: [[f32; 2]; 3],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CountedOwnerTail(NativeBytes);

impl CountedOwnerTail {
    pub(crate) fn new(bytes: Vec<u8>) -> Option<Self> {
        (!bytes.is_empty()).then_some(Self(bytes.into()))
    }
    pub(crate) fn as_slice(&self) -> &[u8] {
        &self.0
    }

    pub(crate) fn numeric_bounds(&self, reference_count: usize) -> Option<CountedOwnerBounds> {
        if reference_count < 5 || reference_count.is_multiple_of(2) {
            return None;
        }
        let bytes = self.as_slice();
        let header_len = match bytes.len() {
            62 => 5,
            63 => 6,
            _ => return None,
        };
        let pair_count = u8::try_from(reference_count / 2).ok()?;
        if bytes.first().copied()? != 0x80u8.checked_add(pair_count)?
            || !matches!(bytes.get(1), Some(0x41 | 0xc1))
            || bytes.get(header_len - 1) != Some(&0x0d)
        {
            return None;
        }
        let mut view = View::over_retained(bytes);
        view.seek(header_len)?;
        let lower = [view.f64_le()?, view.f64_le()?];
        let upper = [view.f64_le()?, view.f64_le()?];
        if view.u8()? != 1 {
            return None;
        }
        let mut bounds = [[0.0; 2]; 3];
        for bound in &mut bounds {
            *bound = [view.f32_le()?, view.f32_le()?];
        }
        if (0..2).any(|i| !lower[i].is_finite() || !upper[i].is_finite() || lower[i] >= upper[i])
            || bounds
                .iter()
                .any(|[a, b]| !a.is_finite() || !b.is_finite() || a >= b)
        {
            return None;
        }
        Some(CountedOwnerBounds {
            lower,
            upper,
            bounds,
        })
    }
}

impl serde::Serialize for CountedOwnerTail {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(serializer)
    }
}

impl<'de> serde::Deserialize<'de> for CountedOwnerTail {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::new(NativeBytes::<Vec<u8>>::deserialize(deserializer)?.into_inner())
            .ok_or_else(|| serde::de::Error::custom("counted-owner tail is empty"))
    }
}

#[cfg(test)]
mod tests {
    use super::CountedOwnerTail;

    fn tail(header: &[u8]) -> CountedOwnerTail {
        let mut bytes = header.to_vec();
        for value in [0.0_f64, -1.0, 2.0, 3.0] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.push(1);
        for value in [-2.0_f32, 2.0, -3.0, 3.0, -4.0, 4.0] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        CountedOwnerTail::new(bytes).expect("nonempty tail")
    }

    #[test]
    fn numeric_bounds_require_matching_count_and_complete_header() {
        for header in [
            &[0x85, 0x41, 1, 2, 0x0d][..],
            &[0x85, 0x41, 1, 2, 3, 0x0d][..],
        ] {
            let tail = tail(header);
            let bounds = tail.numeric_bounds(11).expect("valid numeric tail");
            assert!(bounds.lower[0].abs() < 1.0e-12);
            assert!((bounds.lower[1] + 1.0).abs() < 1.0e-12);
            assert!((bounds.upper[0] - 2.0).abs() < 1.0e-12);
            assert!((bounds.bounds[2][1] - 4.0).abs() < f32::EPSILON);
            assert!(tail.numeric_bounds(13).is_none());
            assert!(tail.numeric_bounds(10).is_none());
        }
        assert!(tail(&[0x85, 0x41, 1, 2, 0x05]).numeric_bounds(11).is_none());
    }
}
