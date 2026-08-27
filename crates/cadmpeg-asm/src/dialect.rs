// SPDX-License-Identifier: Apache-2.0
//! The `acis:` kernel-layer dialect rows, and which of them this crate's
//! record decoders are verified against.
//!
//! `docs/dialects.toml` declares the `acis:` namespace as the embedded kernel
//! layer. The rows belong here, with the decoders they describe, and the host
//! codecs — `sat`, `inventor`, `f3d` — cite them. Keeping the band in one place
//! is the point: two hosts reading the same stream must not disagree about
//! whether the grammar applied to it is the one it declares.
//!
//! # The band moves the admission, never the decode
//!
//! [`VERIFIED_ACIS_MAJORS`] are the save-format majors the Spatial ACIS record
//! decoders were built and witnessed against. A stream outside them is framed
//! and decoded exactly as one inside them: the record grammar is applied, and
//! whatever it reads is reported as it read. What the band decides is how the
//! host labels the result — [`acis_admission`] returns either
//! `Admission::Admitted` or `Admission::AdmittedUnverified` with
//! [`nearest_verified_acis`] as `nearest`, and the host charges its kernel-layer
//! recovery loss.

use cadmpeg_core::dialect::{Admission, DialectId};

/// Save-format majors the Spatial ACIS record decoders are verified against.
pub const VERIFIED_ACIS_MAJORS: [u32; 2] = [217, 218];

/// Registry row of the lower verified Spatial ACIS band.
pub const ACIS_SAVE_FORMAT_217: DialectId = DialectId::pinned("acis:save-format-217");
/// Registry row of the upper verified Spatial ACIS band.
pub const ACIS_SAVE_FORMAT_218: DialectId = DialectId::pinned("acis:save-format-218");
/// Registry row of every other Spatial ACIS binary save format.
pub const ACIS_SAVE_FORMAT_BINARY_OTHER: DialectId =
    DialectId::pinned("acis:save-format-binary-other");

/// Whether a Spatial ACIS save format is one the record decoders are verified
/// against.
///
/// A header without a readable save-format word declares no band, so it is not
/// the verified one.
#[must_use]
pub fn acis_band_verified(save_format_major: Option<u32>) -> bool {
    save_format_major.is_some_and(|major| VERIFIED_ACIS_MAJORS.contains(&major))
}

/// The verified band row whose record grammar an unverified stream is read
/// with: the nearer of the two, by declared major.
///
/// A stream declaring no band at all, or one below the lower verified major,
/// takes [`ACIS_SAVE_FORMAT_217`].
#[must_use]
pub fn nearest_verified_acis(save_format_major: Option<u32>) -> DialectId {
    if save_format_major.is_some_and(|major| major >= 218) {
        ACIS_SAVE_FORMAT_218
    } else {
        ACIS_SAVE_FORMAT_217
    }
}

/// Admission of a Spatial ACIS save format under the verified decoder band.
#[must_use]
pub fn acis_admission(save_format_major: Option<u32>) -> Admission {
    if acis_band_verified(save_format_major) {
        Admission::Admitted
    } else {
        Admission::AdmittedUnverified {
            nearest: nearest_verified_acis(save_format_major),
        }
    }
}

/// The `acis:` binary row one save format satisfies.
#[must_use]
pub fn acis_binary_row(save_format_major: Option<u32>) -> DialectId {
    match save_format_major {
        Some(217) => ACIS_SAVE_FORMAT_217,
        Some(218) => ACIS_SAVE_FORMAT_218,
        _ => ACIS_SAVE_FORMAT_BINARY_OTHER,
    }
}

/// Registry row of an ASM binary stream at four-byte reference width.
pub const ACIS_ASM_BINARYFILE_4: DialectId = DialectId::pinned("acis:asm-binaryfile-4");
/// Registry row of an ASM binary stream at eight-byte reference width.
pub const ACIS_ASM_BINARYFILE_8: DialectId = DialectId::pinned("acis:asm-binaryfile-8");

/// The `acis:` ASM binary row one reference width satisfies.
///
/// The ASM record decoders compare no save format, so this row carries no band
/// and is always admitted.
#[must_use]
pub fn asm_binary_row(width: u8) -> DialectId {
    if width == 4 {
        ACIS_ASM_BINARYFILE_4
    } else {
        ACIS_ASM_BINARYFILE_8
    }
}

#[cfg(test)]
mod tests {
    use super::{
        acis_admission, acis_band_verified, acis_binary_row, nearest_verified_acis,
        ACIS_SAVE_FORMAT_217, ACIS_SAVE_FORMAT_218, ACIS_SAVE_FORMAT_BINARY_OTHER,
    };
    use cadmpeg_core::dialect::Admission;

    #[test]
    fn only_the_two_witnessed_majors_are_verified() {
        for major in [None, Some(7), Some(216), Some(219), Some(700)] {
            assert!(!acis_band_verified(major), "{major:?}");
            assert_eq!(acis_binary_row(major), ACIS_SAVE_FORMAT_BINARY_OTHER);
        }
        assert!(acis_band_verified(Some(217)));
        assert!(acis_band_verified(Some(218)));
        assert_eq!(acis_binary_row(Some(217)), ACIS_SAVE_FORMAT_217);
        assert_eq!(acis_binary_row(Some(218)), ACIS_SAVE_FORMAT_218);
    }

    #[test]
    fn the_nearest_verified_band_is_the_nearer_major() {
        for major in [None, Some(7), Some(216), Some(217)] {
            assert_eq!(nearest_verified_acis(major), ACIS_SAVE_FORMAT_217);
        }
        for major in [Some(218), Some(232), Some(700)] {
            assert_eq!(nearest_verified_acis(major), ACIS_SAVE_FORMAT_218);
        }
    }

    #[test]
    fn admission_folds_the_verified_band_and_nearest_row() {
        assert_eq!(acis_admission(Some(217)), Admission::Admitted);
        assert_eq!(
            acis_admission(Some(700)),
            Admission::AdmittedUnverified {
                nearest: ACIS_SAVE_FORMAT_218
            }
        );
    }
}
