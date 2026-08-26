// SPDX-License-Identifier: Apache-2.0
//! Native B-rep transfer and FC05 cap-pair cylinders.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::{Curve, CurveGeometry, Pcurve, Surface, SurfaceGeometry};
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

use super::super::analytic::{
    canonicalized_pcurve_endpoints, exact_line_edge_parameter_range,
    full_periodic_conic_edge_parameter_range, full_periodic_nurbs_edge_parameter_range,
    geometry_section_record, meridian_circle_pcurve, native_face_orientations,
    nonperiodic_conic_edge_parameter_range, ordered_face_loops, ordered_parameter_face_loops,
    orient_line_edge_carrier, orient_nonperiodic_nurbs_edge_carrier,
    pcurve_backed_periodic_conic_parameter_range, placed_carriers, planar_curve_pcurve,
    ruled_generator_line_pcurve, solve_topological_vertices, surface_of_revolution_parallel_pcurve,
    unique_oriented_native_pcurve, CarrierEquation, NativePcurveCandidates,
    TopologicalVertexSolveDiagnostics,
};
use super::super::expanded::half_edge_ref;
use super::super::native::annotate;
use super::super::records::CreoFaceAdmissionRejectionRecord;
use super::super::sweep::line_pcurve;
use super::super::uniqueness::exactly_one;

use super::{fc05_cap_pair_model_frame, fc05_model_frame, native_surface_id};

const EPS_PARAMETER_AGREE: f64 = 1e-9;
const EPS_GEOMETRY_AGREE: f64 = 1e-9;
const FACE_REJECTION_SAMPLE_LIMIT: usize = 4;
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

    pub(in super::super) const fn key(self) -> &'static str {
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
    pub(in super::super) reason: FaceAdmissionRejection,
    pub(in super::super) detail: FaceAdmissionDetail,
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
        face_id: u32,
        loops: &[&crate::topology::Loop],
        edge_vertices: &BTreeMap<u32, [u32; 2]>,
        incidence: &BTreeMap<HalfEdgeId, &crate::topology::HalfEdgeVertexIncidence>,
    ) -> Self {
        let mut detail = Self::face(face_id);
        for half_edge in loops.iter().flat_map(|lp| lp.half_edges.iter()) {
            if edge_vertices.contains_key(&half_edge.curve_id) {
                continue;
            }
            if detail.boundary_half_edges.len() < FACE_REJECTION_OPERAND_SAMPLE_LIMIT {
                detail.boundary_half_edges.push(*half_edge);
            }
            if let Some(binding) = incidence.get(half_edge) {
                if detail.vertex_ids.len() < FACE_REJECTION_OPERAND_SAMPLE_LIMIT
                    && !detail.vertex_ids.contains(&binding.start_vertex_id)
                {
                    detail.vertex_ids.push(binding.start_vertex_id);
                }
                if let Some(end_vertex_id) = binding.end_vertex_id {
                    if detail.vertex_ids.len() < FACE_REJECTION_OPERAND_SAMPLE_LIMIT
                        && !detail.vertex_ids.contains(&end_vertex_id)
                    {
                        detail.vertex_ids.push(end_vertex_id);
                    }
                }
            }
        }
        detail
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
pub(in super::super) struct FaceAdmissionEvidence {
    pub(in super::super) count: usize,
    pub(in super::super) sample_ids: Vec<u32>,
    pub(in super::super) sample_details: Vec<FaceAdmissionDetail>,
}

#[derive(Debug, Default, PartialEq)]
pub(in super::super) struct BrepTransferDiagnostics {
    pub(in super::super) candidate_face_count: usize,
    pub(in super::super) admitted_face_count: usize,
    pub(in super::super) emitted_face_count: usize,
    pub(in super::super) boundary_curve_count: usize,
    pub(in super::super) boundary_curve_missing_incidence_count: usize,
    pub(in super::super) boundary_curve_unsolved_vertex_count: usize,
    pub(in super::super) vertex_solve: TopologicalVertexSolveDiagnostics,
    pub(in super::super) rejected_faces: BTreeMap<FaceAdmissionRejection, FaceAdmissionEvidence>,
    pub(in super::super) face_rejection_diagnostics: Vec<FaceAdmissionDiagnostic>,
    pub(in super::super) legacy_nonvisible_face_reference_count: usize,
    pub(in super::super) body_count_mismatch: bool,
    pub(in super::super) legacy_body_ownership_ambiguous: bool,
    pub(in super::super) empty_component_count: usize,
    pub(in super::super) admitted_component_count: usize,
    pub(in super::super) selected_body_count: Option<usize>,
}

impl BrepTransferDiagnostics {
    fn reject_face(&mut self, reason: FaceAdmissionRejection, face_id: u32) {
        self.reject_face_with_detail(reason, FaceAdmissionDetail::face(face_id));
    }

    fn reject_face_with_detail(
        &mut self,
        reason: FaceAdmissionRejection,
        detail: FaceAdmissionDetail,
    ) {
        self.face_rejection_diagnostics
            .push(FaceAdmissionDiagnostic {
                reason,
                detail: detail.clone(),
            });
        let evidence = self.rejected_faces.entry(reason).or_default();
        evidence.count += 1;
        if evidence.sample_ids.len() < FACE_REJECTION_SAMPLE_LIMIT {
            evidence.sample_ids.push(detail.face_id);
        }
        if evidence.sample_details.len() < FACE_REJECTION_SAMPLE_LIMIT {
            evidence.sample_details.push(detail);
        }
    }

    pub(in super::super) fn face_admission_rejection_records(
        &self,
    ) -> Vec<CreoFaceAdmissionRejectionRecord> {
        self.face_rejection_diagnostics
            .iter()
            .map(|diagnostic| {
                let detail = &diagnostic.detail;
                CreoFaceAdmissionRejectionRecord {
                    id: format!("creo:brep:face_admission_rejection#{}", detail.face_id),
                    face_id: detail.face_id,
                    reason: diagnostic.reason.key(),
                    boundary_half_edges: detail
                        .boundary_half_edges
                        .iter()
                        .copied()
                        .map(half_edge_ref)
                        .collect(),
                    vertex_ids: detail.vertex_ids.clone(),
                }
            })
            .collect()
    }

    pub(in super::super) fn record_coverage(&self, coverage: &mut BTreeMap<String, usize>) {
        coverage.insert(
            "brep_candidate_face_count".to_string(),
            self.candidate_face_count,
        );
        coverage.insert(
            "brep_admitted_face_count".to_string(),
            self.admitted_face_count,
        );
        coverage.insert(
            "brep_emitted_face_count".to_string(),
            self.emitted_face_count,
        );
        coverage.insert(
            "brep_boundary_curve_count".to_string(),
            self.boundary_curve_count,
        );
        coverage.insert(
            "brep_boundary_curve_missing_incidence_count".to_string(),
            self.boundary_curve_missing_incidence_count,
        );
        coverage.insert(
            "brep_boundary_curve_unsolved_vertex_count".to_string(),
            self.boundary_curve_unsolved_vertex_count,
        );
        if self.legacy_nonvisible_face_reference_count > 0 {
            coverage.insert(
                "brep_legacy_nonvisible_face_reference_count".to_string(),
                self.legacy_nonvisible_face_reference_count,
            );
        }
        coverage.insert(
            "brep_vertex_topological_count".to_string(),
            self.vertex_solve.topological_vertices,
        );
        coverage.insert(
            "brep_vertex_carrier_incident_count".to_string(),
            self.vertex_solve.carrier_incident_vertices,
        );
        coverage.insert(
            "brep_vertex_carrier_pair_intersection_candidate_count".to_string(),
            self.vertex_solve.carrier_pair_candidates,
        );
        coverage.insert(
            "brep_vertex_carrier_triple_intersection_candidate_count".to_string(),
            self.vertex_solve.carrier_triple_candidates,
        );
        coverage.insert(
            "brep_vertex_carrier_valid_intersection_candidate_count".to_string(),
            self.vertex_solve.carrier_valid_candidates,
        );
        coverage.insert(
            "brep_vertex_carrier_zero_candidate_count".to_string(),
            self.vertex_solve.carrier_zero_candidate_vertices,
        );
        if self.vertex_solve.carrier_no_geometric_candidate_vertices != 0 {
            coverage.insert(
                "brep_vertex_carrier_no_geometric_candidate_count".to_string(),
                self.vertex_solve.carrier_no_geometric_candidate_vertices,
            );
        }
        if self.vertex_solve.carrier_no_valid_candidate_vertices != 0 {
            coverage.insert(
                "brep_vertex_carrier_no_valid_candidate_count".to_string(),
                self.vertex_solve.carrier_no_valid_candidate_vertices,
            );
        }
        coverage.insert(
            "brep_vertex_carrier_ambiguous_candidate_count".to_string(),
            self.vertex_solve.carrier_ambiguous_candidate_vertices,
        );
        coverage.insert(
            "brep_vertex_carrier_point_count".to_string(),
            self.vertex_solve.carrier_points,
        );
        coverage.insert(
            "brep_pcurve_record_count".to_string(),
            self.vertex_solve.pcurve.records,
        );
        coverage.insert(
            "brep_pcurve_path_count".to_string(),
            self.vertex_solve.pcurve.paths,
        );
        let pcurve = &self.vertex_solve.pcurve;
        if pcurve.inactive_paths > 0
            || pcurve.inactive_records > 0
            || pcurve.partial_records > 0
            || pcurve.topology_mismatch_records > 0
        {
            coverage.insert(
                "brep_pcurve_inactive_path_count".to_string(),
                pcurve.inactive_paths,
            );
            coverage.insert(
                "brep_pcurve_inactive_record_count".to_string(),
                pcurve.inactive_records,
            );
            coverage.insert(
                "brep_pcurve_partial_record_count".to_string(),
                pcurve.partial_records,
            );
            coverage.insert(
                "brep_pcurve_topology_mismatch_record_count".to_string(),
                pcurve.topology_mismatch_records,
            );
        }
        coverage.insert(
            "brep_pcurve_missing_surface_path_count".to_string(),
            self.vertex_solve.pcurve.missing_surfaces,
        );
        coverage.insert(
            "brep_pcurve_unevaluable_path_count".to_string(),
            self.vertex_solve.pcurve.unevaluable_paths,
        );
        coverage.insert(
            "brep_pcurve_mapped_path_count".to_string(),
            self.vertex_solve.pcurve.mapped_paths,
        );
        if pcurve.carrier_validated_paths > 0
            || pcurve.carrier_rejected_paths > 0
            || pcurve.carrier_unknown_paths > 0
            || pcurve.carrier_rejected_records > 0
        {
            coverage.insert(
                "brep_pcurve_carrier_validated_path_count".to_string(),
                pcurve.carrier_validated_paths,
            );
            coverage.insert(
                "brep_pcurve_carrier_rejected_path_count".to_string(),
                pcurve.carrier_rejected_paths,
            );
            coverage.insert(
                "brep_pcurve_carrier_unknown_path_count".to_string(),
                pcurve.carrier_unknown_paths,
            );
            coverage.insert(
                "brep_pcurve_carrier_unknown_missing_surface_path_count".to_string(),
                pcurve.carrier_unknown_missing_surface_paths,
            );
            coverage.insert(
                "brep_pcurve_carrier_unknown_missing_carrier_path_count".to_string(),
                pcurve.carrier_unknown_missing_carrier_paths,
            );
            coverage.insert(
                "brep_pcurve_carrier_unknown_unsupported_pair_path_count".to_string(),
                pcurve.carrier_unknown_unsupported_pair_paths,
            );
            coverage.insert(
                "brep_pcurve_carrier_unknown_parallel_plane_path_count".to_string(),
                pcurve.carrier_unknown_parallel_plane_paths,
            );
            coverage.insert(
                "brep_pcurve_carrier_unknown_unsupported_path_count".to_string(),
                pcurve.carrier_unknown_unsupported_path_paths,
            );
            coverage.insert(
                "brep_pcurve_carrier_rejected_record_count".to_string(),
                pcurve.carrier_rejected_records,
            );
        }
        coverage.insert(
            "brep_pcurve_unmapped_record_count".to_string(),
            self.vertex_solve.pcurve.unmapped_records,
        );
        coverage.insert(
            "brep_pcurve_inconsistent_record_count".to_string(),
            self.vertex_solve.pcurve.inconsistent_records,
        );
        coverage.insert(
            "brep_pcurve_accepted_record_count".to_string(),
            self.vertex_solve.pcurve.accepted_records,
        );
        coverage.insert(
            "brep_pcurve_complete_record_count".to_string(),
            self.vertex_solve.pcurve.complete_records,
        );
        if self.vertex_solve.pcurve.two_chart_records > 0 {
            coverage.insert(
                "brep_pcurve_two_chart_record_count".to_string(),
                self.vertex_solve.pcurve.two_chart_records,
            );
            coverage.insert(
                "brep_pcurve_two_chart_mapped_record_count".to_string(),
                self.vertex_solve.pcurve.two_chart_mapped_records,
            );
            coverage.insert(
                "brep_pcurve_two_chart_complete_record_count".to_string(),
                self.vertex_solve.pcurve.two_chart_complete_records,
            );
            coverage.insert(
                "brep_pcurve_two_chart_partial_record_count".to_string(),
                self.vertex_solve.pcurve.two_chart_partial_records,
            );
            coverage.insert(
                "brep_pcurve_two_chart_missing_surface_path_count".to_string(),
                self.vertex_solve.pcurve.two_chart_missing_surface_paths,
            );
            coverage.insert(
                "brep_pcurve_two_chart_unevaluable_path_count".to_string(),
                self.vertex_solve.pcurve.two_chart_unevaluable_paths,
            );
            coverage.insert(
                "brep_pcurve_two_chart_surface_mismatch_record_count".to_string(),
                self.vertex_solve.pcurve.two_chart_surface_mismatch_records,
            );
            coverage.insert(
                "brep_pcurve_two_chart_no_sample_record_count".to_string(),
                self.vertex_solve.pcurve.two_chart_no_sample_records,
            );
            coverage.insert(
                "brep_pcurve_two_chart_unmapped_record_count".to_string(),
                self.vertex_solve.pcurve.two_chart_unmapped_records,
            );
        }
        coverage.insert(
            "brep_pcurve_conflicting_curve_count".to_string(),
            self.vertex_solve.pcurve.conflicting_curves,
        );
        coverage.insert(
            "brep_vertex_pcurve_endpoint_evidence_count".to_string(),
            self.vertex_solve.pcurve.evidence,
        );
        coverage.insert(
            "brep_vertex_complete_pcurve_endpoint_evidence_count".to_string(),
            self.vertex_solve.pcurve.complete_evidence,
        );
        coverage.insert(
            "brep_vertex_pcurve_constraint_count".to_string(),
            self.vertex_solve.pcurve_constraints,
        );
        if self.vertex_solve.pcurve_fixed_endpoint_conflicts > 0 {
            coverage.insert(
                "brep_vertex_pcurve_fixed_endpoint_conflict_count".to_string(),
                self.vertex_solve.pcurve_fixed_endpoint_conflicts,
            );
        }
        if self.vertex_solve.pcurve_ambiguous_endpoint_vertices > 0 {
            coverage.insert(
                "brep_vertex_pcurve_ambiguous_endpoint_vertex_count".to_string(),
                self.vertex_solve.pcurve_ambiguous_endpoint_vertices,
            );
        }
        coverage.insert(
            "brep_vertex_directed_endpoint_assignment_count".to_string(),
            self.vertex_solve.directed_endpoint_assignments,
        );
        coverage.insert(
            "brep_vertex_directed_endpoint_conflict_count".to_string(),
            self.vertex_solve.directed_endpoint_conflicts,
        );
        coverage.insert(
            "brep_vertex_nurbs_endpoint_constraint_count".to_string(),
            self.vertex_solve.nurbs_endpoint_constraints,
        );
        coverage.insert(
            "brep_vertex_analytic_domain_count".to_string(),
            self.vertex_solve.analytic_domain_vertices,
        );
        coverage.insert(
            "brep_vertex_solved_count".to_string(),
            self.vertex_solve.solved_vertices,
        );
        coverage.insert(
            "brep_rejected_face_count".to_string(),
            self.rejected_faces
                .values()
                .map(|evidence| evidence.count)
                .sum(),
        );
        for reason in FaceAdmissionRejection::ALL {
            coverage.insert(
                format!("brep_rejected_face_{}_count", reason.key()),
                self.rejected_faces
                    .get(&reason)
                    .map_or(0, |evidence| evidence.count),
            );
        }
        coverage.insert(
            "brep_body_count_mismatch_count".to_string(),
            usize::from(self.body_count_mismatch),
        );
        coverage.insert(
            "brep_legacy_body_ownership_ambiguous_count".to_string(),
            usize::from(self.legacy_body_ownership_ambiguous),
        );
        coverage.insert(
            "brep_empty_component_count".to_string(),
            self.empty_component_count,
        );
        coverage.insert(
            "brep_admitted_component_count".to_string(),
            self.admitted_component_count,
        );
        coverage.insert(
            "brep_selected_body_count".to_string(),
            self.selected_body_count.unwrap_or_default(),
        );
        coverage.insert(
            "brep_selected_body_count_unresolved".to_string(),
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

fn admitted_face_components(
    scan: &ContainerScan,
    eligible_face_ids: &BTreeSet<u32>,
) -> Vec<crate::topology::FaceComponent> {
    if scan.framing.layout != crate::container::Layout::LegacyAscii {
        return scan.topology.face_components.clone();
    }
    scan.topology
        .face_components
        .iter()
        .filter(|component| {
            component
                .face_ids
                .iter()
                .any(|face_id| eligible_face_ids.contains(face_id))
        })
        .cloned()
        .collect()
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
    scan.framing.layout != crate::container::Layout::LegacyAscii
        || scan.surfaces.rows.iter().any(|row| row.id == face_id)
}

fn merge_body_components(
    components: Vec<(Vec<u32>, BTreeSet<u32>)>,
) -> Vec<(Vec<u32>, BTreeSet<u32>)> {
    let mut faces = Vec::new();
    let mut curves = BTreeSet::new();
    for (component_faces, component_curves) in components {
        faces.extend(component_faces);
        curves.extend(component_curves);
    }
    vec![(faces, curves)]
}

fn legacy_body_ownership_is_unambiguous(scan: &ContainerScan, component_count: usize) -> bool {
    scan.framing.layout != crate::container::Layout::LegacyAscii
        || scan.framing.declared_body_count.is_some()
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
    faces: &[u32],
    wire_curves: &BTreeSet<u32>,
    face_adjacency: &BTreeMap<u32, BTreeSet<u32>>,
    face_vertices: &BTreeMap<u32, BTreeSet<u32>>,
    edge_vertices: &BTreeMap<u32, [u32; 2]>,
) -> Vec<NeutralShellSpec> {
    let mut remaining_faces = faces.iter().copied().collect::<BTreeSet<_>>();
    let mut face_groups = Vec::<Vec<u32>>::new();
    while let Some(start) = remaining_faces.pop_first() {
        let mut group = BTreeSet::from([start]);
        let mut pending = vec![start];
        while let Some(face_id) = pending.pop() {
            for neighbour in face_adjacency.get(&face_id).into_iter().flatten().copied() {
                if remaining_faces.remove(&neighbour) {
                    group.insert(neighbour);
                    pending.push(neighbour);
                }
            }
        }
        face_groups.push(group.into_iter().collect());
    }

    let mut shell_specs = face_groups
        .into_iter()
        .map(|faces| NeutralShellSpec {
            faces,
            wire_curves: BTreeSet::new(),
        })
        .collect::<Vec<_>>();
    let mut unattached_wire_curves = BTreeSet::new();
    for curve_id in wire_curves {
        let curve_vertices = edge_vertices[curve_id].into_iter().collect::<BTreeSet<_>>();
        let matching_shell = exactly_one(
            shell_specs
                .iter()
                .enumerate()
                .filter(|(_, shell)| {
                    shell
                        .faces
                        .iter()
                        .any(|face_id| !face_vertices[face_id].is_disjoint(&curve_vertices))
                })
                .map(|(index, _)| index),
        );
        if let Some(index) = matching_shell {
            shell_specs[index].wire_curves.insert(*curve_id);
        } else {
            unattached_wire_curves.insert(*curve_id);
        }
    }
    if !unattached_wire_curves.is_empty() {
        shell_specs.push(NeutralShellSpec {
            faces: Vec::new(),
            wire_curves: unattached_wire_curves,
        });
    }
    shell_specs
}

fn component_is_closed(
    component_face_curves: &BTreeSet<u32>,
    emitted_half_edges: &BTreeSet<HalfEdgeId>,
    half_edges: &BTreeMap<HalfEdgeId, &crate::topology::HalfEdge>,
    faces: &[u32],
) -> bool {
    component_face_curves.iter().all(|curve_id| {
        let face_uses = emitted_half_edges
            .iter()
            .filter(|half_edge| half_edge.curve_id == *curve_id)
            .filter_map(|half_edge| half_edges.get(half_edge))
            .map(|half_edge| half_edge.face_id)
            .collect::<Vec<_>>();
        face_uses.len() == 2
            && face_uses
                .iter()
                .all(|face_id| *face_id != 0 && faces.contains(face_id))
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

fn curve_geometry_is_typed_nonlinear(geometry: &CurveGeometry) -> bool {
    match geometry {
        CurveGeometry::Circle { .. }
        | CurveGeometry::Ellipse { .. }
        | CurveGeometry::Parabola { .. }
        | CurveGeometry::Hyperbola { .. } => true,
        CurveGeometry::Transformed { basis, .. } => curve_geometry_is_typed_nonlinear(basis),
        _ => false,
    }
}

fn model_typed_nonlinear_curve_ids(ir: &CadIr) -> BTreeSet<u32> {
    ir.model
        .curves
        .iter()
        .filter_map(|curve| {
            let id = curve
                .id
                .0
                .strip_prefix("creo:visibgeom:curve#")?
                .parse()
                .ok()?;
            curve_geometry_is_typed_nonlinear(&curve.geometry).then_some(id)
        })
        .collect()
}

fn scalar_values_agree(first: f64, second: f64) -> bool {
    if !first.is_finite() || !second.is_finite() {
        return false;
    }
    let scale = first.abs().max(second.abs()).max(1.0);
    (first - second).abs() <= EPS_GEOMETRY_AGREE * scale
}

fn points_are_geometrically_coincident(first: Point3, second: Point3) -> bool {
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
    center: Point3,
    axis: Vector3,
    radius: f64,
}

#[derive(Clone, Copy)]
struct NativeCurveEvidence<'a> {
    typed_nonlinear_curve_ids: &'a BTreeSet<u32>,
    model_curves: &'a [Curve],
}

fn native_circle_loop_geometry(
    lp: &crate::topology::Loop,
    model_curves: &[Curve],
) -> Option<NativeCircleLoop> {
    let [first, second] = lp.half_edges.as_slice() else {
        return None;
    };
    if first.curve_id == second.curve_id {
        return None;
    }
    let first_id = CurveId(format!("creo:visibgeom:curve#{}", first.curve_id));
    let second_id = CurveId(format!("creo:visibgeom:curve#{}", second.curve_id));
    let first = exactly_one(model_curves.iter().filter(|curve| curve.id == first_id))?;
    let second = exactly_one(model_curves.iter().filter(|curve| curve.id == second_id))?;
    let (
        CurveGeometry::Circle {
            center: first_center,
            axis: first_axis,
            radius: first_radius,
            ..
        },
        CurveGeometry::Circle {
            center: second_center,
            axis: second_axis,
            radius: second_radius,
            ..
        },
    ) = (&first.geometry, &second.geometry)
    else {
        return None;
    };
    if !first_radius.is_finite()
        || *first_radius <= 0.0
        || !scalar_values_agree(*first_radius, *second_radius)
        || !points_are_geometrically_coincident(*first_center, *second_center)
        || !vectors_are_parallel(*first_axis, *second_axis)
    {
        return None;
    }
    Some(NativeCircleLoop {
        center: *first_center,
        axis: *first_axis,
        radius: *first_radius,
    })
}

fn ordered_two_edge_circle_loops<'a>(
    loops: &[&'a crate::topology::Loop],
    polygons: &[Vec<[f64; 2]>],
    surface: &SurfaceGeometry,
    model_curves: &[Curve],
) -> Option<Vec<&'a crate::topology::Loop>> {
    if loops.len() < 2 || loops.len() != polygons.len() {
        return None;
    }
    let SurfaceGeometry::Plane { origin, normal, .. } = surface else {
        return None;
    };
    let circle_loops = loops
        .iter()
        .map(|lp| native_circle_loop_geometry(lp, model_curves))
        .collect::<Option<Vec<_>>>()?;
    let normal_length = normal.norm();
    if !normal_length.is_finite() || normal_length <= 0.0 {
        return None;
    }
    let reference = circle_loops[0];
    if circle_loops.iter().any(|circle| {
        let center_scale = reference
            .radius
            .max(circle.radius)
            .max(1.0)
            .max(circle.center.x.abs())
            .max(circle.center.y.abs())
            .max(circle.center.z.abs());
        let distance_from_surface = circle.center.vector_from(*origin).dot(*normal).abs();
        !points_are_geometrically_coincident(circle.center, reference.center)
            || !vectors_are_parallel(circle.axis, reference.axis)
            || !vectors_are_parallel(circle.axis, *normal)
            || distance_from_surface > EPS_GEOMETRY_AGREE * normal_length * center_scale
    }) {
        return None;
    }
    let center_uv = cadmpeg_ir::eval::analytic_surface_parameters(surface, reference.center)?;
    for (circle, polygon) in circle_loops.iter().zip(polygons) {
        let [first, second] = polygon.as_slice() else {
            return None;
        };
        if [first[0], first[1], second[0], second[1]]
            .into_iter()
            .any(|value| !value.is_finite())
        {
            return None;
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
            return None;
        }
    }
    if circle_loops.iter().enumerate().any(|(index, first)| {
        circle_loops
            .iter()
            .skip(index + 1)
            .any(|second| scalar_values_agree(first.radius, second.radius))
    }) {
        return None;
    }
    let mut order = (0..loops.len()).collect::<Vec<_>>();
    order.sort_by(|first, second| {
        circle_loops[*second]
            .radius
            .total_cmp(&circle_loops[*first].radius)
    });
    Some(order.into_iter().map(|index| loops[index]).collect())
}

fn native_parameter_loop_polygon(
    lp: &crate::topology::Loop,
    face_id: u32,
    surface: &SurfaceGeometry,
    incidence: &BTreeMap<HalfEdgeId, &crate::topology::HalfEdgeVertexIncidence>,
    solved_vertices: &BTreeMap<u32, [f64; 3]>,
    native_pcurves: &NativePcurveCandidates,
    typed_nonlinear_curve_ids: &BTreeSet<u32>,
) -> Option<Vec<[f64; 2]>> {
    let segments = lp
        .half_edges
        .iter()
        .map(|half_edge| {
            let binding = incidence.get(half_edge)?;
            let end_vertex_id = binding.end_vertex_id?;
            let candidates = native_pcurves.get(&(half_edge.curve_id, face_id))?;
            let traversal = [
                solved_vertices.get(&binding.start_vertex_id).copied()?,
                solved_vertices.get(&end_vertex_id).copied()?,
            ];
            unique_oriented_native_pcurve(surface, candidates, traversal)
                .map(|(endpoints, _)| endpoints)
        })
        .collect::<Option<Vec<_>>>()?;
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
        return None;
    }
    Some(segments.into_iter().map(|segment| segment[0]).collect())
}

fn ordered_native_parameter_face_loops<'a>(
    loops: &[&'a crate::topology::Loop],
    face_id: u32,
    surface: &SurfaceGeometry,
    incidence: &BTreeMap<HalfEdgeId, &crate::topology::HalfEdgeVertexIncidence>,
    solved_vertices: &BTreeMap<u32, [f64; 3]>,
    native_pcurves: &NativePcurveCandidates,
    curve_evidence: NativeCurveEvidence<'_>,
) -> Option<Vec<&'a crate::topology::Loop>> {
    let polygons = loops
        .iter()
        .map(|lp| {
            native_parameter_loop_polygon(
                lp,
                face_id,
                surface,
                incidence,
                solved_vertices,
                native_pcurves,
                curve_evidence.typed_nonlinear_curve_ids,
            )
        })
        .collect::<Option<Vec<_>>>()?;
    ordered_parameter_face_loops(loops.to_owned(), &polygons).or_else(|| {
        ordered_two_edge_circle_loops(loops, &polygons, surface, curve_evidence.model_curves)
    })
}

#[cfg(test)]
mod tests;

pub(in super::super) fn transfer_native_brep(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    derived_intersection_curves: &BTreeSet<CurveId>,
    analytic_pcurve_carriers: &BTreeSet<CurveId>,
    nurbs_endpoint_witnesses: &BTreeSet<CurveId>,
) -> NativeBrepTransferSummary {
    let carriers = placed_carriers(scan, ir);
    let planes = carriers
        .iter()
        .filter_map(|(id, carrier)| match carrier {
            CarrierEquation::Plane(plane) => Some((*id, *plane)),
            _ => None,
        })
        .collect::<BTreeMap<_, _>>();
    let face_orientations = native_face_orientations(scan, ir);
    let half_edges = scan
        .topology
        .half_edges
        .iter()
        .map(|half_edge| (half_edge.id, half_edge))
        .collect::<BTreeMap<_, _>>();
    let incidence = scan
        .topology
        .half_edge_vertex_incidence
        .iter()
        .map(|binding| (binding.half_edge, binding))
        .collect::<BTreeMap<_, _>>();
    let solved_vertex_result =
        solve_topological_vertices(scan, ir, &carriers, nurbs_endpoint_witnesses);
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
        native_pcurves
            .entry((curve_id, faces[0]))
            .or_default()
            .push((face_0_endpoints, offset));
        native_pcurves
            .entry((curve_id, faces[1]))
            .or_default()
            .push((face_1_endpoints, offset));
    }
    for pcurve in &scan.curves.two_chart_pcurves {
        let Some(endpoint_sets) =
            super::super::analytic::mapped_two_chart_endpoint_sets(scan, ir, pcurve)
        else {
            continue;
        };
        for (face_id, endpoints) in pcurve.faces.into_iter().zip(endpoint_sets.paths) {
            if let Some(endpoints) = endpoints {
                native_pcurves
                    .entry((pcurve.curve_id, face_id))
                    .or_default()
                    .push((endpoints, pcurve.offset));
            }
        }
    }
    for pcurve in crate::curve::fc02_short_pcurve_endpoints(
        &scan.curves.parameters,
        &scan.curves.topology_rows,
    ) {
        let [face_0_endpoints, _] = canonicalized_pcurve_endpoints(
            scan,
            pcurve.faces,
            pcurve.face_0_endpoints,
            pcurve.face_0_endpoints,
        );
        native_pcurves
            .entry((pcurve.curve_id, pcurve.faces[0]))
            .or_default()
            .push((face_0_endpoints, pcurve.offset));
    }
    let native_edge_vertices =
        crate::topology::edge_vertex_pairs(&scan.topology.half_edge_vertex_incidence);
    let edge_vertices = crate::topology::uniquely_identified_rows(&scan.curves.topology_rows)
        .into_iter()
        .filter_map(|row| {
            let vertices = native_edge_vertices.get(&row.id).copied()?;
            vertices
                .iter()
                .all(|vertex| solved_vertices.contains_key(vertex))
                .then_some((row.id, vertices))
        })
        .collect::<BTreeMap<_, _>>();
    let model_curve_counts = edge_vertices
        .keys()
        .map(|curve_id| {
            let id = CurveId(format!("creo:visibgeom:curve#{curve_id}"));
            let count = ir
                .model
                .curves
                .iter()
                .filter(|curve| curve.id == id)
                .count();
            (*curve_id, count)
        })
        .collect::<BTreeMap<_, _>>();
    let admitted_edge_curves = edge_vertices
        .keys()
        .copied()
        .filter(|curve_id| model_curve_counts[curve_id] <= 1)
        .collect::<BTreeSet<_>>();
    let mut loops_by_face = BTreeMap::<u32, Vec<&crate::topology::Loop>>::new();
    for lp in &scan.topology.loops {
        if lp.face_id != 0 {
            loops_by_face.entry(lp.face_id).or_default().push(lp);
        }
    }
    let topology_face_reference_ids = scan
        .topology
        .face_components
        .iter()
        .flat_map(|component| component.face_ids.iter().copied())
        .chain(loops_by_face.keys().copied())
        .collect::<BTreeSet<_>>();
    let legacy_nonvisible_face_reference_count = topology_face_reference_ids
        .iter()
        .filter(|face_id| !is_neutral_face_reference(scan, **face_id))
        .count();
    loops_by_face.retain(|face_id, _| is_neutral_face_reference(scan, *face_id));
    let candidate_face_ids = scan
        .topology
        .face_components
        .iter()
        .flat_map(|component| component.face_ids.iter().copied())
        .chain(loops_by_face.keys().copied())
        .filter(|face_id| is_neutral_face_reference(scan, *face_id))
        .collect::<BTreeSet<_>>();
    let model_surface_counts = candidate_face_ids
        .iter()
        .map(|face_id| {
            let id = native_surface_id(scan, *face_id);
            let count = ir
                .model
                .surfaces
                .iter()
                .filter(|surface| surface.id == id)
                .count();
            (*face_id, count)
        })
        .collect::<BTreeMap<_, _>>();
    let typed_nonlinear_curve_ids = model_typed_nonlinear_curve_ids(ir);
    let mut diagnostics = BrepTransferDiagnostics {
        candidate_face_count: candidate_face_ids.len(),
        legacy_nonvisible_face_reference_count,
        vertex_solve: solved_vertex_result.diagnostics,
        ..BrepTransferDiagnostics::default()
    };
    let boundary_curve_ids = loops_by_face
        .values()
        .flatten()
        .flat_map(|lp| lp.half_edges.iter().map(|half_edge| half_edge.curve_id))
        .collect::<BTreeSet<_>>();
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
            diagnostics.reject_face(FaceAdmissionRejection::MissingSurfaceCarrier, face_id);
            continue;
        }
        if !face_orientations.contains_key(&face_id) {
            diagnostics.reject_face(FaceAdmissionRejection::MissingOrientation, face_id);
            continue;
        }
        if model_surface_counts[&face_id] > 1 {
            diagnostics.reject_face(FaceAdmissionRejection::AmbiguousSurfaceCarrier, face_id);
            continue;
        }
        let Some(loops) = loops_by_face.get(&face_id) else {
            diagnostics.reject_face(FaceAdmissionRejection::MissingLoops, face_id);
            continue;
        };
        let has_unresolved_boundary_vertices = loops.iter().any(|lp| {
            lp.half_edges
                .iter()
                .any(|half_edge| !edge_vertices.contains_key(&half_edge.curve_id))
        });
        if has_unresolved_boundary_vertices {
            diagnostics.reject_face_with_detail(
                FaceAdmissionRejection::UnresolvedBoundaryVertices,
                FaceAdmissionDetail::unresolved_boundary(
                    face_id,
                    loops,
                    &edge_vertices,
                    &incidence,
                ),
            );
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
            diagnostics.reject_face(FaceAdmissionRejection::AmbiguousBoundaryCurve, face_id);
            continue;
        }
        let two_edge_loops_are_proven =
            loops
                .iter()
                .filter(|lp| lp.half_edges.len() == 2)
                .all(|lp| {
                    let surface_id = native_surface_id(scan, face_id);
                    let Some(surface) = exactly_one(
                        ir.model
                            .surfaces
                            .iter()
                            .filter(|candidate| candidate.id == surface_id),
                    ) else {
                        return false;
                    };
                    native_parameter_loop_polygon(
                        lp,
                        face_id,
                        &surface.geometry,
                        &incidence,
                        solved_vertices,
                        &native_pcurves,
                        &typed_nonlinear_curve_ids,
                    )
                    .is_some()
                });
        if !two_edge_loops_are_proven {
            diagnostics.reject_face(FaceAdmissionRejection::TwoEdgeParameterProof, face_id);
            continue;
        }
        let ordered = ordered_face_loops(
            loops.clone(),
            planes.get(&face_id).copied(),
            &incidence,
            solved_vertices,
        )
        .or_else(|| {
            let surface_id = native_surface_id(scan, face_id);
            let surface = exactly_one(
                ir.model
                    .surfaces
                    .iter()
                    .filter(|candidate| candidate.id == surface_id),
            )?;
            ordered_native_parameter_face_loops(
                loops,
                face_id,
                &surface.geometry,
                &incidence,
                solved_vertices,
                &native_pcurves,
                NativeCurveEvidence {
                    typed_nonlinear_curve_ids: &typed_nonlinear_curve_ids,
                    model_curves: &ir.model.curves,
                },
            )
        });
        let Some(ordered) = ordered else {
            diagnostics.reject_face(FaceAdmissionRejection::LoopOrdering, face_id);
            continue;
        };
        eligible_faces.insert(face_id, ordered);
    }
    diagnostics.admitted_face_count = eligible_faces.len();
    let eligible_loops = eligible_faces
        .values()
        .flatten()
        .copied()
        .collect::<Vec<_>>();

    let emitted_half_edges = eligible_loops
        .iter()
        .flat_map(|lp| lp.half_edges.iter().copied())
        .collect::<BTreeSet<_>>();
    let face_curves = emitted_half_edges
        .iter()
        .map(|half_edge| half_edge.curve_id)
        .collect::<BTreeSet<_>>();
    let closed_single_edge_curves = face_curves
        .iter()
        .filter(|curve_id| {
            let uses = eligible_loops
                .iter()
                .filter(|lp| {
                    lp.half_edges
                        .iter()
                        .any(|half_edge| half_edge.curve_id == **curve_id)
                })
                .collect::<Vec<_>>();
            !uses.is_empty() && uses.iter().all(|lp| lp.half_edges.len() == 1)
        })
        .copied()
        .collect::<BTreeSet<_>>();
    let row_offsets = scan
        .curves
        .topology_rows
        .iter()
        .map(|row| (row.id, row.offset))
        .collect::<BTreeMap<_, _>>();
    let curve_faces = crate::topology::uniquely_identified_rows(&scan.curves.topology_rows)
        .into_iter()
        .map(|row| (row.id, row.faces))
        .collect::<BTreeMap<_, _>>();

    let eligible_face_ids = eligible_faces.keys().copied().collect::<BTreeSet<_>>();
    let admitted_components = admitted_face_components(scan, &eligible_face_ids);
    let neutral_edge_curves = admitted_components
        .iter()
        .flat_map(|component| component.curve_ids.iter().copied())
        .filter(|curve_id| admitted_edge_curves.contains(curve_id))
        .filter(|curve_id| {
            scan.framing.layout != crate::container::Layout::LegacyAscii
                || curve_faces
                    .get(curve_id)
                    .is_some_and(|faces| faces.iter().any(|face| eligible_face_ids.contains(face)))
        })
        .collect::<BTreeSet<_>>();
    let body_components = admitted_components
        .iter()
        .map(|component| {
            let faces = component
                .face_ids
                .iter()
                .copied()
                .filter(|face_id| eligible_faces.contains_key(face_id))
                .collect::<Vec<_>>();
            let curves = component
                .curve_ids
                .iter()
                .copied()
                .filter(|curve_id| neutral_edge_curves.contains(curve_id))
                .collect::<BTreeSet<_>>();
            (faces, curves)
        })
        .collect::<Vec<_>>();
    let selected_body_count = crate::topology::selected_body_count(
        scan.framing.declared_body_count,
        scan.framing.first_quilt_ptr,
        admitted_components.len(),
    );
    let empty_component_count = body_components
        .iter()
        .filter(|(faces, curves)| faces.is_empty() && curves.is_empty())
        .count();
    let explicit_single_body =
        scan.framing.declared_body_count == Some(1) || scan.framing.first_quilt_ptr == Some(0);
    let body_components =
        if explicit_single_body && empty_component_count == 0 && !body_components.is_empty() {
            merge_body_components(body_components)
        } else {
            body_components
        };
    let solved_point_count = solved_vertices.len();
    for (vertex_id, position) in solved_vertices {
        let point_id = PointId(format!("creo:visibgeom:point#{vertex_id}"));
        if ir.model.points.iter().any(|item| item.id == point_id) {
            continue;
        }
        annotate(
            annotations,
            &point_id,
            "VisibGeom",
            0,
            "topological_vertex_point",
            Exactness::Derived,
        );
        ir.model.points.push(Point {
            id: point_id,
            position: Point3::new(position[0], position[1], position[2]),
            source_object: Some(SourceObjectAssociation {
                format: "creo".to_string(),
                object_id: format!("topology:vertex#{vertex_id}"),
                name: None,
                color: None,
                visible: None,
                layer: None,
                instance_path: Vec::new(),
            }),
        });
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
        return NativeBrepTransferSummary {
            topological_point_count: solved_point_count,
            diagnostics,
            ..NativeBrepTransferSummary::default()
        };
    }
    diagnostics.emitted_face_count = body_components.iter().map(|(faces, _)| faces.len()).sum();

    let used_vertices = neutral_edge_curves
        .iter()
        .filter_map(|curve| edge_vertices.get(curve))
        .flatten()
        .copied()
        .collect::<BTreeSet<_>>();
    for vertex_id in used_vertices {
        let vertex = VertexId(format!("creo:visibgeom:vertex#{vertex_id}"));
        if ir.model.vertices.iter().any(|item| item.id == vertex) {
            continue;
        }
        let point_id = PointId(format!("creo:visibgeom:point#{vertex_id}"));
        annotate(
            annotations,
            &vertex,
            "VisibGeom",
            0,
            "topological_vertex_orbit",
            Exactness::Derived,
        );
        ir.model.vertices.push(Vertex {
            id: vertex,
            point: point_id,
            tolerance: None,
        });
    }
    for curve_id in &neutral_edge_curves {
        let [start, end] = edge_vertices[curve_id];
        let curve = CurveId(format!("creo:visibgeom:curve#{curve_id}"));
        let points = [solved_vertices[&start], solved_vertices[&end]];
        let unbacked_closed_edge = start == end
            && closed_single_edge_curves.contains(curve_id)
            && curve_faces.get(curve_id).is_some_and(|face_ids| {
                !face_ids
                    .iter()
                    .any(|face_id| native_pcurves.contains_key(&(*curve_id, *face_id)))
            });
        let model_curve_count = model_curve_counts[curve_id];
        let derived_line = (derived_intersection_curves.contains(&curve)
            || analytic_pcurve_carriers.contains(&curve))
            && exactly_one(
                ir.model
                    .curves
                    .iter()
                    .filter(|candidate| candidate.id == curve),
            )
            .is_some_and(|candidate| matches!(&candidate.geometry, CurveGeometry::Line { .. }));
        let param_range = if model_curve_count == 0 {
            None
        } else if derived_line {
            exactly_one(
                ir.model
                    .curves
                    .iter_mut()
                    .filter(|candidate| candidate.id == curve),
            )
            .and_then(|candidate| orient_line_edge_carrier(&mut candidate.geometry, points))
        } else {
            exactly_one(
                ir.model
                    .curves
                    .iter_mut()
                    .filter(|candidate| candidate.id == curve),
            )
            .and_then(|candidate| {
                orient_nonperiodic_nurbs_edge_carrier(&mut candidate.geometry, points).or_else(
                    || {
                        exact_line_edge_parameter_range(&candidate.geometry, points).or_else(|| {
                            nonperiodic_conic_edge_parameter_range(&candidate.geometry, points)
                                .or_else(|| {
                                    pcurve_backed_periodic_conic_parameter_range(
                                        &candidate.geometry,
                                        *curve_id,
                                        *curve_faces.get(curve_id)?,
                                        &native_pcurves,
                                        &ir.model.surfaces,
                                        points,
                                    )
                                })
                                .or_else(|| {
                                    unbacked_closed_edge.then_some(()).and_then(|()| {
                                        full_periodic_conic_edge_parameter_range(
                                            &candidate.geometry,
                                            points[0],
                                        )
                                    })
                                })
                                .or_else(|| {
                                    unbacked_closed_edge.then_some(()).and_then(|()| {
                                        full_periodic_nurbs_edge_parameter_range(
                                            &candidate.geometry,
                                            points[0],
                                        )
                                    })
                                })
                        })
                    },
                )
            })
        };
        let id = EdgeId(format!("creo:visibgeom:edge#{curve_id}"));
        annotate(
            annotations,
            &id,
            "VisibGeom",
            row_offsets.get(curve_id).copied().unwrap_or(0) as u64,
            "curve_topology_edge",
            Exactness::Derived,
        );
        ir.model.edges.push(Edge {
            id,
            curve: Some(curve.clone()),
            start: VertexId(format!("creo:visibgeom:vertex#{start}")),
            end: VertexId(format!("creo:visibgeom:vertex#{end}")),
            param_range,
            tolerance: None,
        });
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
            ir.model.curves.push(Curve {
                id: curve,
                geometry: CurveGeometry::Unknown {
                    record: geometry_section_record(scan, offset),
                },
                source_object: Some(SourceObjectAssociation {
                    format: "creo".to_string(),
                    object_id: format!("VisibGeom:{curve_id}"),
                    name: None,
                    color: None,
                    visible: None,
                    layer: None,
                    instance_path: Vec::new(),
                }),
            });
        }
    }

    for (component_index, (faces, component_curves)) in body_components.iter().enumerate() {
        let body_id = BodyId(format!("creo:visibgeom:body#{}", component_index + 1));
        let region_id = RegionId(format!("creo:visibgeom:region#{}", component_index + 1));
        for (id, tag) in [
            (body_id.to_string(), "native_component_body"),
            (region_id.to_string(), "native_component_region"),
        ] {
            annotate(annotations, id, "VisibGeom", 0, tag, Exactness::Derived);
        }
        let component_face_curves = component_curves
            .intersection(&face_curves)
            .copied()
            .collect::<BTreeSet<_>>();
        let wire_curves = component_curves
            .difference(&face_curves)
            .copied()
            .collect::<BTreeSet<_>>();
        let closed = component_is_closed(
            &component_face_curves,
            &emitted_half_edges,
            &half_edges,
            faces,
        );

        let mut face_adjacency = faces
            .iter()
            .copied()
            .map(|face_id| (face_id, BTreeSet::new()))
            .collect::<BTreeMap<_, _>>();
        let mut face_vertices = BTreeMap::<u32, BTreeSet<u32>>::new();
        let mut faces_by_curve = BTreeMap::<u32, BTreeSet<u32>>::new();
        let mut faces_by_vertex = BTreeMap::<u32, BTreeSet<u32>>::new();
        for face_id in faces {
            let vertices = face_vertices.entry(*face_id).or_default();
            for native_loop in &eligible_faces[face_id] {
                for half_edge in &native_loop.half_edges {
                    faces_by_curve
                        .entry(half_edge.curve_id)
                        .or_default()
                        .insert(*face_id);
                    let [start, end] = edge_vertices[&half_edge.curve_id];
                    vertices.extend([start, end]);
                    faces_by_vertex.entry(start).or_default().insert(*face_id);
                    faces_by_vertex.entry(end).or_default().insert(*face_id);
                }
            }
        }
        for incident_faces in faces_by_curve.values().chain(faces_by_vertex.values()) {
            let incident_faces = incident_faces.iter().copied().collect::<Vec<_>>();
            for (index, first) in incident_faces.iter().enumerate() {
                face_adjacency
                    .entry(*first)
                    .or_default()
                    .extend(incident_faces.iter().skip(index + 1).copied());
                for second in incident_faces.iter().skip(index + 1) {
                    face_adjacency.entry(*second).or_default().insert(*first);
                }
            }
        }
        let shell_specs = split_neutral_component_shells(
            faces,
            &wire_curves,
            &face_adjacency,
            &face_vertices,
            &edge_vertices,
        );

        let mut face_shell_ids = BTreeMap::<u32, ShellId>::new();
        let shell_ids = shell_specs
            .iter()
            .enumerate()
            .map(|(shell_index, shell)| {
                let shell_id = if shell_index == 0 {
                    ShellId(format!("creo:visibgeom:shell#{}", component_index + 1))
                } else {
                    ShellId(format!(
                        "creo:visibgeom:shell#{}:{}",
                        component_index + 1,
                        shell_index + 1
                    ))
                };
                annotate(
                    annotations,
                    shell_id.to_string(),
                    "VisibGeom",
                    0,
                    "native_component_shell",
                    Exactness::Derived,
                );
                for face_id in &shell.faces {
                    face_shell_ids.insert(*face_id, shell_id.clone());
                }
                ir.model.shells.push(Shell {
                    id: shell_id.clone(),
                    region: region_id.clone(),
                    faces: shell
                        .faces
                        .iter()
                        .map(|face| FaceId(format!("creo:visibgeom:face#{face}")))
                        .collect(),
                    wire_edges: shell
                        .wire_curves
                        .iter()
                        .map(|curve_id| EdgeId(format!("creo:visibgeom:edge#{curve_id}")))
                        .collect(),
                    free_vertices: Vec::new(),
                });
                shell_id
            })
            .collect::<Vec<_>>();
        ir.model.bodies.push(Body {
            id: body_id.clone(),
            kind: if !wire_curves.is_empty() {
                BodyKind::General
            } else if closed {
                BodyKind::Solid
            } else {
                BodyKind::Sheet
            },
            regions: vec![region_id.clone()],
            transform: None,
            name: None,
            color: None,
            visible: None,
        });
        ir.model.regions.push(Region {
            id: region_id.clone(),
            body: body_id,
            shells: shell_ids,
        });
        for face_id in faces {
            let native_loops = &eligible_faces[face_id];
            let face = FaceId(format!("creo:visibgeom:face#{face_id}"));
            let shell_id = face_shell_ids[face_id].clone();
            let loop_ids = (0..native_loops.len())
                .map(|index| {
                    if index == 0 {
                        LoopId(format!("creo:visibgeom:loop#{face_id}"))
                    } else {
                        LoopId(format!("creo:visibgeom:loop#{face_id}:{index}"))
                    }
                })
                .collect::<Vec<_>>();
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
            let surface = native_surface_id(scan, *face_id);
            if !ir.model.surfaces.iter().any(|item| item.id == surface) {
                annotate(
                    annotations,
                    &surface,
                    face_source_namespace,
                    face_offset as u64,
                    "opaque_native_surface_carrier",
                    Exactness::Unknown,
                );
                ir.model.surfaces.push(Surface {
                    id: surface.clone(),
                    geometry: SurfaceGeometry::Unknown {
                        record: geometry_section_record(scan, face_offset),
                    },
                    source_object: Some(SourceObjectAssociation {
                        format: "creo".to_string(),
                        object_id: format!("VisibGeom:{face_id}"),
                        name: None,
                        color: None,
                        visible: None,
                        layer: None,
                        instance_path: Vec::new(),
                    }),
                });
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
            ir.model.faces.push(Face {
                id: face.clone(),
                shell: shell_id.clone(),
                surface,
                sense: face_sense,
                loops: loop_ids.clone(),
                name: None,
                color: None,
                tolerance: None,
            });
            for (boundary_index, (native_loop, loop_id)) in
                native_loops.iter().zip(loop_ids).enumerate()
            {
                let coedge_ids = native_loop
                    .half_edges
                    .iter()
                    .map(|half_edge| {
                        CoedgeId(format!(
                            "creo:visibgeom:coedge#{}:{}",
                            half_edge.curve_id, half_edge.side
                        ))
                    })
                    .collect::<Vec<_>>();
                ir.model.loops.push(IrLoop {
                    id: loop_id.clone(),
                    face: face.clone(),
                    boundary_role: if boundary_index == 0 {
                        cadmpeg_ir::topology::LoopBoundaryRole::Outer
                    } else {
                        cadmpeg_ir::topology::LoopBoundaryRole::Inner
                    },
                    coedges: coedge_ids.clone(),
                    vertex_uses: Vec::new(),
                });
                for (index, half_edge) in native_loop.half_edges.iter().enumerate() {
                    let id = coedge_ids[index].clone();
                    let twin = HalfEdgeId {
                        curve_id: half_edge.curve_id,
                        side: 1 - half_edge.side,
                    };
                    let radial_next = if emitted_half_edges.contains(&twin) {
                        CoedgeId(format!(
                            "creo:visibgeom:coedge#{}:{}",
                            twin.curve_id, twin.side
                        ))
                    } else {
                        id.clone()
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
                    let pcurve_geometry = native_candidates
                        .and_then(|candidates| {
                            let incidence = incidence.get(half_edge)?;
                            let end = incidence.end_vertex_id?;
                            let traversal = [
                                solved_vertices[&incidence.start_vertex_id],
                                solved_vertices[&end],
                            ];
                            let surface_id = native_surface_id(scan, *face_id);
                            let surface = exactly_one(
                                ir.model
                                    .surfaces
                                    .iter()
                                    .filter(|candidate| candidate.id == surface_id),
                            )?;
                            unique_oriented_native_pcurve(&surface.geometry, candidates, traversal)
                        })
                        .map(|(endpoints, offset)| {
                            (
                                line_pcurve(endpoints[0], endpoints[1]),
                                Some([0.0, 1.0]),
                                offset,
                                "native_endpoint_pcurve",
                            )
                        })
                        .or_else(|| {
                            native_candidates.is_none().then_some(())?;
                            let surface_id = native_surface_id(scan, *face_id);
                            let surface = exactly_one(
                                ir.model
                                    .surfaces
                                    .iter()
                                    .filter(|candidate| candidate.id == surface_id),
                            )?;
                            let curve_id =
                                CurveId(format!("creo:visibgeom:curve#{}", half_edge.curve_id));
                            let curve = exactly_one(
                                ir.model
                                    .curves
                                    .iter()
                                    .filter(|candidate| candidate.id == curve_id),
                            )?;
                            let edge_id =
                                EdgeId(format!("creo:visibgeom:edge#{}", half_edge.curve_id));
                            let edge = exactly_one(
                                ir.model
                                    .edges
                                    .iter()
                                    .filter(|candidate| candidate.id == edge_id),
                            )?;
                            let (geometry, tag) =
                                planar_curve_pcurve(&surface.geometry, &curve.geometry)
                                    .map(|geometry| (geometry, "projected_planar_pcurve"))
                                    .or_else(|| {
                                        surface_of_revolution_parallel_pcurve(
                                            &surface.geometry,
                                            &curve.geometry,
                                        )
                                        .map(|geometry| {
                                            (geometry, "projected_parallel_conic_pcurve")
                                        })
                                    })
                                    .or_else(|| {
                                        meridian_circle_pcurve(&surface.geometry, &curve.geometry)
                                            .map(|geometry| (geometry, "projected_meridian_pcurve"))
                                    })
                                    .or_else(|| {
                                        ruled_generator_line_pcurve(
                                            &surface.geometry,
                                            &curve.geometry,
                                        )
                                        .map(|geometry| {
                                            (geometry, "projected_ruled_generator_pcurve")
                                        })
                                    })?;
                            Some((
                                geometry,
                                edge.param_range,
                                row_offsets.get(&half_edge.curve_id).copied().unwrap_or(0),
                                tag,
                            ))
                        });
                    let pcurves = pcurve_geometry
                        .map(|(geometry, parameter_range, offset, tag)| {
                            let pcurve = PcurveId(format!(
                                "creo:visibgeom:pcurve#{}:{face_id}",
                                half_edge.curve_id
                            ));
                            if !ir.model.pcurves.iter().any(|item| item.id == pcurve) {
                                annotate(
                                    annotations,
                                    &pcurve,
                                    "VisibGeom",
                                    offset as u64,
                                    tag,
                                    Exactness::Derived,
                                );
                                ir.model.pcurves.push(Pcurve {
                                    id: pcurve.clone(),
                                    geometry,
                                    wrapper_reversed: None,
                                    native_tail_flags: None,
                                    parameter_range,
                                    fit_tolerance: None,
                                });
                            }
                            PcurveUse {
                                pcurve,
                                isoparametric: None,
                                parameter_range: None,
                            }
                        })
                        .into_iter()
                        .collect();
                    ir.model.coedges.push(Coedge {
                        id,
                        owner_loop: loop_id.clone(),
                        edge: EdgeId(format!("creo:visibgeom:edge#{}", half_edge.curve_id)),
                        next: coedge_ids[(index + 1) % coedge_ids.len()].clone(),
                        previous: coedge_ids[(index + coedge_ids.len() - 1) % coedge_ids.len()]
                            .clone(),
                        radial_next,
                        sense: if half_edge.side == 0 {
                            Sense::Forward
                        } else {
                            Sense::Reversed
                        },
                        pcurves,
                        use_curve: None,
                        use_curve_parameter_range: None,
                    });
                }
            }
        }
    }
    NativeBrepTransferSummary {
        topological_point_count: solved_point_count,
        native_topological_edge_count: neutral_edge_curves.len(),
        diagnostics,
    }
}

pub(in super::super) fn transfer_cap_pair_cylinders(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) {
    for pair in &scan.curves.fc05_cylinder_cap_pairs {
        let Some(frame) = fc05_cap_pair_model_frame(scan, pair) else {
            continue;
        };
        let id = SurfaceId(format!("creo:visibgeom:surface#{}", pair.surface_id));
        if ir.model.surfaces.iter().any(|surface| surface.id == id) {
            continue;
        }
        annotate(
            annotations,
            &id,
            "VisibGeom",
            pair.offset as u64,
            "fc05_cap_pair_cylinder",
            Exactness::Derived,
        );
        ir.model.surfaces.push(Surface {
            id,
            geometry: SurfaceGeometry::Cylinder {
                origin: Point3::new(frame.origin[0], frame.origin[1], frame.origin[2]),
                axis: Vector3::new(frame.axis[0], frame.axis[1], frame.axis[2]),
                ref_direction: Vector3::new(
                    frame.ref_direction[0],
                    frame.ref_direction[1],
                    frame.ref_direction[2],
                ),
                radius: pair.radius_mm,
            },
            source_object: Some(SourceObjectAssociation {
                format: "creo".to_string(),
                object_id: format!("VisibGeom:{}", pair.surface_id),
                name: None,
                color: None,
                visible: None,
                layer: None,
                instance_path: Vec::new(),
            }),
        });
        for ((curve_id, ordinate), cap_plane_id) in pair
            .curve_ids
            .iter()
            .zip(&pair.curve_cap_ordinates_row_frame)
            .zip(&pair.cap_plane_ids)
        {
            let cap_offset =
                crate::surface::unique_outline_plane(&scan.planes.outlines, *cap_plane_id)
                    .map_or_else(
                        || frame.origin[frame.axis_index] + frame.axis_sign * ordinate,
                        |plane| plane.origin[frame.axis_index],
                    );
            let (center, _, _) = fc05_model_frame(
                frame.axis_index,
                cap_offset,
                pair.center_row_frame,
                pair.reference_direction_row_frame,
                frame.axis_sign,
            );
            let id = CurveId(format!("creo:visibgeom:curve#{curve_id}"));
            if ir.model.curves.iter().any(|curve| curve.id == id) {
                continue;
            }
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
            ir.model.curves.push(Curve {
                id,
                geometry: CurveGeometry::Circle {
                    center: Point3::new(center[0], center[1], center[2]),
                    axis: Vector3::new(frame.axis[0], frame.axis[1], frame.axis[2]),
                    ref_direction: Vector3::new(
                        frame.ref_direction[0],
                        frame.ref_direction[1],
                        frame.ref_direction[2],
                    ),
                    radius: pair.radius_mm,
                },
                source_object: Some(SourceObjectAssociation {
                    format: "creo".to_string(),
                    object_id: format!("VisibGeom:{curve_id}"),
                    name: None,
                    color: None,
                    visible: None,
                    layer: None,
                    instance_path: Vec::new(),
                }),
            });
        }
    }
}
