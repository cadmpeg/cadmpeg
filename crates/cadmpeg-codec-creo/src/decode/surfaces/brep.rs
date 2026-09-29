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
use super::super::uniqueness::exactly_one;
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

use super::{fc05_cap_pair_model_frame, fc05_model_frame, matches_native_surface_id, native_surface_id, native_surface_namespace};

const EPS_PARAMETER_AGREE: f64 = 1.0e-9;
const EPS_GEOMETRY_AGREE: f64 = 1.0e-9;
const FACE_REJECTION_SAMPLE_LIMIT: usize = 4;

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
        for half_edge in loops.iter().flat_map(|lp| lp.half_edges.iter()) {
            if edge_vertices.contains_key(&half_edge.curve_id) {
                continue;
            }
            if detail.boundary_half_edges.len() < FACE_REJECTION_OPERAND_SAMPLE_LIMIT {
                ctx.try_reserve_items(
                    &mut detail.boundary_half_edges,
                    1,
                    "creo B-rep rejection boundary samples",
                )?;
                detail.boundary_half_edges.push(*half_edge);
            }
            if let Some(binding) = incidence.get(half_edge) {
                if detail.vertex_ids.len() < FACE_REJECTION_OPERAND_SAMPLE_LIMIT
                    && !detail.vertex_ids.contains(&binding.start_vertex_id)
                {
                    ctx.try_reserve_items(
                        &mut detail.vertex_ids,
                        1,
                        "creo B-rep rejection vertex samples",
                    )?;
                    detail.vertex_ids.push(binding.start_vertex_id);
                }
                if let Some(end_vertex_id) = binding.end_vertex_id {
                    if detail.vertex_ids.len() < FACE_REJECTION_OPERAND_SAMPLE_LIMIT
                        && !detail.vertex_ids.contains(&end_vertex_id)
                    {
                        ctx.try_reserve_items(
                            &mut detail.vertex_ids,
                            1,
                            "creo B-rep rejection vertex samples",
                        )?;
                        detail.vertex_ids.push(end_vertex_id);
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
        ctx.try_reserve_items(
            &mut self.face_rejection_diagnostics,
            1,
            "creo B-rep face rejection diagnostics",
        )?;
        self.face_rejection_diagnostics
            .push(FaceAdmissionDiagnostic { reason, detail });
        Ok(())
    }

    /// The rejection count and bounded detail samples for a reason.
    pub(in super::super) fn evidence(
        &self,
        reason: FaceAdmissionRejection,
    ) -> (usize, impl Iterator<Item = &FaceAdmissionDetail>) {
        let matching = self
            .face_rejection_diagnostics
            .iter()
            .filter(move |diagnostic| diagnostic.reason == reason);
        (
            matching.clone().count(),
            matching
                .take(FACE_REJECTION_SAMPLE_LIMIT)
                .map(|diagnostic| &diagnostic.detail),
        )
    }

    pub(in super::super) fn face_admission_rejection_records(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<Vec<CreoFaceAdmissionRejectionRecord>, cadmpeg_core::CodecError> {
        let mut records = Vec::new();
        for diagnostic in &self.face_rejection_diagnostics {
            let detail = &diagnostic.detail;
            let id = ctx.format_retained(
                format_args!("creo:brep:face_admission_rejection#{}", detail.face_id),
                "creo B-rep rejection record IDs",
            )?;
            let mut boundary_half_edges = Vec::new();
            ctx.try_reserve_items(
                &mut boundary_half_edges,
                detail.boundary_half_edges.len(),
                "creo B-rep rejection half edges",
            )?;
            boundary_half_edges.extend(detail.boundary_half_edges.iter().copied().map(half_edge_ref));
            let mut vertex_ids = Vec::new();
            ctx.try_reserve_items(
                &mut vertex_ids,
                detail.vertex_ids.len(),
                "creo B-rep rejection vertex IDs",
            )?;
            vertex_ids.extend_from_slice(&detail.vertex_ids);
            ctx.try_reserve_items(&mut records, 1, "creo B-rep rejection records")?;
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
        coverage: &mut cadmpeg_ir::report::decode::Coverage,
    ) {
        coverage.record(
            crate::coverage::BREP_CANDIDATE_FACE_COUNT,
            self.candidate_face_count,
        );
        coverage.record(
            crate::coverage::BREP_ADMITTED_FACE_COUNT,
            self.admitted_face_count,
        );
        coverage.record(
            crate::coverage::BREP_EMITTED_FACE_COUNT,
            self.emitted_face_count,
        );
        coverage.record(
            crate::coverage::BREP_BOUNDARY_CURVE_COUNT,
            self.boundary_curve_count,
        );
        coverage.record(
            crate::coverage::BREP_BOUNDARY_CURVE_MISSING_INCIDENCE_COUNT,
            self.boundary_curve_missing_incidence_count,
        );
        coverage.record(
            crate::coverage::BREP_BOUNDARY_CURVE_UNSOLVED_VERTEX_COUNT,
            self.boundary_curve_unsolved_vertex_count,
        );
        if self.legacy_nonvisible_face_reference_count > 0 {
            coverage.record(
                crate::coverage::BREP_LEGACY_NONVISIBLE_FACE_REFERENCE_COUNT,
                self.legacy_nonvisible_face_reference_count,
            );
        }
        coverage.record(
            crate::coverage::BREP_VERTEX_TOPOLOGICAL_COUNT,
            self.vertex_solve.topological_vertices,
        );
        coverage.record(
            crate::coverage::BREP_VERTEX_CARRIER_INCIDENT_COUNT,
            self.vertex_solve.carrier_incident_vertices,
        );
        coverage.record(
            crate::coverage::BREP_VERTEX_CARRIER_PAIR_INTERSECTION_CANDIDATE_COUNT,
            self.vertex_solve.carrier_pair_candidates,
        );
        coverage.record(
            crate::coverage::BREP_VERTEX_CARRIER_TRIPLE_INTERSECTION_CANDIDATE_COUNT,
            self.vertex_solve.carrier_triple_candidates,
        );
        coverage.record(
            crate::coverage::BREP_VERTEX_CARRIER_VALID_INTERSECTION_CANDIDATE_COUNT,
            self.vertex_solve.carrier_valid_candidates,
        );
        coverage.record(
            crate::coverage::BREP_VERTEX_CARRIER_ZERO_CANDIDATE_COUNT,
            self.vertex_solve.carrier_no_geometric_candidate_vertices
                + self.vertex_solve.carrier_no_valid_candidate_vertices,
        );
        if self.vertex_solve.carrier_no_geometric_candidate_vertices != 0 {
            coverage.record(
                crate::coverage::BREP_VERTEX_CARRIER_NO_GEOMETRIC_CANDIDATE_COUNT,
                self.vertex_solve.carrier_no_geometric_candidate_vertices,
            );
        }
        if self.vertex_solve.carrier_no_valid_candidate_vertices != 0 {
            coverage.record(
                crate::coverage::BREP_VERTEX_CARRIER_NO_VALID_CANDIDATE_COUNT,
                self.vertex_solve.carrier_no_valid_candidate_vertices,
            );
        }
        coverage.record(
            crate::coverage::BREP_VERTEX_CARRIER_AMBIGUOUS_CANDIDATE_COUNT,
            self.vertex_solve.carrier_ambiguous_candidate_vertices,
        );
        coverage.record(
            crate::coverage::BREP_VERTEX_CARRIER_POINT_COUNT,
            self.vertex_solve.carrier_points,
        );
        coverage.record(
            crate::coverage::BREP_PCURVE_RECORD_COUNT,
            self.vertex_solve.pcurve.records,
        );
        coverage.record(
            crate::coverage::BREP_PCURVE_PATH_COUNT,
            self.vertex_solve.pcurve.paths(),
        );
        let pcurve = &self.vertex_solve.pcurve;
        if pcurve.inactive_paths > 0
            || pcurve.inactive_records > 0
            || pcurve.partial_records > 0
            || pcurve.topology_mismatch_records > 0
        {
            coverage.record(
                crate::coverage::BREP_PCURVE_INACTIVE_PATH_COUNT,
                pcurve.inactive_paths,
            );
            coverage.record(
                crate::coverage::BREP_PCURVE_INACTIVE_RECORD_COUNT,
                pcurve.inactive_records,
            );
            coverage.record(
                crate::coverage::BREP_PCURVE_PARTIAL_RECORD_COUNT,
                pcurve.partial_records,
            );
            coverage.record(
                crate::coverage::BREP_PCURVE_TOPOLOGY_MISMATCH_RECORD_COUNT,
                pcurve.topology_mismatch_records,
            );
        }
        coverage.record(
            crate::coverage::BREP_PCURVE_MISSING_SURFACE_PATH_COUNT,
            self.vertex_solve.pcurve.missing_surfaces,
        );
        coverage.record(
            crate::coverage::BREP_PCURVE_UNEVALUABLE_PATH_COUNT,
            self.vertex_solve.pcurve.unevaluable_paths,
        );
        coverage.record(
            crate::coverage::BREP_PCURVE_MAPPED_PATH_COUNT,
            self.vertex_solve.pcurve.mapped_paths,
        );
        if pcurve.carrier_validated_paths > 0
            || pcurve.carrier_rejected_paths > 0
            || pcurve.carrier_unknown_paths() > 0
            || pcurve.carrier_rejected_records > 0
        {
            coverage.record(
                crate::coverage::BREP_PCURVE_CARRIER_VALIDATED_PATH_COUNT,
                pcurve.carrier_validated_paths,
            );
            coverage.record(
                crate::coverage::BREP_PCURVE_CARRIER_REJECTED_PATH_COUNT,
                pcurve.carrier_rejected_paths,
            );
            coverage.record(
                crate::coverage::BREP_PCURVE_CARRIER_UNKNOWN_PATH_COUNT,
                pcurve.carrier_unknown_paths(),
            );
            coverage.record(
                crate::coverage::BREP_PCURVE_CARRIER_UNKNOWN_MISSING_SURFACE_PATH_COUNT,
                pcurve.carrier_unknown_missing_surface_paths,
            );
            coverage.record(
                crate::coverage::BREP_PCURVE_CARRIER_UNKNOWN_MISSING_CARRIER_PATH_COUNT,
                pcurve.carrier_unknown_missing_carrier_paths,
            );
            coverage.record(
                crate::coverage::BREP_PCURVE_CARRIER_UNKNOWN_UNSUPPORTED_PAIR_PATH_COUNT,
                pcurve.carrier_unknown_unsupported_pair_paths,
            );
            coverage.record(
                crate::coverage::BREP_PCURVE_CARRIER_UNKNOWN_PARALLEL_PLANE_PATH_COUNT,
                pcurve.carrier_unknown_parallel_plane_paths,
            );
            coverage.record(
                crate::coverage::BREP_PCURVE_CARRIER_UNKNOWN_UNSUPPORTED_PATH_COUNT,
                pcurve.carrier_unknown_unsupported_path_paths,
            );
            coverage.record(
                crate::coverage::BREP_PCURVE_CARRIER_REJECTED_RECORD_COUNT,
                pcurve.carrier_rejected_records,
            );
        }
        coverage.record(
            crate::coverage::BREP_PCURVE_UNMAPPED_RECORD_COUNT,
            self.vertex_solve.pcurve.unmapped_records,
        );
        coverage.record(
            crate::coverage::BREP_PCURVE_INCONSISTENT_RECORD_COUNT,
            self.vertex_solve.pcurve.inconsistent_records,
        );
        coverage.record(
            crate::coverage::BREP_PCURVE_ACCEPTED_RECORD_COUNT,
            self.vertex_solve.pcurve.accepted_records,
        );
        coverage.record(
            crate::coverage::BREP_PCURVE_COMPLETE_RECORD_COUNT,
            self.vertex_solve.pcurve.complete_records,
        );
        if self.vertex_solve.pcurve.two_chart_records > 0 {
            coverage.record(
                crate::coverage::BREP_PCURVE_TWO_CHART_RECORD_COUNT,
                self.vertex_solve.pcurve.two_chart_records,
            );
            coverage.record(
                crate::coverage::BREP_PCURVE_TWO_CHART_MAPPED_RECORD_COUNT,
                self.vertex_solve.pcurve.two_chart_mapped_records(),
            );
            coverage.record(
                crate::coverage::BREP_PCURVE_TWO_CHART_COMPLETE_RECORD_COUNT,
                self.vertex_solve.pcurve.two_chart_complete_records,
            );
            coverage.record(
                crate::coverage::BREP_PCURVE_TWO_CHART_PARTIAL_RECORD_COUNT,
                self.vertex_solve.pcurve.two_chart_partial_records,
            );
            coverage.record(
                crate::coverage::BREP_PCURVE_TWO_CHART_MISSING_SURFACE_PATH_COUNT,
                self.vertex_solve.pcurve.two_chart_missing_surface_paths,
            );
            coverage.record(
                crate::coverage::BREP_PCURVE_TWO_CHART_UNEVALUABLE_PATH_COUNT,
                self.vertex_solve.pcurve.two_chart_unevaluable_paths,
            );
            coverage.record(
                crate::coverage::BREP_PCURVE_TWO_CHART_SURFACE_MISMATCH_RECORD_COUNT,
                self.vertex_solve.pcurve.two_chart_surface_mismatch_records,
            );
            coverage.record(
                crate::coverage::BREP_PCURVE_TWO_CHART_NO_SAMPLE_RECORD_COUNT,
                self.vertex_solve.pcurve.two_chart_no_sample_records,
            );
            coverage.record(
                crate::coverage::BREP_PCURVE_TWO_CHART_UNMAPPED_RECORD_COUNT,
                self.vertex_solve.pcurve.two_chart_unmapped_records,
            );
        }
        coverage.record(
            crate::coverage::BREP_PCURVE_CONFLICTING_CURVE_COUNT,
            self.vertex_solve.pcurve.conflicting_curves,
        );
        coverage.record(
            crate::coverage::BREP_VERTEX_PCURVE_ENDPOINT_EVIDENCE_COUNT,
            self.vertex_solve.pcurve.evidence,
        );
        coverage.record(
            crate::coverage::BREP_VERTEX_COMPLETE_PCURVE_ENDPOINT_EVIDENCE_COUNT,
            self.vertex_solve.pcurve.complete_evidence,
        );
        coverage.record(
            crate::coverage::BREP_VERTEX_PCURVE_CONSTRAINT_COUNT,
            self.vertex_solve.pcurve_constraints,
        );
        if self.vertex_solve.pcurve_fixed_endpoint_conflicts > 0 {
            coverage.record(
                crate::coverage::BREP_VERTEX_PCURVE_FIXED_ENDPOINT_CONFLICT_COUNT,
                self.vertex_solve.pcurve_fixed_endpoint_conflicts,
            );
        }
        if self.vertex_solve.pcurve_ambiguous_endpoint_vertices > 0 {
            coverage.record(
                crate::coverage::BREP_VERTEX_PCURVE_AMBIGUOUS_ENDPOINT_VERTEX_COUNT,
                self.vertex_solve.pcurve_ambiguous_endpoint_vertices,
            );
        }
        coverage.record(
            crate::coverage::BREP_VERTEX_DIRECTED_ENDPOINT_ASSIGNMENT_COUNT,
            self.vertex_solve.directed_endpoint_assignments,
        );
        coverage.record(
            crate::coverage::BREP_VERTEX_DIRECTED_ENDPOINT_CONFLICT_COUNT,
            self.vertex_solve.directed_endpoint_conflicts,
        );
        coverage.record(
            crate::coverage::BREP_VERTEX_NURBS_ENDPOINT_CONSTRAINT_COUNT,
            self.vertex_solve.nurbs_endpoint_constraints,
        );
        coverage.record(
            crate::coverage::BREP_VERTEX_ANALYTIC_DOMAIN_COUNT,
            self.vertex_solve.analytic_domain_vertices,
        );
        coverage.record(
            crate::coverage::BREP_VERTEX_SOLVED_COUNT,
            self.vertex_solve.solved_vertices,
        );
        coverage.record(
            crate::coverage::BREP_REJECTED_FACE_COUNT,
            self.face_rejection_diagnostics.len(),
        );
        for reason in FaceAdmissionRejection::ALL {
            coverage.record(reason.coverage_key(), self.evidence(reason).0);
        }
        coverage.record(
            crate::coverage::BREP_BODY_COUNT_MISMATCH_COUNT,
            usize::from(self.body_count_mismatch),
        );
        coverage.record(
            crate::coverage::BREP_LEGACY_BODY_OWNERSHIP_AMBIGUOUS_COUNT,
            usize::from(self.legacy_body_ownership_ambiguous),
        );
        coverage.record(
            crate::coverage::BREP_EMPTY_COMPONENT_COUNT,
            self.empty_component_count,
        );
        coverage.record(
            crate::coverage::BREP_ADMITTED_COMPONENT_COUNT,
            self.admitted_component_count,
        );
        coverage.record(
            crate::coverage::BREP_SELECTED_BODY_COUNT,
            self.selected_body_count.unwrap_or_default(),
        );
        coverage.record(
            crate::coverage::BREP_SELECTED_BODY_COUNT_UNRESOLVED,
            usize::from(self.selected_body_count.is_none()),
        );
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
        ctx.try_reserve_items(
            &mut admitted,
            scan.topology.face_components.len(),
            "creo B-rep admitted component refs",
        )?;
        admitted.extend(&scan.topology.face_components);
        return Ok(admitted);
    }
    for component in &scan.topology.face_components {
        if component
            .face_ids
            .iter()
            .any(|face_id| eligible_face_ids.contains(face_id))
        {
            ctx.try_reserve_items(&mut admitted, 1, "creo B-rep admitted component refs")?;
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
    ) || scan.surfaces.rows.iter().any(|row| row.id == face_id)
}

fn merge_body_components(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    components: Vec<NeutralShellSpec>,
) -> Result<Vec<NeutralShellSpec>, cadmpeg_core::CodecError> {
    let mut components = components.into_iter();
    let Some(mut first) = components.next() else {
        return Ok(Vec::new());
    };
    for component in components {
        ctx.try_reserve_items(
            &mut first.faces,
            component.faces.len(),
            "creo B-rep merged component faces",
        )?;
        first.faces.extend(component.faces);
        for curve_id in component.wire_curves {
            if !first.wire_curves.contains(&curve_id) {
                ctx.charge_collection_items(1, "creo B-rep merged component wire nodes")?;
                first.wire_curves.insert(curve_id);
            }
        }
    }
    let mut merged = Vec::new();
    ctx.try_reserve_items(&mut merged, 1, "creo B-rep merged component records")?;
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
    let mut remaining_faces = BTreeSet::new();
    for face_id in faces {
        if !remaining_faces.contains(face_id) {
            ctx.charge_collection_items(1, "creo B-rep remaining face nodes")?;
            remaining_faces.insert(*face_id);
        }
    }
    let mut shell_specs = Vec::new();
    while let Some(start) = remaining_faces.pop_first() {
        let mut group = BTreeSet::new();
        ctx.charge_collection_items(1, "creo B-rep shell group face nodes")?;
        group.insert(start);
        let mut pending = Vec::new();
        ctx.try_reserve_items(&mut pending, 1, "creo B-rep pending shell faces")?;
        pending.push(start);
        while let Some(face_id) = pending.pop() {
            for neighbour in face_adjacency.get(&face_id).into_iter().flatten().copied() {
                if remaining_faces.remove(&neighbour) {
                    ctx.charge_collection_items(1, "creo B-rep shell group face nodes")?;
                    group.insert(neighbour);
                    ctx.try_reserve_items(&mut pending, 1, "creo B-rep pending shell faces")?;
                    pending.push(neighbour);
                }
            }
        }
        let mut group_faces = Vec::new();
        ctx.try_reserve_items(&mut group_faces, group.len(), "creo B-rep shell face IDs")?;
        group_faces.extend(group);
        ctx.try_reserve_items(&mut shell_specs, 1, "creo B-rep shell records")?;
        shell_specs.push(NeutralShellSpec {
            faces: group_faces,
            wire_curves: BTreeSet::new(),
        });
    }
    let mut unattached_wire_curves = BTreeSet::new();
    for curve_id in wire_curves {
        let curve_vertices = edge_vertices[curve_id];
        let matching_shell = exactly_one(
            shell_specs
                .iter()
                .enumerate()
                .filter(|(_, shell)| {
                    shell
                        .faces
                        .iter()
                        .any(|face_id| {
                            curve_vertices
                                .iter()
                                .any(|vertex_id| face_vertices[face_id].contains(vertex_id))
                        })
                })
                .map(|(index, _)| index),
        );
        if let Some(index) = matching_shell {
            ctx.charge_collection_items(1, "creo B-rep attached wire nodes")?;
            shell_specs[index].wire_curves.insert(*curve_id);
        } else {
            ctx.charge_collection_items(1, "creo B-rep unattached wire nodes")?;
            unattached_wire_curves.insert(*curve_id);
        }
    }
    if !unattached_wire_curves.is_empty() {
        ctx.try_reserve_items(&mut shell_specs, 1, "creo B-rep shell records")?;
        shell_specs.push(NeutralShellSpec {
            faces: Vec::new(),
            wire_curves: unattached_wire_curves,
        });
    }
    Ok(shell_specs)
}

fn component_is_closed(
    component_face_curves: &BTreeSet<u32>,
    emitted_half_edges: &BTreeSet<HalfEdgeId>,
    half_edges: &BTreeMap<HalfEdgeId, &crate::topology::HalfEdge>,
    faces: &[u32],
) -> bool {
    component_face_curves.iter().all(|curve_id| {
        let mut face_uses = emitted_half_edges
            .iter()
            .filter(|half_edge| half_edge.curve_id == *curve_id)
            .filter_map(|half_edge| half_edges.get(half_edge))
            .filter_map(|half_edge| half_edge.face_id);
        let (Some(first), Some(second), None) =
            (face_uses.next(), face_uses.next(), face_uses.next())
        else {
            return false;
        };
        faces.contains(&first.get()) && faces.contains(&second.get())
    })
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
    for curve in &ir.model.curves {
        let Some(id) = curve
                .id
                .as_str()
                .strip_prefix("creo:visibgeom:curve#")
                .and_then(|text| text.parse::<u32>().ok())
        else {
            continue;
        };
        if source_carriers
            .curve_geometry(curve)
            .solved()
            .is_some_and(curve_geometry_is_typed_nonlinear)
            && !ids.contains(&id)
        {
            ctx.charge_collection_items(1, "creo B-rep typed curve ID nodes")?;
            ids.insert(id);
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
    source_carriers: &'a crate::decode::source_carriers::SourceUnitCarriers,
}

fn native_circle_loop_geometry(
    lp: &crate::topology::Loop,
    model_curves: &[Curve],
    source_carriers: &crate::decode::source_carriers::SourceUnitCarriers,
) -> Option<NativeCircleLoop> {
    let [first, second] = lp.half_edges.as_slice() else {
        return None;
    };
    if first.curve_id == second.curve_id {
        return None;
    }
    let first_id = CurveId::compose(&crate::identity::VISIBGEOM_CURVE, first.curve_id);
    let second_id = CurveId::compose(&crate::identity::VISIBGEOM_CURVE, second.curve_id);
    let first = exactly_one(model_curves.iter().filter(|curve| curve.id == first_id))?;
    let second = exactly_one(model_curves.iter().filter(|curve| curve.id == second_id))?;
    let (
        CurveGeometry::Solved(SolvedCurveGeometry::Circle(circle_curve)),
        CurveGeometry::Solved(SolvedCurveGeometry::Circle(circle_curve_2)),
    ) = (
        source_carriers.curve_geometry(first),
        source_carriers.curve_geometry(second),
    )
    else {
        return None;
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
        return None;
    }
    Some(NativeCircleLoop {
        center: first_center,
        axis: *first_axis,
        radius: first_radius,
    })
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
    let mut circle_loops = Vec::new();
    for lp in loops {
        let Some(circle) = native_circle_loop_geometry(lp, model_curves, source_carriers) else {
            return Ok(None);
        };
        ctx.try_reserve_items(&mut circle_loops, 1, "creo native circle loop geometry")?;
        circle_loops.push(circle);
    }
    let normal_length = normal.norm();
    if !normal_length.is_finite() || normal_length <= 0.0 {
        return Ok(None);
    }
    let reference = circle_loops[0];
    if circle_loops.iter().any(|circle| {
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
    }) {
        return Ok(None);
    }
    let Some(center_uv) = cadmpeg_ir::eval::analytic_surface_parameters(
        surface,
        reference.center.get(),
    ) else {
        return Ok(None);
    };
    let center_uv = cadmpeg_ir::math::Point2::from(center_uv);
    for (circle, polygon) in circle_loops.iter().zip(polygons) {
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
    if circle_loops.iter().enumerate().any(|(index, first)| {
        circle_loops
            .iter()
            .skip(index + 1)
            .any(|second| scalar_values_agree(first.radius, second.radius))
    }) {
        return Ok(None);
    }
    let mut order = Vec::new();
    for index in 0..loops.len() {
        ctx.try_reserve_items(&mut order, 1, "creo native circle loop order")?;
        order.push(index);
    }
    order.sort_by(|first, second| {
        circle_loops[*second]
            .radius
            .total_cmp(&circle_loops[*first].radius)
    });
    let mut ordered = Vec::new();
    for index in order {
        ctx.try_reserve_items(&mut ordered, 1, "creo native ordered circle loops")?;
        ordered.push(loops[index]);
    }
    Ok(Some(ordered))
}

fn native_parameter_loop_polygon(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    lp: &crate::topology::Loop,
    face_id: u32,
    surface: &SurfaceGeometry,
    incidence: &BTreeMap<HalfEdgeId, &crate::topology::HalfEdgeVertexIncidence>,
    solved_vertices: &BTreeMap<u32, [f64; 3]>,
    native_pcurves: &NativePcurveCandidates,
    typed_nonlinear_curve_ids: &BTreeSet<u32>,
) -> Result<Option<Vec<[f64; 2]>>, cadmpeg_core::CodecError> {
    let mut segments = Vec::new();
    for half_edge in &lp.half_edges {
        let Some(binding) = incidence.get(half_edge) else {
            return Ok(None);
        };
        let Some(end_vertex_id) = binding.end_vertex_id else {
            return Ok(None);
        };
        let Some(candidates) = native_pcurves.get(&(half_edge.curve_id, face_id)) else {
            return Ok(None);
        };
        let (Some(start), Some(end)) = (
            solved_vertices.get(&binding.start_vertex_id).copied(),
            solved_vertices.get(&end_vertex_id).copied(),
        ) else {
            return Ok(None);
        };
        let Some((endpoints, _)) = unique_oriented_native_pcurve(surface, candidates, [start, end]) else {
            return Ok(None);
        };
        ctx.try_reserve_items(&mut segments, 1, "creo native loop pcurve segments")?;
        segments.push(endpoints);
    }
    if segments.len() < 3
        && (segments.len() != 2
            || lp.half_edges[0].curve_id == lp.half_edges[1].curve_id
            || lp
                .half_edges
                .iter()
                .any(|half_edge| !typed_nonlinear_curve_ids.contains(&half_edge.curve_id))
            || segments
                .iter()
                .any(|segment| parameter_points_agree(segment[0], segment[1])))
        || segments
            .iter()
            .flatten()
            .flatten()
            .any(|value| !value.is_finite())
        || segments.iter().enumerate().any(|(index, segment)| {
            let next = segments[(index + 1) % segments.len()];
            !parameter_points_agree(segment[1], next[0])
        })
    {
        return Ok(None);
    }
    let mut polygon = Vec::new();
    for segment in segments {
        ctx.try_reserve_items(&mut polygon, 1, "creo native loop polygon points")?;
        polygon.push(segment[0]);
    }
    Ok(Some(polygon))
}

fn ordered_native_parameter_face_loops<'a>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    loops: &[&'a crate::topology::Loop],
    face_id: u32,
    surface: &SurfaceGeometry,
    incidence: &BTreeMap<HalfEdgeId, &crate::topology::HalfEdgeVertexIncidence>,
    solved_vertices: &BTreeMap<u32, [f64; 3]>,
    native_pcurves: &NativePcurveCandidates,
    curve_evidence: NativeCurveEvidence<'_>,
) -> Result<Option<Vec<&'a crate::topology::Loop>>, cadmpeg_core::CodecError> {
    let mut polygons = Vec::new();
    for lp in loops {
        let Some(polygon) = native_parameter_loop_polygon(
            ctx,
            lp,
            face_id,
            surface,
            incidence,
            solved_vertices,
            native_pcurves,
            curve_evidence.typed_nonlinear_curve_ids,
        )? else {
            return Ok(None);
        };
        ctx.try_reserve_items(&mut polygons, 1, "creo native face loop polygons")?;
        polygons.push(polygon);
    }
    let mut copied_loops = Vec::new();
    ctx.try_reserve_items(&mut copied_loops, loops.len(), "creo native face loop references")?;
    copied_loops.extend_from_slice(loops);
    if let Some(ordered) = ordered_parameter_face_loops(ctx, copied_loops, &polygons)? {
        Ok(Some(ordered))
    } else {
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

#[cfg(test)]
mod tests;

fn push_native_pcurve_candidate(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    candidates: &mut NativePcurveCandidates,
    curve_id: u32,
    face_id: u32,
    endpoints: [[f64; 2]; 2],
    offset: usize,
) -> Result<(), cadmpeg_core::CodecError> {
    let values = match candidates.entry((curve_id, face_id)) {
        std::collections::btree_map::Entry::Occupied(entry) => entry.into_mut(),
        std::collections::btree_map::Entry::Vacant(entry) => {
            ctx.charge_collection_items(1, "creo B-rep pcurve candidate nodes")?;
            entry.insert(Vec::new())
        }
    };
    ctx.try_reserve_items(values, 1, "creo B-rep pcurve candidates")?;
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
        for (id, carrier) in carriers {
            if let CarrierEquation::Plane(plane) = carrier {
                ctx.charge_collection_items(1, "creo B-rep plane index nodes")?;
                planes.insert(*id, *plane);
            }
        }
        let mut half_edges = BTreeMap::new();
        for half_edge in &scan.topology.half_edges {
            match half_edges.entry(half_edge.id) {
                std::collections::btree_map::Entry::Occupied(mut entry) => {
                    entry.insert(half_edge);
                }
                std::collections::btree_map::Entry::Vacant(entry) => {
                    ctx.charge_collection_items(1, "creo B-rep half-edge index nodes")?;
                    entry.insert(half_edge);
                }
            }
        }
        let mut incidence = BTreeMap::new();
        for binding in &scan.topology.half_edge_vertex_incidence {
            match incidence.entry(binding.half_edge) {
                std::collections::btree_map::Entry::Occupied(mut entry) => {
                    entry.insert(binding);
                }
                std::collections::btree_map::Entry::Vacant(entry) => {
                    ctx.charge_collection_items(1, "creo B-rep incidence index nodes")?;
                    entry.insert(binding);
                }
            }
        }
        Ok(Self { planes, half_edges, incidence })
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
        for lp in &scan.topology.loops {
            if let Some(face_id) = lp.face_id {
                let loops = match loops_by_face.entry(face_id.get()) {
                    std::collections::btree_map::Entry::Occupied(entry) => entry.into_mut(),
                    std::collections::btree_map::Entry::Vacant(entry) => {
                        ctx.charge_collection_items(1, "creo B-rep face-loop index nodes")?;
                        entry.insert(Vec::new())
                    }
                };
                ctx.try_reserve_items(loops, 1, "creo B-rep face-loop references")?;
                loops.push(lp);
            }
        }
        let mut topology_face_reference_ids = BTreeSet::new();
        for face_id in scan
            .topology
            .face_components
            .iter()
            .flat_map(|component| component.face_ids.iter().copied())
            .chain(loops_by_face.keys().copied())
        {
            if !topology_face_reference_ids.contains(&face_id) {
                ctx.charge_collection_items(1, "creo B-rep topology face ID nodes")?;
                topology_face_reference_ids.insert(face_id);
            }
        }
        let legacy_nonvisible_face_reference_count = topology_face_reference_ids
            .iter()
            .filter(|face_id| !is_neutral_face_reference(scan, **face_id))
            .count();
        loops_by_face.retain(|face_id, _| is_neutral_face_reference(scan, *face_id));
        let mut candidate_face_ids = BTreeSet::new();
        for face_id in scan
            .topology
            .face_components
            .iter()
            .flat_map(|component| component.face_ids.iter().copied())
            .chain(loops_by_face.keys().copied())
            .filter(|face_id| is_neutral_face_reference(scan, *face_id))
        {
            if !candidate_face_ids.contains(&face_id) {
                ctx.charge_collection_items(1, "creo B-rep candidate face ID nodes")?;
                candidate_face_ids.insert(face_id);
            }
        }
        let mut model_surface_counts = BTreeMap::new();
        for face_id in &candidate_face_ids {
            let prefix = native_surface_namespace(scan, *face_id).1;
            let count = ir
                .model
                .surfaces
                .iter()
                .filter(|surface| {
                    crate::identity::matches_numbered_identity(
                        surface.id.as_str(), prefix, *face_id,
                    )
                })
                .count();
            ctx.charge_collection_items(1, "creo B-rep model surface count nodes")?;
            model_surface_counts.insert(*face_id, count);
        }
        let mut boundary_curve_ids = BTreeSet::new();
        for curve_id in loops_by_face
            .values()
            .flatten()
            .flat_map(|lp| lp.half_edges.iter().map(|half_edge| half_edge.curve_id))
        {
            if !boundary_curve_ids.contains(&curve_id) {
                ctx.charge_collection_items(1, "creo B-rep boundary curve ID nodes")?;
                boundary_curve_ids.insert(curve_id);
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
        native_edge_vertices: &BTreeMap<u32, [u32; 2]>,
        solved_vertices: &BTreeMap<u32, [f64; 3]>,
        ir: &CadIr,
    ) -> Result<Self, cadmpeg_core::CodecError> {
        let mut edge_vertices = BTreeMap::new();
        for row in crate::identity::uniquely_identified_rows_checked(ctx, rows, |row| row.id)? {
            let Some(vertices) = native_edge_vertices.get(&row.id).copied() else {
                continue;
            };
            if vertices
                .iter()
                .all(|vertex| solved_vertices.contains_key(vertex))
            {
                ctx.charge_collection_items(1, "creo B-rep edge-vertex nodes")?;
                edge_vertices.insert(row.id, vertices);
            }
        }
        let mut model_curve_counts = BTreeMap::new();
        let mut admitted_edge_curves = BTreeSet::new();
        for curve_id in edge_vertices.keys() {
            let count = ir
                .model
                .curves
                .iter()
                .filter(|curve| {
                    crate::identity::matches_numbered_identity(
                        curve.id.as_str(),
                        "creo:visibgeom:curve#",
                        *curve_id,
                    )
                })
                .count();
            ctx.charge_collection_items(1, "creo B-rep model curve count nodes")?;
            model_curve_counts.insert(*curve_id, count);
            if count <= 1 {
                ctx.charge_collection_items(1, "creo B-rep admitted edge ID nodes")?;
                admitted_edge_curves.insert(*curve_id);
            }
        }
        Ok(Self { edge_vertices, model_curve_counts, admitted_edge_curves })
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
        let mut eligible_loops = Vec::new();
        for lp in eligible_faces.values().flatten() {
            ctx.try_reserve_items(&mut eligible_loops, 1, "creo B-rep eligible loop refs")?;
            eligible_loops.push(*lp);
        }
        let mut emitted_half_edges = BTreeSet::new();
        for half_edge in eligible_loops
            .iter()
            .flat_map(|lp| lp.half_edges.iter().copied())
        {
            if !emitted_half_edges.contains(&half_edge) {
                ctx.charge_collection_items(1, "creo B-rep emitted half-edge nodes")?;
                emitted_half_edges.insert(half_edge);
            }
        }
        let mut face_curves = BTreeSet::new();
        for half_edge in &emitted_half_edges {
            if !face_curves.contains(&half_edge.curve_id) {
                ctx.charge_collection_items(1, "creo B-rep face curve ID nodes")?;
                face_curves.insert(half_edge.curve_id);
            }
        }
        let mut closed_single_edge_curves = BTreeSet::new();
        for curve_id in &face_curves {
            let mut uses = eligible_loops.iter().filter(|lp| {
                lp.half_edges
                    .iter()
                    .any(|half_edge| half_edge.curve_id == *curve_id)
            });
            if uses.next().is_some_and(|lp| lp.half_edges.len() == 1)
                && uses.all(|lp| lp.half_edges.len() == 1)
            {
                ctx.charge_collection_items(1, "creo B-rep single-edge curve nodes")?;
                closed_single_edge_curves.insert(*curve_id);
            }
        }
        let mut row_offsets = BTreeMap::new();
        for row in topology_rows {
            match row_offsets.entry(row.id) {
                std::collections::btree_map::Entry::Occupied(mut entry) => {
                    entry.insert(row.offset);
                }
                std::collections::btree_map::Entry::Vacant(entry) => {
                    ctx.charge_collection_items(1, "creo B-rep row-offset nodes")?;
                    entry.insert(row.offset);
                }
            }
        }
        let mut curve_faces = BTreeMap::new();
        for row in crate::identity::uniquely_identified_rows_checked(ctx, topology_rows, |row| row.id)? {
            ctx.charge_collection_items(1, "creo B-rep curve-face nodes")?;
            curve_faces.insert(row.id, row.stored_face_ids());
        }
        let mut eligible_face_ids = BTreeSet::new();
        for face_id in eligible_faces.keys() {
            ctx.charge_collection_items(1, "creo B-rep eligible face ID nodes")?;
            eligible_face_ids.insert(*face_id);
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
        for curve_id in admitted_components
            .iter()
            .flat_map(|component| component.curve_ids.iter().copied())
            .filter(|curve_id| admitted_edge_curves.contains(curve_id))
            .filter(|curve_id| {
                !matches!(scan.framing.layout, crate::container::Layout::LegacyAscii(_))
                    || curve_faces.get(curve_id).is_some_and(|faces| {
                        faces.iter().any(|face| eligible_face_ids.contains(face))
                    })
            })
        {
            if !neutral_edge_curves.contains(&curve_id) {
                ctx.charge_collection_items(1, "creo B-rep neutral edge curve nodes")?;
                neutral_edge_curves.insert(curve_id);
            }
        }
        let mut body_components = Vec::new();
        for component in admitted_components {
            let mut faces = Vec::new();
            for face_id in component
                .face_ids
                .iter()
                .copied()
                .filter(|face_id| eligible_faces.contains_key(face_id))
            {
                ctx.try_reserve_items(&mut faces, 1, "creo B-rep component face IDs")?;
                faces.push(face_id);
            }
            let mut curves = BTreeSet::new();
            for curve_id in component
                .curve_ids
                .iter()
                .copied()
                .filter(|curve_id| neutral_edge_curves.contains(curve_id))
            {
                if !curves.contains(&curve_id) {
                    ctx.charge_collection_items(1, "creo B-rep component wire nodes")?;
                    curves.insert(curve_id);
                }
            }
            ctx.try_reserve_items(&mut body_components, 1, "creo B-rep component records")?;
            body_components.push(NeutralShellSpec {
                faces,
                wire_curves: curves,
            });
        }
        Ok(Self { neutral_edge_curves, body_components })
    }
}

fn used_brep_vertices(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    neutral_edge_curves: &BTreeSet<u32>,
    edge_vertices: &BTreeMap<u32, [u32; 2]>,
) -> Result<BTreeSet<u32>, cadmpeg_core::CodecError> {
    let mut used_vertices = BTreeSet::new();
    for vertex_id in neutral_edge_curves
        .iter()
        .filter_map(|curve_id| edge_vertices.get(curve_id))
        .flatten()
        .copied()
    {
        if !used_vertices.contains(&vertex_id) {
            ctx.charge_collection_items(1, "creo B-rep used vertex nodes")?;
            used_vertices.insert(vertex_id);
        }
    }
    Ok(used_vertices)
}

fn brep_set_at<'a>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    map: &'a mut BTreeMap<u32, BTreeSet<u32>>,
    key: u32,
    operation: &'static str,
) -> Result<&'a mut BTreeSet<u32>, cadmpeg_core::CodecError> {
    Ok(match map.entry(key) {
        std::collections::btree_map::Entry::Occupied(entry) => entry.into_mut(),
        std::collections::btree_map::Entry::Vacant(entry) => {
            ctx.charge_collection_items(1, operation)?;
            entry.insert(BTreeSet::new())
        }
    })
}

fn insert_brep_set_node(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    set: &mut BTreeSet<u32>,
    value: u32,
    operation: &'static str,
) -> Result<(), cadmpeg_core::CodecError> {
    if !set.contains(&value) {
        ctx.charge_collection_items(1, operation)?;
        set.insert(value);
    }
    Ok(())
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
        for curve_id in component_curves.intersection(face_curves) {
            ctx.charge_collection_items(1, "creo B-rep component face curve nodes")?;
            component_face_curves.insert(*curve_id);
        }
        let mut wire_curves = BTreeSet::new();
        for curve_id in component_curves.difference(face_curves) {
            ctx.charge_collection_items(1, "creo B-rep component wire curve nodes")?;
            wire_curves.insert(*curve_id);
        }
        let mut face_adjacency = BTreeMap::new();
        let mut face_vertices = BTreeMap::new();
        let mut faces_by_curve = BTreeMap::new();
        let mut faces_by_vertex = BTreeMap::new();
        for face_id in faces {
            brep_set_at(ctx, &mut face_adjacency, *face_id, "creo B-rep adjacency face nodes")?;
            let vertices =
                brep_set_at(ctx, &mut face_vertices, *face_id, "creo B-rep face vertex map nodes")?;
            for native_loop in &eligible_faces[face_id] {
                for half_edge in &native_loop.half_edges {
                    let curve_faces = brep_set_at(
                        ctx,
                        &mut faces_by_curve,
                        half_edge.curve_id,
                        "creo B-rep curve incidence map nodes",
                    )?;
                    insert_brep_set_node(
                        ctx,
                        curve_faces,
                        *face_id,
                        "creo B-rep curve incident face nodes",
                    )?;
                    let [start, end] = edge_vertices[&half_edge.curve_id];
                    for vertex_id in [start, end] {
                        insert_brep_set_node(
                            ctx,
                            vertices,
                            vertex_id,
                            "creo B-rep face vertex nodes",
                        )?;
                        let vertex_faces = brep_set_at(
                            ctx,
                            &mut faces_by_vertex,
                            vertex_id,
                            "creo B-rep vertex incidence map nodes",
                        )?;
                        insert_brep_set_node(
                            ctx,
                            vertex_faces,
                            *face_id,
                            "creo B-rep vertex incident face nodes",
                        )?;
                    }
                }
            }
        }
        for incident_faces in faces_by_curve.values().chain(faces_by_vertex.values()) {
            for (index, first) in incident_faces.iter().enumerate() {
                for second in incident_faces.iter().skip(index + 1) {
                    insert_brep_set_node(
                        ctx,
                        brep_set_at(
                            ctx,
                            &mut face_adjacency,
                            *first,
                            "creo B-rep adjacency face nodes",
                        )?,
                        *second,
                        "creo B-rep adjacency neighbour nodes",
                    )?;
                    insert_brep_set_node(
                        ctx,
                        brep_set_at(
                            ctx,
                            &mut face_adjacency,
                            *second,
                            "creo B-rep adjacency face nodes",
                        )?,
                        *first,
                        "creo B-rep adjacency neighbour nodes",
                    )?;
                }
            }
        }
        Ok(Self { component_face_curves, wire_curves, face_adjacency, face_vertices })
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
    ) -> Result<Self, cadmpeg_core::CodecError> {
        let mut face_ids = Vec::new();
        for face_id in &shell.faces {
            if !face_shell_ids.contains_key(face_id) {
                ctx.charge_collection_items(1, "creo B-rep face-shell nodes")?;
            }
            face_shell_ids.insert(
                *face_id,
                crate::identity::copy_checked_id(
                    ctx,
                    shell_id.as_str(),
                    "creo B-rep face-shell identity copies",
                )?,
            );
            ctx.try_reserve_items(&mut face_ids, 1, "creo B-rep shell face references")?;
            face_ids.push(crate::identity::compose_checked::<FaceId>(
                ctx,
                &crate::identity::VISIBGEOM_FACE,
                *face_id,
                "creo B-rep shell face identities",
            )?);
        }
        let mut edge_ids = Vec::new();
        for curve_id in &shell.wire_curves {
            ctx.try_reserve_items(&mut edge_ids, 1, "creo B-rep shell edge references")?;
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
        for index in 0..loop_count {
            ctx.try_reserve_items(&mut loop_ids, 1, "creo B-rep face loop IDs")?;
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
                for id in inner {
                    ctx.try_reserve_items(&mut inner_ids, 1, "creo B-rep inner loop IDs")?;
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
        Ok(Self { face, shell_id, loop_ids, face_loops })
    }
}

fn native_loop_ring(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    native_loop: &crate::topology::Loop,
    face_id: u32,
) -> Result<cadmpeg_ir::topology::LoopRing, cadmpeg_core::CodecError> {
    let mut coedge_ids = Vec::new();
    for half_edge in &native_loop.half_edges {
        ctx.try_reserve_items(&mut coedge_ids, 1, "creo B-rep ring coedge IDs")?;
        coedge_ids.push(crate::identity::compose_checked::<CoedgeId>(
            ctx,
            &crate::identity::VISIBGEOM_COEDGE,
            format_args!("{}:{}", half_edge.curve_id, half_edge.side.index()),
            "creo B-rep ring coedge identities",
        )?);
    }
    cadmpeg_ir::topology::LoopRing::new_admitted(
        ctx,
        coedge_ids,
        Vec::new(),
        "creo native loop ring validation nodes",
    )
    .map_err(|error| match error {
        cadmpeg_core::CodecError::Malformed(message) => {
            cadmpeg_core::CodecError::malformed(format!(
                "VisibGeom face {face_id} loop ring: {message}"
            ))
        }
        other => other,
    })
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
    ctx.try_reserve_items(losses, 1, "creo B-rep untransferred pcurve losses")?;
    losses.push(crate::loss::CreoLossCode::VisibGeomCurveUntransferred.note(message));
    Ok(())
}

fn one_coedge_pcurve_use(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    value: Option<PcurveUse>,
) -> Result<Vec<PcurveUse>, cadmpeg_core::CodecError> {
    let mut pcurves = Vec::new();
    if let Some(value) = value {
        ctx.try_reserve_items(&mut pcurves, 1, "creo B-rep coedge pcurve uses")?;
        pcurves.push(value);
    }
    Ok(pcurves)
}

/// Transfer the native `VisibGeom` B-rep: bodies, faces, loops, and coedges.
///
/// A coedge whose projected pcurve lane the IR carrier refuses is emitted
/// without a pcurve use, which the model carries, so the refusal is a loss
/// note naming the curve row and the face.
pub(in super::super) fn transfer_native_brep(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    curve_evidence: NativeBrepCurveEvidence<'_>,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
    source_carriers: &mut crate::decode::source_carriers::SourceUnitCarriers,
) -> Result<NativeBrepTransferSummary, cadmpeg_core::CodecError> {
    let carriers = placed_carriers(ctx, scan, ir, source_carriers)?;
    let BrepSourceIndexes { planes, half_edges, incidence } =
        BrepSourceIndexes::from_scan(ctx, &carriers, scan)?;
    let face_orientations = native_face_orientations(ctx, scan, ir)?;
    let solved_vertex_result = solve_topological_vertices(
        ctx,
        scan,
        ir,
        &carriers,
        curve_evidence.nurbs_endpoints,
        source_carriers,
    )?;
    let solved_vertices = &solved_vertex_result.points;
    let mut native_pcurves = NativePcurveCandidates::new();
    for (curve_id, faces, face_0_endpoints, face_1_endpoints, offset) in scan
        .curves
        .pcurves
        .iter()
        .map(|pcurve| {
            let [face_0_endpoints, face_1_endpoints] = canonicalized_pcurve_endpoints(
                scan,
                pcurve.faces,
                pcurve.face_0_endpoints,
                pcurve.face_1_endpoints,
            );
            (
                pcurve.curve_id,
                pcurve.faces,
                face_0_endpoints,
                face_1_endpoints,
                pcurve.offset,
            )
        })
        .chain(scan.curves.bound_prototype_pcurves.iter().map(|pcurve| {
            let [face_0_endpoints, face_1_endpoints] = canonicalized_pcurve_endpoints(
                scan,
                pcurve.faces,
                pcurve.face_0_endpoints,
                pcurve.face_1_endpoints,
            );
            (
                pcurve.curve_id,
                pcurve.faces,
                face_0_endpoints,
                face_1_endpoints,
                pcurve.offset,
            )
        }))
    {
        for (face, endpoints) in faces.into_iter().zip([face_0_endpoints, face_1_endpoints]) {
            if let Some(face) = face {
                push_native_pcurve_candidate(
                    ctx, &mut native_pcurves, curve_id, face.get(), endpoints, offset,
                )?;
            }
        }
    }
    for pcurve in &scan.curves.two_chart_pcurves {
        let Some(endpoint_sets) = crate::decode::analytic::pcurves::mapped_two_chart_endpoint_sets(
            scan,
            ir,
            pcurve,
            source_carriers,
        ) else {
            continue;
        };
        for (face_id, endpoints) in pcurve.faces.into_iter().zip(endpoint_sets.paths()) {
            if let Some(endpoints) = endpoints {
                push_native_pcurve_candidate(
                    ctx, &mut native_pcurves, pcurve.curve_id, face_id, endpoints, pcurve.offset,
                )?;
            }
        }
    }
    for pcurve in crate::curve::fc02_short_pcurve_endpoints(
        ctx,
        &scan.curves.parameters,
        &scan.curves.topology_rows,
    )? {
        let [face_0_endpoints, _] = canonicalized_pcurve_endpoints(
            scan,
            pcurve.faces.map(std::num::NonZeroU32::new),
            pcurve.face_0_endpoints,
            pcurve.face_0_endpoints,
        );
        push_native_pcurve_candidate(
            ctx,
            &mut native_pcurves,
            pcurve.curve_id,
            pcurve.faces[0],
            face_0_endpoints,
            pcurve.offset,
        )?;
    }
    let native_edge_vertices =
        crate::topology::edge_vertex_pairs(ctx, &scan.topology.half_edge_vertex_incidence)?;
    let BrepEdgeIndexes { edge_vertices, model_curve_counts, admitted_edge_curves } =
        BrepEdgeIndexes::from_rows(
            ctx,
            &scan.curves.topology_rows,
            &native_edge_vertices,
            solved_vertices,
            ir,
        )?;
    let BrepFaceCandidateIndexes {
        loops_by_face,
        candidate_face_ids,
        model_surface_counts,
        boundary_curve_ids,
        legacy_nonvisible_face_reference_count,
    } = BrepFaceCandidateIndexes::from_scan(ctx, scan, ir)?;
    let typed_nonlinear_curve_ids = model_typed_nonlinear_curve_ids(ctx, ir, source_carriers)?;
    let mut diagnostics = BrepTransferDiagnostics {
        candidate_face_count: candidate_face_ids.len(),
        legacy_nonvisible_face_reference_count,
        vertex_solve: solved_vertex_result.diagnostics,
        ..BrepTransferDiagnostics::default()
    };
    diagnostics.boundary_curve_count = boundary_curve_ids.len();
    diagnostics.boundary_curve_missing_incidence_count = boundary_curve_ids
        .iter()
        .filter(|curve_id| !native_edge_vertices.contains_key(curve_id))
        .count();
    diagnostics.boundary_curve_unsolved_vertex_count = boundary_curve_ids
        .iter()
        .filter(|curve_id| {
            native_edge_vertices.get(curve_id).is_some_and(|vertices| {
                vertices
                    .iter()
                    .any(|vertex| !solved_vertices.contains_key(vertex))
            })
        })
        .count();
    let mut eligible_faces = BTreeMap::new();
    for face_id in candidate_face_ids {
        if model_surface_counts[&face_id] == 0 {
            diagnostics.reject_face(ctx, FaceAdmissionRejection::MissingSurfaceCarrier, face_id)?;
            continue;
        }
        if !face_orientations.contains_key(&face_id) {
            diagnostics.reject_face(ctx, FaceAdmissionRejection::MissingOrientation, face_id)?;
            continue;
        }
        if model_surface_counts[&face_id] > 1 {
            diagnostics.reject_face(ctx, FaceAdmissionRejection::AmbiguousSurfaceCarrier, face_id)?;
            continue;
        }
        let Some(loops) = loops_by_face.get(&face_id) else {
            diagnostics.reject_face(ctx, FaceAdmissionRejection::MissingLoops, face_id)?;
            continue;
        };
        let has_unresolved_boundary_vertices = loops.iter().any(|lp| {
            lp.half_edges
                .iter()
                .any(|half_edge| !edge_vertices.contains_key(&half_edge.curve_id))
        });
        if has_unresolved_boundary_vertices {
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
            )?;
            continue;
        }
        let has_ambiguous_boundary_curve = loops.iter().any(|lp| {
            lp.half_edges.iter().any(|half_edge| {
                model_curve_counts
                    .get(&half_edge.curve_id)
                    .is_some_and(|count| *count > 1)
            })
        });
        if has_ambiguous_boundary_curve {
            diagnostics.reject_face(ctx, FaceAdmissionRejection::AmbiguousBoundaryCurve, face_id)?;
            continue;
        }
        let mut two_edge_loops_are_proven = true;
        for lp in loops.iter().filter(|lp| lp.half_edges.len() == 2) {
            let Some(surface) = exactly_one(
                ir.model
                    .surfaces
                    .iter()
                    .filter(|candidate| matches_native_surface_id(scan, face_id, &candidate.id)),
            ) else {
                two_edge_loops_are_proven = false;
                break;
            };
            if native_parameter_loop_polygon(
                ctx,
                lp,
                face_id,
                source_carriers.surface_geometry(surface),
                &incidence,
                solved_vertices,
                &native_pcurves,
                &typed_nonlinear_curve_ids,
            )?
            .is_none()
            {
                two_edge_loops_are_proven = false;
                break;
            }
        }
        if !two_edge_loops_are_proven {
            diagnostics.reject_face(ctx, FaceAdmissionRejection::TwoEdgeParameterProof, face_id)?;
            continue;
        }
        let ordered = ordered_face_loops(
            ctx,
            loops,
            planes.get(&face_id).copied(),
            &incidence,
            solved_vertices,
        )?;
        let ordered = if ordered.is_some() {
            ordered
        } else {
            let surface = exactly_one(
                ir.model
                    .surfaces
                    .iter()
                    .filter(|candidate| matches_native_surface_id(scan, face_id, &candidate.id)),
            );
            if let Some(surface) = surface {
                ordered_native_parameter_face_loops(
                    ctx,
                    &loops,
                    face_id,
                    source_carriers.surface_geometry(surface),
                    &incidence,
                    solved_vertices,
                    &native_pcurves,
                    NativeCurveEvidence {
                        typed_nonlinear_curve_ids: &typed_nonlinear_curve_ids,
                        model_curves: &ir.model.curves,
                        source_carriers,
                    },
                )?
            } else {
                None
            }
        };
        let Some(ordered) = ordered else {
            diagnostics.reject_face(ctx, FaceAdmissionRejection::LoopOrdering, face_id)?;
            continue;
        };
        ctx.charge_collection_items(1, "creo B-rep eligible face nodes")?;
        eligible_faces.insert(face_id, ordered);
    }
    diagnostics.admitted_face_count = eligible_faces.len();
    let BrepEligibleFaceIndexes {
        emitted_half_edges,
        face_curves,
        closed_single_edge_curves,
        row_offsets,
        curve_faces,
        eligible_face_ids,
    } = BrepEligibleFaceIndexes::from_faces(ctx, &eligible_faces, &scan.curves.topology_rows)?;
    let admitted_components = admitted_face_components(ctx, scan, &eligible_face_ids)?;
    let BrepBodyIndexes { neutral_edge_curves, body_components } =
        BrepBodyIndexes::from_components(
            ctx,
            scan,
            &admitted_components,
            &admitted_edge_curves,
            &eligible_faces,
            &eligible_face_ids,
            &curve_faces,
        )?;
    let selected_body_count = crate::topology::selected_body_count(
        scan.framing.declared_body_count,
        scan.framing.first_quilt_ptr,
        admitted_components.len(),
    );
    let empty_component_count = body_components
        .iter()
        .filter(|component| component.faces.is_empty() && component.wire_curves.is_empty())
        .count();
    let explicit_single_body =
        scan.framing.declared_body_count == Some(1) || scan.framing.first_quilt_ptr == Some(0);
    let body_components =
        if explicit_single_body && empty_component_count == 0 && !body_components.is_empty() {
            merge_body_components(ctx, body_components)?
        } else {
            body_components
        };
    let solved_point_count = solved_vertices.len();
    for (vertex_id, position) in solved_vertices {
        if ir.model.points.iter().any(|item| crate::identity::matches_numbered_identity(
            item.id.as_str(), "creo:visibgeom:point#", *vertex_id,
        )) {
            continue;
        }
        let point_id = crate::identity::compose_checked::<PointId>(
            ctx, &crate::identity::VISIBGEOM_POINT, vertex_id,
            "creo B-rep topological point identities",
        )?;
        annotate(
            annotations,
            &point_id,
            "VisibGeom",
            0,
            "topological_vertex_point",
            Exactness::Derived,
        );
        let source_object = SourceObjectAssociation {
            format: cadmpeg_ir::CodecFormat::Creo,
            object_id: cadmpeg_core::text::NonBlankString::new(ctx.format_retained(
                format_args!("topology:vertex#{vertex_id}"),
                "creo B-rep point source object IDs",
            )?)
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
        source_carriers.admit_point(ctx, ir, Point::new(point_id, position, Some(source_object)))?;
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
        return Ok(NativeBrepTransferSummary {
            topological_point_count: solved_point_count,
            diagnostics,
            ..NativeBrepTransferSummary::default()
        });
    }
    diagnostics.emitted_face_count = body_components
        .iter()
        .map(|component| component.faces.len())
        .sum();

    let used_vertices = used_brep_vertices(ctx, &neutral_edge_curves, &edge_vertices)?;

    for vertex_id in used_vertices {
        if ir.model.vertices.iter().any(|item| crate::identity::matches_numbered_identity(
            item.id.as_str(), "creo:visibgeom:vertex#", vertex_id,
        )) {
            continue;
        }
        let vertex = crate::identity::compose_checked::<VertexId>(
            ctx, &crate::identity::VISIBGEOM_VERTEX, vertex_id,
            "creo B-rep vertex identities",
        )?;
        let point_id = crate::identity::compose_checked::<PointId>(
            ctx, &crate::identity::VISIBGEOM_POINT, vertex_id,
            "creo B-rep vertex point identities",
        )?;
        annotate(
            annotations,
            &vertex,
            "VisibGeom",
            0,
            "topological_vertex_orbit",
            Exactness::Derived,
        );
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
    for curve_id in &neutral_edge_curves {
        let [start, end] = edge_vertices[curve_id];
        let curve = crate::identity::compose_checked::<CurveId>(
            ctx, &crate::identity::VISIBGEOM_CURVE, *curve_id,
            "creo B-rep edge curve identities",
        )?;
        let points = [solved_vertices[&start], solved_vertices[&end]];
        let unbacked_closed_edge = start == end
            && closed_single_edge_curves.contains(curve_id)
            && curve_faces.get(curve_id).is_some_and(|face_ids| {
                !face_ids
                    .iter()
                    .any(|face_id| native_pcurves.contains_key(&(*curve_id, *face_id)))
            });
        let model_curve_count = model_curve_counts[curve_id];
        let param_range = if model_curve_count == 0 {
            None
        } else {
            let candidate = exactly_one(
                ir.model
                    .curves
                    .iter_mut()
                    .filter(|candidate| candidate.id == curve),
            );
            if let Some(candidate) = candidate {
                let mut geometry = source_carriers
                    .curve_geometry(candidate)
                    .copy_admitted(ctx, "creo B-rep edge source curve geometry")?;
                let derived_line = curve_evidence.derived_intersections.contains(&curve)
                    && matches!(geometry.solved(), Some(SolvedCurveGeometry::Line(_)));
                let range = if derived_line {
                    orient_line_edge_carrier(&mut geometry, points)
                } else {
                    orient_nonperiodic_nurbs_edge_carrier(&mut geometry, points).or_else(|| {
                        exact_line_edge_parameter_range(&geometry, points).or_else(|| {
                            nonperiodic_conic_edge_parameter_range(&geometry, points)
                                .or_else(|| {
                                    pcurve_backed_periodic_conic_parameter_range(
                                        &geometry,
                                        *curve_id,
                                        *curve_faces.get(curve_id)?,
                                        &native_pcurves,
                                        &ir.model.surfaces,
                                        points,
                                        source_carriers,
                                    )
                                })
                                .or_else(|| {
                                    unbacked_closed_edge.then_some(()).and_then(|()| {
                                        full_periodic_conic_edge_parameter_range(
                                            &geometry, points[0],
                                        )
                                    })
                                })
                                .or_else(|| {
                                    unbacked_closed_edge.then_some(()).and_then(|()| {
                                        full_periodic_nurbs_edge_parameter_range(
                                            &geometry, points[0],
                                        )
                                    })
                                })
                        })
                    })
                };
                source_carriers.replace_curve_geometry(ctx, candidate, geometry)?;
                range
            } else {
                None
            }
        };
        let id = crate::identity::compose_checked::<EdgeId>(
            ctx, &crate::identity::VISIBGEOM_EDGE, *curve_id,
            "creo B-rep edge identities",
        )?;
        annotate(
            annotations,
            &id,
            "VisibGeom",
            row_offsets.get(curve_id).copied().unwrap_or(0) as u64,
            "curve_topology_edge",
            Exactness::Derived,
        );
        ctx.charge_entities(1, "admit Creo model edges")?;
        source_carriers.admit_edge(
            ctx,
            ir,
            Edge {
                id,
                carrier: cadmpeg_ir::topology::EdgeCarrier::new(Some(crate::identity::copy_checked_id(
                    ctx, curve.as_str(), "creo B-rep edge carrier curve ID copies",
                )?), param_range)
                    .map_err(cadmpeg_core::CodecError::malformed)?,
                start: crate::identity::compose_checked(
                    ctx, &crate::identity::VISIBGEOM_VERTEX, start,
                    "creo B-rep edge start vertex identities",
                )?,
                end: crate::identity::compose_checked(
                    ctx, &crate::identity::VISIBGEOM_VERTEX, end,
                    "creo B-rep edge end vertex identities",
                )?,
                tolerance: None,
            },
        )?;
        if !ir.model.curves.iter().any(|item| item.id == curve) {
            let offset = row_offsets.get(curve_id).copied().unwrap_or(0);
            annotate(
                annotations,
                &curve,
                "VisibGeom",
                offset as u64,
                "opaque_native_curve_carrier",
                Exactness::Unknown,
            );
            ctx.charge_entities(1, "admit Creo model curves")?;
            source_carriers.admit_curve(
                ctx,
                ir,
                Curve {
                    id: curve,
                    geometry: CurveGeometry::Solved(SolvedCurveGeometry::Unknown {
                        record: geometry_section_record(scan, offset),
                    }),
                    source_object: Some(SourceObjectAssociation {
                        format: cadmpeg_ir::CodecFormat::Creo,
                        object_id: cadmpeg_core::text::NonBlankString::new(ctx.format_retained(
                            format_args!("VisibGeom:{curve_id}"),
                            "creo B-rep curve source object IDs",
                        )?)
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

    for (component_index, component) in body_components.iter().enumerate() {
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
        annotate(annotations, &body_id, "VisibGeom", 0, "native_component_body", Exactness::Derived);
        annotate(annotations, &region_id, "VisibGeom", 0, "native_component_region", Exactness::Derived);
        let BrepComponentTopology {
            component_face_curves,
            wire_curves,
            face_adjacency,
            face_vertices,
        } = BrepComponentTopology::from_component(
            ctx,
            component_curves,
            &face_curves,
            faces,
            &eligible_faces,
            &edge_vertices,
        )?;
        let closed = component_is_closed(
            &component_face_curves,
            &emitted_half_edges,
            &half_edges,
            faces,
        );

        let shell_specs = split_neutral_component_shells(
            ctx,
            faces,
            &wire_curves,
            &face_adjacency,
            &face_vertices,
            &edge_vertices,
        )?;

        let mut face_shell_ids = BTreeMap::<u32, ShellId>::new();
        let mut shell_ids = Vec::new();
        for (shell_index, shell) in shell_specs.iter().enumerate() {
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
                annotations,
                &shell_id,
                "VisibGeom",
                0,
                "native_component_shell",
                Exactness::Derived,
            );
            if shell.faces.is_empty() && shell.wire_curves.is_empty() {
                diagnostics.empty_component_count += 1;
                continue;
            }
            let BrepShellReferences { face_ids, edge_ids } =
                BrepShellReferences::from_shell(ctx, shell, &shell_id, &mut face_shell_ids)?;
            ctx.charge_entities(1, "admit Creo model shells")?;
            let Ok(shell_entity) = Shell::new(
                crate::identity::copy_checked_id(ctx, shell_id.as_str(), "creo B-rep shell entity ID copy")?,
                crate::identity::copy_checked_id(ctx, region_id.as_str(), "creo B-rep shell region ID copy")?,
                face_ids,
                edge_ids,
                Vec::new(),
            ) else {
                diagnostics.empty_component_count += 1;
                continue;
            };
            ctx.try_reserve_items(&mut ir.model.shells, 1, "creo model shells")?;
            ir.model.shells.push(shell_entity);
            ctx.try_reserve_items(&mut shell_ids, 1, "creo B-rep body shell IDs")?;
            shell_ids.push(shell_id);
        }
        let mut region_ids = Vec::new();
        ctx.try_reserve_items(&mut region_ids, 1, "creo B-rep body region IDs")?;
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
                id: crate::identity::copy_checked_id(ctx, body_id.as_str(), "creo B-rep body entity ID copy")?,
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
        ctx.try_reserve_items(&mut ir.model.regions, 1, "creo model regions")?;
        ir.model.regions.push(Region {
            id: crate::identity::copy_checked_id(ctx, region_id.as_str(), "creo B-rep region entity ID copy")?,
            body: body_id,
            shells: shell_ids,
        });
        for face_id in faces {
            let native_loops = &eligible_faces[face_id];
            let BrepFaceReferences { face, shell_id, loop_ids, face_loops } =
                BrepFaceReferences::from_loops(
                    ctx,
                    *face_id,
                    &face_shell_ids[face_id],
                    native_loops.len(),
                )?;
            let visible_row = crate::surface::unique_surface_row(&scan.surfaces.rows, *face_id);
            let active_datum = scan
                .planes
                .datum_cylinders
                .iter()
                .find(|datum| datum.id == *face_id);
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
            if !ir.model.surfaces.iter().any(|item| item.id == surface) {
                annotate(
                    annotations,
                    &surface,
                    face_source_namespace,
                    face_offset as u64,
                    "opaque_native_surface_carrier",
                    Exactness::Unknown,
                );
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
                            record: geometry_section_record(scan, face_offset),
                        }),
                        source_object: Some(SourceObjectAssociation {
                            format: cadmpeg_ir::CodecFormat::Creo,
                            object_id: cadmpeg_core::text::NonBlankString::new(ctx.format_retained(
                                format_args!("VisibGeom:{face_id}"),
                                "creo B-rep surface source object IDs",
                            )?)
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
            let face_sense = if face_orientations[face_id] {
                Sense::Reversed
            } else {
                Sense::Forward
            };
            annotate(
                annotations,
                &face,
                "VisibGeom",
                face_offset as u64,
                "native_face",
                Exactness::Derived,
            );
            for loop_id in &loop_ids {
                annotate(
                    annotations,
                    loop_id,
                    "VisibGeom",
                    face_offset as u64,
                    "native_face_loop",
                    Exactness::Derived,
                );
            }
            ctx.charge_entities(1, "admit Creo model faces")?;
            source_carriers.admit_face(
                ctx,
                ir,
                Face {
                    id: crate::identity::copy_checked_id(ctx, face.as_str(), "creo B-rep face entity ID copy")?,
                    shell: shell_id,
                    surface,
                    sense: face_sense,
                    loops: face_loops,
                    name: None,
                    color: None,
                    tolerance: None,
                },
            )?;
            for (native_loop, loop_id) in native_loops.iter().zip(loop_ids) {
                let ring = native_loop_ring(ctx, native_loop, *face_id)?;
                ctx.charge_entities(1, "admit Creo model loops")?;
                ctx.try_reserve_items(&mut ir.model.loops, 1, "creo model native loops")?;
                ir.model.loops.push(IrLoop {
                    id: crate::identity::copy_checked_id(ctx, loop_id.as_str(), "creo B-rep model loop ID copy")?,
                    face: crate::identity::copy_checked_id(ctx, face.as_str(), "creo B-rep model loop face ID copy")?,
                    boundary: cadmpeg_ir::topology::LoopBoundary::Ring(ring),
                });
                for half_edge in &native_loop.half_edges {
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
                    let radial_next = if emitted_half_edges.contains(&twin) {
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
                        annotations,
                        &id,
                        "VisibGeom",
                        row_offsets.get(&half_edge.curve_id).copied().unwrap_or(0) as u64,
                        "native_half_edge",
                        Exactness::Derived,
                    );
                    let native_candidates = native_pcurves.get(&(half_edge.curve_id, *face_id));
                    let mut refusal = crate::lane_refusal::LaneRefusals::new();
                    let refusal_cell = &mut refusal;
                    let mut planar_resource_error = None;
                    let pcurve_geometry = native_candidates
                        .and_then(|candidates| {
                            let incidence = incidence.get(half_edge)?;
                            let end = incidence.end_vertex_id?;
                            let traversal = [
                                solved_vertices[&incidence.start_vertex_id],
                                solved_vertices[&end],
                            ];
                            let surface = exactly_one(
                                ir.model
                                    .surfaces
                                    .iter()
                                    .filter(|candidate| matches_native_surface_id(scan, *face_id, &candidate.id)),
                            )?;
                            unique_oriented_native_pcurve(
                                source_carriers.surface_geometry(surface),
                                candidates,
                                traversal,
                            )
                        })
                        .and_then(|(endpoints, offset)| {
                            Some((
                                line_pcurve(endpoints[0], endpoints[1])?,
                                Some([0.0, 1.0]),
                                offset,
                                "native_endpoint_pcurve",
                            ))
                        })
                        .or_else(|| {
                            native_candidates.is_none().then_some(())?;
                            let surface = exactly_one(
                                ir.model
                                    .surfaces
                                    .iter()
                                    .filter(|candidate| matches_native_surface_id(scan, *face_id, &candidate.id)),
                            )?;
                            let curve = exactly_one(
                                ir.model
                                    .curves
                                    .iter()
                                    .filter(|candidate| crate::identity::matches_numbered_identity(
                                        candidate.id.as_str(),
                                        "creo:visibgeom:curve#",
                                        half_edge.curve_id,
                                    )),
                            )?;
                            let edge = exactly_one(
                                ir.model
                                    .edges
                                    .iter()
                                    .filter(|candidate| crate::identity::matches_numbered_identity(
                                        candidate.id.as_str(),
                                        "creo:visibgeom:edge#",
                                        half_edge.curve_id,
                                    )),
                            )?;
                            let planar = match planar_curve_pcurve(
                                ctx,
                                source_carriers.surface_geometry(surface),
                                source_carriers.curve_geometry(curve),
                                &format_args!(
                                    "VisibGeom curve-topology row {} on face {face_id}",
                                    half_edge.curve_id
                                ),
                                refusal_cell,
                            ) {
                                Ok(planar) => planar,
                                Err(error) => {
                                    planar_resource_error = Some(error);
                                    None
                                }
                            };
                            if planar_resource_error.is_some() {
                                return None;
                            }
                            let (geometry, tag) = planar
                            .map(|geometry| (geometry, "projected_planar_pcurve"))
                            .or_else(|| {
                                surface_of_revolution_parallel_pcurve(
                                    source_carriers.surface_geometry(surface),
                                    source_carriers.curve_geometry(curve),
                                )
                                .map(|geometry| (geometry, "projected_parallel_conic_pcurve"))
                            })
                            .or_else(|| {
                                meridian_circle_pcurve(
                                    source_carriers.surface_geometry(surface),
                                    source_carriers.curve_geometry(curve),
                                )
                                .map(|geometry| (geometry, "projected_meridian_pcurve"))
                            })
                            .or_else(|| {
                                ruled_generator_line_pcurve(
                                    source_carriers.surface_geometry(surface),
                                    source_carriers.curve_geometry(curve),
                                )
                                .map(|geometry| (geometry, "projected_ruled_generator_pcurve"))
                            })?;
                            Some((
                                geometry,
                                source_carriers.source_edge_parameter_range(edge),
                                row_offsets.get(&half_edge.curve_id).copied().unwrap_or(0),
                                tag,
                            ))
                        });
                    if let Some(error) = planar_resource_error {
                        return Err(error);
                    }
                    let refused = refusal.take_records_checked()?;
                    if pcurve_geometry.is_none() {
                        for record in refused {
                            push_untransferred_pcurve_loss(
                                ctx,
                                losses,
                                half_edge.curve_id,
                                *face_id,
                                record,
                            )?;
                        }
                    }
                    let pcurve_use = pcurve_geometry
                        .map(|(geometry, parameter_range, offset, tag)| -> Result<_, cadmpeg_core::CodecError> {
                            let parameter_range = match parameter_range {
                                Some(range) => {
                                    let Some(range) = cadmpeg_ir::units::FiniteVector::new(range) else {
                                        return Ok(None);
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
                            if !ir.model.pcurves.iter().any(|item| item.id == pcurve) {
                                annotate(
                                    annotations,
                                    &pcurve,
                                    "VisibGeom",
                                    offset as u64,
                                    tag,
                                    Exactness::Derived,
                                );
                                ctx.charge_entities(1, "admit Creo model pcurves")?;
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
                                    &native_surface_id(ctx, scan, *face_id)?,
                                )?;
                            }
                            Ok(Some(PcurveUse {
                                pcurve,
                                isoparametric: None,
                                parameter_range: None,
                            }))
                        })
                        .transpose()?
                        .flatten();
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
    Ok(NativeBrepTransferSummary {
        topological_point_count: solved_point_count,
        native_topological_edge_count: neutral_edge_curves.len(),
        diagnostics,
    })
}

pub(in super::super) fn transfer_cap_pair_cylinders(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    source_carriers: &mut crate::decode::source_carriers::SourceUnitCarriers,
) -> Result<(), cadmpeg_core::CodecError> {
    for pair in &scan.curves.fc05_cylinder_cap_pairs {
        let Some(frame) = fc05_cap_pair_model_frame(scan, pair) else {
            continue;
        };
        let id = SurfaceId::compose(&crate::identity::VISIBGEOM_SURFACE, pair.surface_id);
        if ir.model.surfaces.iter().any(|surface| surface.id == id) {
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
        annotate(
            annotations,
            &id,
            "VisibGeom",
            pair.offset as u64,
            "fc05_cap_pair_cylinder",
            Exactness::Derived,
        );
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
                    object_id: cadmpeg_core::text::NonBlankString::new(format!(
                        "VisibGeom:{}",
                        pair.surface_id
                    ))
                    .ok_or_else(|| {
                        cadmpeg_core::CodecError::malformed("source object_id must not be empty")
                    })?,
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
        } in &pair.cap_edges
        {
            let cap_offset =
                crate::surface::unique_outline_plane(&scan.planes.outlines, *cap_plane_id)
                    .map_or_else(
                        || {
                            frame.origin[frame.axis_index.index()]
                                + frame.axis_sign.scale() * ordinate
                        },
                        |plane| plane.origin[frame.axis_index.index()],
                    );
            let (center, _, _) = fc05_model_frame(
                frame.axis_index,
                cap_offset,
                pair.center_row_frame,
                pair.reference_direction_row_frame,
                frame.axis_sign,
            );
            let id = CurveId::compose(&crate::identity::VISIBGEOM_CURVE, curve_id);
            if ir.model.curves.iter().any(|curve| curve.id == id) {
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
            annotate(
                annotations,
                &id,
                "VisibGeom",
                scan.curves
                    .fc05_circles
                    .iter()
                    .find(|circle| circle.curve_id == *curve_id)
                    .map_or(pair.offset, |circle| circle.offset) as u64,
                "fc05_cap_circle",
                Exactness::Derived,
            );
            ctx.charge_entities(1, "admit Creo model curves")?;
            source_carriers.admit_curve(
                ctx,
                ir,
                Curve {
                    id,
                    geometry: CurveGeometry::Solved(SolvedCurveGeometry::Circle(circle_curve)),
                    source_object: Some(SourceObjectAssociation {
                        format: cadmpeg_ir::CodecFormat::Creo,
                        object_id: cadmpeg_core::text::NonBlankString::new(format!(
                            "VisibGeom:{curve_id}"
                        ))
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
    Ok(())
}
