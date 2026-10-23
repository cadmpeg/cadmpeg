// SPDX-License-Identifier: Apache-2.0
//! `.pvs` structure members.

use cadmpeg_core::decode::View;

/// Leading member bytes that carry the structure signature.
const SIGNATURE_LEN: usize = 10;

/// Structure-member generation named by the member's leading signature.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Generation {
    /// Signature `PV-PVS0301`.
    Pvs0301,
    /// Signature `PV-PVS0304`.
    Pvs0304,
}

impl Generation {
    const ALL: [Self; 2] = [Self::Pvs0301, Self::Pvs0304];

    /// The ASCII signature at the start of the member.
    pub(crate) const fn signature(self) -> &'static str {
        match self {
            Self::Pvs0301 => "PV-PVS0301",
            Self::Pvs0304 => "PV-PVS0304",
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
            member.extend_from_slice(b"\0\x01 body");
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
            b"PV-PVS0302 body".as_slice(),
            b"PV-PVS030",
            b"DIV-OL09BL body",
            b"",
        ] {
            assert_eq!(generation(View::over_retained(member)), None, "{member:?}");
        }
    }
}
