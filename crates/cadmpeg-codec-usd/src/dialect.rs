// SPDX-License-Identifier: Apache-2.0
//! Layer dialect identity: which registry row a layer is, and how it was
//! admitted.
//!
//! The discriminant is the header (spec §3.1, §9): the version token of a
//! text header, or the version bytes of a binary bootstrap header. [`classify`]
//! is the single construction path for a [`DialectMatch`] in this codec.

use cadmpeg_core::dialect::{DialectId, DialectLayers, DialectMatch};
use cadmpeg_core::CodecError;
use cadmpeg_ir::report::loss::LossNote;

use crate::detect::{Cookie, Header};
use crate::loss::UsdLossCode;

include!("dialect/registry_ids.rs");

/// Key of the text header version in [`DialectMatch::declared`].
const DECLARED_HEADER: &str = "header";
/// Key of the binary version in [`DialectMatch::declared`].
const DECLARED_VERSION: &str = "version";
/// Key of the root layer member of a package in [`DialectMatch::declared`].
const DECLARED_ROOT: &str = "root";

/// A completed classification whose recovery state cannot drift from its
/// match.
pub(crate) struct DialectClassification(State);

enum State {
    /// A declared version: the layer reads with its grammar or the crate
    /// reader.
    Admitted(DialectMatch),
    /// An undeclared text version: the residual identity.
    Residual(DialectMatch),
    /// A binary layer of an undeclared version: identified and refused.
    Refused(DialectMatch),
}

fn text_row(cookie: Cookie, version: &str) -> Option<DialectId> {
    Some(match (cookie, version) {
        (Cookie::Usda, "1.0") => USD_USDA_1_0,
        (Cookie::Usda, "1.1") => USD_USDA_1_1,
        (Cookie::Usda, "1.2") => USD_USDA_1_2,
        (Cookie::Usda, "1.3") => USD_USDA_1_3,
        (Cookie::Sdf, "1.4.32") => USD_SDF_1_4_32,
        _ => return None,
    })
}

fn binary_row(version: [u8; 3]) -> Option<DialectId> {
    Some(match version {
        [0, 0, 1] => USD_USDC_0_0_1,
        [0, 1, 0] => USD_USDC_0_1_0,
        [0, 2, 0] => USD_USDC_0_2_0,
        [0, 4, 0] => USD_USDC_0_4_0,
        [0, 5, 0] => USD_USDC_0_5_0,
        [0, 6, 0] => USD_USDC_0_6_0,
        [0, 7, 0] => USD_USDC_0_7_0,
        [0, 8, 0] => USD_USDC_0_8_0,
        [0, 9, 0] => USD_USDC_0_9_0,
        [0, 10, 0] => USD_USDC_0_10_0,
        [0, 11, 0] => USD_USDC_0_11_0,
        [0, 12, 0] => USD_USDC_0_12_0,
        [0, 13, 0] => USD_USDC_0_13_0,
        [0, 14, 0] => USD_USDC_0_14_0,
        [0, 15, 0] => USD_USDC_0_15_0,
        _ => return None,
    })
}

/// The match of a package whose root layer is the member `root`.
pub(crate) fn package(root: &str) -> DialectMatch {
    DialectMatch::admitted(USD_USDZ)
        .with_declared_entry(cadmpeg_core::nonblank_const!(DECLARED_ROOT), root)
}

/// Classifies a layer from its header.
pub(crate) fn classify(header: &Header) -> DialectClassification {
    match header {
        Header::Text { cookie, version } => {
            let declared = format!("{} {version}", cookie.text());
            let key = cadmpeg_core::nonblank_const!(DECLARED_HEADER);
            match text_row(*cookie, version) {
                Some(row) => DialectClassification(State::Admitted(
                    DialectMatch::admitted(row).with_declared_entry(key, &declared),
                )),
                None => DialectClassification(State::Residual(
                    DialectMatch::residual(USD_UNKNOWN).with_declared_entry(key, &declared),
                )),
            }
        }
        Header::Binary { version } => {
            let [major, minor, patch] = *version;
            let declared = format!("{major}.{minor}.{patch}");
            let key = cadmpeg_core::nonblank_const!(DECLARED_VERSION);
            match binary_row(*version) {
                Some(row) => DialectClassification(State::Admitted(
                    DialectMatch::admitted(row).with_declared_entry(key, &declared),
                )),
                None => DialectClassification(State::Refused(
                    DialectMatch::refused(USD_UNKNOWN).with_declared_entry(key, &declared),
                )),
            }
        }
    }
}

impl DialectClassification {
    /// The admitted or residual match of a layer, or the refusal of a
    /// binary layer of an undeclared version.
    pub(crate) fn admit(self) -> Result<(DialectMatch, Option<LossNote>), CodecError> {
        match self.0 {
            State::Admitted(matched) => Ok((matched, None)),
            State::Residual(matched) => Ok((
                matched,
                Some(UsdLossCode::SourceDialectUnverified.note(
                    "the text header names no declared USDA version; the layer was read with \
                     the USDA 1.3 grammar",
                )),
            )),
            State::Refused(matched) => Err(CodecError::UnsupportedDialect {
                dialects: Box::new(DialectLayers::of(matched)),
                message: "the USDC bootstrap header names no declared crate version".to_owned(),
            }),
        }
    }
}

#[cfg(test)]
mod tests;
