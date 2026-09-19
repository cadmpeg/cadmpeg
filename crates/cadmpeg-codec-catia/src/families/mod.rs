//! Per-family CATIA record decoders.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_ir::codec::DecodeBody;
use cadmpeg_ir::unknown::UnknownRecord;
use cadmpeg_ir::{Annotations, CadIr};

use crate::container::ContainerScan;
use crate::variant::Variant;

pub(crate) mod a5a8;
pub(crate) mod b2;
pub(crate) mod b5;
pub(crate) mod consolidated;
pub(crate) mod e5;
mod freeform;
pub(crate) mod standard;
pub(crate) mod zero_entity;

/// Model layers a family route emits for one decoded storage stream.
pub(crate) struct FamilyOutput {
    pub(crate) ir: CadIr,
    pub(crate) report: DecodeBody,
    pub(crate) annotations: Annotations,
    pub(crate) unknowns: Vec<UnknownRecord>,
}

/// One entry in the ordered decode route table.
///
/// `applicable` gates the route on the identified container [`Variant`].
/// `decode` returns `None` when the stream does not yield a transferable model;
/// any carrier refusal it read is already in the caller's lane-refusal sink, so
/// the fall-through to the next route does not lose it.
pub(crate) struct Route {
    /// Name the decode report states when this route falls through.
    pub(crate) name: &'static str,
    pub(crate) applicable: fn(Variant) -> bool,
    pub(crate) decode: fn(
        &DecodeContext<'_>,
        &ContainerScan,
        &mut crate::nurbs::LaneRefusals,
    ) -> Option<FamilyOutput>,
    /// The route emits the standard FBB face population.
    pub(crate) standard_face_population: bool,
}

/// Ordered decode routes.
///
/// INVARIANT: slice order is the fallback order. Try each applicable route;
/// finish on the first `Some`. Only [`Variant::FbbOnly`] matches more than one
/// route (standard, then freeform). Every other variant matches exactly one.
pub(crate) const ROUTES: &[Route] = &[
    Route {
        name: "the standard route",
        applicable: |v| matches!(v, Variant::StandardNested | Variant::FbbOnly),
        decode: standard::decode::try_decode_standard,
        standard_face_population: true,
    },
    Route {
        name: "the zero-entity route",
        applicable: |v| v == Variant::ZeroEntity,
        decode: zero_entity::decode::try_decode_zero_entity,
        standard_face_population: false,
    },
    Route {
        name: "the E5 route",
        applicable: |v| v == Variant::E5Stream,
        decode: e5::decode::try_decode_e5,
        standard_face_population: false,
    },
    Route {
        name: "the freeform route",
        applicable: |v| {
            matches!(
                v,
                Variant::FloatPackedInnerNoFbb | Variant::FbbOnly | Variant::InnerNoDirectory
            )
        },
        decode: freeform::try_decode_freeform_surfaces,
        standard_face_population: false,
    },
];
