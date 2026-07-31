//! Per-family CATIA record decoders.
//!
//! Each family owns a `records` module holding its record vocabulary: the
//! struct/enum types it produces and the parser functions that decode them.
//! Families that also drive a full decode pipeline own a `decode` module whose
//! entry point is registered in [`ROUTES`].

use cadmpeg_ir::annotations::Annotations;
use cadmpeg_ir::report::DecodeReport;
use cadmpeg_ir::unknown::UnknownRecord;
use cadmpeg_ir::CadIr;

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
///
/// Replaces the former `ProjectedDecode` tuple alias with named fields. `ir`
/// carries the transferred neutral model, `report` its loss accounting,
/// `annotations` the byte-provenance stream, and `unknowns` the raw payload
/// records preserved for round-trip fidelity.
pub(crate) struct FamilyOutput {
    pub(crate) ir: CadIr,
    pub(crate) report: DecodeReport,
    pub(crate) annotations: Annotations,
    pub(crate) unknowns: Vec<UnknownRecord>,
}

/// One entry in the ordered decode route table.
///
/// `applicable` gates the route on the identified container [`Variant`];
/// `decode` runs the family pipeline, returning `None` when the stream does not
/// yield a transferable model so the orchestrator falls through to the next
/// applicable route.
pub(crate) struct Route {
    pub(crate) applicable: fn(Variant) -> bool,
    pub(crate) decode: fn(&ContainerScan) -> Option<FamilyOutput>,
}

/// Ordered decode routes tried by the orchestrator.
///
/// INVARIANT: the slice order IS the fallback semantics and must be preserved
/// exactly. The orchestrator tries each route whose `applicable` predicate
/// accepts the scan's variant, in this order, and finishes on the first `Some`.
/// A `None` return falls through to the next applicable route. Only [`Variant::FbbOnly`]
/// matches more than one route (standard, then freeform): an FBB-only file is
/// offered to the standard pipeline first and reaches freeform only when
/// standard declines. Every other variant matches exactly one route.
pub(crate) const ROUTES: &[Route] = &[
    Route {
        applicable: |v| matches!(v, Variant::StandardNested | Variant::FbbOnly),
        decode: standard::decode::try_decode_standard,
    },
    Route {
        applicable: |v| v == Variant::ZeroEntity,
        decode: zero_entity::decode::try_decode_zero_entity,
    },
    Route {
        applicable: |v| v == Variant::E5Stream,
        decode: e5::decode::try_decode_e5,
    },
    Route {
        applicable: |v| {
            matches!(
                v,
                Variant::FloatPackedInnerNoFbb | Variant::FbbOnly | Variant::InnerNoDirectory
            )
        },
        decode: freeform::try_decode_freeform_surfaces,
    },
];
