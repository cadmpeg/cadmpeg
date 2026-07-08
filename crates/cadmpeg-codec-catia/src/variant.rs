// SPDX-License-Identifier: Apache-2.0
//! CATIA V5 stores its geometry in one of five distinct families, and the family
//! determines the whole decode path, so it must be identified before anything
//! else. This module names the family from container-level evidence (presence of
//! a nested container and its reconstructed BREP stream, plus a small set of
//! record-family census counts). Detection is deliberately conservative: a
//! family is only claimed when its structural invariant holds, and everything
//! else is reported as [`Variant::Unknown`] rather than guessed.

/// The five documented storage families, plus two honest "not one of the
/// decodable five" outcomes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Variant {
    /// Nested `V5_CFV2` with a `30 04 04 ff` FBB spine followed by the standard
    /// `10 24 04 ff ff 00 00 00` edge-table delimiter: the family this codec
    /// decodes geometry for.
    StandardNested,
    /// Nested `V5_CFV2` with an FBB spine and `05 08 01` vertices but no standard
    /// edge-table delimiter (its post-FBB edge rows use `u24be` handles).
    FbbOnly,
    /// No nested `V5_CFV2`; the outer preamble carries `a9 03` record families.
    ZeroEntity,
    /// A nested `V5_CFV2` with no FBB spine whose topology lives in the object
    /// stream (`b5 03` grammar) or a pure surface-marker inner body.
    FloatPackedInnerNoFbb,
    /// A coherent E5 (`E5 0D 03`) record stream carries the geometry.
    E5Stream,
    /// A nested `V5_CFV2` whose directory catalogues no BREP body (the body sits
    /// in the contiguous inner region before the directory); not decoded here.
    InnerNoDirectory,
    /// None of the decodable families' invariants held.
    Unknown,
}

impl Variant {
    /// A short, stable token for reports and container attributes.
    pub fn token(self) -> &'static str {
        match self {
            Variant::StandardNested => "standard_nested",
            Variant::FbbOnly => "fbb_only",
            Variant::ZeroEntity => "zero_entity",
            Variant::FloatPackedInnerNoFbb => "float_packed_inner_no_fbb",
            Variant::E5Stream => "e5_stream",
            Variant::InnerNoDirectory => "inner_no_directory",
            Variant::Unknown => "unknown",
        }
    }

    /// A one-line human description for container notes.
    pub fn description(self) -> &'static str {
        match self {
            Variant::StandardNested => {
                "standard nested V5_CFV2 (FBB spine + standard edge-table delimiter): geometry \
                 decode supported"
            }
            Variant::FbbOnly => {
                "FBB-only partial spine (u24be edge tables, no standard delimiter): vertex and \
                 analytic carrier decode supported"
            }
            Variant::ZeroEntity => {
                "zero-entity a9 03 (no nested container): analytic carriers decoded"
            }
            Variant::FloatPackedInnerNoFbb => {
                "float-packed inner-no-FBB (object-stream b5 03 topology): freeform carrier \
                 decode supported"
            }
            Variant::E5Stream => "E5 0D 03 record stream: direct analytic carrier decode supported",
            Variant::InnerNoDirectory => {
                "nested V5_CFV2 with no BREP-body directory (contiguous inner body): freeform \
                 carrier decode supported"
            }
            Variant::Unknown => "unrecognized storage layout",
        }
    }

    /// Whether this codec attempts a geometry decode for the variant.
    pub fn is_decoded(self) -> bool {
        matches!(self, Variant::StandardNested | Variant::FbbOnly)
    }
}
