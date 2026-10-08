// SPDX-License-Identifier: Apache-2.0
//! Native B-rep transfer and FC05 cap-pair cylinders.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::{
    pcurve::Pcurve, Curve, CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, Surface,
    SurfaceGeometry,
};
use cadmpeg_ir::ids::{
    BodyId, CoedgeId, CurveId, EdgeId, FaceId, LoopId, PcurveId, PointId, RegionId, ShellId,
    SurfaceId, VertexId,
};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::topology::{
    Body, BodyKind, Coedge, Edge, Face, Loop as IrLoop, PcurveUse, Point, Region, Sense, Shell,
    Vertex,
};
use cadmpeg_ir::{AnnotationBuilder, Exactness, SourceObjectAssociation};

use crate::container::ContainerScan;
use crate::topology::HalfEdgeId;

use super::super::expanded::half_edge_ref;
use super::super::native::annotate;
use super::super::records::CreoFaceAdmissionRejectionRecord;
use super::super::sweep::profiles::line_pcurve;
use crate::decode::analytic::carriers::{
    geometry_section_record, native_face_orientations, ordered_face_loops,
    ordered_parameter_face_loops, placed_carriers,
};
use crate::decode::analytic::edges::{
    exact_line_edge_parameter_range, full_periodic_conic_edge_parameter_range,
    full_periodic_nurbs_edge_parameter_range, nonperiodic_conic_edge_parameter_range,
    orient_line_edge_carrier, orient_nonperiodic_nurbs_edge_carrier,
};
use crate::decode::analytic::equations::{CarrierEquation, PlaneEquation};
use crate::decode::analytic::pcurve_geometry::{
    meridian_circle_pcurve, ruled_generator_line_pcurve, surface_of_revolution_parallel_pcurve,
};
use crate::decode::analytic::pcurves::{
    canonicalized_pcurve_endpoints, pcurve_backed_periodic_conic_parameter_range,
    planar_curve_pcurve, unique_oriented_native_pcurve, NativePcurveCandidates,
};
use crate::decode::analytic::vertices::{
    solve_topological_vertices, TopologicalVertexSolveDiagnostics,
};

use super::{fc05_model_frame, native_surface_id, native_surface_namespace, Fc05CapPairFrame};

const EPS_PARAMETER_AGREE: f64 = 1.0e-9;
const EPS_GEOMETRY_AGREE: f64 = 1.0e-9;
pub(in crate::decode) const FACE_REJECTION_SAMPLE_LIMIT: usize = 4;

#[derive(Clone, Copy)]
pub(in crate::decode) struct NativeBrepCurveEvidence<'a> {
    pub(in crate::decode) derived_intersections: &'a BTreeSet<CurveId>,
    pub(in crate::decode) nurbs_endpoints: &'a BTreeSet<CurveId>,
}
const FACE_REJECTION_OPERAND_SAMPLE_LIMIT: usize = 8;

/// The first admission predicate that rejected one native face candidate.
///
/// The order is part of the diagnostic contract: one candidate contributes to
/// one bucket, so corpus totals can be compared without double-counting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(in super::super) enum FaceAdmissionRejection {
    /// The topology face has no unique transferred neutral surface carrier.
    MissingSurfaceCarrier,
    /// No unique native orientation exists for the candidate face.
    MissingOrientation,
    /// More than one typed model surface claims the candidate face identity.
    AmbiguousSurfaceCarrier,
    /// The topology component names the face but no closed native loop decoded.
    MissingLoops,
    /// At least one boundary curve lacks a solved endpoint vertex pair.
    UnresolvedBoundaryVertices,
    /// At least one boundary curve has more than one typed model carrier.
    AmbiguousBoundaryCurve,
    /// A two-edge loop did not satisfy the strict native parameter proof.
    TwoEdgeParameterProof,
    /// No deterministic loop order was established from geometry or pcurves.
    LoopOrdering,
}

impl FaceAdmissionRejection {
    pub(in super::super) const ALL: [Self; 8] = [
        Self::MissingSurfaceCarrier,
        Self::MissingOrientation,
        Self::AmbiguousSurfaceCarrier,
        Self::MissingLoops,
        Self::UnresolvedBoundaryVertices,
        Self::AmbiguousBoundaryCurve,
        Self::TwoEdgeParameterProof,
        Self::LoopOrdering,
    ];

    const fn key(self) -> &'static str {
        match self {
            Self::MissingSurfaceCarrier => "missing_surface_carrier",
            Self::MissingOrientation => "missing_orientation",
            Self::AmbiguousSurfaceCarrier => "ambiguous_surface_carrier",
            Self::MissingLoops => "missing_loops",
            Self::UnresolvedBoundaryVertices => "unresolved_boundary_vertices",
            Self::AmbiguousBoundaryCurve => "ambiguous_boundary_curve",
            Self::TwoEdgeParameterProof => "two_edge_parameter_proof",
            Self::LoopOrdering => "loop_ordering",
        }
    }

    const fn coverage_key(self) -> cadmpeg_ir::report::decode::CoverageKey {
        match self {
            Self::MissingSurfaceCarrier => {
                crate::coverage::BREP_REJECTED_FACE_MISSING_SURFACE_CARRIER_COUNT
            }
            Self::MissingOrientation => {
                crate::coverage::BREP_REJECTED_FACE_MISSING_ORIENTATION_COUNT
            }
            Self::AmbiguousSurfaceCarrier => {
                crate::coverage::BREP_REJECTED_FACE_AMBIGUOUS_SURFACE_CARRIER_COUNT
            }
            Self::MissingLoops => crate::coverage::BREP_REJECTED_FACE_MISSING_LOOPS_COUNT,
            Self::UnresolvedBoundaryVertices => {
                crate::coverage::BREP_REJECTED_FACE_UNRESOLVED_BOUNDARY_VERTICES_COUNT
            }
            Self::AmbiguousBoundaryCurve => {
                crate::coverage::BREP_REJECTED_FACE_AMBIGUOUS_BOUNDARY_CURVE_COUNT
            }
            Self::TwoEdgeParameterProof => {
                crate::coverage::BREP_REJECTED_FACE_TWO_EDGE_PARAMETER_PROOF_COUNT
            }
            Self::LoopOrdering => crate::coverage::BREP_REJECTED_FACE_LOOP_ORDERING_COUNT,
        }
    }

    pub(in super::super) const fn label(self) -> &'static str {
        match self {
            Self::MissingSurfaceCarrier => "missing surface carrier",
            Self::MissingOrientation => "missing orientation",
            Self::AmbiguousSurfaceCarrier => "ambiguous surface carrier",
            Self::MissingLoops => "missing loops",
            Self::UnresolvedBoundaryVertices => "unresolved boundary vertices",
            Self::AmbiguousBoundaryCurve => "ambiguous boundary curve",
            Self::TwoEdgeParameterProof => "two-edge parameter proof",
            Self::LoopOrdering => "loop ordering",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(in super::super) struct FaceAdmissionDetail {
    pub(in super::super) face_id: u32,
    pub(in super::super) boundary_half_edges: Vec<HalfEdgeId>,
    pub(in super::super) vertex_ids: Vec<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in super::super) struct FaceAdmissionDiagnostic {
    reason: FaceAdmissionRejection,
    detail: FaceAdmissionDetail,
}

impl FaceAdmissionDetail {
    fn face(face_id: u32) -> Self {
        Self {
            face_id,
            boundary_half_edges: Vec::new(),
            vertex_ids: Vec::new(),
        }
    }

    fn unresolved_boundary(
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        face_id: u32,
        loops: &[&crate::topology::Loop],
        edge_vertices: &BTreeMap<u32, [u32; 2]>,
        incidence: &BTreeMap<HalfEdgeId, &crate::topology::HalfEdgeVertexIncidence>,
    ) -> Result<Self, cadmpeg_core::CodecError> {
        let mut detail = Self::face(face_id);
        for lp in ctx.admit_iter(loops, "creo B-rep rejection loop traversal")? {
            for half_edge in
                ctx.admit_iter(lp.half_edges(), "creo B-rep rejection half edge traversal")?
            {
                if ctx.contains_key_btree_map(
                    edge_vertices,
                    &half_edge.curve_id,
                    "creo edge vertices lookup",
                )? {
                    continue;
                }
                if detail.boundary_half_edges.len() < FACE_REJECTION_OPERAND_SAMPLE_LIMIT {
                    ctx.reserve_vec(
                        &mut detail.boundary_half_edges,
                        1,
                        "creo B-rep rejection boundary samples",
                    )?;
                    detail.boundary_half_edges.push(*half_edge);
                }
                if let Some(binding) =
                    ctx.get_btree_map(incidence, half_edge, "creo incidence lookup")?
                {
                    if detail.vertex_ids.len() < FACE_REJECTION_OPERAND_SAMPLE_LIMIT
                        && !detail.vertex_ids.contains(&binding.start_vertex_id.get())
                    {
                        ctx.reserve_vec(
                            &mut detail.vertex_ids,
                            1,
                            "creo B-rep rejection vertex samples",
                        )?;
                        detail.vertex_ids.push(binding.start_vertex_id.get());
                    }
                    if let Some(end_vertex_id) = binding.end_vertex_id {
                        if detail.vertex_ids.len() < FACE_REJECTION_OPERAND_SAMPLE_LIMIT
                            && !detail.vertex_ids.contains(&end_vertex_id.get())
                        {
                            ctx.reserve_vec(
                                &mut detail.vertex_ids,
                                1,
                                "creo B-rep rejection vertex samples",
                            )?;
                            detail.vertex_ids.push(end_vertex_id.get());
                        }
                    }
                }
            }
        }
        Ok(detail)
    }
}

#[derive(Debug, Default, PartialEq)]
pub(in super::super) struct BrepTransferDiagnostics {
    pub(in super::super) rejected_extrusion_bodies: Vec<(BodyId, String)>,
    pub(in super::super) candidate_face_count: usize,
    pub(in super::super) admitted_face_count: usize,
    pub(in super::super) emitted_face_count: usize,
    pub(in super::super) boundary_curve_count: usize,
    pub(in super::super) boundary_curve_missing_incidence_count: usize,
    pub(in super::super) boundary_curve_unsolved_vertex_count: usize,
    pub(in super::super) vertex_solve: TopologicalVertexSolveDiagnostics,
    pub(in super::super) face_rejection_diagnostics: Vec<FaceAdmissionDiagnostic>,
    legacy_nonvisible_face_reference_count: usize,
    pub(in super::super) body_count_mismatch: bool,
    pub(in super::super) legacy_body_ownership_ambiguous: bool,
    pub(in super::super) empty_component_count: usize,
    pub(in super::super) admitted_component_count: usize,
    pub(in super::super) selected_body_count: Option<usize>,
}

impl BrepTransferDiagnostics {
    fn reject_face(
        &mut self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        reason: FaceAdmissionRejection,
        face_id: u32,
    ) -> Result<(), cadmpeg_core::CodecError> {
        self.reject_face_with_detail(ctx, reason, FaceAdmissionDetail::face(face_id))
    }

    fn reject_face_with_detail(
        &mut self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        reason: FaceAdmissionRejection,
        detail: FaceAdmissionDetail,
    ) -> Result<(), cadmpeg_core::CodecError> {
        ctx.reserve_vec(
            &mut self.face_rejection_diagnostics,
            1,
            "creo B-rep face rejection diagnostics",
        )?;
        self.face_rejection_diagnostics
            .push(FaceAdmissionDiagnostic { reason, detail });
        Ok(())
    }

    /// The rejection count and bounded detail samples for a reason.
    pub(in super::super) fn evidence<'a>(
        &'a self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        reason: FaceAdmissionRejection,
    ) -> Result<(usize, impl Iterator<Item = &'a FaceAdmissionDetail> + 'a), cadmpeg_core::CodecError>
    {
        let mut count = 0;
        let mut samples = [None; FACE_REJECTION_SAMPLE_LIMIT];
        for diagnostic in ctx.admit_iter(
            &self.face_rejection_diagnostics,
            "creo B-rep rejection evidence count",
        )? {
            if diagnostic.reason == reason {
                if count < samples.len() {
                    samples[count] = Some(&diagnostic.detail);
                }
                count += 1;
            }
        }
        let samples = samples.into_iter().flatten();
        Ok((count, samples))
    }

    pub(in super::super) fn face_admission_rejection_records(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<Vec<CreoFaceAdmissionRejectionRecord>, cadmpeg_core::CodecError> {
        let mut records = Vec::new();
        for diagnostic in ctx.admit_iter(
            &self.face_rejection_diagnostics,
            "creo face admission rejection records face rejection diagnostics traversal",
        )? {
            let detail = &diagnostic.detail;
            let id = ctx.format_retained(
                format_args!("creo:brep:face_admission_rejection#{}", detail.face_id),
                "creo B-rep rejection record IDs",
            )?;
            let mut boundary_half_edges = Vec::new();
            ctx.reserve_vec(
                &mut boundary_half_edges,
                detail.boundary_half_edges.len(),
                "creo B-rep rejection half edges",
            )?;
            boundary_half_edges.extend(
                detail
                    .boundary_half_edges
                    .iter()
                    .copied()
                    .map(half_edge_ref),
            );
            let mut vertex_ids = Vec::new();
            ctx.reserve_vec(
                &mut vertex_ids,
                detail.vertex_ids.len(),
                "creo B-rep rejection vertex IDs",
            )?;
            vertex_ids.extend_from_slice(&detail.vertex_ids);
            ctx.reserve_vec(&mut records, 1, "creo B-rep rejection records")?;
            records.push(CreoFaceAdmissionRejectionRecord {
                id,
                face_id: detail.face_id,
                reason: diagnostic.reason.key(),
                boundary_half_edges,
                vertex_ids,
            });
        }
        Ok(records)
    }

    pub(in super::super) fn record_coverage(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        coverage: &mut cadmpeg_ir::report::decode::Coverage,
    ) -> Result<(), cadmpeg_core::CodecError> {
        coverage.record(
            ctx,
            crate::coverage::BREP_CANDIDATE_FACE_COUNT,
            self.candidate_face_count,
        )?;
        coverage.record(
            ctx,
            crate::coverage::BREP_ADMITTED_FACE_COUNT,
            self.admitted_face_count,
        )?;
        coverage.record(
            ctx,
            crate::coverage::BREP_EMITTED_FACE_COUNT,
            self.emitted_face_count,
        )?;
        coverage.record(
            ctx,
            crate::coverage::BREP_BOUNDARY_CURVE_COUNT,
            self.boundary_curve_count,
        )?;
        coverage.record(
            ctx,
            crate::coverage::BREP_BOUNDARY_CURVE_MISSING_INCIDENCE_COUNT,
            self.boundary_curve_missing_incidence_count,
        )?;
        coverage.record(
            ctx,
            crate::coverage::BREP_BOUNDARY_CURVE_UNSOLVED_VERTEX_COUNT,
            self.boundary_curve_unsolved_vertex_count,
        )?;
        if self.legacy_nonvisible_face_reference_count > 0 {
            coverage.record(
                ctx,
                crate::coverage::BREP_LEGACY_NONVISIBLE_FACE_REFERENCE_COUNT,
                self.legacy_nonvisible_face_reference_count,
            )?;
        }
        coverage.record(
            ctx,
            crate::coverage::BREP_VERTEX_TOPOLOGICAL_COUNT,
            self.vertex_solve.topological_vertices,
        )?;
        coverage.record(
            ctx,
            crate::coverage::BREP_VERTEX_CARRIER_INCIDENT_COUNT,
            self.vertex_solve.carrier_incident_vertices,
        )?;
        coverage.record(
            ctx,
            crate::coverage::BREP_VERTEX_CARRIER_PAIR_INTERSECTION_CANDIDATE_COUNT,
            self.vertex_solve.carrier_pair_candidates,
        )?;
        coverage.record(
            ctx,
            crate::coverage::BREP_VERTEX_CARRIER_TRIPLE_INTERSECTION_CANDIDATE_COUNT,
            self.vertex_solve.carrier_triple_candidates,
        )?;
        coverage.record(
            ctx,
            crate::coverage::BREP_VERTEX_CARRIER_VALID_INTERSECTION_CANDIDATE_COUNT,
            self.vertex_solve.carrier_valid_candidates,
        )?;
        coverage.record(
            ctx,
            crate::coverage::BREP_VERTEX_CARRIER_ZERO_CANDIDATE_COUNT,
            self.vertex_solve.carrier_no_geometric_candidate_vertices
                + self.vertex_solve.carrier_no_valid_candidate_vertices,
        )?;
        if self.vertex_solve.carrier_no_geometric_candidate_vertices != 0 {
            coverage.record(
                ctx,
                crate::coverage::BREP_VERTEX_CARRIER_NO_GEOMETRIC_CANDIDATE_COUNT,
                self.vertex_solve.carrier_no_geometric_candidate_vertices,
            )?;
        }
        if self.vertex_solve.carrier_no_valid_candidate_vertices != 0 {
            coverage.record(
                ctx,
                crate::coverage::BREP_VERTEX_CARRIER_NO_VALID_CANDIDATE_COUNT,
                self.vertex_solve.carrier_no_valid_candidate_vertices,
            )?;
        }
        coverage.record(
            ctx,
            crate::coverage::BREP_VERTEX_CARRIER_AMBIGUOUS_CANDIDATE_COUNT,
            self.vertex_solve.carrier_ambiguous_candidate_vertices,
        )?;
        coverage.record(
            ctx,
            crate::coverage::BREP_VERTEX_CARRIER_POINT_COUNT,
            self.vertex_solve.carrier_points,
        )?;
        coverage.record(
            ctx,
            crate::coverage::BREP_PCURVE_RECORD_COUNT,
            self.vertex_solve.pcurve.records,
        )?;
        coverage.record(
            ctx,
            crate::coverage::BREP_PCURVE_PATH_COUNT,
            self.vertex_solve.pcurve.paths(),
        )?;
        let pcurve = &self.vertex_solve.pcurve;
        if pcurve.inactive_paths > 0
            || pcurve.inactive_records > 0
            || pcurve.partial_records > 0
            || pcurve.topology_mismatch_records > 0
        {
            coverage.record(
                ctx,
                crate::coverage::BREP_PCURVE_INACTIVE_PATH_COUNT,
                pcurve.inactive_paths,
            )?;
            coverage.record(
                ctx,
                crate::coverage::BREP_PCURVE_INACTIVE_RECORD_COUNT,
                pcurve.inactive_records,
            )?;
            coverage.record(
                ctx,
                crate::coverage::BREP_PCURVE_PARTIAL_RECORD_COUNT,
                pcurve.partial_records,
            )?;
            coverage.record(
                ctx,
                crate::coverage::BREP_PCURVE_TOPOLOGY_MISMATCH_RECORD_COUNT,
                pcurve.topology_mismatch_records,
            )?;
        }
        coverage.record(
            ctx,
            crate::coverage::BREP_PCURVE_MISSING_SURFACE_PATH_COUNT,
            self.vertex_solve.pcurve.missing_surfaces,
        )?;
        coverage.record(
            ctx,
            crate::coverage::BREP_PCURVE_UNEVALUABLE_PATH_COUNT,
            self.vertex_solve.pcurve.unevaluable_paths,
        )?;
        coverage.record(
            ctx,
            crate::coverage::BREP_PCURVE_MAPPED_PATH_COUNT,
            self.vertex_solve.pcurve.mapped_paths,
        )?;
        if pcurve.carrier_validated_paths > 0
            || pcurve.carrier_rejected_paths > 0
            || pcurve.carrier_unknown_paths() > 0
            || pcurve.carrier_rejected_records > 0
        {
            coverage.record(
                ctx,
                crate::coverage::BREP_PCURVE_CARRIER_VALIDATED_PATH_COUNT,
                pcurve.carrier_validated_paths,
            )?;
            coverage.record(
                ctx,
                crate::coverage::BREP_PCURVE_CARRIER_REJECTED_PATH_COUNT,
                pcurve.carrier_rejected_paths,
            )?;
            coverage.record(
                ctx,
                crate::coverage::BREP_PCURVE_CARRIER_UNKNOWN_PATH_COUNT,
                pcurve.carrier_unknown_paths(),
            )?;
            coverage.record(
                ctx,
                crate::coverage::BREP_PCURVE_CARRIER_UNKNOWN_MISSING_SURFACE_PATH_COUNT,
                pcurve.carrier_unknown_missing_surface_paths,
            )?;
            coverage.record(
                ctx,
                crate::coverage::BREP_PCURVE_CARRIER_UNKNOWN_MISSING_CARRIER_PATH_COUNT,
                pcurve.carrier_unknown_missing_carrier_paths,
            )?;
            coverage.record(
                ctx,
                crate::coverage::BREP_PCURVE_CARRIER_UNKNOWN_UNSUPPORTED_PAIR_PATH_COUNT,
                pcurve.carrier_unknown_unsupported_pair_paths,
            )?;
            coverage.record(
                ctx,
                crate::coverage::BREP_PCURVE_CARRIER_UNKNOWN_PARALLEL_PLANE_PATH_COUNT,
                pcurve.carrier_unknown_parallel_plane_paths,
            )?;
            coverage.record(
                ctx,
                crate::coverage::BREP_PCURVE_CARRIER_UNKNOWN_UNSUPPORTED_PATH_COUNT,
                pcurve.carrier_unknown_unsupported_path_paths,
            )?;
            coverage.record(
                ctx,
                crate::coverage::BREP_PCURVE_CARRIER_REJECTED_RECORD_COUNT,
                pcurve.carrier_rejected_records,
            )?;
        }
        coverage.record(
            ctx,
            crate::coverage::BREP_PCURVE_UNMAPPED_RECORD_COUNT,
            self.vertex_solve.pcurve.unmapped_records,
        )?;
        coverage.record(
            ctx,
            crate::coverage::BREP_PCURVE_INCONSISTENT_RECORD_COUNT,
            self.vertex_solve.pcurve.inconsistent_records,
        )?;
        coverage.record(
            ctx,
            crate::coverage::BREP_PCURVE_ACCEPTED_RECORD_COUNT,
            self.vertex_solve.pcurve.accepted_records,
        )?;
        coverage.record(
            ctx,
            crate::coverage::BREP_PCURVE_COMPLETE_RECORD_COUNT,
            self.vertex_solve.pcurve.complete_records,
        )?;
        if self.vertex_solve.pcurve.two_chart_records > 0 {
            coverage.record(
                ctx,
                crate::coverage::BREP_PCURVE_TWO_CHART_RECORD_COUNT,
                self.vertex_solve.pcurve.two_chart_records,
            )?;
            coverage.record(
                ctx,
                crate::coverage::BREP_PCURVE_TWO_CHART_MAPPED_RECORD_COUNT,
                self.vertex_solve.pcurve.two_chart_mapped_records(),
            )?;
            coverage.record(
                ctx,
                crate::coverage::BREP_PCURVE_TWO_CHART_COMPLETE_RECORD_COUNT,
                self.vertex_solve.pcurve.two_chart_complete_records,
            )?;
            coverage.record(
                ctx,
                crate::coverage::BREP_PCURVE_TWO_CHART_PARTIAL_RECORD_COUNT,
                self.vertex_solve.pcurve.two_chart_partial_records,
            )?;
            coverage.record(
                ctx,
                crate::coverage::BREP_PCURVE_TWO_CHART_MISSING_SURFACE_PATH_COUNT,
                self.vertex_solve.pcurve.two_chart_missing_surface_paths,
            )?;
            coverage.record(
                ctx,
                crate::coverage::BREP_PCURVE_TWO_CHART_UNEVALUABLE_PATH_COUNT,
                self.vertex_solve.pcurve.two_chart_unevaluable_paths,
            )?;
            coverage.record(
                ctx,
                crate::coverage::BREP_PCURVE_TWO_CHART_SURFACE_MISMATCH_RECORD_COUNT,
                self.vertex_solve.pcurve.two_chart_surface_mismatch_records,
            )?;
            coverage.record(
                ctx,
                crate::coverage::BREP_PCURVE_TWO_CHART_NO_SAMPLE_RECORD_COUNT,
                self.vertex_solve.pcurve.two_chart_no_sample_records,
            )?;
            coverage.record(
                ctx,
                crate::coverage::BREP_PCURVE_TWO_CHART_UNMAPPED_RECORD_COUNT,
                self.vertex_solve.pcurve.two_chart_unmapped_records,
            )?;
        }
        coverage.record(
            ctx,
            crate::coverage::BREP_PCURVE_CONFLICTING_CURVE_COUNT,
            self.vertex_solve.pcurve.conflicting_curves,
        )?;
        coverage.record(
            ctx,
            crate::coverage::BREP_VERTEX_PCURVE_ENDPOINT_EVIDENCE_COUNT,
            self.vertex_solve.pcurve.evidence,
        )?;
        coverage.record(
            ctx,
            crate::coverage::BREP_VERTEX_COMPLETE_PCURVE_ENDPOINT_EVIDENCE_COUNT,
            self.vertex_solve.pcurve.complete_evidence,
        )?;
        coverage.record(
            ctx,
            crate::coverage::BREP_VERTEX_PCURVE_CONSTRAINT_COUNT,
            self.vertex_solve.pcurve_constraints,
        )?;
        if self.vertex_solve.pcurve_fixed_endpoint_conflicts > 0 {
            coverage.record(
                ctx,
                crate::coverage::BREP_VERTEX_PCURVE_FIXED_ENDPOINT_CONFLICT_COUNT,
                self.vertex_solve.pcurve_fixed_endpoint_conflicts,
            )?;
        }
        if self.vertex_solve.pcurve_ambiguous_endpoint_vertices > 0 {
            coverage.record(
                ctx,
                crate::coverage::BREP_VERTEX_PCURVE_AMBIGUOUS_ENDPOINT_VERTEX_COUNT,
                self.vertex_solve.pcurve_ambiguous_endpoint_vertices,
            )?;
        }
        coverage.record(
            ctx,
            crate::coverage::BREP_VERTEX_DIRECTED_ENDPOINT_ASSIGNMENT_COUNT,
            self.vertex_solve.directed_endpoint_assignments,
        )?;
        coverage.record(
            ctx,
            crate::coverage::BREP_VERTEX_DIRECTED_ENDPOINT_CONFLICT_COUNT,
            self.vertex_solve.directed_endpoint_conflicts,
        )?;
        coverage.record(
            ctx,
            crate::coverage::BREP_VERTEX_NURBS_ENDPOINT_CONSTRAINT_COUNT,
            self.vertex_solve.nurbs_endpoint_constraints,
        )?;
        coverage.record(
            ctx,
            crate::coverage::BREP_VERTEX_ANALYTIC_DOMAIN_COUNT,
            self.vertex_solve.analytic_domain_vertices,
        )?;
        coverage.record(
            ctx,
            crate::coverage::BREP_VERTEX_SOLVED_COUNT,
            self.vertex_solve.solved_vertices,
        )?;
        coverage.record(
            ctx,
            crate::coverage::BREP_REJECTED_FACE_COUNT,
            self.face_rejection_diagnostics.len(),
        )?;
        let mut rejection_counts = [0; FaceAdmissionRejection::ALL.len()];
        for diagnostic in ctx.admit_iter(
            &self.face_rejection_diagnostics,
            "creo B-rep rejection coverage count",
        )? {
            for (reason, count) in FaceAdmissionRejection::ALL
                .into_iter()
                .zip(&mut rejection_counts)
            {
                if diagnostic.reason == reason {
                    *count += 1;
                    break;
                }
            }
        }
        for (reason, count) in FaceAdmissionRejection::ALL
            .into_iter()
            .zip(rejection_counts)
        {
            coverage.record(ctx, reason.coverage_key(), count)?;
        }
        coverage.record(
            ctx,
            crate::coverage::BREP_BODY_COUNT_MISMATCH_COUNT,
            usize::from(self.body_count_mismatch),
        )?;
        coverage.record(
            ctx,
            crate::coverage::BREP_LEGACY_BODY_OWNERSHIP_AMBIGUOUS_COUNT,
            usize::from(self.legacy_body_ownership_ambiguous),
        )?;
        coverage.record(
            ctx,
            crate::coverage::BREP_EMPTY_COMPONENT_COUNT,
            self.empty_component_count,
        )?;
        coverage.record(
            ctx,
            crate::coverage::BREP_ADMITTED_COMPONENT_COUNT,
            self.admitted_component_count,
        )?;
        coverage.record(
            ctx,
            crate::coverage::BREP_SELECTED_BODY_COUNT,
            self.selected_body_count.unwrap_or_default(),
        )?;
        coverage.record(
            ctx,
            crate::coverage::BREP_SELECTED_BODY_COUNT_UNRESOLVED,
            usize::from(self.selected_body_count.is_none()),
        )?;
        Ok(())
    }
}

#[derive(Debug, Default, PartialEq)]
pub(in super::super) struct NativeBrepTransferSummary {
    pub(in super::super) topological_point_count: usize,
    pub(in super::super) native_topological_edge_count: usize,
    pub(in super::super) diagnostics: BrepTransferDiagnostics,
}

#[derive(Debug, PartialEq, Eq)]
struct NeutralShellSpec {
    faces: Vec<u32>,
    wire_curves: BTreeSet<u32>,
}

fn admitted_face_components<'a>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &'a ContainerScan,
    eligible_face_ids: &BTreeSet<u32>,
) -> Result<Vec<&'a crate::topology::FaceComponent>, cadmpeg_core::CodecError> {
    let mut admitted = Vec::new();
    if !matches!(
        scan.framing.layout,
        crate::container::Layout::LegacyAscii(_)
    ) {
        for component in ctx.admit_iter(
            &scan.topology.face_components,
            "creo admitted face components face components traversal",
        )? {
            ctx.push_vec(
                &mut admitted,
                component,
                "creo B-rep admitted component refs",
            )?;
        }
        return Ok(admitted);
    }
    for component in ctx.admit_iter(
        &scan.topology.face_components,
        "creo admitted face components face components traversal",
    )? {
        if ctx.any_by(
            component.face_ids(),
            |face_id| {
                ctx.contains_btree_set(eligible_face_ids, face_id, "creo eligible face ids lookup")
            },
            "creo B-rep component face search",
        )? {
            ctx.reserve_vec(&mut admitted, 1, "creo B-rep admitted component refs")?;
            admitted.push(component);
        }
    }
    Ok(admitted)
}

/// Return whether a topology face reference belongs to the model-face
/// namespace used by legacy neutral B-rep admission.
///
/// Legacy `NovisGeom` rows can participate in the shared topology reference
/// space. They describe inactive or construction surfaces, not faces of the
/// model body. Their analytic carriers remain available as native geometry,
/// but admitting their references here would manufacture disconnected body
/// components and make body ownership appear ambiguous.
fn is_neutral_face_reference(scan: &ContainerScan, face_id: u32) -> bool {
    !matches!(
        scan.framing.layout,
        crate::container::Layout::LegacyAscii(_)
    ) || scan.surfaces.rows.contains_id(face_id)
}

fn merge_body_components(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    components: Vec<NeutralShellSpec>,
) -> Result<Vec<NeutralShellSpec>, cadmpeg_core::CodecError> {
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    if components.is_empty() {
        return Ok(Vec::new());
    }
    let mut components = components.into_iter();
    let Some(mut first) =
        ctx.next_charged(&mut components, "creo B-rep merged component traversal")?
    else {
        return Ok(Vec::new());
    };
    while components.len() != 0 {
        let Some(component) = ctx.next_charged(&mut components, "creo B-rep merged component traversal")? else {
            break;
        };
        ctx.extend_vec(
            &mut first.faces,
            component.faces,
            "creo B-rep merged component faces",
        )?;
        for curve_id in ctx
            .admit_iter(
                &component.wire_curves,
                "creo B-rep merged wire curve traversal",
            )?
            .copied()
        {
            ctx.insert_btree_set(
                &mut first.wire_curves,
                curve_id,
                "creo B-rep merged component wire nodes",
            )?;
        }
    }
    let mut merged = Vec::new();
    ctx.reserve_vec(&mut merged, 1, "creo B-rep merged component records")?;
    merged.push(first);
    Ok(merged)
}

fn legacy_body_ownership_is_unambiguous(scan: &ContainerScan, component_count: usize) -> bool {
    !matches!(
        scan.framing.layout,
        crate::container::Layout::LegacyAscii(_)
    ) || scan.framing.declared_body_count.is_some()
        || scan.framing.first_quilt_ptr == Some(0)
        || component_count <= 1
}

/// Partition one native component into valid neutral shells.
///
/// Face shells follow admitted face connectivity through edges or vertices.
/// Solved curves excluded from a face loop remain wire topology, attached to
/// a face shell only when exactly one shell touches an endpoint and otherwise
/// grouped in a wire shell.
fn split_neutral_component_shells(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    faces: &[u32],
    wire_curves: &BTreeSet<u32>,
    face_adjacency: &BTreeMap<u32, BTreeSet<u32>>,
    face_vertices: &BTreeMap<u32, BTreeSet<u32>>,
    edge_vertices: &BTreeMap<u32, [u32; 2]>,
) -> Result<Vec<NeutralShellSpec>, cadmpeg_core::CodecError> {
    let mut remaining_storage = ctx.reserve_scoped(0, "creo shell partition workspace")?;
    let mut remaining_faces = BTreeSet::new();
    for face_id in ctx.admit_iter(faces, "creo B-rep face traversal")? {
        remaining_storage.with_storage(|| {
            ctx.insert_btree_set(
                &mut remaining_faces,
                *face_id,
                "creo B-rep remaining face nodes",
            )
        })?;
    }
    let mut shell_specs = Vec::new();
    while !remaining_faces.is_empty() {
        ctx.charge_work(1, "creo B-rep shell component face visits")?;
        let Some(start) = remaining_faces.pop_first() else {
            break;
        };
        let mut group_storage = ctx.reserve_scoped(0, "creo shell group workspace")?;
        let mut group = BTreeSet::new();
        group_storage.with_storage(|| {
            ctx.insert_btree_set(&mut group, start, "creo B-rep shell group face nodes")
        })?;
        let mut pending = Vec::new();
        group_storage
            .with_storage(|| ctx.reserve_vec(&mut pending, 1, "creo B-rep pending shell faces"))?;
        pending.push(start);
        while !pending.is_empty() {
            let Some(face_id) = ctx.next_charged(
            &mut std::iter::from_fn(|| pending.pop()),
            "creo B-rep pending shell face traversal",
        )? else {
                break;
            };
            let Some(neighbours) =
                ctx.get_btree_map(face_adjacency, &face_id, "creo face adjacency lookup")?
            else {
                continue;
            };
            for neighbour in ctx
                .admit_iter(neighbours, "creo B-rep adjacent face traversal")?
                .copied()
            {
                if ctx.remove_btree_set(
                    &mut remaining_faces,
                    &neighbour,
                    "creo remaining faces lookup",
                )? {
                    group_storage.with_storage(|| {
                        ctx.insert_btree_set(
                            &mut group,
                            neighbour,
                            "creo B-rep shell group face nodes",
                        )
                    })?;
                    group_storage.with_storage(|| {
                        ctx.reserve_vec(&mut pending, 1, "creo B-rep pending shell faces")
                    })?;
                    pending.push(neighbour);
                }
            }
        }
        let mut group_faces = Vec::new();
        ctx.reserve_vec(&mut group_faces, group.len(), "creo B-rep shell face IDs")?;
        for face in ctx.admit_iter(&group, "creo B-rep shell face projection")? {
            group_faces.push(*face);
        }
        ctx.reserve_vec(&mut shell_specs, 1, "creo B-rep shell records")?;
        shell_specs.push(NeutralShellSpec {
            faces: group_faces,
            wire_curves: BTreeSet::new(),
        });
    }
    let mut unattached_wire_curves = BTreeSet::new();
    for curve_id in ctx.admit_iter(wire_curves, "creo B-rep wire curve traversal")? {
        let curve_vertices = *ctx
            .get_btree_map(edge_vertices, curve_id, "creo edge vertices lookup")?
            .ok_or_else(|| cadmpeg_core::CodecError::malformed("edge vertices indexed record"))?;
        let mut matching_shell = None;
        let mut shells = shell_specs.iter().enumerate();
        while shells.len() != 0 {
            let Some((index, shell)) = ctx.next_charged(&mut shells, "creo B-rep wire shell search")? else {
                break;
            };
            if ctx.any_by(
                &shell.faces,
                |face_id| {
                    let vertices = ctx
                        .get_btree_map(face_vertices, face_id, "creo face vertices lookup")?
                        .ok_or_else(|| {
                            cadmpeg_core::CodecError::malformed("shell face vertices")
                        })?;
                    Ok(ctx.contains_btree_set(
                        vertices,
                        &curve_vertices[0],
                        "creo face vertex membership",
                    )? || ctx.contains_btree_set(
                        vertices,
                        &curve_vertices[1],
                        "creo face vertex membership",
                    )?)
                },
                "creo B-rep wire shell face search",
            )? {
                if matching_shell.is_some() {
                    matching_shell = None;
                    break;
                }
                matching_shell = Some(index);
            }
        }
        if let Some(index) = matching_shell {
            ctx.insert_btree_set(
                &mut shell_specs[index].wire_curves,
                *curve_id,
                "creo B-rep attached wire nodes",
            )?;
        } else {
            ctx.insert_btree_set(
                &mut unattached_wire_curves,
                *curve_id,
                "creo B-rep unattached wire nodes",
            )?;
        }
    }
    if !unattached_wire_curves.is_empty() {
        ctx.reserve_vec(&mut shell_specs, 1, "creo B-rep shell records")?;
        shell_specs.push(NeutralShellSpec {
            faces: Vec::new(),
            wire_curves: unattached_wire_curves,
        });
    }
    Ok(shell_specs)
}

fn component_is_closed(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    component_face_curves: &BTreeSet<u32>,
    emitted_half_edges: &BTreeSet<HalfEdgeId>,
    half_edges: &BTreeMap<HalfEdgeId, &crate::topology::HalfEdge>,
    faces: &[u32],
) -> Result<bool, cadmpeg_core::CodecError> {
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    let mut curves = component_face_curves.iter();
    while curves.len() != 0 {
        let Some(curve_id) = ctx.next_charged(&mut curves, "creo B-rep closed component curve traversal")? else {
            break;
        };
        let mut faces_used = [None; 2];
        for side in [crate::topology::Side::Zero, crate::topology::Side::One] {
            let id = HalfEdgeId {
                curve_id: *curve_id,
                side,
            };
            if ctx.contains_btree_set(emitted_half_edges, &id, "creo emitted half edges lookup")? {
                faces_used[side.index()] = ctx
                    .get_btree_map(half_edges, &id, "creo half edges lookup")?
                    .and_then(|edge| edge.face_id);
            }
        }
        let [Some(first), Some(second)] = faces_used else {
            return Ok(false);
        };
        if !ctx.contains(
            faces,
            &first.get(),
            "creo closed component first face lookup",
        )? || !ctx.contains(
            faces,
            &second.get(),
            "creo closed component second face lookup",
        )? {
            return Ok(false);
        }
    }
    Ok(true)
}

fn parameter_points_agree(first: [f64; 2], second: [f64; 2]) -> bool {
    let scale = first
        .into_iter()
        .chain(second)
        .map(f64::abs)
        .fold(1.0, f64::max);
    first
        .into_iter()
        .zip(second)
        .all(|(first, second)| (first - second).abs() <= EPS_PARAMETER_AGREE * scale)
}

fn curve_geometry_is_typed_nonlinear(geometry: &SolvedCurveGeometry) -> bool {
    match geometry {
        SolvedCurveGeometry::Circle(_) => true,
        SolvedCurveGeometry::Ellipse(_) => true,
        SolvedCurveGeometry::Parabola(_) => true,
        SolvedCurveGeometry::Hyperbola(_) => true,
        SolvedCurveGeometry::Transformed(placed) => {
            curve_geometry_is_typed_nonlinear(placed.basis())
        }
        _ => false,
    }
}

fn model_typed_nonlinear_curve_ids(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ir: &CadIr,
    source_carriers: &crate::decode::source_carriers::SourceUnitCarriers,
) -> Result<BTreeSet<u32>, cadmpeg_core::CodecError> {
    let mut ids = BTreeSet::new();
    for curve in ctx.admit_iter(
        &ir.model.curves,
        "creo model typed nonlinear curve ids curves traversal",
    )? {
        let Some(suffix) = ctx.strip_prefix(
            curve.id.as_str(),
            "creo:visibgeom:curve#",
            "creo nonlinear curve namespace",
        )?
        else {
            continue;
        };
        let Ok(id) = ctx.parse_text::<u32>(suffix, "creo nonlinear curve number")? else {
            continue;
        };
        if source_carriers
            .curve_geometry(curve)?
            .solved()
            .is_some_and(curve_geometry_is_typed_nonlinear)
        {
            ctx.insert_btree_set(&mut ids, id, "creo B-rep typed curve ID nodes")?;
        }
    }
    Ok(ids)
}

fn scalar_values_agree(first: f64, second: f64) -> bool {
    if !first.is_finite() || !second.is_finite() {
        return false;
    }
    let scale = first.abs().max(second.abs()).max(1.0);
    (first - second).abs() <= EPS_GEOMETRY_AGREE * scale
}

fn points_are_geometrically_coincident(
    first: cadmpeg_ir::features::FinitePoint3,
    second: cadmpeg_ir::features::FinitePoint3,
) -> bool {
    let (first, second) = (first.get(), second.get());
    let scale = [first.x, first.y, first.z, second.x, second.y, second.z]
        .into_iter()
        .map(f64::abs)
        .fold(1.0, f64::max);
    first.distance(second) <= EPS_GEOMETRY_AGREE * scale
}

fn vectors_are_parallel(first: Vector3, second: Vector3) -> bool {
    let scale = first.norm() * second.norm();
    scale.is_finite() && scale > 0.0 && first.cross(second).norm() <= EPS_GEOMETRY_AGREE * scale
}

#[derive(Clone, Copy)]
struct NativeCircleLoop {
    center: cadmpeg_ir::features::FinitePoint3,
    axis: Vector3,
    radius: f64,
}

#[derive(Clone, Copy)]
struct NativeCurveEvidence<'a> {
    typed_nonlinear_curve_ids: &'a BTreeSet<u32>,
    model_curves: &'a [Curve],
    source_carriers: &'a crate::decode::source_carriers::SourceUnitCarriers<'a, 'a>,
}

fn native_circle_loop_geometry(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    index: &mut super::model_ids::ModelIdentityIndex<'_>,
    lp: &crate::topology::Loop,
    model_curves: &[Curve],
    source_carriers: &crate::decode::source_carriers::SourceUnitCarriers,
) -> Result<Option<NativeCircleLoop>, cadmpeg_core::CodecError> {
    let [first, second] = lp.half_edges() else {
        return Ok(None);
    };
    if first.curve_id == second.curve_id {
        return Ok(None);
    }
    let (first_id, _first_storage) = crate::identity::compose_scoped::<CurveId>(
        ctx,
        &crate::identity::VISIBGEOM_CURVE,
        first.curve_id,
        "creo native circle curve query",
    )?;
    let Some(first_position) = index
        .lookup(
            ctx,
            model_curves,
            |curve| curve.id.as_str(),
            first_id.as_str(),
        )?
        .unique_position()
    else {
        return Ok(None);
    };
    let (second_id, _second_storage) = crate::identity::compose_scoped::<CurveId>(
        ctx,
        &crate::identity::VISIBGEOM_CURVE,
        second.curve_id,
        "creo native circle curve query",
    )?;
    let Some(second_position) = index
        .lookup(
            ctx,
            model_curves,
            |curve| curve.id.as_str(),
            second_id.as_str(),
        )?
        .unique_position()
    else {
        return Ok(None);
    };
    let first = &model_curves[first_position];
    let second = &model_curves[second_position];
    let (
        CurveGeometry::Solved(SolvedCurveGeometry::Circle(circle_curve)),
        CurveGeometry::Solved(SolvedCurveGeometry::Circle(circle_curve_2)),
    ) = (
        source_carriers.curve_geometry(first)?,
        source_carriers.curve_geometry(second)?,
    )
    else {
        return Ok(None);
    };
    let first_center = circle_curve.center();
    let first_axis = circle_curve.frame().axis().as_raw();
    let first_radius = circle_curve.radius().get();
    let second_center = circle_curve_2.center();
    let second_axis = circle_curve_2.frame().axis().as_raw();
    let second_radius = circle_curve_2.radius().get();
    if !scalar_values_agree(first_radius, second_radius)
        || !points_are_geometrically_coincident(first_center, second_center)
        || !vectors_are_parallel(*first_axis, *second_axis)
    {
        return Ok(None);
    }
    Ok(Some(NativeCircleLoop {
        center: first_center,
        axis: *first_axis,
        radius: first_radius,
    }))
}

fn ordered_two_edge_circle_loops<'a>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    loops: &[&'a crate::topology::Loop],
    polygons: &[Vec<[f64; 2]>],
    surface: &SurfaceGeometry,
    model_curves: &[Curve],
    source_carriers: &crate::decode::source_carriers::SourceUnitCarriers,
) -> Result<Option<Vec<&'a crate::topology::Loop>>, cadmpeg_core::CodecError> {
    if loops.len() < 2 || loops.len() != polygons.len() {
        return Ok(None);
    }
    let SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane_surface)) = surface else {
        return Ok(None);
    };
    let origin = plane_surface.origin().get();
    let normal = plane_surface.frame().axis().as_raw();
    let mut curves_index = super::model_ids::ModelIdentityIndex::new(ctx)?;
    let mut circle_storage = ctx.reserve_scoped(0, "creo native circle loop workspace")?;
    let mut circle_loops = Vec::new();
    let mut visits = loops.iter();
    while visits.len() != 0 {
        let Some(lp) = ctx.next_charged(&mut visits, "creo B-rep loop traversal")? else {
            break;
        };
        let Some(circle) =
            native_circle_loop_geometry(ctx, &mut curves_index, lp, model_curves, source_carriers)?
        else {
            return Ok(None);
        };
        circle_storage.with_storage(|| {
            ctx.reserve_vec(&mut circle_loops, 1, "creo native circle loop geometry")
        })?;
        circle_loops.push(circle);
    }
    let normal_length = normal.norm();
    if !normal_length.is_finite() || normal_length <= 0.0 {
        return Ok(None);
    }
    let reference = circle_loops[0];
    if ctx.any_by(
        &circle_loops,
        |circle| {
            Ok({
                let center = circle.center.get();
                let center_scale = reference
                    .radius
                    .max(circle.radius)
                    .max(1.0)
                    .max(center.x.abs())
                    .max(center.y.abs())
                    .max(center.z.abs());
                let distance_from_surface = center.vector_from(origin).dot(*normal).abs();
                !points_are_geometrically_coincident(circle.center, reference.center)
                    || !vectors_are_parallel(circle.axis, reference.axis)
                    || !vectors_are_parallel(circle.axis, *normal)
                    || distance_from_surface > EPS_GEOMETRY_AGREE * normal_length * center_scale
            })
        },
        "creo B-rep circle loop agreement",
    )? {
        return Ok(None);
    }
    let Some(center_uv) =
        cadmpeg_ir::eval::analytic_surface_parameters(surface, reference.center.get())
    else {
        return Ok(None);
    };
    let center_uv = cadmpeg_ir::math::Point2::from(center_uv);
    let mut pairs = circle_loops.iter().zip(polygons);
    while pairs.len() != 0 {
        let Some((circle, polygon)) = ctx.next_charged(&mut pairs, "creo B-rep circle loop traversal")? else {
            break;
        };
        let [first, second] = polygon.as_slice() else {
            return Ok(None);
        };
        if [first[0], first[1], second[0], second[1]]
            .into_iter()
            .any(|value| !value.is_finite())
        {
            return Ok(None);
        }
        let first_delta = [first[0] - center_uv.u, first[1] - center_uv.v];
        let second_delta = [second[0] - center_uv.u, second[1] - center_uv.v];
        let radius_squared = circle.radius * circle.radius;
        let first_radius_squared =
            first_delta[0].mul_add(first_delta[0], first_delta[1] * first_delta[1]);
        let second_radius_squared =
            second_delta[0].mul_add(second_delta[0], second_delta[1] * second_delta[1]);
        let endpoints_dot =
            first_delta[0].mul_add(second_delta[0], first_delta[1] * second_delta[1]);
        if !scalar_values_agree(first_radius_squared, radius_squared)
            || !scalar_values_agree(second_radius_squared, radius_squared)
            || !scalar_values_agree(endpoints_dot, -radius_squared)
        {
            return Ok(None);
        }
    }
    let mut circles = circle_loops.iter().enumerate();
    while circles.len() != 0 {
        let Some((index, first)) = ctx.next_charged(&mut circles, "creo B-rep circle radius traversal")? else {
            break;
        };
        if ctx.any_by(
            &circle_loops[index + 1..],
            |second| Ok(scalar_values_agree(first.radius, second.radius)),
            "creo B-rep circle radius duplicate search",
        )? {
            return Ok(None);
        }
    }
    let mut order = Vec::new();
    for (index, _) in ctx
        .admit_iter(loops, "creo B-rep circle loop index traversal")?
        .enumerate()
    {
        ctx.reserve_scoped_vec(
            &mut circle_storage,
            &mut order,
            1,
            "creo native circle loop order",
        )?;
        order.push(index);
    }
    ctx.stable_sort_by(
        order.as_mut_slice(),
        |value| value,
        |first, second| {
            circle_loops[*second]
                .radius
                .total_cmp(&circle_loops[*first].radius)
        },
        "creo ordered two edge circle loops order ordering",
    )?;
    let mut ordered = Vec::new();
    for index in ctx
        .admit_iter(&order, "creo B-rep circle loop order traversal")?
        .copied()
    {
        ctx.reserve_vec(&mut ordered, 1, "creo native ordered circle loops")?;
        ordered.push(loops[index]);
    }
    Ok(Some(ordered))
}

fn native_parameter_loop_polygon(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    lp: &crate::topology::Loop,
    face: (u32, &SurfaceGeometry),
    incidence: &BTreeMap<HalfEdgeId, &crate::topology::HalfEdgeVertexIncidence>,
    solved_vertices: &BTreeMap<u32, [f64; 3]>,
    native_pcurves: &NativePcurveCandidates,
    typed_nonlinear_curve_ids: &BTreeSet<u32>,
) -> Result<Option<Vec<[f64; 2]>>, cadmpeg_core::CodecError> {
    let (face_id, surface) = face;

    let mut segment_storage = ctx.reserve_scoped(0, "creo loop parameter segment workspace")?;
    let mut segments = Vec::new();
    let mut visits = lp.half_edges().iter();
    while visits.len() != 0 {
        let Some(half_edge) = ctx.next_charged(&mut visits, "creo B-rep loop half edge traversal")? else {
            break;
        };
        let Some(binding) = ctx.get_btree_map(incidence, half_edge, "creo incidence lookup")?
        else {
            return Ok(None);
        };
        let Some(end_vertex_id) = binding.end_vertex_id else {
            return Ok(None);
        };
        let Some(candidates) = ctx.get_btree_map(
            native_pcurves,
            &(half_edge.curve_id, face_id),
            "creo native pcurves lookup",
        )?
        else {
            return Ok(None);
        };
        let [Some(start), Some(end)] = [
            ctx.get_btree_map(
                solved_vertices,
                &binding.start_vertex_id.get(),
                "creo solved vertices lookup",
            )?
            .copied(),
            ctx.get_btree_map(
                solved_vertices,
                &end_vertex_id.get(),
                "creo solved vertices lookup",
            )?
            .copied(),
        ] else {
            return Ok(None);
        };
        let Some(crate::decode::analytic::pcurves::OrientedNativePcurve { endpoints, .. }) =
            unique_oriented_native_pcurve(ctx, surface, candidates, [start, end])?
        else {
            return Ok(None);
        };
        ctx.reserve_scoped_vec(
            &mut segment_storage,
            &mut segments,
            1,
            "creo native loop pcurve segments",
        )?;
        segments.push(endpoints);
    }
    if segments.len() < 3
        && (segments.len() != 2
            || lp.half_edges()[0].curve_id == lp.half_edges()[1].curve_id
            || ctx.any_by(
                lp.half_edges(),
                |half_edge| {
                    Ok(!ctx.contains_btree_set(
                        typed_nonlinear_curve_ids,
                        &half_edge.curve_id,
                        "creo typed nonlinear curve ids lookup",
                    )?)
                },
                "creo B-rep nonlinear half edge search",
            )?
            || ctx.any_by(
                &segments,
                |segment| Ok(parameter_points_agree(segment[0], segment[1])),
                "creo B-rep parameter segment search",
            )?)
        || ctx.any_by(
            &segments,
            |segment| Ok(segment.iter().flatten().any(|value| !value.is_finite())),
            "creo B-rep parameter coordinate search",
        )?
        || ctx.any_by(
            segments.iter().enumerate(),
            |(index, segment)| {
                let next = segments[(index + 1) % segments.len()];
                Ok(!parameter_points_agree(segment[1], next[0]))
            },
            "creo B-rep parameter chain search",
        )?
    {
        return Ok(None);
    }
    let mut polygon = Vec::new();
    for segment in ctx.admit_iter(&segments, "creo B-rep parameter segment traversal")? {
        ctx.reserve_vec(&mut polygon, 1, "creo native loop polygon points")?;
        polygon.push(segment[0]);
    }
    Ok(Some(polygon))
}

fn ordered_native_parameter_face_loops<'a>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    loops: &[&'a crate::topology::Loop],
    face: (u32, &SurfaceGeometry),
    incidence: &BTreeMap<HalfEdgeId, &crate::topology::HalfEdgeVertexIncidence>,
    solved_vertices: &BTreeMap<u32, [f64; 3]>,
    native_pcurves: &NativePcurveCandidates,
    curve_evidence: NativeCurveEvidence<'_>,
) -> Result<Option<Vec<&'a crate::topology::Loop>>, cadmpeg_core::CodecError> {
    let (face_id, surface) = face;

    let mut polygon_storage = ctx.reserve_scoped(0, "creo face parameter polygon workspace")?;
    let mut polygons = Vec::new();
    let mut visits = loops.iter();
    while visits.len() != 0 {
        let Some(lp) = ctx.next_charged(&mut visits, "creo B-rep loop traversal")? else {
            break;
        };
        let Some(polygon) = polygon_storage.with_storage(|| {
            native_parameter_loop_polygon(
                ctx,
                lp,
                (face_id, surface),
                incidence,
                solved_vertices,
                native_pcurves,
                curve_evidence.typed_nonlinear_curve_ids,
            )
        })?
        else {
            return Ok(None);
        };
        ctx.reserve_scoped_vec(
            &mut polygon_storage,
            &mut polygons,
            1,
            "creo native face loop polygons",
        )?;
        polygons.push(polygon);
    }
    let mut input_storage = ctx.reserve_scoped(0, "creo native face ordering candidate references")?;
    let mut copied_loops = Vec::new();
    input_storage.with_storage(|| {
        ctx.extend_from_slice(&mut copied_loops, loops, "creo native face loop references")
    })?;
    if let Some(ordered) = ordered_parameter_face_loops(ctx, copied_loops, &polygons)? {
        input_storage.commit()?;
        Ok(Some(ordered))
    } else {
        drop(input_storage);
        ordered_two_edge_circle_loops(
            ctx,
            loops,
            &polygons,
            surface,
            curve_evidence.model_curves,
            curve_evidence.source_carriers,
        )
    }
}

fn push_native_pcurve_candidate(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    candidates: &mut NativePcurveCandidates,
    curve_id: u32,
    face_id: u32,
    endpoints: [[f64; 2]; 2],
    offset: usize,
) -> Result<(), cadmpeg_core::CodecError> {
    let key = (curve_id, face_id);
    let values = match ctx.entry_btree_map(candidates, key, "creo B-rep pcurve candidate nodes")? {
        std::collections::btree_map::Entry::Occupied(entry) => entry.into_mut(),
        std::collections::btree_map::Entry::Vacant(entry) => entry.insert(Vec::new()),
    };
    ctx.reserve_vec(values, 1, "creo B-rep pcurve candidates")?;
    values.push((endpoints, offset));
    Ok(())
}

struct BrepSourceIndexes<'a> {
    planes: BTreeMap<u32, PlaneEquation>,
    half_edges: BTreeMap<HalfEdgeId, &'a crate::topology::HalfEdge>,
    incidence: BTreeMap<HalfEdgeId, &'a crate::topology::HalfEdgeVertexIncidence>,
}

impl<'a> BrepSourceIndexes<'a> {
    fn from_scan(
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        carriers: &BTreeMap<u32, CarrierEquation>,
        scan: &'a ContainerScan<'_>,
    ) -> Result<Self, cadmpeg_core::CodecError> {
        let mut planes = BTreeMap::new();
        for (id, carrier) in ctx.admit_iter(carriers, "creo B-rep carrier traversal")? {
            if let CarrierEquation::Plane(plane) = carrier {
                ctx.insert_btree_map(&mut planes, *id, *plane, "creo B-rep plane index nodes")?;
            }
        }
        let mut half_edges = BTreeMap::new();
        for half_edge in ctx.admit_iter(
            &scan.topology.half_edges,
            "creo from scan half edges traversal",
        )? {
            ctx.insert_btree_map(
                &mut half_edges,
                half_edge.id,
                half_edge,
                "creo B-rep half-edge index nodes",
            )?;
        }
        let mut incidence = BTreeMap::new();
        for binding in ctx.admit_iter(
            &scan.topology.half_edge_vertex_incidence,
            "creo from scan half edge vertex incidence traversal",
        )? {
            ctx.insert_btree_map(
                &mut incidence,
                binding.half_edge,
                binding,
                "creo B-rep incidence index nodes",
            )?;
        }
        Ok(Self {
            planes,
            half_edges,
            incidence,
        })
    }
}

struct BrepFaceCandidateIndexes<'a> {
    loops_by_face: BTreeMap<u32, Vec<&'a crate::topology::Loop>>,
    candidate_face_ids: BTreeSet<u32>,
    model_surface_counts: BTreeMap<u32, usize>,
    boundary_curve_ids: BTreeSet<u32>,
    legacy_nonvisible_face_reference_count: usize,
}

impl<'a> BrepFaceCandidateIndexes<'a> {
    fn from_scan(
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        scan: &'a ContainerScan<'_>,
        ir: &CadIr,
    ) -> Result<Self, cadmpeg_core::CodecError> {
        let mut loops_by_face = BTreeMap::<u32, Vec<&crate::topology::Loop>>::new();
        for lp in ctx.admit_iter(&scan.topology.loops, "creo from scan loops traversal")? {
            if let Some(face_id) = lp.face_id() {
                let face_key = face_id.get();
                let loops = ctx
                    .entry_btree_map(
                        &mut loops_by_face,
                        face_key,
                        "creo B-rep face-loop index nodes",
                    )?
                    .or_default();
                ctx.reserve_vec(loops, 1, "creo B-rep face-loop references")?;
                loops.push(lp);
            }
        }
        let mut reference_storage =
            ctx.reserve_scoped(0, "creo topology face reference workspace")?;
        let mut topology_face_reference_ids = BTreeSet::new();
        for component in ctx.admit_iter(
            &scan.topology.face_components,
            "creo B-rep topology component traversal",
        )? {
            for face_id in ctx
                .admit_iter(component.face_ids(), "creo B-rep topology face traversal")?
                .copied()
            {
                reference_storage.with_storage(|| {
                    ctx.insert_btree_set(
                        &mut topology_face_reference_ids,
                        face_id,
                        "creo B-rep topology face ID nodes",
                    )
                })?;
            }
        }
        for (face_id, _) in
            ctx.admit_iter(&loops_by_face, "creo B-rep topology loop face traversal")?
        {
            reference_storage.with_storage(|| {
                ctx.insert_btree_set(
                    &mut topology_face_reference_ids,
                    *face_id,
                    "creo B-rep topology face ID nodes",
                )
            })?;
        }
        let mut legacy_nonvisible_face_reference_count = 0usize;
        let mut candidate_face_ids = BTreeSet::new();
        for face_id in ctx.admit_iter(
            &topology_face_reference_ids,
            "creo B-rep topology face partition",
        )? {
            if is_neutral_face_reference(scan, *face_id) {
                ctx.insert_btree_set(
                    &mut candidate_face_ids,
                    *face_id,
                    "creo B-rep candidate face ID nodes",
                )?;
            } else {
                legacy_nonvisible_face_reference_count += 1;
            }
        }
        ctx.retain_btree_map(
            &mut loops_by_face,
            |face_id, _| Ok::<_, cadmpeg_core::CodecError>(is_neutral_face_reference(scan, *face_id)),
            "creo B-rep neutral loop faces",
        )?;
        let mut model_surface_counts = BTreeMap::new();
        let mut identities = super::model_ids::ModelIdentityIndex::new(ctx)?;
        for face_id in ctx.admit_iter(
            &candidate_face_ids,
            "creo from scan candidate face ids traversal",
        )? {
            let (key, _key_storage) = crate::identity::compose_scoped::<SurfaceId>(
                ctx,
                &native_surface_namespace(ctx, scan, *face_id)?.0,
                *face_id,
                "creo B-rep surface count query",
            )?;
            let count = identities.count(
                ctx,
                &ir.model.surfaces,
                |surface| surface.id.as_str(),
                key.as_str(),
            )?;
            ctx.insert_btree_map(
                &mut model_surface_counts,
                *face_id,
                count,
                "creo B-rep model surface count nodes",
            )?;
        }
        let mut boundary_curve_ids = BTreeSet::new();
        for (_, loops) in ctx.admit_iter(&loops_by_face, "creo B-rep boundary face traversal")? {
            for lp in ctx.admit_iter(loops, "creo B-rep boundary loop traversal")? {
                for half_edge in
                    ctx.admit_iter(lp.half_edges(), "creo B-rep boundary half edge traversal")?
                {
                    ctx.insert_btree_set(
                        &mut boundary_curve_ids,
                        half_edge.curve_id,
                        "creo B-rep boundary curve ID nodes",
                    )?;
                }
            }
        }
        Ok(Self {
            loops_by_face,
            candidate_face_ids,
            model_surface_counts,
            boundary_curve_ids,
            legacy_nonvisible_face_reference_count,
        })
    }
}

struct BrepEdgeIndexes {
    edge_vertices: BTreeMap<u32, [u32; 2]>,
    model_curve_counts: BTreeMap<u32, usize>,
    admitted_edge_curves: BTreeSet<u32>,
}

impl BrepEdgeIndexes {
    fn from_rows(
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        rows: &[crate::curve::CurveTopologyRow],
        native_edge_vertices: &BTreeMap<u32, [std::num::NonZeroU32; 2]>,
        solved_vertices: &BTreeMap<u32, [f64; 3]>,
        ir: &CadIr,
    ) -> Result<Self, cadmpeg_core::CodecError> {
        let mut edge_vertices = BTreeMap::new();
        let (unique_rows, _unique_rows_storage) = crate::identity::uniquely_identified_rows_checked(ctx, rows, |row| row.id)?;
        for row in ctx
            .admit_iter(&unique_rows, "creo B-rep unique edge row traversal")?
            .copied()
        {
            let Some(vertices) = ctx
                .get_btree_map(
                    native_edge_vertices,
                    &row.id,
                    "creo native edge vertices lookup",
                )?
                .copied()
                .map(|pair| pair.map(std::num::NonZeroU32::get))
            else {
                continue;
            };
            if ctx.contains_key_btree_map(
                solved_vertices,
                &vertices[0],
                "creo solved vertices lookup",
            )? && ctx.contains_key_btree_map(
                solved_vertices,
                &vertices[1],
                "creo solved vertices lookup",
            )? {
                ctx.insert_btree_map(
                    &mut edge_vertices,
                    row.id,
                    vertices,
                    "creo B-rep edge-vertex nodes",
                )?;
            }
        }
        let mut model_curve_counts = BTreeMap::new();
        let mut identities = super::model_ids::ModelIdentityIndex::new(ctx)?;
        let mut admitted_edge_curves = BTreeSet::new();
        for (curve_id, _) in
            ctx.admit_iter(&edge_vertices, "creo B-rep edge vertex map traversal")?
        {
            let (key, _key_storage) = crate::identity::compose_scoped::<CurveId>(
                ctx,
                &crate::identity::VISIBGEOM_CURVE,
                *curve_id,
                "creo B-rep curve count query",
            )?;
            let count = identities.count(
                ctx,
                &ir.model.curves,
                |curve| curve.id.as_str(),
                key.as_str(),
            )?;
            ctx.insert_btree_map(
                &mut model_curve_counts,
                *curve_id,
                count,
                "creo B-rep model curve count nodes",
            )?;
            if count <= 1 {
                ctx.insert_btree_set(
                    &mut admitted_edge_curves,
                    *curve_id,
                    "creo B-rep admitted edge ID nodes",
                )?;
            }
        }
        Ok(Self {
            edge_vertices,
            model_curve_counts,
            admitted_edge_curves,
        })
    }
}

struct BrepEligibleFaceIndexes {
    emitted_half_edges: BTreeSet<HalfEdgeId>,
    face_curves: BTreeSet<u32>,
    closed_single_edge_curves: BTreeSet<u32>,
    row_offsets: BTreeMap<u32, usize>,
    curve_faces: BTreeMap<u32, [u32; 2]>,
    eligible_face_ids: BTreeSet<u32>,
}

impl BrepEligibleFaceIndexes {
    fn from_faces(
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        eligible_faces: &BTreeMap<u32, Vec<&crate::topology::Loop>>,
        topology_rows: &[crate::curve::CurveTopologyRow],
    ) -> Result<Self, cadmpeg_core::CodecError> {
        let mut loop_storage = ctx.reserve_scoped(0, "creo eligible loop workspace")?;
        let mut single_edge_uses = std::collections::HashMap::new();
        let mut emitted_half_edges = BTreeSet::new();
        for (_, loops) in ctx.admit_iter(eligible_faces, "creo B-rep eligible face traversal")? {
            for lp in ctx.admit_iter(loops, "creo B-rep eligible loop traversal")? {
                for half_edge in
                    ctx.admit_iter(lp.half_edges(), "creo B-rep emitted half edge traversal")?
                {
                    ctx.insert_btree_set(
                        &mut emitted_half_edges,
                        *half_edge,
                        "creo B-rep emitted half-edge nodes",
                    )?;
                    loop_storage.with_storage(|| {
                        let all_single_edge = ctx
                            .entry_hash_map(
                                &mut single_edge_uses,
                                half_edge.curve_id,
                                "creo B-rep single-edge use index",
                            )?
                            .or_insert(true);
                        *all_single_edge &= lp.half_edges().len() == 1;
                        Ok::<_, cadmpeg_core::CodecError>(())
                    })?;
                }
            }
        }
        let mut face_curves = BTreeSet::new();
        let mut closed_single_edge_curves = BTreeSet::new();
        for half_edge in ctx.admit_iter(
            &emitted_half_edges,
            "creo from faces emitted half edges traversal",
        )? {
            ctx.insert_btree_set(
                &mut face_curves,
                half_edge.curve_id,
                "creo B-rep face curve ID nodes",
            )?;
        }
        for curve_id in ctx.admit_iter(&face_curves, "creo from faces face curves traversal")? {
            if single_edge_uses.get(curve_id) == Some(&true) {
                ctx.insert_btree_set(
                    &mut closed_single_edge_curves,
                    *curve_id,
                    "creo B-rep single-edge curve nodes",
                )?;
            }
        }
        let mut row_offsets = BTreeMap::new();
        for row in ctx.admit_iter(topology_rows, "creo B-rep topology row traversal")? {
            ctx.insert_btree_map(
                &mut row_offsets,
                row.id,
                row.offset,
                "creo B-rep row-offset nodes",
            )?;
        }
        let mut curve_faces = BTreeMap::new();
        let (unique_rows, _unique_rows_storage) =
            crate::identity::uniquely_identified_rows_checked(ctx, topology_rows, |row| row.id)?;
        for row in ctx
            .admit_iter(&unique_rows, "creo B-rep unique topology row traversal")?
            .copied()
        {
            ctx.insert_btree_map(
                &mut curve_faces,
                row.id,
                row.stored_face_ids(),
                "creo B-rep curve-face nodes",
            )?;
        }
        let mut eligible_face_ids = BTreeSet::new();
        for (face_id, _) in
            ctx.admit_iter(eligible_faces, "creo B-rep eligible face map traversal")?
        {
            ctx.insert_btree_set(
                &mut eligible_face_ids,
                *face_id,
                "creo B-rep eligible face ID nodes",
            )?;
        }
        Ok(Self {
            emitted_half_edges,
            face_curves,
            closed_single_edge_curves,
            row_offsets,
            curve_faces,
            eligible_face_ids,
        })
    }
}

struct BrepBodyIndexes {
    neutral_edge_curves: BTreeSet<u32>,
    body_components: Vec<NeutralShellSpec>,
}

impl BrepBodyIndexes {
    fn from_components(
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        scan: &ContainerScan,
        admitted_components: &[&crate::topology::FaceComponent],
        admitted_edge_curves: &BTreeSet<u32>,
        eligible_faces: &BTreeMap<u32, Vec<&crate::topology::Loop>>,
        eligible_face_ids: &BTreeSet<u32>,
        curve_faces: &BTreeMap<u32, [u32; 2]>,
    ) -> Result<Self, cadmpeg_core::CodecError> {
        let mut neutral_edge_curves = BTreeSet::new();
        for component in ctx.admit_iter(
            admitted_components,
            "creo B-rep neutral component traversal",
        )? {
            for curve_id in ctx
                .admit_iter(
                    component.curve_ids(),
                    "creo B-rep neutral component curve traversal",
                )?
                .copied()
            {
                if !ctx.contains_btree_set(
                    admitted_edge_curves,
                    &curve_id,
                    "creo admitted edge curves lookup",
                )? {
                    continue;
                }
                if matches!(
                    scan.framing.layout,
                    crate::container::Layout::LegacyAscii(_)
                ) {
                    let Some(faces) =
                        ctx.get_btree_map(curve_faces, &curve_id, "creo curve faces lookup")?
                    else {
                        continue;
                    };
                    if !ctx.contains_btree_set(
                        eligible_face_ids,
                        &faces[0],
                        "creo eligible face ids lookup",
                    )? && !ctx.contains_btree_set(
                        eligible_face_ids,
                        &faces[1],
                        "creo eligible face ids lookup",
                    )? {
                        continue;
                    }
                }
                ctx.insert_btree_set(
                    &mut neutral_edge_curves,
                    curve_id,
                    "creo B-rep neutral edge curve nodes",
                )?;
            }
        }
        let mut body_components = Vec::new();
        for component in ctx.admit_iter(admitted_components, "creo B-rep component traversal")? {
            let mut faces = Vec::new();
            for face_id in ctx
                .admit_iter(component.face_ids(), "creo B-rep component face traversal")?
                .copied()
            {
                if !ctx.contains_key_btree_map(
                    eligible_faces,
                    &face_id,
                    "creo eligible faces lookup",
                )? {
                    continue;
                }
                ctx.reserve_vec(&mut faces, 1, "creo B-rep component face IDs")?;
                faces.push(face_id);
            }
            let mut curves = BTreeSet::new();
            for curve_id in ctx
                .admit_iter(
                    component.curve_ids(),
                    "creo B-rep component curve traversal",
                )?
                .copied()
            {
                if !ctx.contains_btree_set(
                    &neutral_edge_curves,
                    &curve_id,
                    "creo neutral edge curves lookup",
                )? {
                    continue;
                }
                ctx.insert_btree_set(&mut curves, curve_id, "creo B-rep component wire nodes")?;
            }
            ctx.reserve_vec(&mut body_components, 1, "creo B-rep component records")?;
            body_components.push(NeutralShellSpec {
                faces,
                wire_curves: curves,
            });
        }
        Ok(Self {
            neutral_edge_curves,
            body_components,
        })
    }
}

fn used_brep_vertices(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    neutral_edge_curves: &BTreeSet<u32>,
    edge_vertices: &BTreeMap<u32, [u32; 2]>,
) -> Result<BTreeSet<u32>, cadmpeg_core::CodecError> {
    let mut used_vertices = BTreeSet::new();
    for curve_id in ctx.admit_iter(
        neutral_edge_curves,
        "creo B-rep neutral edge vertex traversal",
    )? {
        let Some(vertices) =
            ctx.get_btree_map(edge_vertices, curve_id, "creo edge vertices lookup")?
        else {
            continue;
        };
        for vertex_id in *vertices {
            ctx.insert_btree_set(
                &mut used_vertices,
                vertex_id,
                "creo B-rep used vertex nodes",
            )?;
        }
    }
    Ok(used_vertices)
}

struct BrepComponentTopology {
    component_face_curves: BTreeSet<u32>,
    wire_curves: BTreeSet<u32>,
    face_adjacency: BTreeMap<u32, BTreeSet<u32>>,
    face_vertices: BTreeMap<u32, BTreeSet<u32>>,
}

impl BrepComponentTopology {
    fn from_component(
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        component_curves: &BTreeSet<u32>,
        face_curves: &BTreeSet<u32>,
        faces: &[u32],
        eligible_faces: &BTreeMap<u32, Vec<&crate::topology::Loop>>,
        edge_vertices: &BTreeMap<u32, [u32; 2]>,
    ) -> Result<Self, cadmpeg_core::CodecError> {
        let mut component_face_curves = BTreeSet::new();
        let mut wire_curves = BTreeSet::new();
        for curve_id in ctx.admit_iter(
            component_curves,
            "creo B-rep component curve partition traversal",
        )? {
            if ctx.contains_btree_set(face_curves, curve_id, "creo face curves lookup")? {
                ctx.insert_btree_set(
                    &mut component_face_curves,
                    *curve_id,
                    "creo B-rep component face curve nodes",
                )?;
            } else {
                ctx.insert_btree_set(
                    &mut wire_curves,
                    *curve_id,
                    "creo B-rep component wire curve nodes",
                )?;
            }
        }
        let mut face_adjacency = BTreeMap::new();
        let mut face_vertices = BTreeMap::new();
        let mut incidence_storage = ctx.reserve_scoped(0, "creo component incidence workspace")?;
        let mut faces_by_curve = BTreeMap::new();
        let mut faces_by_vertex = BTreeMap::new();
        for face_id in ctx.admit_iter(faces, "creo B-rep face traversal")? {
            ctx.entry_btree_map(
                &mut face_adjacency,
                *face_id,
                "creo B-rep adjacency face nodes",
            )?
            .or_default();
            let vertices = ctx
                .entry_btree_map(
                    &mut face_vertices,
                    *face_id,
                    "creo B-rep face vertex map nodes",
                )?
                .or_default();
            for native_loop in ctx.admit_iter(
                ctx.get_btree_map(eligible_faces, face_id, "creo eligible faces lookup")?
                    .ok_or_else(|| {
                        cadmpeg_core::CodecError::malformed("eligible faces indexed record")
                    })?,
                "creo B-rep eligible face loop traversal",
            )? {
                for half_edge in ctx.admit_iter(
                    native_loop.half_edges(),
                    "creo B-rep native loop half edge traversal",
                )? {
                    incidence_storage.with_storage(|| {
                        let curve_faces = ctx
                            .entry_btree_map(
                                &mut faces_by_curve,
                                half_edge.curve_id,
                                "creo B-rep curve incidence map nodes",
                            )?
                            .or_default();
                        ctx.insert_btree_set(
                            curve_faces,
                            *face_id,
                            "creo B-rep curve incident face nodes",
                        )?;
                        Ok::<_, cadmpeg_core::CodecError>(())
                    })?;
                    let [start, end] = *ctx
                        .get_btree_map(
                            edge_vertices,
                            &half_edge.curve_id,
                            "creo edge vertices lookup",
                        )?
                        .ok_or_else(|| {
                            cadmpeg_core::CodecError::malformed("edge vertices indexed record")
                        })?;
                    for vertex_id in [start, end] {
                        ctx.insert_btree_set(vertices, vertex_id, "creo B-rep face vertex nodes")?;
                        incidence_storage.with_storage(|| {
                            let vertex_faces = ctx
                                .entry_btree_map(
                                    &mut faces_by_vertex,
                                    vertex_id,
                                    "creo B-rep vertex incidence map nodes",
                                )?
                                .or_default();
                            ctx.insert_btree_set(
                                vertex_faces,
                                *face_id,
                                "creo B-rep vertex incident face nodes",
                            )?;
                            Ok::<_, cadmpeg_core::CodecError>(())
                        })?;
                    }
                }
            }
        }
        for incident_faces in ctx
            .admit_iter(&faces_by_curve, "creo B-rep curve incidence traversal")?
            .map(|(_, faces)| faces)
            .chain(
                ctx.admit_iter(&faces_by_vertex, "creo B-rep vertex incidence traversal")?
                    .map(|(_, faces)| faces),
            )
        {
            for (index, first) in ctx
                .admit_iter(incident_faces, "creo B-rep incident face traversal")?
                .enumerate()
            {
                for second in ctx
                    .admit_iter(
                        incident_faces,
                        "creo B-rep incident face neighbour traversal",
                    )?
                    .skip(index + 1)
                {
                    ctx.insert_btree_set(
                        ctx.entry_btree_map(
                            &mut face_adjacency,
                            *first,
                            "creo B-rep adjacency face nodes",
                        )?
                        .or_default(),
                        *second,
                        "creo B-rep adjacency neighbour nodes",
                    )?;
                    ctx.insert_btree_set(
                        ctx.entry_btree_map(
                            &mut face_adjacency,
                            *second,
                            "creo B-rep adjacency face nodes",
                        )?
                        .or_default(),
                        *first,
                        "creo B-rep adjacency neighbour nodes",
                    )?;
                }
            }
        }
        Ok(Self {
            component_face_curves,
            wire_curves,
            face_adjacency,
            face_vertices,
        })
    }
}

struct BrepShellReferences {
    face_ids: Vec<FaceId>,
    edge_ids: Vec<EdgeId>,
}

impl BrepShellReferences {
    fn from_shell(
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        shell: &NeutralShellSpec,
        shell_id: &ShellId,
        face_shell_ids: &mut BTreeMap<u32, ShellId>,
        storage: &mut cadmpeg_core::decode::ScopedReservation<'_>,
    ) -> Result<Self, cadmpeg_core::CodecError> {
        let mut face_ids = Vec::new();
        for face_id in ctx.admit_iter(&shell.faces, "creo from shell faces traversal")? {
            storage.with_storage(|| {
                ctx.insert_btree_map(
                    face_shell_ids,
                    *face_id,
                    crate::identity::copy_checked_id(
                        ctx,
                        shell_id.as_str(),
                        "creo B-rep face-shell identity copies",
                    )?,
                    "creo B-rep face-shell nodes",
                )
            })?;
            ctx.reserve_vec(&mut face_ids, 1, "creo B-rep shell face references")?;
            face_ids.push(crate::identity::compose_checked::<FaceId>(
                ctx,
                &crate::identity::VISIBGEOM_FACE,
                *face_id,
                "creo B-rep shell face identities",
            )?);
        }
        let mut edge_ids = Vec::new();
        for curve_id in
            ctx.admit_iter(&shell.wire_curves, "creo from shell wire curves traversal")?
        {
            ctx.reserve_vec(&mut edge_ids, 1, "creo B-rep shell edge references")?;
            edge_ids.push(crate::identity::compose_checked::<EdgeId>(
                ctx,
                &crate::identity::VISIBGEOM_EDGE,
                *curve_id,
                "creo B-rep shell edge identities",
            )?);
        }
        Ok(Self { face_ids, edge_ids })
    }
}

struct BrepFaceReferences {
    face: FaceId,
    shell_id: ShellId,
    loop_ids: Vec<LoopId>,
    face_loops: cadmpeg_ir::topology::FaceLoops,
}

impl BrepFaceReferences {
    fn from_loops(
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        face_id: u32,
        shell_id: &ShellId,
        loop_count: usize,
    ) -> Result<Self, cadmpeg_core::CodecError> {
        let face = crate::identity::compose_checked(
            ctx,
            &crate::identity::VISIBGEOM_FACE,
            face_id,
            "creo B-rep face identity",
        )?;
        let shell_id = crate::identity::copy_checked_id(
            ctx,
            shell_id.as_str(),
            "creo B-rep face shell identity copy",
        )?;
        let mut loop_ids = Vec::new();
        let loop_range = 0..loop_count;
        for index in ctx.admit_iter(&loop_range, "creo B-rep face loop index traversal")? {
            ctx.reserve_vec(&mut loop_ids, 1, "creo B-rep face loop IDs")?;
            let id: LoopId = if index == 0 {
                crate::identity::compose_checked(
                    ctx,
                    &crate::identity::VISIBGEOM_LOOP,
                    face_id,
                    "creo B-rep loop identities",
                )?
            } else {
                crate::identity::compose_checked(
                    ctx,
                    &crate::identity::VISIBGEOM_LOOP,
                    format_args!("{face_id}:{index}"),
                    "creo B-rep loop identities",
                )?
            };
            loop_ids.push(id);
        }
        let face_loops = match loop_ids.split_first() {
            Some((outer, inner)) => {
                let outer = crate::identity::copy_checked_id(
                    ctx,
                    outer.as_str(),
                    "creo B-rep outer loop ID copy",
                )?;
                let mut inner_ids = Vec::new();
                for id in ctx.admit_iter(inner, "creo B-rep inner loop identity traversal")? {
                    ctx.reserve_vec(&mut inner_ids, 1, "creo B-rep inner loop IDs")?;
                    inner_ids.push(crate::identity::copy_checked_id(
                        ctx,
                        id.as_str(),
                        "creo B-rep inner loop ID copies",
                    )?);
                }
                cadmpeg_ir::topology::FaceLoops::classified(outer, inner_ids)
            }
            None => cadmpeg_ir::topology::FaceLoops::unspecified(Vec::new()),
        };
        Ok(Self {
            face,
            shell_id,
            loop_ids,
            face_loops,
        })
    }
}

fn unique_native_model_surface<'a>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan<'_>,
    surfaces: &'a [Surface],
    face_id: u32,
    index: &mut super::model_ids::ModelIdentityIndex<'_>,
) -> Result<Option<&'a Surface>, cadmpeg_core::CodecError> {
    let (key, _key_storage) = crate::identity::compose_scoped::<SurfaceId>(
        ctx,
        &native_surface_namespace(ctx, scan, face_id)?.0,
        face_id,
        "creo B-rep surface query",
    )?;
    Ok(index
        .lookup(ctx, surfaces, |surface| surface.id.as_str(), key.as_str())?
        .unique_position()
        .map(|position| &surfaces[position]))
}

fn native_loop_ring(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    native_loop: &crate::topology::Loop,
    face_id: u32,
) -> Result<cadmpeg_ir::topology::LoopRing, cadmpeg_core::CodecError> {
    let mut coedge_ids = Vec::new();
    for half_edge in ctx.admit_iter(
        native_loop.half_edges(),
        "creo B-rep native loop half edge traversal",
    )? {
        ctx.reserve_vec(&mut coedge_ids, 1, "creo B-rep ring coedge IDs")?;
        coedge_ids.push(crate::identity::compose_checked::<CoedgeId>(
            ctx,
            &crate::identity::VISIBGEOM_COEDGE,
            format_args!("{}:{}", half_edge.curve_id, half_edge.side.index()),
            "creo B-rep ring coedge identities",
        )?);
    }
    match cadmpeg_ir::topology::LoopRing::new(ctx, coedge_ids, Vec::new())
        .map_err(cadmpeg_core::CodecError::from)?
    {
        Ok(ring) => Ok(ring),
        Err(error) => Err(cadmpeg_core::CodecError::Malformed(ctx.format_retained(
            format_args!("VisibGeom face {face_id} loop ring: {error}"),
            "creo B-rep loop ring error text",
        )?)),
    }
}

fn push_untransferred_pcurve_loss(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
    curve_id: u32,
    face_id: u32,
    record: impl std::fmt::Display,
) -> Result<(), cadmpeg_core::CodecError> {
    let message = ctx.format_retained(
        format_args!(
            "VisibGeom curve row {curve_id} on face {face_id} states no \
             pcurve carrier: {record}"
        ),
        "creo B-rep untransferred pcurve loss text",
    )?;
    ctx.reserve_vec(losses, 1, "creo B-rep untransferred pcurve losses")?;
    losses.push(crate::loss::CreoLossCode::VisibGeomCurveUntransferred.note(message));
    Ok(())
}

fn one_coedge_pcurve_use(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    value: Option<PcurveUse>,
) -> Result<Vec<PcurveUse>, cadmpeg_core::CodecError> {
    let mut pcurves = Vec::new();
    if let Some(value) = value {
        ctx.reserve_vec(&mut pcurves, 1, "creo B-rep coedge pcurve uses")?;
        pcurves.push(value);
    }
    Ok(pcurves)
}

/// Transfer the native `VisibGeom` B-rep: bodies, faces, loops, and coedges.
///
/// A coedge whose projected pcurve lane the IR carrier refuses is emitted
/// without a pcurve use, which the model carries, so the refusal is a loss
/// note naming the curve row and the face.
pub(in super::super) fn transfer_native_brep<'ctx>(
    ctx: &'ctx cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    curve_evidence: NativeBrepCurveEvidence<'_>,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
    source_carriers: &mut crate::decode::source_carriers::SourceUnitCarriers,
) -> Result<
    (
        NativeBrepTransferSummary,
        cadmpeg_core::decode::ScopedReservation<'ctx>,
    ),
    cadmpeg_core::CodecError,
> {
    let mut diagnostic_storage = ctx.reserve_scoped(0, "creo B-rep diagnostic workspace")?;
    let mut workspace = ctx.reserve_scoped(0, "creo B-rep reconstruction workspace")?;
    let mut curves_index = super::model_ids::ModelIdentityIndex::new(ctx)?;
    let mut pcurves_index = super::model_ids::ModelIdentityIndex::new(ctx)?;
    let mut edges_index = super::model_ids::ModelIdentityIndex::new(ctx)?;
    let mut points_index = super::model_ids::ModelIdentityIndex::new(ctx)?;
    let mut vertices_index = super::model_ids::ModelIdentityIndex::new(ctx)?;
    let mut surfaces_index = super::model_ids::ModelIdentityIndex::new(ctx)?;
    let carriers = workspace.with_storage(|| placed_carriers(ctx, scan, ir, source_carriers))?;
    let BrepSourceIndexes {
        planes,
        half_edges,
        incidence,
    } = workspace.with_storage(|| BrepSourceIndexes::from_scan(ctx, &carriers, scan))?;
    let face_orientations = workspace.with_storage(|| native_face_orientations(ctx, scan, ir))?;
    let solved_vertex_result = workspace.with_storage(|| {
        solve_topological_vertices(
            ctx,
            scan,
            ir,
            &carriers,
            curve_evidence.nurbs_endpoints,
            source_carriers,
        )
    })?;
    let solved_vertices = &solved_vertex_result.points;
    let mut native_pcurves = NativePcurveCandidates::new();
    for candidate in ctx
        .admit_iter(&scan.curves.pcurves, "creo B-rep native pcurve traversal")?
        .map(|pcurve| -> Result<_, cadmpeg_core::CodecError> {
            let [face_0_endpoints, face_1_endpoints] = canonicalized_pcurve_endpoints(
                ctx,
                scan,
                pcurve.faces,
                pcurve.face_0_endpoints,
                pcurve.face_1_endpoints,
            )?;
            Ok((
                pcurve.curve_id,
                pcurve.faces,
                face_0_endpoints,
                face_1_endpoints,
                pcurve.offset,
            ))
        })
        .chain(
            ctx.admit_iter(
                &scan.curves.bound_prototype_pcurves,
                "creo B-rep bound prototype pcurve traversal",
            )?
            .map(|pcurve| -> Result<_, cadmpeg_core::CodecError> {
                let [face_0_endpoints, face_1_endpoints] = canonicalized_pcurve_endpoints(
                    ctx,
                    scan,
                    pcurve.faces,
                    pcurve.face_0_endpoints,
                    pcurve.face_1_endpoints,
                )?;
                Ok((
                    pcurve.curve_id,
                    pcurve.faces,
                    face_0_endpoints,
                    face_1_endpoints,
                    pcurve.offset,
                ))
            }),
        )
    {
        let (curve_id, faces, face_0_endpoints, face_1_endpoints, offset) = candidate?;
        for (face, endpoints) in faces.into_iter().zip([face_0_endpoints, face_1_endpoints]) {
            if let Some(face) = face {
                workspace.with_storage(|| {
                    push_native_pcurve_candidate(
                        ctx,
                        &mut native_pcurves,
                        curve_id,
                        face.get(),
                        endpoints,
                        offset,
                    )
                })?;
            }
        }
    }
    for pcurve in ctx.admit_iter(
        &scan.curves.two_chart_pcurves,
        "creo transfer native brep two chart pcurves traversal",
    )? {
        let Some(endpoint_sets) = crate::decode::analytic::pcurves::mapped_two_chart_endpoint_sets(
            ctx,
            scan,
            ir,
            pcurve,
            source_carriers,
        )?
        else {
            continue;
        };
        for (face_id, endpoints) in pcurve.faces.into_iter().zip(endpoint_sets.paths()) {
            if let Some(endpoints) = endpoints {
                workspace.with_storage(|| {
                    push_native_pcurve_candidate(
                        ctx,
                        &mut native_pcurves,
                        pcurve.curve_id,
                        face_id,
                        endpoints,
                        pcurve.offset,
                    )
                })?;
            }
        }
    }
    let short_pcurves = workspace.with_storage(|| {
        crate::curve::fc02_short_pcurve_endpoints(
            ctx,
            &scan.curves.parameters,
            &scan.curves.topology_rows,
        )
    })?;
    for pcurve in ctx.admit_iter(&short_pcurves, "creo B-rep short pcurve traversal")? {
        let [face_0_endpoints, _] = canonicalized_pcurve_endpoints(
            ctx,
            scan,
            pcurve.faces.map(std::num::NonZeroU32::new),
            pcurve.face_0_endpoints,
            pcurve.face_0_endpoints,
        )?;
        workspace.with_storage(|| {
            push_native_pcurve_candidate(
                ctx,
                &mut native_pcurves,
                pcurve.curve_id,
                pcurve.faces[0],
                face_0_endpoints,
                pcurve.offset,
            )
        })?;
    }
    let native_edge_vertices = workspace.with_storage(|| {
        crate::topology::edge_vertex_pairs(ctx, &scan.topology.half_edge_vertex_incidence)
    })?;
    let BrepEdgeIndexes {
        edge_vertices,
        model_curve_counts,
        admitted_edge_curves,
    } = workspace.with_storage(|| {
        BrepEdgeIndexes::from_rows(
            ctx,
            &scan.curves.topology_rows,
            &native_edge_vertices,
            solved_vertices,
            ir,
        )
    })?;
    let BrepFaceCandidateIndexes {
        loops_by_face,
        candidate_face_ids,
        model_surface_counts,
        boundary_curve_ids,
        legacy_nonvisible_face_reference_count,
    } = workspace.with_storage(|| BrepFaceCandidateIndexes::from_scan(ctx, scan, ir))?;
    let typed_nonlinear_curve_ids =
        workspace.with_storage(|| model_typed_nonlinear_curve_ids(ctx, ir, source_carriers))?;
    let mut diagnostics = BrepTransferDiagnostics {
        candidate_face_count: candidate_face_ids.len(),
        legacy_nonvisible_face_reference_count,
        vertex_solve: solved_vertex_result.diagnostics,
        ..BrepTransferDiagnostics::default()
    };
    diagnostics.boundary_curve_count = boundary_curve_ids.len();
    for curve_id in ctx.admit_iter(
        &boundary_curve_ids,
        "creo B-rep boundary incidence diagnostics",
    )? {
        let Some(vertices) = ctx.get_btree_map(
            &native_edge_vertices,
            curve_id,
            "creo native edge vertices lookup",
        )?
        else {
            diagnostics.boundary_curve_missing_incidence_count += 1;
            continue;
        };
        if !ctx.contains_key_btree_map(
            solved_vertices,
            &vertices[0].get(),
            "creo solved vertices lookup",
        )? || !ctx.contains_key_btree_map(
            solved_vertices,
            &vertices[1].get(),
            "creo solved vertices lookup",
        )? {
            diagnostics.boundary_curve_unsolved_vertex_count += 1;
        }
    }
    let mut eligible_faces = BTreeMap::new();
    for face_id in ctx
        .admit_iter(&candidate_face_ids, "creo B-rep candidate face traversal")?
        .copied()
    {
        let model_surface_count = *ctx
            .get_btree_map(
                &model_surface_counts,
                &face_id,
                "creo model surface counts lookup",
            )?
            .ok_or_else(|| cadmpeg_core::CodecError::malformed("candidate face count"))?;
        if model_surface_count == 0 {
            diagnostic_storage.with_storage(|| {
                diagnostics.reject_face(ctx, FaceAdmissionRejection::MissingSurfaceCarrier, face_id)
            })?;
            continue;
        }
        if !ctx.contains_key_btree_map(
            &face_orientations,
            &face_id,
            "creo face orientations lookup",
        )? {
            diagnostic_storage.with_storage(|| {
                diagnostics.reject_face(ctx, FaceAdmissionRejection::MissingOrientation, face_id)
            })?;
            continue;
        }
        if model_surface_count > 1 {
            diagnostic_storage.with_storage(|| {
                diagnostics.reject_face(
                    ctx,
                    FaceAdmissionRejection::AmbiguousSurfaceCarrier,
                    face_id,
                )
            })?;
            continue;
        }
        let Some(loops) =
            ctx.get_btree_map(&loops_by_face, &face_id, "creo loops by face lookup")?
        else {
            diagnostic_storage.with_storage(|| {
                diagnostics.reject_face(ctx, FaceAdmissionRejection::MissingLoops, face_id)
            })?;
            continue;
        };
        let mut has_unresolved_boundary_vertices = false;
        let mut visits = loops.iter();
        while visits.len() != 0 {
            let Some(lp) = ctx.next_charged(&mut visits, "creo B-rep unresolved boundary loop traversal")? else {
                break;
            };
            if ctx.any_by(
                lp.half_edges(),
                |half_edge| {
                    Ok(!ctx.contains_key_btree_map(
                        &edge_vertices,
                        &half_edge.curve_id,
                        "creo edge vertices lookup",
                    )?)
                },
                "creo B-rep unresolved boundary edge search",
            )? {
                has_unresolved_boundary_vertices = true;
                break;
            }
        }
        if has_unresolved_boundary_vertices {
            diagnostic_storage.with_storage(|| {
                diagnostics.reject_face_with_detail(
                    ctx,
                    FaceAdmissionRejection::UnresolvedBoundaryVertices,
                    FaceAdmissionDetail::unresolved_boundary(
                        ctx,
                        face_id,
                        loops,
                        &edge_vertices,
                        &incidence,
                    )?,
                )
            })?;
            continue;
        }
        let mut has_ambiguous_boundary_curve = false;
        let mut visits = loops.iter();
        while visits.len() != 0 {
            let Some(lp) = ctx.next_charged(&mut visits, "creo B-rep ambiguous boundary loop traversal")? else {
                break;
            };
            if ctx.any_by(
                lp.half_edges(),
                |half_edge| {
                    Ok(ctx
                        .get_btree_map(
                            &model_curve_counts,
                            &half_edge.curve_id,
                            "creo model curve counts lookup",
                        )?
                        .is_some_and(|count| *count > 1))
                },
                "creo B-rep ambiguous boundary edge search",
            )? {
                has_ambiguous_boundary_curve = true;
                break;
            }
        }
        if has_ambiguous_boundary_curve {
            diagnostic_storage.with_storage(|| {
                diagnostics.reject_face(
                    ctx,
                    FaceAdmissionRejection::AmbiguousBoundaryCurve,
                    face_id,
                )
            })?;
            continue;
        }
        let mut two_edge_loops_are_proven = true;
        let mut visits = loops.iter();
        while visits.len() != 0 {
            let Some(lp) = ctx.next_charged(&mut visits, "creo B-rep two edge loop traversal")? else {
                break;
            };
            if lp.half_edges().len() != 2 {
                continue;
            }
            let Some(surface) = unique_native_model_surface(
                ctx,
                scan,
                &ir.model.surfaces,
                face_id,
                &mut surfaces_index,
            )?
            else {
                two_edge_loops_are_proven = false;
                break;
            };
            let mut proof_storage =
                ctx.reserve_scoped(0, "creo two edge parameter proof workspace")?;
            if proof_storage
                .with_storage(|| {
                    native_parameter_loop_polygon(
                        ctx,
                        lp,
                        (face_id, source_carriers.surface_geometry(surface)?),
                        &incidence,
                        solved_vertices,
                        &native_pcurves,
                        &typed_nonlinear_curve_ids,
                    )
                })?
                .is_none()
            {
                two_edge_loops_are_proven = false;
                break;
            }
        }
        if !two_edge_loops_are_proven {
            diagnostic_storage.with_storage(|| {
                diagnostics.reject_face(ctx, FaceAdmissionRejection::TwoEdgeParameterProof, face_id)
            })?;
            continue;
        }
        let ordered = workspace.with_storage(|| {
            ordered_face_loops(
                ctx,
                loops,
                ctx.get_btree_map(&planes, &face_id, "creo planes lookup")?
                    .copied(),
                &incidence,
                solved_vertices,
            )
        })?;
        let ordered = if ordered.is_some() {
            ordered
        } else {
            let surface = unique_native_model_surface(
                ctx,
                scan,
                &ir.model.surfaces,
                face_id,
                &mut surfaces_index,
            )?;
            if let Some(surface) = surface {
                workspace.with_storage(|| {
                    ordered_native_parameter_face_loops(
                        ctx,
                        loops,
                        (face_id, source_carriers.surface_geometry(surface)?),
                        &incidence,
                        solved_vertices,
                        &native_pcurves,
                        NativeCurveEvidence {
                            typed_nonlinear_curve_ids: &typed_nonlinear_curve_ids,
                            model_curves: &ir.model.curves,
                            source_carriers,
                        },
                    )
                })?
            } else {
                None
            }
        };
        let Some(ordered) = ordered else {
            diagnostic_storage.with_storage(|| {
                diagnostics.reject_face(ctx, FaceAdmissionRejection::LoopOrdering, face_id)
            })?;
            continue;
        };
        workspace.with_storage(|| {
            ctx.insert_btree_map(
                &mut eligible_faces,
                face_id,
                ordered,
                "creo B-rep eligible face nodes",
            )
        })?;
    }
    diagnostics.admitted_face_count = eligible_faces.len();
    let BrepEligibleFaceIndexes {
        emitted_half_edges,
        face_curves,
        closed_single_edge_curves,
        row_offsets,
        curve_faces,
        eligible_face_ids,
    } = workspace.with_storage(|| {
        BrepEligibleFaceIndexes::from_faces(ctx, &eligible_faces, &scan.curves.topology_rows)
    })?;
    let admitted_components =
        workspace.with_storage(|| admitted_face_components(ctx, scan, &eligible_face_ids))?;
    let BrepBodyIndexes {
        neutral_edge_curves,
        body_components,
    } = workspace.with_storage(|| {
        BrepBodyIndexes::from_components(
            ctx,
            scan,
            &admitted_components,
            &admitted_edge_curves,
            &eligible_faces,
            &eligible_face_ids,
            &curve_faces,
        )
    })?;
    let selected_body_count = crate::topology::selected_body_count(
        scan.framing.declared_body_count,
        scan.framing.first_quilt_ptr,
        admitted_components.len(),
    );
    let empty_component_count = ctx
        .admit_iter(&body_components, "creo B-rep empty component count")?
        .filter(|component| component.faces.is_empty() && component.wire_curves.is_empty())
        .count();
    let explicit_single_body =
        scan.framing.declared_body_count == Some(1) || scan.framing.first_quilt_ptr == Some(0);
    let body_components =
        if explicit_single_body && empty_component_count == 0 && !body_components.is_empty() {
            workspace.with_storage(|| merge_body_components(ctx, body_components))?
        } else {
            body_components
        };
    let solved_point_count = solved_vertices.len();
    for (vertex_id, position) in
        ctx.admit_iter(solved_vertices, "creo B-rep solved vertex traversal")?
    {
        let (query_id, _query_storage) = crate::identity::compose_scoped::<PointId>(
            ctx,
            &crate::identity::VISIBGEOM_POINT,
            *vertex_id,
            "creo B-rep point query",
        )?;
        if points_index
            .lookup(
                ctx,
                &ir.model.points,
                |record| record.id.as_str(),
                query_id.as_str(),
            )?
            .exists()
        {
            continue;
        }
        let point_id = crate::identity::compose_checked::<PointId>(
            ctx,
            &crate::identity::VISIBGEOM_POINT,
            vertex_id,
            "creo B-rep topological point identities",
        )?;
        annotate(
            ctx,
            annotations,
            &point_id,
            "VisibGeom",
            0,
            "topological_vertex_point",
            Exactness::Derived,
        )?;
        let source_object = SourceObjectAssociation {
            format: cadmpeg_ir::CodecFormat::Creo,
            object_id: cadmpeg_core::text::NonBlankString::for_decode(
                ctx,
                ctx.format_retained(
                    format_args!("topology:vertex#{vertex_id}"),
                    "creo B-rep point source object IDs",
                )?,
                "validate nonblank text",
            )?
            .ok_or_else(|| {
                cadmpeg_core::CodecError::malformed("source object_id must not be empty")
            })?,
            name: None,
            color: None,
            visible: None,
            layer: None,
            instance_path: Vec::new(),
        };
        let position = cadmpeg_ir::features::FinitePoint3::new(Point3::from(*position))
            .ok_or(Point::NON_FINITE_POSITION)
            .map_err(cadmpeg_core::CodecError::malformed)?;
        source_carriers.admit_point(
            ctx,
            ir,
            Point::new(point_id, position, Some(source_object)),
        )?;
    }
    diagnostics.body_count_mismatch =
        !body_components.is_empty() && selected_body_count != Some(body_components.len());
    diagnostics.legacy_body_ownership_ambiguous =
        !legacy_body_ownership_is_unambiguous(scan, admitted_components.len());
    diagnostics.empty_component_count = empty_component_count;
    diagnostics.admitted_component_count = admitted_components.len();
    diagnostics.selected_body_count = selected_body_count;
    if diagnostics.body_count_mismatch
        || diagnostics.legacy_body_ownership_ambiguous
        || diagnostics.empty_component_count != 0
    {
        return Ok((
            NativeBrepTransferSummary {
                topological_point_count: solved_point_count,
                diagnostics,
                ..NativeBrepTransferSummary::default()
            },
            diagnostic_storage,
        ));
    }
    diagnostics.emitted_face_count = ctx
        .admit_iter(&body_components, "creo B-rep emitted face count")?
        .map(|component| component.faces.len())
        .sum();

    let used_vertices =
        workspace.with_storage(|| used_brep_vertices(ctx, &neutral_edge_curves, &edge_vertices))?;

    for vertex_id in ctx
        .admit_iter(&used_vertices, "creo B-rep used vertex traversal")?
        .copied()
    {
        let (query_id, _query_storage) = crate::identity::compose_scoped::<VertexId>(
            ctx,
            &crate::identity::VISIBGEOM_VERTEX,
            vertex_id,
            "creo B-rep vertex query",
        )?;
        if vertices_index
            .lookup(
                ctx,
                &ir.model.vertices,
                |record| record.id.as_str(),
                query_id.as_str(),
            )?
            .exists()
        {
            continue;
        }
        let vertex = crate::identity::compose_checked::<VertexId>(
            ctx,
            &crate::identity::VISIBGEOM_VERTEX,
            vertex_id,
            "creo B-rep vertex identities",
        )?;
        let point_id = crate::identity::compose_checked::<PointId>(
            ctx,
            &crate::identity::VISIBGEOM_POINT,
            vertex_id,
            "creo B-rep vertex point identities",
        )?;
        annotate(
            ctx,
            annotations,
            &vertex,
            "VisibGeom",
            0,
            "topological_vertex_orbit",
            Exactness::Derived,
        )?;
        ctx.charge_entities(1, "admit Creo model vertices")?;
        source_carriers.admit_vertex(
            ctx,
            ir,
            Vertex {
                id: vertex,
                point: point_id,
                tolerance: None,
            },
        )?;
    }
    for curve_id in ctx.admit_iter(
        &neutral_edge_curves,
        "creo transfer native brep neutral edge curves traversal",
    )? {
        let [start, end] = *ctx
            .get_btree_map(&edge_vertices, curve_id, "creo edge vertices lookup")?
            .ok_or_else(|| cadmpeg_core::CodecError::malformed("edge vertices indexed record"))?;
        let (curve, curve_storage) = crate::identity::compose_scoped::<CurveId>(
            ctx,
            &crate::identity::VISIBGEOM_CURVE,
            *curve_id,
            "creo B-rep edge curve identities",
        )?;
        let points = [
            *ctx.get_btree_map(solved_vertices, &start, "creo solved vertices lookup")?
                .ok_or_else(|| {
                    cadmpeg_core::CodecError::malformed("solved vertices indexed record")
                })?,
            *ctx.get_btree_map(solved_vertices, &end, "creo solved vertices lookup")?
                .ok_or_else(|| {
                    cadmpeg_core::CodecError::malformed("solved vertices indexed record")
                })?,
        ];
        let unbacked_closed_edge = if start == end
            && ctx.contains_btree_set(
                &closed_single_edge_curves,
                curve_id,
                "creo closed single edge curves lookup",
            )? {
            if let Some(face_ids) =
                ctx.get_btree_map(&curve_faces, curve_id, "creo curve faces lookup")?
            {
                !ctx.contains_key_btree_map(
                    &native_pcurves,
                    &(*curve_id, face_ids[0]),
                    "creo native pcurves lookup",
                )? && !ctx.contains_key_btree_map(
                    &native_pcurves,
                    &(*curve_id, face_ids[1]),
                    "creo native pcurves lookup",
                )?
            } else {
                false
            }
        } else {
            false
        };
        let model_curve_count = *ctx
            .get_btree_map(
                &model_curve_counts,
                curve_id,
                "creo model curve counts lookup",
            )?
            .ok_or_else(|| {
                cadmpeg_core::CodecError::malformed("model curve counts indexed record")
            })?;
        let param_range = if model_curve_count == 0 {
            None
        } else {
            let candidate = curves_index
                .lookup(
                    ctx,
                    &ir.model.curves,
                    |record| record.id.as_str(),
                    curve.as_str(),
                )?
                .unique_position()
                .map(|index| &mut ir.model.curves[index]);
            if let Some(candidate) = candidate {
                let mut geometry = source_carriers
                    .curve_geometry(candidate)?
                    .try_clone_for_decode(ctx, "creo B-rep edge source curve geometry")?;
                let derived_line =
                    ctx.contains_btree_set(
                        curve_evidence.derived_intersections,
                        &curve,
                        "creo derived intersection curve lookup",
                    )? && matches!(geometry.solved(), Some(SolvedCurveGeometry::Line(_)));
                let range = if derived_line {
                    orient_line_edge_carrier(&mut geometry, points)
                } else {
                    let mut range =
                        orient_nonperiodic_nurbs_edge_carrier(ctx, &mut geometry, points)?;
                    if range.is_none() {
                        range = exact_line_edge_parameter_range(&geometry, points);
                    }
                    if range.is_none() {
                        range = nonperiodic_conic_edge_parameter_range(&geometry, points);
                    }
                    if range.is_none() {
                        if let Some(faces) = ctx
                            .get_btree_map(&curve_faces, curve_id, "creo curve faces lookup")?
                            .copied()
                        {
                            range = pcurve_backed_periodic_conic_parameter_range(
                                ctx,
                                &geometry,
                                (*curve_id, faces),
                                &native_pcurves,
                                &ir.model.surfaces,
                                points,
                                source_carriers,
                            )?;
                        }
                    }
                    if range.is_none() && unbacked_closed_edge {
                        range = full_periodic_conic_edge_parameter_range(&geometry, points[0]);
                    }
                    if range.is_none() && unbacked_closed_edge {
                        range =
                            full_periodic_nurbs_edge_parameter_range(ctx, &geometry, points[0])?;
                    }
                    range
                };
                source_carriers.replace_curve_geometry(ctx, candidate, geometry)?;
                range
            } else {
                None
            }
        };
        let id = crate::identity::compose_checked::<EdgeId>(
            ctx,
            &crate::identity::VISIBGEOM_EDGE,
            *curve_id,
            "creo B-rep edge identities",
        )?;
        annotate(
            ctx,
            annotations,
            &id,
            "VisibGeom",
            cadmpeg_core::decode::u64_from_index(
                ctx.get_btree_map(&row_offsets, curve_id, "creo row offsets lookup")?
                    .copied()
                    .unwrap_or(0),
            ),
            "curve_topology_edge",
            Exactness::Derived,
        )?;
        ctx.charge_entities(1, "admit Creo model edges")?;
        source_carriers.admit_edge(
            ctx,
            ir,
            Edge {
                id,
                carrier: cadmpeg_ir::topology::EdgeCarrier::new(
                    Some(crate::identity::copy_checked_id(
                        ctx,
                        curve.as_str(),
                        "creo B-rep edge carrier curve ID copies",
                    )?),
                    param_range,
                )
                .map_err(cadmpeg_core::CodecError::malformed)?,
                start: crate::identity::compose_checked(
                    ctx,
                    &crate::identity::VISIBGEOM_VERTEX,
                    start,
                    "creo B-rep edge start vertex identities",
                )?,
                end: crate::identity::compose_checked(
                    ctx,
                    &crate::identity::VISIBGEOM_VERTEX,
                    end,
                    "creo B-rep edge end vertex identities",
                )?,
                tolerance: None,
            },
        )?;
        let identity_present = curves_index
            .lookup(
                ctx,
                &ir.model.curves,
                |record| record.id.as_str(),
                curve.as_str(),
            )?
            .exists();
        if !identity_present {
            let offset = ctx
                .get_btree_map(&row_offsets, curve_id, "creo row offsets lookup")?
                .copied()
                .unwrap_or(0);
            curve_storage.commit()?;
            annotate(
                ctx,
                annotations,
                &curve,
                "VisibGeom",
                cadmpeg_core::decode::u64_from_index(offset),
                "opaque_native_curve_carrier",
                Exactness::Unknown,
            )?;
            ctx.charge_entities(1, "admit Creo model curves")?;
            source_carriers.admit_curve(
                ctx,
                ir,
                Curve {
                    id: curve,
                    geometry: CurveGeometry::Solved(SolvedCurveGeometry::Unknown {
                        record: geometry_section_record(ctx, scan, offset)?,
                    }),
                    source_object: Some(SourceObjectAssociation {
                        format: cadmpeg_ir::CodecFormat::Creo,
                        object_id: cadmpeg_core::text::NonBlankString::for_decode(
                            ctx,
                            ctx.format_retained(
                                format_args!("VisibGeom:{curve_id}"),
                                "creo B-rep curve source object IDs",
                            )?,
                            "validate nonblank text",
                        )?
                        .ok_or_else(|| {
                            cadmpeg_core::CodecError::malformed(
                                "source object_id must not be empty",
                            )
                        })?,
                        name: None,
                        color: None,
                        visible: None,
                        layer: None,
                        instance_path: Vec::new(),
                    }),
                },
            )?;
        }
    }

    for (component_index, component) in ctx
        .admit_iter(&body_components, "creo B-rep body component traversal")?
        .enumerate()
    {
        let faces = &component.faces;
        let component_curves = &component.wire_curves;
        let body_id = crate::identity::compose_checked::<BodyId>(
            ctx,
            &crate::identity::VISIBGEOM_BODY,
            component_index + 1,
            "creo B-rep body identity",
        )?;
        let region_id = crate::identity::compose_checked::<RegionId>(
            ctx,
            &crate::identity::VISIBGEOM_REGION,
            component_index + 1,
            "creo B-rep region identity",
        )?;
        annotate(
            ctx,
            annotations,
            &body_id,
            "VisibGeom",
            0,
            "native_component_body",
            Exactness::Derived,
        )?;
        annotate(
            ctx,
            annotations,
            &region_id,
            "VisibGeom",
            0,
            "native_component_region",
            Exactness::Derived,
        )?;
        let mut component_workspace = ctx.reserve_scoped(0, "creo B-rep component workspace")?;
        let BrepComponentTopology {
            component_face_curves,
            wire_curves,
            face_adjacency,
            face_vertices,
        } = component_workspace.with_storage(|| {
            BrepComponentTopology::from_component(
                ctx,
                component_curves,
                &face_curves,
                faces,
                &eligible_faces,
                &edge_vertices,
            )
        })?;
        let closed = component_is_closed(
            ctx,
            &component_face_curves,
            &emitted_half_edges,
            &half_edges,
            faces,
        )?;

        let shell_specs = component_workspace.with_storage(|| {
            split_neutral_component_shells(
                ctx,
                faces,
                &wire_curves,
                &face_adjacency,
                &face_vertices,
                &edge_vertices,
            )
        })?;

        let mut face_shell_ids = BTreeMap::<u32, ShellId>::new();
        let mut shell_ids = Vec::new();
        for (shell_index, shell) in ctx
            .admit_iter(&shell_specs, "creo B-rep shell traversal")?
            .enumerate()
        {
            let shell_id = if shell_index == 0 {
                crate::identity::compose_checked::<ShellId>(
                    ctx,
                    &crate::identity::VISIBGEOM_SHELL,
                    component_index + 1,
                    "creo B-rep shell identity",
                )?
            } else {
                crate::identity::compose_checked::<ShellId>(
                    ctx,
                    &crate::identity::VISIBGEOM_SHELL,
                    format_args!("{}:{}", component_index + 1, shell_index + 1),
                    "creo B-rep shell identity",
                )?
            };
            annotate(
                ctx,
                annotations,
                &shell_id,
                "VisibGeom",
                0,
                "native_component_shell",
                Exactness::Derived,
            )?;
            if shell.faces.is_empty() && shell.wire_curves.is_empty() {
                diagnostics.empty_component_count += 1;
                continue;
            }
            let BrepShellReferences { face_ids, edge_ids } = BrepShellReferences::from_shell(
                ctx,
                shell,
                &shell_id,
                &mut face_shell_ids,
                &mut component_workspace,
            )?;
            ctx.charge_entities(1, "admit Creo model shells")?;
            let Ok(shell_entity) = Shell::new(
                crate::identity::copy_checked_id(
                    ctx,
                    shell_id.as_str(),
                    "creo B-rep shell entity ID copy",
                )?,
                crate::identity::copy_checked_id(
                    ctx,
                    region_id.as_str(),
                    "creo B-rep shell region ID copy",
                )?,
                face_ids,
                edge_ids,
                Vec::new(),
            ) else {
                diagnostics.empty_component_count += 1;
                continue;
            };
            ctx.reserve_vec(&mut ir.model.shells, 1, "creo model shells")?;
            ir.model.shells.push(shell_entity);
            ctx.reserve_vec(&mut shell_ids, 1, "creo B-rep body shell IDs")?;
            shell_ids.push(shell_id);
        }
        let mut region_ids = Vec::new();
        ctx.reserve_vec(&mut region_ids, 1, "creo B-rep body region IDs")?;
        region_ids.push(crate::identity::copy_checked_id(
            ctx,
            region_id.as_str(),
            "creo B-rep body region identity copy",
        )?);
        ctx.charge_entities(1, "admit Creo model bodies")?;
        source_carriers.admit_body(
            ctx,
            ir,
            Body {
                id: crate::identity::copy_checked_id(
                    ctx,
                    body_id.as_str(),
                    "creo B-rep body entity ID copy",
                )?,
                kind: if !wire_curves.is_empty() {
                    BodyKind::General
                } else if closed {
                    BodyKind::Solid
                } else {
                    BodyKind::Sheet
                },
                regions: region_ids,
                transform: None,
                name: None,
                color: None,
                visible: None,
            },
        )?;
        ctx.charge_entities(1, "admit Creo model regions")?;
        ctx.reserve_vec(&mut ir.model.regions, 1, "creo model regions")?;
        ir.model.regions.push(Region {
            id: crate::identity::copy_checked_id(
                ctx,
                region_id.as_str(),
                "creo B-rep region entity ID copy",
            )?,
            body: body_id,
            shells: shell_ids,
        });
        for face_id in ctx.admit_iter(faces, "creo B-rep face traversal")? {
            let native_loops = ctx
                .get_btree_map(&eligible_faces, face_id, "creo eligible faces lookup")?
                .ok_or_else(|| {
                    cadmpeg_core::CodecError::malformed("eligible faces indexed record")
                })?;
            let BrepFaceReferences {
                face,
                shell_id,
                loop_ids,
                face_loops,
            } = BrepFaceReferences::from_loops(
                ctx,
                *face_id,
                ctx.get_btree_map(&face_shell_ids, face_id, "creo face shell ids lookup")?
                    .ok_or_else(|| {
                        cadmpeg_core::CodecError::malformed("face shell ids indexed record")
                    })?,
                native_loops.len(),
            )?;
            let visible_row = crate::surface::unique_surface_row(&scan.surfaces.rows, *face_id);
            let active_datum = ctx.find_by(
                &scan.planes.datum_cylinders,
                |datum| Ok(datum.id == *face_id),
                "creo B-rep active datum search",
            )?;
            let face_offset = visible_row
                .map(|row| row.offset)
                .or_else(|| active_datum.map(|datum| datum.offset_in_payload))
                .unwrap_or(0);
            let face_source_namespace = if visible_row.is_none() && active_datum.is_some() {
                "ActDatums"
            } else {
                "VisibGeom"
            };
            let surface = native_surface_id(ctx, scan, *face_id)?;
            let identity_present = surfaces_index
                .lookup(
                    ctx,
                    &ir.model.surfaces,
                    |record| record.id.as_str(),
                    surface.as_str(),
                )?
                .exists();
            if !identity_present {
                annotate(
                    ctx,
                    annotations,
                    &surface,
                    face_source_namespace,
                    cadmpeg_core::decode::u64_from_index(face_offset),
                    "opaque_native_surface_carrier",
                    Exactness::Unknown,
                )?;
                ctx.charge_entities(1, "admit Creo model surfaces")?;
                source_carriers.admit_surface(
                    ctx,
                    ir,
                    Surface {
                        id: crate::identity::copy_checked_id(
                            ctx,
                            surface.as_str(),
                            "creo B-rep surface entity ID copies",
                        )?,
                        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown {
                            record: geometry_section_record(ctx, scan, face_offset)?,
                        }),
                        source_object: Some(SourceObjectAssociation {
                            format: cadmpeg_ir::CodecFormat::Creo,
                            object_id: cadmpeg_core::text::NonBlankString::for_decode(
                                ctx,
                                ctx.format_retained(
                                    format_args!("VisibGeom:{face_id}"),
                                    "creo B-rep surface source object IDs",
                                )?,
                                "validate nonblank text",
                            )?
                            .ok_or_else(|| {
                                cadmpeg_core::CodecError::malformed(
                                    "source object_id must not be empty",
                                )
                            })?,
                            name: None,
                            color: None,
                            visible: None,
                            layer: None,
                            instance_path: Vec::new(),
                        }),
                    },
                )?;
            }
            let face_sense = if *ctx
                .get_btree_map(&face_orientations, face_id, "creo face orientations lookup")?
                .ok_or_else(|| {
                    cadmpeg_core::CodecError::malformed("face orientations indexed record")
                })? {
                Sense::Reversed
            } else {
                Sense::Forward
            };
            annotate(
                ctx,
                annotations,
                &face,
                "VisibGeom",
                cadmpeg_core::decode::u64_from_index(face_offset),
                "native_face",
                Exactness::Derived,
            )?;
            for loop_id in
                ctx.admit_iter(&loop_ids, "creo transfer native brep loop ids traversal")?
            {
                annotate(
                    ctx,
                    annotations,
                    loop_id,
                    "VisibGeom",
                    cadmpeg_core::decode::u64_from_index(face_offset),
                    "native_face_loop",
                    Exactness::Derived,
                )?;
            }
            ctx.charge_entities(1, "admit Creo model faces")?;
            source_carriers.admit_face(
                ctx,
                ir,
                Face {
                    id: crate::identity::copy_checked_id(
                        ctx,
                        face.as_str(),
                        "creo B-rep face entity ID copy",
                    )?,
                    shell: shell_id,
                    surface,
                    sense: face_sense,
                    loops: face_loops,
                    name: None,
                    color: None,
                    tolerance: None,
                },
            )?;
            for (native_loop, loop_id) in ctx
                .admit_iter(native_loops, "creo B-rep native loop traversal")?
                .zip(ctx.admit_iter(&loop_ids, "creo B-rep loop identity traversal")?)
            {
                let ring = native_loop_ring(ctx, native_loop, *face_id)?;
                ctx.charge_entities(1, "admit Creo model loops")?;
                ctx.reserve_vec(&mut ir.model.loops, 1, "creo model native loops")?;
                ir.model.loops.push(IrLoop {
                    id: crate::identity::copy_checked_id(
                        ctx,
                        loop_id.as_str(),
                        "creo B-rep model loop ID copy",
                    )?,
                    face: crate::identity::copy_checked_id(
                        ctx,
                        face.as_str(),
                        "creo B-rep model loop face ID copy",
                    )?,
                    boundary: cadmpeg_ir::topology::LoopBoundary::Ring(ring),
                });
                for half_edge in ctx.admit_iter(
                    native_loop.half_edges(),
                    "creo B-rep native loop half edge traversal",
                )? {
                    let id = crate::identity::compose_checked::<CoedgeId>(
                        ctx,
                        &crate::identity::VISIBGEOM_COEDGE,
                        format_args!("{}:{}", half_edge.curve_id, half_edge.side.index()),
                        "creo B-rep coedge identities",
                    )?;
                    let twin = HalfEdgeId {
                        curve_id: half_edge.curve_id,
                        side: half_edge.side.flip(),
                    };
                    let radial_next = if ctx.contains_btree_set(
                        &emitted_half_edges,
                        &twin,
                        "creo emitted half edges lookup",
                    )? {
                        crate::identity::compose_checked::<CoedgeId>(
                            ctx,
                            &crate::identity::VISIBGEOM_COEDGE,
                            format_args!("{}:{}", twin.curve_id, twin.side.index()),
                            "creo B-rep radial coedge identities",
                        )?
                    } else {
                        crate::identity::copy_checked_id(
                            ctx,
                            id.as_str(),
                            "creo B-rep self radial ID copies",
                        )?
                    };
                    annotate(
                        ctx,
                        annotations,
                        &id,
                        "VisibGeom",
                        cadmpeg_core::decode::u64_from_index(
                            ctx.get_btree_map(
                                &row_offsets,
                                &half_edge.curve_id,
                                "creo row offsets lookup",
                            )?
                            .copied()
                            .unwrap_or(0),
                        ),
                        "native_half_edge",
                        Exactness::Derived,
                    )?;
                    let native_candidates = ctx.get_btree_map(
                        &native_pcurves,
                        &(half_edge.curve_id, *face_id),
                        "creo native pcurves lookup",
                    )?;
                    let mut refusal = crate::lane_refusal::LaneRefusals::new();
                    let refusal_cell = &mut refusal;
                    let native_selection = if let Some(candidates) = native_candidates {
                        let inputs = 'inputs: {
                            let Some(binding) =
                                ctx.get_btree_map(&incidence, half_edge, "creo incidence lookup")?
                            else {
                                break 'inputs None;
                            };
                            let Some(end) = binding.end_vertex_id else {
                                break 'inputs None;
                            };
                            let traversal = [
                                *ctx.get_btree_map(
                                    solved_vertices,
                                    &binding.start_vertex_id.get(),
                                    "creo solved vertices lookup",
                                )?
                                .ok_or_else(|| {
                                    cadmpeg_core::CodecError::malformed(
                                        "solved vertices indexed record",
                                    )
                                })?,
                                *ctx.get_btree_map(
                                    solved_vertices,
                                    &end.get(),
                                    "creo solved vertices lookup",
                                )?
                                .ok_or_else(|| {
                                    cadmpeg_core::CodecError::malformed(
                                        "solved vertices indexed record",
                                    )
                                })?,
                            ];
                            let Some(surface) = unique_native_model_surface(
                                ctx,
                                scan,
                                &ir.model.surfaces,
                                *face_id,
                                &mut surfaces_index,
                            )?
                            else {
                                break 'inputs None;
                            };
                            Some((source_carriers.surface_geometry(surface)?, traversal))
                        };
                        match inputs {
                            Some((surface, traversal)) => {
                                unique_oriented_native_pcurve(ctx, surface, candidates, traversal)?
                            }
                            None => None,
                        }
                    } else {
                        None
                    };
                    let mut pcurve_geometry = native_selection.and_then(
                        |crate::decode::analytic::pcurves::OrientedNativePcurve {
                             endpoints,
                             offset,
                         }| {
                            Some((
                                line_pcurve(endpoints[0], endpoints[1])?,
                                Some([0.0, 1.0]),
                                offset,
                                "native_endpoint_pcurve",
                            ))
                        },
                    );
                    if pcurve_geometry.is_none() && native_candidates.is_none() {
                        pcurve_geometry = 'projected: {
                            let Some(surface) = unique_native_model_surface(
                                ctx,
                                scan,
                                &ir.model.surfaces,
                                *face_id,
                                &mut surfaces_index,
                            )?
                            else {
                                break 'projected None;
                            };
                            let (curve_id, _curve_storage) =
                                crate::identity::compose_scoped::<CurveId>(
                                    ctx,
                                    &crate::identity::VISIBGEOM_CURVE,
                                    half_edge.curve_id,
                                    "creo B-rep planar curve query",
                                )?;
                            let Some(curve_position) = curves_index
                                .lookup(
                                    ctx,
                                    &ir.model.curves,
                                    |curve| curve.id.as_str(),
                                    curve_id.as_str(),
                                )?
                                .unique_position()
                            else {
                                break 'projected None;
                            };
                            let curve = &ir.model.curves[curve_position];
                            let (edge_id, _edge_storage) = crate::identity::compose_scoped::<EdgeId>(
                                ctx,
                                &crate::identity::VISIBGEOM_EDGE,
                                half_edge.curve_id,
                                "creo B-rep planar edge query",
                            )?;
                            let Some(edge_position) = edges_index
                                .lookup(
                                    ctx,
                                    &ir.model.edges,
                                    |edge| edge.id.as_str(),
                                    edge_id.as_str(),
                                )?
                                .unique_position()
                            else {
                                break 'projected None;
                            };
                            let edge = &ir.model.edges[edge_position];
                            let source_surface = source_carriers.surface_geometry(surface)?;
                            let source_curve = source_carriers.curve_geometry(curve)?;
                            let planar = planar_curve_pcurve(
                                ctx,
                                source_surface,
                                source_curve,
                                &format_args!(
                                    "VisibGeom curve-topology row {} on face {face_id}",
                                    half_edge.curve_id
                                ),
                                refusal_cell,
                            )?;
                            let Some((geometry, tag)) = planar
                                .map(|geometry| (geometry, "projected_planar_pcurve"))
                                .or_else(|| {
                                    surface_of_revolution_parallel_pcurve(
                                        source_surface,
                                        source_curve,
                                    )
                                    .map(|geometry| (geometry, "projected_parallel_conic_pcurve"))
                                })
                                .or_else(|| {
                                    meridian_circle_pcurve(
                                        source_surface,
                                        source_curve,
                                    )
                                    .map(|geometry| (geometry, "projected_meridian_pcurve"))
                                })
                                .or_else(|| {
                                    ruled_generator_line_pcurve(
                                        source_surface,
                                        source_curve,
                                    )
                                    .map(|geometry| (geometry, "projected_ruled_generator_pcurve"))
                                })
                            else {
                                break 'projected None;
                            };
                            Some((
                                geometry,
                                source_carriers.source_edge_parameter_range(edge)?,
                                ctx.get_btree_map(
                                    &row_offsets,
                                    &half_edge.curve_id,
                                    "creo row offsets lookup",
                                )?
                                .copied()
                                .unwrap_or(0),
                                tag,
                            ))
                        };
                    }
                    let refused = refusal.take_records_checked()?;
                    if pcurve_geometry.is_none() {
                        for record in
                            ctx.admit_iter(&refused, "creo B-rep pcurve refusal traversal")?
                        {
                            push_untransferred_pcurve_loss(
                                ctx,
                                losses,
                                half_edge.curve_id,
                                *face_id,
                                record,
                            )?;
                        }
                    }
                    let pcurve_use = 'pcurve: {
                        let Some((geometry, parameter_range, offset, tag)) = pcurve_geometry else {
                            break 'pcurve None;
                        };

                        let parameter_range = match parameter_range {
                            Some(range) => {
                                let Some(range) = cadmpeg_ir::units::FiniteVector::new(range)
                                else {
                                    break 'pcurve None;
                                };
                                Some(range)
                            }
                            None => None,
                        };
                        let metadata = cadmpeg_ir::geometry::pcurve::PcurveMetadata::general(
                            None,
                            parameter_range,
                            None,
                        );
                        let pcurve = crate::identity::compose_checked::<PcurveId>(
                            ctx,
                            &crate::identity::VISIBGEOM_PCURVE,
                            format_args!("{}:{face_id}", half_edge.curve_id),
                            "creo B-rep pcurve identities",
                        )?;
                        let identity_present = pcurves_index
                            .lookup(
                                ctx,
                                &ir.model.pcurves,
                                |record| record.id.as_str(),
                                pcurve.as_str(),
                            )?
                            .exists();
                        if !identity_present {
                            annotate(
                                ctx,
                                annotations,
                                &pcurve,
                                "VisibGeom",
                                cadmpeg_core::decode::u64_from_index(offset),
                                tag,
                                Exactness::Derived,
                            )?;
                            ctx.charge_entities(1, "admit Creo model pcurves")?;
                            let mut surface_storage =
                                ctx.reserve_scoped(0, "creo B-rep pcurve surface query")?;
                            let surface = surface_storage
                                .with_storage(|| native_surface_id(ctx, scan, *face_id))?;
                            source_carriers.admit_pcurve(
                                ctx,
                                ir,
                                Pcurve {
                                    id: crate::identity::copy_checked_id(
                                        ctx,
                                        pcurve.as_str(),
                                        "creo B-rep pcurve entity ID copies",
                                    )?,
                                    geometry,
                                    metadata,
                                },
                                &surface,
                            )?;
                        }
                        Some(PcurveUse {
                            pcurve,
                            isoparametric: None,
                            parameter_range: None,
                        })
                    };
                    let pcurves = one_coedge_pcurve_use(ctx, pcurve_use)?;
                    ctx.charge_entities(1, "admit Creo model coedges")?;
                    source_carriers.admit_coedge(
                        ctx,
                        ir,
                        Coedge {
                            id,
                            owner_loop: crate::identity::copy_checked_id(
                                ctx,
                                loop_id.as_str(),
                                "creo B-rep coedge owner loop ID copies",
                            )?,
                            edge: crate::identity::compose_checked(
                                ctx,
                                &crate::identity::VISIBGEOM_EDGE,
                                half_edge.curve_id,
                                "creo B-rep coedge edge identities",
                            )?,
                            radial_next,
                            sense: match half_edge.side {
                                crate::topology::Side::Zero => Sense::Forward,
                                crate::topology::Side::One => Sense::Reversed,
                            },
                            pcurves,
                            use_curve: None,
                        },
                    )?;
                }
            }
        }
    }
    Ok((
        NativeBrepTransferSummary {
            topological_point_count: solved_point_count,
            native_topological_edge_count: neutral_edge_curves.len(),
            diagnostics,
        },
        diagnostic_storage,
    ))
}

pub(in super::super) fn transfer_cap_pair_cylinders(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    source_carriers: &mut crate::decode::source_carriers::SourceUnitCarriers,
) -> Result<(), cadmpeg_core::CodecError> {
    let mut curves_index = super::model_ids::ModelIdentityIndex::new(ctx)?;
    let mut surfaces_index = super::model_ids::ModelIdentityIndex::new(ctx)?;
    if scan.curves.fc05_cylinder_cap_pairs.is_empty() {
        return Ok(());
    }
    let outlines = super::native_ids::UniqueRows::new(
        ctx,
        &scan.planes.outlines,
        |plane| Some(plane.surface_id),
        "creo fallback cap outline index",
    )?;
    for pair in ctx.admit_iter(
        &scan.curves.fc05_cylinder_cap_pairs,
        "creo transfer cap pair cylinders fc05 cylinder cap pairs traversal",
    )? {
        let Some(frame) = Fc05CapPairFrame::from_outlines(ctx, pair, &outlines)? else {
            continue;
        };
        let (id, id_storage) = crate::identity::compose_scoped::<SurfaceId>(
            ctx,
            &crate::identity::VISIBGEOM_SURFACE,
            pair.surface_id,
            "creo decoded model identity",
        )?;
        let identity_present = surfaces_index
            .lookup(
                ctx,
                &ir.model.surfaces,
                |record| record.id.as_str(),
                id.as_str(),
            )?
            .exists();
        if identity_present {
            continue;
        }
        let Ok(cylinder_surface) = cadmpeg_ir::geometry::analytic::CylinderSurface::try_new(
            Point3::from(frame.origin),
            Vector3::from(frame.unit_vector()),
            Vector3::from(frame.ref_direction),
            pair.radius_mm,
        ) else {
            continue;
        };
        id_storage.commit()?;
        annotate(
            ctx,
            annotations,
            &id,
            "VisibGeom",
            cadmpeg_core::decode::u64_from_index(pair.offset),
            "fc05_cap_pair_cylinder",
            Exactness::Derived,
        )?;
        ctx.charge_entities(1, "admit Creo model surfaces")?;
        source_carriers.admit_surface(
            ctx,
            ir,
            Surface {
                id,
                geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
                    cylinder_surface,
                )),
                source_object: Some(SourceObjectAssociation {
                    format: cadmpeg_ir::CodecFormat::Creo,
                    object_id: crate::identity::source_object_id_checked(
                        ctx,
                        format_args!("VisibGeom:{}", pair.surface_id),
                        "creo source object identity",
                    )?,
                    name: None,
                    color: None,
                    visible: None,
                    layer: None,
                    instance_path: Vec::new(),
                }),
            },
        )?;
        for crate::curve::Fc05CapEdge {
            curve_id,
            cap_plane_id,
            cap_ordinate_row_frame: ordinate,
        } in ctx.admit_iter(&pair.cap_edges, "creo B-rep cap edge traversal")?
        {
            let cap_offset = outlines.unique(*cap_plane_id).map_or_else(
                || frame.origin[frame.axis_index.index()] + frame.axis_sign.scale() * ordinate,
                |plane| plane.origin[frame.axis_index.index()],
            );
            let (center, _, _) = fc05_model_frame(
                frame.axis_index,
                cap_offset,
                pair.center_row_frame,
                pair.reference_direction_row_frame,
                frame.axis_sign,
            );
            let (id, id_storage) = crate::identity::compose_scoped::<CurveId>(
                ctx,
                &crate::identity::VISIBGEOM_CURVE,
                curve_id,
                "creo decoded model identity",
            )?;
            let identity_present = curves_index
                .lookup(
                    ctx,
                    &ir.model.curves,
                    |record| record.id.as_str(),
                    id.as_str(),
                )?
                .exists();
            if identity_present {
                continue;
            }
            let Ok(circle_curve) = cadmpeg_ir::geometry::analytic::CircleCurve::try_new(
                Point3::from(center),
                Vector3::from(frame.unit_vector()),
                Vector3::from(frame.ref_direction),
                pair.radius_mm,
            ) else {
                continue;
            };
            id_storage.commit()?;
            annotate(
                ctx,
                annotations,
                &id,
                "VisibGeom",
                cadmpeg_core::decode::u64_from_index(
                    ctx.find_by(
                        &scan.curves.fc05_circles,
                        |circle| Ok(circle.curve_id == *curve_id),
                        "creo B-rep cap circle search",
                    )?
                    .map_or(pair.offset, |circle| circle.offset),
                ),
                "fc05_cap_circle",
                Exactness::Derived,
            )?;
            ctx.charge_entities(1, "admit Creo model curves")?;
            source_carriers.admit_curve(
                ctx,
                ir,
                Curve {
                    id,
                    geometry: CurveGeometry::Solved(SolvedCurveGeometry::Circle(circle_curve)),
                    source_object: Some(SourceObjectAssociation {
                        format: cadmpeg_ir::CodecFormat::Creo,
                        object_id: crate::identity::source_object_id_checked(
                            ctx,
                            format_args!("VisibGeom:{curve_id}"),
                            "creo source object identity",
                        )?,
                        name: None,
                        color: None,
                        visible: None,
                        layer: None,
                        instance_path: Vec::new(),
                    }),
                },
            )?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
