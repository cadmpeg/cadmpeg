// SPDX-License-Identifier: Apache-2.0
//! Conversion from a PSB container to [`CadIr`].
//!
//! Decode transfers standard datum planes as derived plane surfaces and
//! preserves each geometry section as an [`UnknownRecord`]. Source metadata
//! records the layout, namespace census, active units, and counts of decoded
//! structural rows.
//!
//! Surface and curve namespaces contain useful topology and prototype data, but
//! the placed body model is incomplete. The report therefore records blocking
//! geometry and topology losses instead of emitting a partial B-rep.

pub(super) use std::collections::{BTreeMap, BTreeSet};
#[allow(unused_imports)]
use std::fmt::Write as _;

pub(super) use cadmpeg_core::decode::{alloc_filled, DecodeContext, View};
pub(super) use cadmpeg_core::CodecError;
pub(super) use cadmpeg_ir::codec::DecodeResult;
pub(super) use cadmpeg_ir::document::{CadIr, SourceMeta};
pub(super) use cadmpeg_ir::features::{
    Angle, BodySelection, BooleanOp, ChamferSpec, DesignParameter, DimensionDisplay, EdgeSelection,
    ExtrudeExtent, ExtrudeSide, ExtrudeStart, FaceSelection, Feature,
    FeatureDefinition as IrFeatureDefinition, FeatureId as IrFeatureId, FeatureResultTopology,
    FeatureSourceContent, FeatureTreeNodeRole, GeneratedEdgeRef, GeneratedFaceRef, HoleBottom,
    HoleForm, HoleKind, Length, ParameterId, ParameterValue, PathRef, PatternForm, PatternKind,
    ProfileRef, RadiusForm, RadiusSpec, RevolutionAxis, RevolutionConstruction, RevolveExtent,
    SurfaceBoundary, SurfaceContinuity, Termination, ThickenSide, VertexSelection,
};
pub(super) use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, NurbsCurve, NurbsSurface, Pcurve, PcurveGeometry, ProceduralCurve,
    ProceduralCurveDefinition, ProceduralSurface, ProceduralSurfaceDefinition, Surface,
    SurfaceGeometry,
};
pub(super) use cadmpeg_ir::hash::sha256_hex;
pub(super) use cadmpeg_ir::ids::{
    BodyId, CoedgeId, CurveId, EdgeId, FaceId, FeatureResultTopologyId, LoopId, OccurrenceId,
    PcurveId, PointId, ProceduralCurveId, ProceduralSurfaceId, ProductDefinitionId, RegionId,
    ShellId, SurfaceId, UnknownId, VertexId,
};
pub(super) use cadmpeg_ir::math::{Point2, Point3, Vector3};
pub(super) use cadmpeg_ir::products::{
    Occurrence, OccurrenceParent, ProductDefinition, ProductDefinitionKind, PrototypeReference,
};
pub(super) use cadmpeg_ir::report::{DecodeReport, LossNote, LossTaxonomy, Severity};
pub(super) use cadmpeg_ir::sketches::{
    Sketch, SketchConstraint, SketchConstraintDefinition, SketchConstraintId, SketchCoordinateAxis,
    SketchEntity, SketchEntityId, SketchEntityUse, SketchGeometry, SketchId, SketchLocus,
    SketchNativeOperand,
};
pub(super) use cadmpeg_ir::tessellation::Tessellation;
pub(super) use cadmpeg_ir::topology::{
    Body, BodyKind, Coedge, Edge, Face, Loop as IrLoop, PcurveUse, Point, Region, Sense, Shell,
    Vertex,
};
pub(super) use cadmpeg_ir::transform::Transform;
pub(super) use cadmpeg_ir::units::Units;
pub(super) use cadmpeg_ir::unknown::UnknownRecord;
pub(super) use cadmpeg_ir::AnnotationBuilder;
pub(super) use cadmpeg_ir::{Exactness, SourceObjectAssociation};
pub(super) use serde::Serialize;

pub(super) use crate::container::{self, role, ContainerScan};
pub(super) use crate::topology::HalfEdgeId;

mod analytic;
mod build;
mod coverage;
mod curve_expressions;
mod expanded;
mod feature_history;
mod holes;
mod native;
mod native_records;
mod records;
mod sketch;
mod sketch_ids;
mod sketch_transfer;
mod surfaces;
mod sweep;
mod uniqueness;
#[allow(clippy::wildcard_imports)]
use analytic::*;
#[allow(clippy::wildcard_imports)]
use build::*;
#[allow(clippy::wildcard_imports)]
pub(crate) use coverage::*;
#[allow(clippy::wildcard_imports)]
pub(crate) use curve_expressions::*;
#[allow(clippy::wildcard_imports)]
pub(crate) use expanded::*;
#[allow(clippy::wildcard_imports)]
use feature_history::*;
#[allow(clippy::wildcard_imports)]
use holes::*;
use native::{annotate, emit_arena, emit_uniform, store_arena};
#[allow(clippy::wildcard_imports)]
pub(crate) use native_records::*;
#[allow(clippy::wildcard_imports)]
use records::*;
#[allow(clippy::wildcard_imports)]
pub(crate) use sketch::*;
#[allow(clippy::wildcard_imports)]
pub(crate) use sketch_ids::*;
#[allow(clippy::wildcard_imports)]
use sketch_transfer::*;
#[allow(clippy::wildcard_imports)]
use surfaces::*;
#[allow(clippy::wildcard_imports)]
use sweep::*;
#[allow(clippy::wildcard_imports)]
pub(crate) use uniqueness::*;

#[cfg(test)]
mod tests;

#[cfg(test)]
mod plane_reconciliation_tests;

#[cfg(test)]
mod topological_vertex_tests;

#[cfg(test)]
mod native_edge_parameter_tests;

#[cfg(test)]
mod native_pcurve_tests;

#[cfg(test)]
mod prototype_local_frame_tests;

#[cfg(test)]
mod prototype_association_tests;

/// Decode a `.prt` stream into an IR document and loss report.
///
/// The stream is read from its beginning. When `options.container_only` is set,
/// the returned IR contains source metadata and preserved geometry sections but
/// no transferred entities.
pub fn decode(ctx: &DecodeContext<'_>, root: View<'_>) -> Result<DecodeResult, CodecError> {
    let scan = container::scan_bytes(root.window());
    // Charge section cardinality before IR construction so max_entities can
    // refuse the build rather than only the finalizer.
    ctx.charge_entities(scan.framing.sections.len() as u64, "admit Creo sections")?;
    let mut admitted_entities = 0_u64;

    let BuiltIr {
        mut ir,
        annotations,
        unknowns,
        coverage,
    } = if ctx.container_only() {
        build_container_ir(&scan)?
    } else {
        build_ir(ctx, &scan)?
    };
    ctx.admit_entities(
        ir.model.entity_count() as u64,
        &mut admitted_entities,
        "admit Creo entities",
    )?;
    let report = build_report(&scan, &ir, coverage, ctx.container_only());
    let mut source_fidelity = cadmpeg_ir::SourceFidelity::with_annotations(annotations);
    source_fidelity.attach_native_unknown_records(&mut ir, "creo", unknowns)?;
    Ok(DecodeResult::new(ir, report, source_fidelity))
}
