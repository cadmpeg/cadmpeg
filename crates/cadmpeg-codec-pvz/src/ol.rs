// SPDX-License-Identifier: Apache-2.0
//! `.ol` geometry members.

use cadmpeg_core::decode::View;

/// Leading member bytes that carry the geometry-member signature.
const SIGNATURE_LEN: usize = 10;

/// Geometry-member generation named by the member's leading signature.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Generation {
    /// Signature `DIV-OL08BL`.
    Ol08,
    /// Signature `DIV-OL09BL`.
    Ol09,
}

impl Generation {
    const ALL: [Self; 2] = [Self::Ol08, Self::Ol09];

    /// The ASCII signature at the start of the member.
    pub(crate) const fn signature(self) -> &'static str {
        match self {
            Self::Ol08 => "DIV-OL08BL",
            Self::Ol09 => "DIV-OL09BL",
        }
    }
}

/// The generation whose signature opens `member`, if any.
pub(crate) fn generation(member: View<'_>) -> Option<Generation> {
    let mut cursor = member;
    let leading = cursor.array::<SIGNATURE_LEN>()?;
    Generation::ALL
        .into_iter()
        .find(|generation| generation.signature().as_bytes() == leading)
}

#[cfg(test)]
mod tests {
    use super::{generation, Generation};
    use cadmpeg_core::decode::View;

    #[test]
    fn each_signature_selects_its_generation() {
        for expected in Generation::ALL {
            let mut member = expected.signature().as_bytes().to_vec();
            member.extend_from_slice(b"\0\x02 body");
            assert_eq!(
                generation(View::over_retained(&member)),
                Some(expected),
                "{}",
                expected.signature()
            );
        }
    }

    #[test]
    fn other_or_short_leading_bytes_select_no_generation() {
        for member in [
            b"DIV-OL07BL body".as_slice(),
            b"DIV-OL09B",
            b"PV-PVS0304 body",
            b"",
        ] {
            assert_eq!(generation(View::over_retained(member)), None, "{member:?}");
        }
    }
}
