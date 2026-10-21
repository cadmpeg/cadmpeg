// SPDX-License-Identifier: Apache-2.0
//! `CATPart` storage-layout classification.
//!
//! The container shape, reconstructed B-rep stream, spine markers, table
//! delimiters, and record-family markers determine which decoder applies.
//! [`Variant::Unknown`] represents layouts that satisfy no recognized
//! structural pattern.

/// Recognized `CATPart` storage families and fallback classifications.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Variant {
    /// Nested `V5_CFV2` with a complete standard FBB edge/vertex walk.
    StandardNested,
    /// Nested `V5_CFV2` with a complete two-table FBB-only walk, or positive
    /// coordinate evidence and no standard delimiter. Complete post-FBB edge
    /// rows use width-selected handles `W∈{1,2,3}` as `u8`/`u16be`/`u24be`.
    FbbOnly,
    /// No nested `V5_CFV2`; the outer preamble carries `a9 03` record families.
    ZeroEntity,
    /// A nested `V5_CFV2` with no FBB spine whose topology lives in the object
    /// stream (`b5 03` grammar).
    FloatPackedInnerNoFbb,
    /// A coherent E5 (`E5 0D 03`) record stream carries the geometry.
    E5Stream,
    /// A nested `V5_CFV2` whose directory catalogues no BREP body. The freeform
    /// route decodes the contiguous inner body before the directory.
    InnerNoDirectory,
    /// None of the decodable families' invariants held.
    Unknown,
}

impl Variant {
    /// A one-line human description for container notes.
    pub(crate) fn description(self) -> &'static str {
        match self {
            Variant::StandardNested => {
                "standard nested V5_CFV2 (complete FBB edge/vertex walk): geometry \
                 decode supported"
            }
            Variant::FbbOnly => {
                "FBB-only partial spine (complete width-selected edge walk or positive \
                 coordinate fallback): vertex and analytic carrier decode supported"
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
}
