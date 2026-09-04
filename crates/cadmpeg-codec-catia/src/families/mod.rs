//! Per-family CATIA record decoders.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_ir::codec::DecodeBody;
use cadmpeg_ir::unknown::UnknownRecord;
use cadmpeg_ir::{Annotations, CadIr};

use crate::container::ContainerScan;
use crate::variant::Variant;

pub mod a5a8;
pub mod b2;
pub mod b5;
pub mod consolidated;
pub mod e5;
pub mod freeform;
pub mod standard;
pub mod zero_entity;

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
/// `decode` returns `None` when the stream does not yield a transferable model.
pub(crate) struct Route {
    pub(crate) applicable: fn(Variant) -> bool,
    pub(crate) decode: fn(&DecodeContext<'_>, &ContainerScan) -> Option<FamilyOutput>,
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
        applicable: |v| matches!(v, Variant::StandardNested | Variant::FbbOnly),
        decode: standard::decode::try_decode_standard,
        standard_face_population: true,
    },
    Route {
        applicable: |v| v == Variant::ZeroEntity,
        decode: zero_entity::decode::try_decode_zero_entity,
        standard_face_population: false,
    },
    Route {
        applicable: |v| v == Variant::E5Stream,
        decode: e5::decode::try_decode_e5,
        standard_face_population: false,
    },
    Route {
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
