// SPDX-License-Identifier: Apache-2.0
//! Standard edge curves selected by consolidated allocation identities.

use std::collections::HashMap;

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::annotations::AnnotationStorage;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::intersection_curves::IntcurveSupportContext;
use cadmpeg_ir::geometry::legacy_cache::FitTolerance;
use cadmpeg_ir::geometry::nurbs::{NurbsCurve, SurfaceParameterAxis};
use cadmpeg_ir::geometry::pcurve::{LinePcurve, Pcurve, PcurveGeometry, PcurveMetadata};
use cadmpeg_ir::geometry::procedural_curves::ProceduralCurveDefinition;
use cadmpeg_ir::geometry::{
    CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, SurfaceGeometry,
};
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::topology::{ParameterInterval, PcurveUse};
use cadmpeg_ir::AnnotationBuilder;

use super::{ConsolidatedCarrierChart, FamilyEntityAdmission};
use crate::families::b2::records::{
    b2_cone_geometry, b2_cones_from_records, b2_cylinders_from_records, b2_edge_nodes_from_records,
    b2_parameter_points_from_records, b2_plane_carriers_from_records, b2_plane_geometry,
    b2_sphere_geometry, b2_spheres_from_records, b2_tori_from_records, b2_torus_geometry,
};
use crate::families::consolidated::a5_chart::{chart_side_payload, ChartAxis};
use crate::families::consolidated::allocation::AllocationJournal;
use crate::families::consolidated::owner_closure::OwnerClosureIndex;
use crate::families::consolidated::parameter_representation::{
    b18_support, class25_representations,
};
use crate::families::consolidated::records::class25::{self, Class25SourceRelation};
use crate::wire::bytes::{allocation_reference, AllocationReference, AllocationReferenceEncoding};
use crate::wire::compact_unsigned::CompactUnsigned;
use crate::wire::records::{ConsolidatedFamily, ConsolidatedRecord, SourceExtent};

const PLANE_OFFSET_TOLERANCE: f64 = 2.0e-3;
const PARAMETER_TOLERANCE: f64 = 1.0e-9;
const PLANE_NORMAL_TOLERANCE: f64 = 8.0 / 8_388_608.0;
const LIFTED_ENDPOINT_TOLERANCE_MM: f64 = 0.02;
const LIFTED_CURVE_FIT_TOLERANCE_MM: f64 = 0.0005;

struct Plan {
    edge: usize,
    curve: usize,
    procedure: usize,
    face_pcurves: Vec<(usize, PcurveGeometry, ParameterInterval, usize)>,
    curve_cache: NurbsCurve,
    curve_range: ParameterInterval,
    plane_pcurve: Option<(usize, PcurveGeometry, ParameterInterval, usize)>,
    plane_fit_tolerance: Option<FitTolerance>,
    plane_projection_gap: Option<f64>,
}

#[derive(Default)]
pub(in crate::families) struct AttachResult {
    pub(in crate::families) edges: usize,
    pub(in crate::families) pcurves: usize,
}

type LiftedCandidate<'a> = (&'a ConsolidatedRecord, Option<[f64; 2]>, Option<u64>);

fn collect_uncompressed_candidates<'a>(
    data: &[u8],
    sources: &[Vec<SourceExtent>],
    journal: &AllocationJournal<'a>,
    owners: &OwnerClosureIndex,
    node_index: usize,
    map_record: Option<(&ConsolidatedRecord, usize)>,
) -> Vec<LiftedCandidate<'a>> {
    let mut map_ranges: HashMap<usize, [f64; 2]> = HashMap::new();
    let mut map_direct: Vec<LiftedCandidate<'a>> = Vec::new();
    if let Some((record, map_index)) = map_record {
        for (rep, opt_range) in
            uncompressed_map_candidates(data, sources, journal, record, map_index)
        {
            if let (Some(rep_idx), Some(r)) = (
                journal.index(rep.source_index(), rep.source_range().start),
                opt_range,
            ) {
                map_ranges.insert(rep_idx, r);
            }
            map_direct.push((rep, opt_range, None));
        }
    }
    let mut candidates: Vec<LiftedCandidate<'a>> = owners
        .all_edge_occurrences(node_index)
        .iter()
        .filter_map(|occ| {
            let rep = journal.record(occ.representation_index)?;
            let opt_range = map_ranges.get(&occ.representation_index).copied();
            Some((rep, opt_range, Some(occ.face_tag)))
        })
        .collect();
    candidates.extend(map_direct);
    candidates
}

fn rounds_to_stored(exact: cadmpeg_ir::math::Point3, stored: cadmpeg_ir::math::Point3) -> bool {
    [
        (exact.x, stored.x),
        (exact.y, stored.y),
        (exact.z, stored.z),
    ]
    .into_iter()
    .all(|(value, expected)| {
        cadmpeg_core::convert::f32_from_f64(value).map(f64::from) == Some(expected)
    })
}

fn endpoint_matches(
    exact: cadmpeg_ir::math::Point3,
    stored: cadmpeg_ir::math::Point3,
    tolerance: f64,
) -> bool {
    exact.distance(stored) <= tolerance || rounds_to_stored(exact, stored)
}

fn source_tag(source: Option<&cadmpeg_ir::SourceObjectAssociation>, kind: &str) -> Option<u64> {
    let source = source?;
    let prefix = format!("cgm-{kind}:");
    u64::from_str_radix(source.object_id.as_str().strip_prefix(&prefix)?, 16).ok()
}

fn endpoint_address(
    journal: &AllocationJournal<'_>,
    origin: usize,
    value: u32,
    encoding: AllocationReferenceEncoding,
) -> Option<u64> {
    let base = journal.address(origin)?;
    let address = match encoding {
        AllocationReferenceEncoding::BackwardDistance | AllocationReferenceEncoding::WidthCoded => {
            base.checked_sub(u64::from(value))?
        }
        AllocationReferenceEncoding::OwnedChild
        | AllocationReferenceEncoding::TaggedU8
        | AllocationReferenceEncoding::TaggedU16
        | AllocationReferenceEncoding::TaggedU24 => {
            base.checked_add(u64::from(value))?.checked_add(1)?
        }
        AllocationReferenceEncoding::Selector2 => return None,
    };
    let record = journal.resolve_address(origin, address)?;
    if record.family() != ConsolidatedFamily::B || record.class() != 0x5d {
        return None;
    }
    let index = journal.index(record.source_index(), record.source_range().start)?;
    journal.absolute_address(index)
}

fn curve_record<'a>(
    journal: &AllocationJournal<'a>,
    origin: usize,
    value: u32,
    encoding: AllocationReferenceEncoding,
) -> Option<&'a ConsolidatedRecord> {
    let is_class25 = |record: &ConsolidatedRecord| {
        matches!(
            (record.family(), record.class()),
            (ConsolidatedFamily::A | ConsolidatedFamily::B, 0x23 | 0x25)
        )
    };
    let record = match encoding {
        AllocationReferenceEncoding::WidthCoded => journal.resolve_weighted_distance(origin, value),
        AllocationReferenceEncoding::BackwardDistance => journal
            .resolve_record_distance(origin, value)
            .filter(|candidate| is_class25(candidate))
            .or_else(|| journal.resolve_weighted_distance(origin, value)),
        AllocationReferenceEncoding::OwnedChild
        | AllocationReferenceEncoding::TaggedU8
        | AllocationReferenceEncoding::TaggedU16
        | AllocationReferenceEncoding::TaggedU24 => journal.resolve_address(
            origin,
            journal
                .address(origin)?
                .checked_add(u64::from(value))?
                .checked_add(1)?,
        ),
        AllocationReferenceEncoding::Selector2 => None,
    }?;
    is_class25(record).then_some(record)
}

struct CompactEdgeEndpoints {
    start_vertex: AllocationReference,
    end_vertex: AllocationReference,
}

fn uncompressed_map_candidates<'a>(
    data: &[u8],
    sources: &[Vec<SourceExtent>],
    journal: &AllocationJournal<'a>,
    map_record: &ConsolidatedRecord,
    map_index: usize,
) -> Vec<(&'a ConsolidatedRecord, Option<[f64; 2]>)> {
    let mut out = Vec::new();
    let Some(payload) = map_record.payload_bytes(data, sources) else {
        return out;
    };
    let Some(count) = payload.first().and_then(|byte| byte.checked_sub(0x80)) else {
        return out;
    };
    let mut at = 1usize;
    let mut refs = Vec::new();
    for _ in 0..usize::from(count).min(2) {
        let Some(rep_ref) = allocation_reference(&payload, &mut at) else {
            break;
        };
        refs.push(rep_ref);
    }
    let mut ranges: [Option<[f64; 2]>; 2] = [None, None];
    if matches!(
        payload.get(at..at + 3),
        Some([0x0a | 0x0b, 0x03 | 0x05, 0x00])
    ) {
        let mut view = cadmpeg_core::decode::View::over_retained(&payload[at + 3..]);
        if let (Some(t0), Some(t1)) = (view.f64_le(), view.f64_le()) {
            if t0.is_finite() && t1.is_finite() && t0 < t1 {
                ranges[0] = Some([t0, t1]);
            }
        }
        let _accuracy = view.f64_le();
        if let (Some(s0), Some(s1)) = (view.f64_le(), view.f64_le()) {
            if s0.is_finite() && s1.is_finite() && s0 < s1 {
                ranges[1] = Some([s0, s1]);
            }
        }
    }
    for (slot, rep_ref) in refs.into_iter().enumerate() {
        if let Some(rep) = super::standard_carriers::resolve_ref(journal, map_index, rep_ref) {
            out.push((rep, ranges.get(slot).copied().flatten()));
        }
    }
    out
}

fn parse_four_ref_edge_node(payload: &[u8]) -> Option<CompactEdgeEndpoints> {
    let mut at = 0;
    let start_vertex = allocation_reference(payload, &mut at)?;
    let end_vertex = allocation_reference(payload, &mut at)?;
    let _start_param = allocation_reference(payload, &mut at)?;
    let _end_param = allocation_reference(payload, &mut at)?;
    let terminal_at = at;
    let terminal = allocation_reference(payload, &mut at)?;
    let terminal_ok = at == payload.len()
        && at == terminal_at + 1
        && matches!(
            (terminal.encoding, terminal.value),
            (
                AllocationReferenceEncoding::BackwardDistance,
                0 | 8 | 9 | 10
            ) | (AllocationReferenceEncoding::Selector2, 0 | 4 | 6 | 8 | 10)
        );
    terminal_ok.then_some(CompactEdgeEndpoints {
        start_vertex,
        end_vertex,
    })
}

fn decode_representation_candidates(
    data: &[u8],
    sources: &[Vec<SourceExtent>],
    record: &ConsolidatedRecord,
    opt_range: Option<[f64; 2]>,
    jets: &HashMap<usize, crate::wire::records::ConsolidatedPcurve>,
    refusal: &mut crate::nurbs::LaneRefusals,
) -> Vec<(PcurveGeometry, [f64; 2], Option<AllocationReference>)> {
    let mut out = Vec::new();
    match (record.family(), record.class()) {
        (ConsolidatedFamily::A | ConsolidatedFamily::B, 0x20) => {
            if let Some(jet) = jets.get(&record.byte_offset()) {
                if let Some(pcurve) = super::consolidated_jet_pcurve(
                    jet,
                    &ConsolidatedCarrierChart::Identity,
                    refusal,
                ) {
                    let support_ref = record.payload_bytes(data, sources).and_then(|payload| {
                        let mut at = 0;
                        allocation_reference(&payload, &mut at)
                    });
                    out.push((
                        pcurve,
                        opt_range.unwrap_or_else(|| jet.range.endpoints()),
                        support_ref,
                    ));
                }
            } else if let Some((support_ref, pcurve, range)) =
                super::standard_carriers::decode_u32_jet_pcurve_in_sources(
                    data, sources, record, refusal,
                )
            {
                out.push((pcurve, opt_range.unwrap_or(range), Some(support_ref)));
            }
        }
        (ConsolidatedFamily::A | ConsolidatedFamily::B, 0x21) => {
            if let Some((support_ref, nurbs, range)) =
                super::standard_carriers::decode_u32_bspline_pcurve_in_sources(
                    data, sources, record,
                )
            {
                out.push((
                    PcurveGeometry::Nurbs { nurbs },
                    opt_range.unwrap_or(range),
                    Some(support_ref),
                ));
            }
        }
        (ConsolidatedFamily::B, 0x18) => {
            let Some(payload) = record.payload_bytes(data, sources) else {
                return out;
            };
            let mut at = 0;
            let Some(support_ref) = allocation_reference(&payload, &mut at) else {
                return out;
            };
            if payload.get(at..at + 2) == Some(&[0x00, 0x00]) && payload.len() == at + 50 {
                let mut view = cadmpeg_core::decode::View::over_retained(&payload[at + 2..]);
                if let (Some(u0), Some(v0), Some(du), Some(dv), Some(t0), Some(t1)) = (
                    view.f64_le(),
                    view.f64_le(),
                    view.f64_le(),
                    view.f64_le(),
                    view.f64_le(),
                    view.f64_le(),
                ) {
                    let range = opt_range.unwrap_or([t0, t1]);
                    if range[0].is_finite() && range[1].is_finite() && range[0] < range[1] {
                        if let Ok(line) =
                            LinePcurve::try_new(Point2::new(u0, v0), Point2::new(du, dv))
                        {
                            out.push((PcurveGeometry::Line(line), range, Some(support_ref)));
                        }
                    }
                }
            }
            if let Some(range) = opt_range {
                let mut cu_at = 0;
                if let Some(operand) = CompactUnsigned::read(&payload, &mut cu_at) {
                    if let Some(side) =
                        chart_side_payload(&payload, record.byte_offset(), operand.value())
                    {
                        for sign in [1.0, -1.0] {
                            if sign < 0.0 && !side.line.sign_ambiguous {
                                continue;
                            }
                            if let Ok(line) = LinePcurve::try_new(
                                Point2::new(side.line.origin[0], side.line.origin[1]),
                                Point2::new(
                                    sign * side.line.direction[0],
                                    sign * side.line.direction[1],
                                ),
                            ) {
                                out.push((PcurveGeometry::Line(line), range, Some(support_ref)));
                            }
                        }
                    }
                }
            }
            if out.is_empty() {
                if let Some(param_pt) =
                    b2_parameter_points_from_records(data, std::slice::from_ref(record)).next()
                {
                    if let Some([uv0, uv1]) = param_pt.axis_line() {
                        if let Ok(line) = LinePcurve::try_new(
                            Point2::new(uv0[0], uv0[1]),
                            Point2::new(uv1[0] - uv0[0], uv1[1] - uv0[1]),
                        ) {
                            out.push((PcurveGeometry::Line(line), [0.0, 1.0], Some(support_ref)));
                        }
                    }
                }
            }
        }
        _ => {}
    }
    out
}

struct LiftedPlanContext<'a> {
    ctx: &'a DecodeContext<'a>,
    ir: &'a CadIr,
    data: &'a [u8],
    sources: &'a [Vec<SourceExtent>],
    journal: &'a AllocationJournal<'a>,
    a5_by_pos: &'a HashMap<usize, &'a cadmpeg_ir::geometry::nurbs::NurbsSurface>,
    a5_multi_extent_cache:
        &'a std::cell::RefCell<HashMap<usize, Option<cadmpeg_ir::geometry::nurbs::NurbsSurface>>>,
    jets: &'a HashMap<usize, crate::wire::records::ConsolidatedPcurve>,
    planes: &'a HashMap<usize, crate::families::b2::records::B2PlaneCarrier>,
    cylinders: &'a HashMap<usize, crate::families::b2::records::B2Cylinder>,
    cones: &'a HashMap<usize, crate::families::b2::records::B2Cone>,
    spheres: &'a HashMap<usize, crate::families::b2::records::B2Sphere>,
    tori: &'a HashMap<usize, crate::families::b2::records::B2Torus>,
    loop_faces: &'a HashMap<cadmpeg_ir::ids::LoopId, cadmpeg_ir::ids::FaceId>,
    faces: &'a HashMap<cadmpeg_ir::ids::FaceId, &'a cadmpeg_ir::topology::Face>,
    surfaces: &'a HashMap<cadmpeg_ir::ids::SurfaceId, &'a cadmpeg_ir::geometry::Surface>,
    edge_coedges: &'a HashMap<cadmpeg_ir::ids::EdgeId, Vec<usize>>,
    vertex_indices: &'a HashMap<cadmpeg_ir::ids::VertexId, usize>,
}

#[derive(Clone, Copy)]
struct LiftedEdgeTarget {
    edge_index: usize,
    curve_index: usize,
    procedure_index: usize,
    start_pos: cadmpeg_ir::math::Point3,
    end_pos: cadmpeg_ir::math::Point3,
}

fn resolve_support_surface_and_chart<'a>(
    env: &'a LiftedPlanContext<'a>,
    rep_index: usize,
    support_ref: Option<AllocationReference>,
    face_surface: Option<&'a SolvedSurfaceGeometry>,
    refusal: &mut crate::nurbs::LaneRefusals,
) -> Option<(SolvedSurfaceGeometry, ConsolidatedCarrierChart<'a>, bool)> {
    if let Some(sref) = support_ref {
        if let Some(support_rec) =
            super::standard_carriers::resolve_ref(env.journal, rep_index, sref)
        {
            match (support_rec.family(), support_rec.class()) {
                (ConsolidatedFamily::A, 0x34) => {
                    let pos = support_rec.byte_offset();
                    if let Some(nurbs) = env.a5_by_pos.get(&pos) {
                        return Some((
                            SolvedSurfaceGeometry::Nurbs((*nurbs).clone()),
                            ConsolidatedCarrierChart::Identity,
                            matches!(face_surface, Some(SolvedSurfaceGeometry::Nurbs(_))),
                        ));
                    }
                    if let Some(cached) = env.a5_multi_extent_cache.borrow().get(&pos) {
                        return cached.clone().map(|nurbs| {
                            (
                                SolvedSurfaceGeometry::Nurbs(nurbs),
                                ConsolidatedCarrierChart::Identity,
                                matches!(face_surface, Some(SolvedSurfaceGeometry::Nurbs(_))),
                            )
                        });
                    }
                    let decoded =
                        crate::families::a5a8::records::a5_owned_surface_from_record_in_sources(
                            env.data,
                            env.sources,
                            support_rec,
                            refusal,
                        )
                        .ok()
                        .flatten()
                        .map(|owned| owned.geometry)
                        .or_else(|| {
                            super::standard_carriers::decode_u32_nurbs_surface(
                                env.data,
                                env.sources,
                                support_rec,
                                refusal,
                            )
                        });
                    env.a5_multi_extent_cache
                        .borrow_mut()
                        .insert(pos, decoded.clone());
                    if let Some(nurbs) = decoded {
                        return Some((
                            SolvedSurfaceGeometry::Nurbs(nurbs),
                            ConsolidatedCarrierChart::Identity,
                            matches!(face_surface, Some(SolvedSurfaceGeometry::Nurbs(_))),
                        ));
                    }
                }
                (ConsolidatedFamily::B, 0x27) => {
                    if let Some(b2_plane) = super::standard_carriers::decode_b2_plane_in_sources(
                        env.data,
                        env.sources,
                        support_rec,
                        env.planes,
                    ) {
                        return Some((
                            SolvedSurfaceGeometry::Plane(b2_plane),
                            ConsolidatedCarrierChart::Identity,
                            false,
                        ));
                    }
                }
                (ConsolidatedFamily::B, 0x28) => {
                    if let Some(cylinder) = env.cylinders.get(&support_rec.byte_offset()) {
                        if let SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(cyl)) =
                            cylinder.surface_geometry()
                        {
                            return Some((
                                SolvedSurfaceGeometry::Cylinder(cyl),
                                ConsolidatedCarrierChart::Cylinder {
                                    radius: cyl.radius().get(),
                                },
                                false,
                            ));
                        }
                    }
                }
                (ConsolidatedFamily::B, 0x29) => {
                    if let Some(cone) = env.cones.get(&support_rec.byte_offset()) {
                        if let SurfaceGeometry::Solved(solved) = b2_cone_geometry(cone) {
                            return Some((solved, ConsolidatedCarrierChart::Cone { cone }, false));
                        }
                    }
                }
                (ConsolidatedFamily::B, 0x2a) => {
                    if let Some(sphere) = env.spheres.get(&support_rec.byte_offset()) {
                        if let SurfaceGeometry::Solved(solved) = b2_sphere_geometry(sphere) {
                            return Some((
                                solved,
                                ConsolidatedCarrierChart::Sphere {
                                    radius: sphere.radius.get(),
                                },
                                false,
                            ));
                        }
                    }
                }
                (ConsolidatedFamily::B, 0x2b) => {
                    if let Some(torus) = env.tori.get(&support_rec.byte_offset()) {
                        if let SurfaceGeometry::Solved(solved) = b2_torus_geometry(torus) {
                            return Some((
                                solved,
                                ConsolidatedCarrierChart::Torus { torus },
                                false,
                            ));
                        }
                    }
                }
                _ => {}
            }
        }
    }
    if let Some(SolvedSurfaceGeometry::Nurbs(nurbs)) = face_surface {
        return Some((
            SolvedSurfaceGeometry::Nurbs(nurbs.clone()),
            ConsolidatedCarrierChart::Identity,
            true,
        ));
    }
    None
}

fn lift_pcurve_with_chart(
    ctx: &DecodeContext<'_>,
    pcurve: &PcurveGeometry,
    surface: &SolvedSurfaceGeometry,
    chart: &ConsolidatedCarrierChart<'_>,
    range: [f64; 2],
    refusal: &mut crate::nurbs::LaneRefusals,
) -> Result<Option<NurbsCurve>, CodecError> {
    if matches!(
        (surface, chart),
        (
            SolvedSurfaceGeometry::Plane(_),
            ConsolidatedCarrierChart::Identity
        )
    ) {
        if let Some(exact) =
            super::standard_carriers::lift_pcurve_to_3d(pcurve, surface, range, refusal)
        {
            return Ok(Some(exact));
        }
    }
    let limit = std::cell::Cell::new(None);
    let fitted = super::rolling_ball_patch::fit_edge_with_error(
        range,
        |t| {
            let uv = match cadmpeg_ir::eval::decode::outer_refusal(
                cadmpeg_ir::eval::decode::pcurve_uv(ctx, pcurve, t),
            )
            .and_then(cadmpeg_ir::eval::finite_or_refusal)
            {
                Ok(Some(value)) => value.get(),
                Ok(None) => return None,
                Err(error) => {
                    limit.set(Some(error));
                    return None;
                }
            };
            let [cu, cv] = chart.point([uv.u, uv.v]);
            let point = match cadmpeg_ir::eval::decode::outer_refusal(
                cadmpeg_ir::eval::decode::surface_point_solved(ctx, surface, cu, cv),
            )
            .and_then(cadmpeg_ir::eval::finite_or_refusal)
            {
                Ok(Some(value)) => value.get(),
                Ok(None) => return None,
                Err(error) => {
                    limit.set(Some(error));
                    return None;
                }
            };
            Some([point.x, point.y, point.z])
        },
        LIFTED_CURVE_FIT_TOLERANCE_MM,
    );
    if let Some(error) = limit.into_inner() {
        return Err(error.into());
    }
    Ok(fitted)
}

type LiftedEdgeCandidate<'a> = (&'a ConsolidatedRecord, Option<[f64; 2]>, Option<u64>);

fn unique_matching_coedge(matching: &[usize]) -> Option<usize> {
    match matching {
        &[single] => Some(single),
        _ => None,
    }
}

#[inline(never)]
fn build_general_lifted_plan(
    env: &LiftedPlanContext<'_>,
    target: LiftedEdgeTarget,
    candidates: &[LiftedEdgeCandidate<'_>],
    refusal: &mut crate::nurbs::LaneRefusals,
) -> Result<Option<Plan>, CodecError> {
    let LiftedPlanContext {
        ctx,
        ir,
        data,
        journal,
        jets,
        loop_faces,
        faces,
        surfaces,
        edge_coedges,
        vertex_indices,
        ..
    } = env;
    let LiftedEdgeTarget {
        edge_index,
        curve_index,
        procedure_index,
        start_pos,
        end_pos,
    } = target;
    let edge = &ir.geometry.edges[edge_index];
    let Some(ends) = &edge.ends else {
        return Ok(None);
    };
    let Some(&start_idx) = vertex_indices.get(&ends.start) else {
        return Ok(None);
    };
    let Some(&end_idx) = vertex_indices.get(&ends.end) else {
        return Ok(None);
    };
    let base_tols = [start_idx, end_idx].map(|index| {
        ir.geometry.vertices[index]
            .tolerance
            .map_or(ir.tolerances.linear.get(), |t| {
                t.get().max(ir.tolerances.linear.get())
            })
            .max(LIFTED_ENDPOINT_TOLERANCE_MM)
    });

    for &(rep, opt_range, opt_face_tag) in candidates {
        let Some(rep_index) = journal.index(rep.source_index(), rep.source_range().start) else {
            continue;
        };
        let coedge_index = opt_face_tag.and_then(|face_tag| {
            let matching: Vec<usize> = edge_coedges
                .get(&edge.id)
                .into_iter()
                .flatten()
                .copied()
                .filter(|&ci| {
                    let c = &ir.geometry.coedges[ci];
                    loop_faces
                        .get(&c.owner_loop)
                        .and_then(|f| faces.get(f))
                        .and_then(|f| surfaces.get(&f.surface))
                        .is_some_and(|s| {
                            source_tag(s.source_object.as_ref(), "carrier") == Some(face_tag)
                        })
                })
                .collect();
            unique_matching_coedge(&matching)
        });
        let face_surface = coedge_index
            .and_then(|ci| loop_faces.get(&ir.geometry.coedges[ci].owner_loop))
            .and_then(|f| faces.get(f))
            .and_then(|f| surfaces.get(&f.surface))
            .and_then(|s| match &s.geometry {
                SurfaceGeometry::Solved(solved)
                    if !matches!(solved, SolvedSurfaceGeometry::Unknown { .. }) =>
                {
                    Some(solved)
                }
                _ => None,
            });

        for (pcurve_raw, range, support_ref) in
            decode_representation_candidates(data, env.sources, rep, opt_range, jets, refusal)
        {
            if !range[0].is_finite() || !range[1].is_finite() || range[0] >= range[1] {
                continue;
            }
            let Some((eval_surface, chart, mut attach_nurbs_pcurve)) =
                resolve_support_surface_and_chart(
                    env,
                    rep_index,
                    support_ref,
                    face_surface,
                    refusal,
                )
            else {
                continue;
            };
            let Some(mut lifted) =
                lift_pcurve_with_chart(ctx, &pcurve_raw, &eval_surface, &chart, range, refusal)?
            else {
                continue;
            };
            let mut lifted_endpoints = Vec::with_capacity(2);
            for parameter in range {
                let point =
                    cadmpeg_ir::eval::finite_or_refusal(cadmpeg_ir::eval::decode::outer_refusal(
                        cadmpeg_ir::eval::decode::nurbs_curve_point_at(*ctx, &lifted, parameter),
                    )?)?;
                if let Some(point) = point {
                    lifted_endpoints.push(point.get());
                }
            }
            if lifted_endpoints.len() != 2 {
                continue;
            }
            let direct = lifted_endpoints[0]
                .distance(start_pos)
                .max(lifted_endpoints[1].distance(end_pos));
            let reverse = lifted_endpoints[0]
                .distance(end_pos)
                .max(lifted_endpoints[1].distance(start_pos));
            let direct_ok = endpoint_matches(lifted_endpoints[0], start_pos, base_tols[0])
                && endpoint_matches(lifted_endpoints[1], end_pos, base_tols[1]);
            let reverse_ok = endpoint_matches(lifted_endpoints[0], end_pos, base_tols[1])
                && endpoint_matches(lifted_endpoints[1], start_pos, base_tols[0]);
            if !direct_ok && !reverse_ok {
                continue;
            }
            let mut pcurve_out = pcurve_raw;
            if reverse_ok && (!direct_ok || reverse < direct) {
                let Ok(rev_curve) = crate::nurbs::reverse_nurbs_curve(&lifted, range) else {
                    continue;
                };
                lifted = rev_curve;
                if attach_nurbs_pcurve {
                    if let Some(rev_pc) = crate::nurbs::reverse_pcurve_geometry(
                        &pcurve_out,
                        range,
                        refusal,
                        "lifted consolidated pcurve",
                    ) {
                        pcurve_out = rev_pc;
                    } else {
                        attach_nurbs_pcurve = false;
                    }
                }
            }
            let geom = CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(lifted.clone()));
            let Some(canonical) = crate::nurbs::canonical_model_curve_range(
                &geom,
                range,
                refusal,
                "lifted consolidated curve",
            ) else {
                continue;
            };
            let Ok(curve_range) = ParameterInterval::new(canonical) else {
                continue;
            };
            let mut matched_ends = 0;
            for (parameter, stored) in [
                (curve_range.endpoints()[0], start_pos),
                (curve_range.endpoints()[1], end_pos),
            ] {
                let point =
                    cadmpeg_ir::eval::finite_or_refusal(cadmpeg_ir::eval::decode::outer_refusal(
                        cadmpeg_ir::eval::decode::nurbs_curve_point_at(*ctx, &lifted, parameter),
                    )?)?;
                let Some(point) = point else { break };
                let miss = point.get().distance(stored);
                if miss > LIFTED_ENDPOINT_TOLERANCE_MM && !rounds_to_stored(point.get(), stored) {
                    break;
                }
                matched_ends += 1;
            }
            if matched_ends != 2 {
                continue;
            }
            let face_pcurves = if let Some(ci) = coedge_index {
                if attach_nurbs_pcurve && ir.geometry.coedges[ci].pcurves.is_empty() {
                    vec![(ci, pcurve_out, curve_range, rep.byte_offset())]
                } else {
                    Vec::new()
                }
            } else {
                Vec::new()
            };
            return Ok(Some(Plan {
                edge: edge_index,
                curve: curve_index,
                procedure: procedure_index,
                face_pcurves,
                curve_cache: lifted,
                curve_range,
                plane_pcurve: None,
                plane_fit_tolerance: None,
                plane_projection_gap: None,
            }));
        }
    }
    Ok(None)
}

fn line_curve(
    ctx: &DecodeContext<'_>,
    refusal: &mut crate::nurbs::LaneRefusals,
    data: &[u8],
    record: &ConsolidatedRecord,
    surface: &cadmpeg_ir::geometry::nurbs::NurbsSurface,
    parameter_range: [f64; 2],
    endpoint_constraints: [(cadmpeg_ir::math::Point3, f64); 2],
) -> Result<Option<(NurbsCurve, PcurveGeometry, ParameterInterval)>, CodecError> {
    let [(start, start_tolerance), (end, end_tolerance)] = endpoint_constraints;
    let endpoints = [start, end];
    let endpoint_tolerances = [start_tolerance, end_tolerance];
    let Some(payload) = record.payload().and_then(|range| data.get(range)) else {
        return Ok(None);
    };
    let mut at = 0;
    let Some(operand) = CompactUnsigned::read(payload, &mut at) else {
        return Ok(None);
    };
    let Some(side) = chart_side_payload(payload, record.byte_offset(), operand.value()) else {
        return Ok(None);
    };
    let geometry = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(surface.clone()));
    let mut chosen: Option<([f64; 2], bool)> = None;
    for sign in [1.0, -1.0] {
        if sign < 0.0 && !side.line.sign_ambiguous {
            continue;
        }
        let mut uv = [[0.0; 2]; 2];
        for (index, t) in parameter_range.iter().copied().enumerate() {
            uv[index] = [
                side.line.origin[0] + sign * t * side.line.direction[0],
                side.line.origin[1] + sign * t * side.line.direction[1],
            ];
        }
        let mut lifted = Vec::new();
        for [u, v] in uv {
            let p = cadmpeg_ir::eval::finite_or_refusal(cadmpeg_ir::eval::decode::outer_refusal(
                cadmpeg_ir::eval::decode::surface_point(ctx, &geometry, u, v),
            )?)?;
            let Some(p) = p else { break };
            lifted.push(p.get());
        }
        if lifted.len() != 2 {
            continue;
        }
        let direct = lifted[0]
            .distance(endpoints[0])
            .max(lifted[1].distance(endpoints[1]));
        let reverse = lifted[0]
            .distance(endpoints[1])
            .max(lifted[1].distance(endpoints[0]));
        let direct_matches = endpoint_matches(lifted[0], endpoints[0], endpoint_tolerances[0])
            && endpoint_matches(lifted[1], endpoints[1], endpoint_tolerances[1]);
        let reverse_matches = endpoint_matches(lifted[0], endpoints[1], endpoint_tolerances[1])
            && endpoint_matches(lifted[1], endpoints[0], endpoint_tolerances[0]);
        if !direct_matches && !reverse_matches {
            continue;
        }
        let varying = match side.axis {
            ChartAxis::U => [uv[0][1], uv[1][1]],
            ChartAxis::V => [uv[0][0], uv[1][0]],
        };
        let interval = [varying[0].min(varying[1]), varying[0].max(varying[1])];
        let start_parameter = if direct_matches && (!reverse_matches || direct <= reverse) {
            varying[0]
        } else {
            varying[1]
        };
        let reversed = start_parameter > interval[0];
        if let Some((previous, previous_reversed)) = chosen {
            if (previous[0] - interval[0]).abs() > PARAMETER_TOLERANCE
                || (previous[1] - interval[1]).abs() > PARAMETER_TOLERANCE
                || previous_reversed != reversed
            {
                return Ok(None);
            }
        } else {
            chosen = Some((interval, reversed));
        }
    }
    let Some((interval, reversed)) = chosen else {
        return Ok(None);
    };
    let axis = match side.axis {
        ChartAxis::U => SurfaceParameterAxis::U,
        ChartAxis::V => SurfaceParameterAxis::V,
    };
    let Some(mut cache) = cadmpeg_ir::eval::nurbs_surface_isocurve(ctx, surface, axis, side.fixed)?
    else {
        return Ok(None);
    };
    let (origin, direction) = match side.axis {
        ChartAxis::U => (Point2::new(side.fixed, 0.0), Point2::new(0.0, 1.0)),
        ChartAxis::V => (Point2::new(0.0, side.fixed), Point2::new(1.0, 0.0)),
    };
    let Ok(line) = LinePcurve::try_new(origin, direction) else {
        return Ok(None);
    };
    let mut line = PcurveGeometry::Line(line);
    if reversed {
        let Ok(reversed_cache) = crate::nurbs::reverse_nurbs_curve(&cache, interval) else {
            return Ok(None);
        };
        cache = reversed_cache;
        let PcurveGeometry::Line(original) = &line else {
            return Ok(None);
        };
        let origin = original.origin().as_raw();
        let direction = original.direction().as_raw();
        let sum = interval[0] + interval[1];
        let Ok(reversed_line) = LinePcurve::try_new(
            Point2::new(origin.u + sum * direction.u, origin.v + sum * direction.v),
            Point2::new(-direction.u, -direction.v),
        ) else {
            return Ok(None);
        };
        line = PcurveGeometry::Line(reversed_line);
    }
    let geometry = CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(cache.clone()));
    let Some(interval) = crate::nurbs::canonical_model_curve_range(
        &geometry,
        interval,
        refusal,
        "weighted surface curve",
    ) else {
        return Ok(None);
    };
    let Ok(range) = ParameterInterval::new(interval) else {
        return Ok(None);
    };
    Ok(Some((cache, line, range)))
}

pub(in crate::families) struct WeightedEdgesSource<'a> {
    pub(in crate::families) data: &'a [u8],
    pub(in crate::families) sources: &'a [Vec<SourceExtent>],
    pub(in crate::families) records: &'a [ConsolidatedRecord],
    pub(in crate::families) journal: &'a AllocationJournal<'a>,
    pub(in crate::families) owners: &'a OwnerClosureIndex,
}

pub(in crate::families) fn attach_with_journal(
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder<impl AnnotationStorage>,
    source: &WeightedEdgesSource<'_>,
    freeform_surfaces: &[crate::families::a5a8::records::FreeformSurface],
    refusal: &mut crate::nurbs::LaneRefusals,
    admission: &mut FamilyEntityAdmission<'_, '_>,
) -> Result<AttachResult, CodecError> {
    let WeightedEdgesSource {
        data,
        sources,
        records,
        journal,
        owners,
    } = *source;
    let mut nodes = HashMap::new();
    let mut four_ref_nodes = HashMap::new();
    for (index, record) in records.iter().enumerate() {
        if record.family() != ConsolidatedFamily::B || record.class() != 0x5e {
            continue;
        }
        let Some(tag) = journal.absolute_address(index) else {
            continue;
        };
        if let Some(node) = b2_edge_nodes_from_records(data, std::slice::from_ref(record)).next() {
            nodes
                .entry(tag)
                .and_modify(|entry| *entry = None)
                .or_insert(Some((index, node)));
        } else if let Some(endpoints) = record
            .payload()
            .and_then(|range| data.get(range))
            .and_then(parse_four_ref_edge_node)
        {
            four_ref_nodes
                .entry(tag)
                .and_modify(|entry| *entry = None)
                .or_insert(Some((index, endpoints)));
        }
    }
    if nodes.is_empty() && four_ref_nodes.is_empty() {
        return Ok(AttachResult::default());
    }
    let a5_multi_extent_cache = std::cell::RefCell::new(HashMap::new());
    let mut jets = HashMap::new();
    for family in [ConsolidatedFamily::A, ConsolidatedFamily::B] {
        for jet in crate::wire::records::family_pcurves_from_records(data, records, family) {
            jets.insert(jet.pos, jet);
        }
    }
    let a5_by_pos: HashMap<_, _> = freeform_surfaces
        .iter()
        .map(|surface| (surface.pos, &surface.geometry))
        .collect();
    let planes: HashMap<_, _> = b2_plane_carriers_from_records(data, records)
        .into_iter()
        .map(|plane| (plane.pos, plane))
        .collect();
    let cylinders: HashMap<_, _> = b2_cylinders_from_records(data, records)
        .map(|cylinder| (cylinder.pos, cylinder))
        .collect();
    let cones: HashMap<_, _> = b2_cones_from_records(data, records)
        .map(|cone| (cone.pos, cone))
        .collect();
    let spheres: HashMap<_, _> = b2_spheres_from_records(data, records)
        .map(|sphere| (sphere.pos, sphere))
        .collect();
    let tori: HashMap<_, _> = b2_tori_from_records(data, records)
        .map(|torus| (torus.pos, torus))
        .collect();
    let curve_indices: HashMap<_, _> = ir
        .geometry
        .curves
        .iter()
        .enumerate()
        .map(|(i, c)| (c.id.clone(), i))
        .collect();
    let procedure_indices: HashMap<_, _> = ir
        .geometry
        .procedural_curves
        .iter()
        .enumerate()
        .map(|(index, procedure)| (procedure.id.clone(), index))
        .collect();
    let points: HashMap<_, _> = ir
        .geometry
        .points
        .iter()
        .map(|p| (p.id.clone(), p))
        .collect();
    let vertices: HashMap<_, _> = ir
        .geometry
        .vertices
        .iter()
        .filter_map(|v| Some((v.id.clone(), *points.get(&v.point)?)))
        .collect();
    let vertex_indices: HashMap<_, _> = ir
        .geometry
        .vertices
        .iter()
        .enumerate()
        .map(|(i, vertex)| (vertex.id.clone(), i))
        .collect();
    let loop_faces: HashMap<_, _> = ir
        .geometry
        .loops
        .iter()
        .map(|l| (l.id.clone(), l.face.clone()))
        .collect();
    let faces: HashMap<_, _> = ir
        .geometry
        .faces
        .iter()
        .map(|f| (f.id.clone(), f))
        .collect();
    let surfaces: HashMap<_, _> = ir
        .geometry
        .surfaces
        .iter()
        .map(|s| (s.id.clone(), s))
        .collect();
    let mut edge_coedges: HashMap<_, Vec<usize>> = HashMap::new();
    for (index, coedge) in ir.geometry.coedges.iter().enumerate() {
        edge_coedges
            .entry(coedge.edge.clone())
            .or_default()
            .push(index);
    }
    let mut plans = Vec::new();
    for (edge_index, edge) in ir.geometry.edges.iter().enumerate() {
        let Some(curve_index) = edge.curve().and_then(|id| curve_indices.get(id)).copied() else {
            continue;
        };
        let curve = &ir.geometry.curves[curve_index];
        let CurveGeometry::Procedural {
            construction,
            cache: Some(SolvedCurveGeometry::Unknown { .. }),
        } = &curve.geometry
        else {
            continue;
        };
        let Some(procedure_index) = procedure_indices.get(construction).copied() else {
            continue;
        };
        let ProceduralCurveDefinition::Intersection { context, .. } =
            ir.geometry.procedural_curves[procedure_index].definition()
        else {
            continue;
        };
        if context.sides().iter().any(|side| side.pcurve.is_some())
            || context
                .discontinuities()
                .iter()
                .any(|lane| !lane.is_empty())
        {
            continue;
        }
        let Some(tag) = source_tag(curve.source_object.as_ref(), "edge-support") else {
            continue;
        };
        let Some(start) = edge.start().and_then(|vertex| vertices.get(vertex)) else {
            continue;
        };
        let Some(end) = edge.end().and_then(|vertex| vertices.get(vertex)) else {
            continue;
        };
        let Some(start_tag) = source_tag(start.source_object.as_ref(), "vertex") else {
            continue;
        };
        let Some(end_tag) = source_tag(end.source_object.as_ref(), "vertex") else {
            continue;
        };
        let Some(Some((node_index, node))) = nodes.get(&tag) else {
            if let Some(Some((node_index, compact))) = four_ref_nodes.get(&tag) {
                let (Some(a), Some(b)) = (
                    endpoint_address(
                        journal,
                        *node_index,
                        compact.start_vertex.value,
                        compact.start_vertex.encoding,
                    ),
                    endpoint_address(
                        journal,
                        *node_index,
                        compact.end_vertex.value,
                        compact.end_vertex.encoding,
                    ),
                ) else {
                    continue;
                };
                if !((a == start_tag && b == end_tag) || (a == end_tag && b == start_tag)) {
                    continue;
                }
                let next_map = journal
                    .record(node_index + 1)
                    .filter(|r| {
                        matches!(
                            (r.family(), r.class()),
                            (ConsolidatedFamily::A | ConsolidatedFamily::B, 0x23 | 0x25)
                        )
                    })
                    .map(|next_rec| (next_rec, node_index + 1));
                let candidates = collect_uncompressed_candidates(
                    data,
                    sources,
                    journal,
                    owners,
                    *node_index,
                    next_map,
                );
                if let Some(plan) = build_general_lifted_plan(
                    &LiftedPlanContext {
                        ctx: admission.context(),
                        ir,
                        data,
                        sources,
                        journal,
                        a5_by_pos: &a5_by_pos,
                        a5_multi_extent_cache: &a5_multi_extent_cache,
                        jets: &jets,
                        planes: &planes,
                        cylinders: &cylinders,
                        cones: &cones,
                        spheres: &spheres,
                        tori: &tori,
                        loop_faces: &loop_faces,
                        faces: &faces,
                        surfaces: &surfaces,
                        edge_coedges: &edge_coedges,
                        vertex_indices: &vertex_indices,
                    },
                    LiftedEdgeTarget {
                        edge_index,
                        curve_index,
                        procedure_index,
                        start_pos: start.position().get(),
                        end_pos: end.position().get(),
                    },
                    &candidates,
                    refusal,
                )? {
                    plans.push(plan);
                }
            }
            continue;
        };
        let Some(a) = endpoint_address(
            journal,
            *node_index,
            node.start_vertex_ref,
            node.reference_encodings[1],
        ) else {
            continue;
        };
        let Some(b) = endpoint_address(
            journal,
            *node_index,
            node.end_vertex_ref,
            node.reference_encodings[2],
        ) else {
            continue;
        };
        if !((a == start_tag && b == end_tag) || (a == end_tag && b == start_tag)) {
            continue;
        }
        let Some(map_record) = curve_record(
            journal,
            *node_index,
            node.curve_ref,
            node.reference_encodings[0],
        ) else {
            continue;
        };
        let Some(map_index) =
            journal.index(map_record.source_index(), map_record.source_range().start)
        else {
            continue;
        };
        let map_payload = map_record.payload_bytes(data, sources);
        let parsed_map = map_payload.as_deref().and_then(|payload| {
            if map_record.class() != 0x25 {
                return None;
            }
            if map_record.family() == ConsolidatedFamily::A {
                class25::parse_a(payload)
            } else {
                class25::parse(payload)
            }
        });
        let Some(map) = parsed_map else {
            let candidates = collect_uncompressed_candidates(
                data,
                sources,
                journal,
                owners,
                *node_index,
                Some((map_record, map_index)),
            );
            if !candidates.is_empty() {
                if let Some(plan) = build_general_lifted_plan(
                    &LiftedPlanContext {
                        ctx: admission.context(),
                        ir,
                        data,
                        sources,
                        journal,
                        a5_by_pos: &a5_by_pos,
                        a5_multi_extent_cache: &a5_multi_extent_cache,
                        jets: &jets,
                        planes: &planes,
                        cylinders: &cylinders,
                        cones: &cones,
                        spheres: &spheres,
                        tori: &tori,
                        loop_faces: &loop_faces,
                        faces: &faces,
                        surfaces: &surfaces,
                        edge_coedges: &edge_coedges,
                        vertex_indices: &vertex_indices,
                    },
                    LiftedEdgeTarget {
                        edge_index,
                        curve_index,
                        procedure_index,
                        start_pos: start.position().get(),
                        end_pos: end.position().get(),
                    },
                    &candidates,
                    refusal,
                )? {
                    plans.push(plan);
                }
            }
            continue;
        };
        let reps = class25_representations(journal, map_index, &map);
        let ranges = [
            Some(map.target_range.endpoints()),
            match &map.source {
                Class25SourceRelation::Affine { range, .. }
                | Class25SourceRelation::Sampled { range, .. } => Some(range.endpoints()),
                Class25SourceRelation::Absent => None,
            },
        ];
        let occurrences = (|| {
            let [Some(first), Some(second)] = [reps.target, reps.source] else {
                return None;
            };
            let mut representation_indices = Vec::new();
            let mut carrier_indices = Vec::new();
            for representation in [first, second] {
                let index = journal.index(
                    representation.source_index(),
                    representation.source_range().start,
                )?;
                let payload = representation.payload().and_then(|range| data.get(range))?;
                let mut at = 0;
                let operand = CompactUnsigned::read(payload, &mut at)?;
                let carrier = journal.resolve_weighted_distance(index, operand.value())?;
                let carrier_index =
                    journal.index(carrier.source_index(), carrier.source_range().start)?;
                representation_indices.push(index);
                carrier_indices.push(carrier_index);
            }
            let representation_indices = <[usize; 2]>::try_from(representation_indices).ok()?;
            let carrier_indices = <[usize; 2]>::try_from(carrier_indices).ok()?;
            let face_tags: Vec<_> = edge_coedges
                .get(&edge.id)
                .into_iter()
                .flatten()
                .filter_map(|index| {
                    let coedge = &ir.geometry.coedges[*index];
                    let surface = loop_faces
                        .get(&coedge.owner_loop)
                        .and_then(|face| faces.get(face))
                        .and_then(|face| surfaces.get(&face.surface))?;
                    source_tag(surface.source_object.as_ref(), "carrier")
                })
                .collect();
            let face_tags = <[u64; 2]>::try_from(face_tags).ok()?;
            owners.bind(
                *node_index,
                representation_indices,
                carrier_indices,
                face_tags,
            )
        })();
        let mut plan: Option<Plan> = None;
        if let Some(occurrences) = occurrences {
            for ((representation, range), occurrence) in [reps.target, reps.source]
                .into_iter()
                .zip(ranges)
                .zip(occurrences)
            {
                let (Some(line), Some(range)) = (representation, range) else {
                    continue;
                };
                if line.family() != ConsolidatedFamily::B || line.class() != 0x18 {
                    continue;
                }
                let Some(line_index) =
                    journal.index(line.source_index(), line.source_range().start)
                else {
                    continue;
                };
                let Some(carrier) = b18_support(data, journal, line_index) else {
                    continue;
                };
                if carrier.family() != ConsolidatedFamily::A || carrier.class() != 0x34 {
                    continue;
                }
                let Some(surface) = a5_by_pos.get(&carrier.byte_offset()) else {
                    continue;
                };
                let face_coedges: Vec<_> = edge_coedges
                    .get(&edge.id)
                    .into_iter()
                    .flatten()
                    .copied()
                    .filter(|i| {
                        let c = &ir.geometry.coedges[*i];
                        loop_faces
                            .get(&c.owner_loop)
                            .and_then(|f| faces.get(f))
                            .and_then(|f| surfaces.get(&f.surface))
                            .is_some_and(|s| {
                                source_tag(s.source_object.as_ref(), "carrier")
                                    == Some(occurrence.face_tag)
                                    && s.geometry
                                        == SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(
                                            (*surface).clone(),
                                        ))
                            })
                    })
                    .collect();
                if face_coedges.len() != 1 {
                    continue;
                }
                let add_face_pcurve = ir.geometry.coedges[face_coedges[0]].pcurves.is_empty();
                let Some(ends) = &edge.ends else {
                    continue;
                };
                let Some(&start_index) = vertex_indices.get(&ends.start) else {
                    continue;
                };
                let Some(&end_index) = vertex_indices.get(&ends.end) else {
                    continue;
                };
                let endpoint_tolerances = [start_index, end_index].map(|index| {
                    ir.geometry.vertices[index]
                        .tolerance
                        .map_or(ir.tolerances.linear.get(), |t| {
                            t.get().max(ir.tolerances.linear.get())
                        })
                });
                let Some((cache, line_geometry, interval)) = line_curve(
                    admission.context(),
                    refusal,
                    data,
                    line,
                    surface,
                    range,
                    [
                        (start.position().get(), endpoint_tolerances[0]),
                        (end.position().get(), endpoint_tolerances[1]),
                    ],
                )?
                else {
                    continue;
                };
                if let Some(existing) = &mut plan {
                    if add_face_pcurve
                        && !existing
                            .face_pcurves
                            .iter()
                            .any(|(index, _, _, _)| *index == face_coedges[0])
                    {
                        existing.face_pcurves.push((
                            face_coedges[0],
                            line_geometry,
                            interval,
                            line.byte_offset(),
                        ));
                    }
                    continue;
                }
                let mut curve_cache = cache;
                let mut curve_range = interval;
                let mut plane_pcurve = None;
                let mut plane_fit_tolerance = None;
                let mut plane_projection_gap = None;
                for ((other, other_range), other_occurrence) in [reps.target, reps.source]
                    .into_iter()
                    .zip(ranges)
                    .zip(occurrences)
                {
                    let (Some(other), Some(other_range)) = (other, other_range) else {
                        continue;
                    };
                    if other.family() != ConsolidatedFamily::B || other.class() != 0x20 {
                        continue;
                    }
                    let Some(jet) = jets.get(&other.byte_offset()) else {
                        continue;
                    };
                    let Some(other_index) =
                        journal.index(other.source_index(), other.source_range().start)
                    else {
                        continue;
                    };
                    let Some(support) =
                        journal.resolve_weighted_distance(other_index, jet.support_id)
                    else {
                        continue;
                    };
                    if support.family() != ConsolidatedFamily::B || support.class() != 0x27 {
                        continue;
                    }
                    let plane = planes
                        .get(&support.byte_offset())
                        .and_then(b2_plane_geometry)
                        .and_then(|geometry| match geometry {
                            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane)) => {
                                Some(plane)
                            }
                            _ => None,
                        })
                        .or_else(|| {
                            crate::families::consolidated::packed_plane::packed_plane(
                                cadmpeg_core::decode::View::over_retained(data),
                                support,
                            )
                        });
                    let Some(plane) = plane else {
                        continue;
                    };
                    let plane_coedges: Vec<_> = edge_coedges
                        .get(&edge.id)
                        .into_iter()
                        .flatten()
                        .copied()
                        .filter(|i| {
                            let c = &ir.geometry.coedges[*i];
                            let Some(face_surface) = loop_faces
                                .get(&c.owner_loop)
                                .and_then(|f| faces.get(f))
                                .and_then(|f| surfaces.get(&f.surface))
                            else {
                                return false;
                            };
                            if source_tag(face_surface.source_object.as_ref(), "carrier")
                                != Some(other_occurrence.face_tag)
                            {
                                return false;
                            }
                            matches!(
                                face_surface.geometry,
                                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(_))
                            )
                        })
                        .collect();
                    if plane_coedges.len() != 1 {
                        continue;
                    }
                    let Some(native_pcurve) = super::consolidated_jet_pcurve(
                        jet,
                        &ConsolidatedCarrierChart::Identity,
                        refusal,
                    ) else {
                        continue;
                    };
                    let Some(lifted) =
                        super::weighted_plane::lift_plane_pcurve(&native_pcurve, &plane, refusal)
                    else {
                        continue;
                    };
                    let Ok(source_range) = ParameterInterval::new(other_range) else {
                        continue;
                    };
                    let mut lifted_endpoints = Vec::new();
                    for parameter in other_range {
                        let point = cadmpeg_ir::eval::finite_or_refusal(
                            cadmpeg_ir::eval::decode::outer_refusal(
                                cadmpeg_ir::eval::decode::nurbs_curve_point_at(
                                    admission.context(),
                                    &lifted,
                                    parameter,
                                ),
                            )?,
                        )?;
                        if let Some(point) = point {
                            lifted_endpoints.push(point.get());
                        }
                    }
                    if lifted_endpoints.len() != 2 {
                        continue;
                    }
                    let endpoint_positions = [start.position().get(), end.position().get()];
                    let direct = lifted_endpoints[0]
                        .distance(endpoint_positions[0])
                        .max(lifted_endpoints[1].distance(endpoint_positions[1]));
                    let reverse = lifted_endpoints[0]
                        .distance(endpoint_positions[1])
                        .max(lifted_endpoints[1].distance(endpoint_positions[0]));
                    let direct_matches = endpoint_matches(
                        lifted_endpoints[0],
                        endpoint_positions[0],
                        endpoint_tolerances[0],
                    ) && endpoint_matches(
                        lifted_endpoints[1],
                        endpoint_positions[1],
                        endpoint_tolerances[1],
                    );
                    let reverse_matches = endpoint_matches(
                        lifted_endpoints[0],
                        endpoint_positions[1],
                        endpoint_tolerances[1],
                    ) && endpoint_matches(
                        lifted_endpoints[1],
                        endpoint_positions[0],
                        endpoint_tolerances[0],
                    );
                    if !direct_matches && !reverse_matches {
                        continue;
                    }
                    let coedge = &ir.geometry.coedges[plane_coedges[0]];
                    let Some(SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(face_plane))) =
                        loop_faces
                            .get(&coedge.owner_loop)
                            .and_then(|f| faces.get(f))
                            .and_then(|f| surfaces.get(&f.surface))
                            .map(|s| &s.geometry)
                    else {
                        continue;
                    };
                    let source_u = *plane.frame().reference().as_raw();
                    let source_v = plane.frame().axis().as_raw().cross(source_u);
                    let face_u = *face_plane.frame().reference().as_raw();
                    let face_v = face_plane.frame().axis().as_raw().cross(face_u);
                    let offset = plane.origin().get().vector_from(face_plane.origin().get());
                    let face_axis = *face_plane.frame().axis().as_raw();
                    let source_axis = *plane.frame().axis().as_raw();
                    let compatible = source_axis.cross(face_axis).norm() <= PLANE_NORMAL_TOLERANCE
                        && source_axis.dot(offset).abs() <= PLANE_OFFSET_TOLERANCE;
                    let chart = ConsolidatedCarrierChart::Rigid {
                        linear: [
                            [source_u.dot(face_u), source_v.dot(face_u)],
                            [source_u.dot(face_v), source_v.dot(face_v)],
                        ],
                        offset: [offset.dot(face_u), offset.dot(face_v)],
                    };
                    let mut pcurve = compatible
                        .then(|| super::consolidated_jet_pcurve(jet, &chart, refusal))
                        .flatten();
                    let mut lifted = lifted;
                    if reverse_matches && (!direct_matches || reverse < direct) {
                        let Ok(reversed) = crate::nurbs::reverse_nurbs_curve(&lifted, other_range)
                        else {
                            continue;
                        };
                        lifted = reversed;
                        pcurve = pcurve.and_then(|pcurve| {
                            crate::nurbs::reverse_pcurve_geometry(
                                &pcurve,
                                other_range,
                                refusal,
                                "weighted plane pcurve",
                            )
                        });
                    }
                    let geometry =
                        CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(lifted.clone()));
                    let Some(canonical) = crate::nurbs::canonical_model_curve_range(
                        &geometry,
                        source_range.endpoints(),
                        refusal,
                        "weighted plane curve",
                    ) else {
                        continue;
                    };
                    let Ok(source_range) = ParameterInterval::new(canonical) else {
                        continue;
                    };
                    if compatible {
                        if ir.geometry.coedges[plane_coedges[0]].pcurves.is_empty() {
                            plane_pcurve = pcurve
                                .map(|pcurve| {
                                    (plane_coedges[0], pcurve, source_range, other.byte_offset())
                                })
                                .or_else(|| {
                                    let (projected, bound) = super::weighted_plane::project_curve(
                                        &curve_cache,
                                        face_plane,
                                        refusal,
                                    )?;
                                    plane_projection_gap = Some(bound.get());
                                    plane_fit_tolerance = Some(bound);
                                    Some((
                                        plane_coedges[0],
                                        projected,
                                        curve_range,
                                        line.byte_offset(),
                                    ))
                                });
                            if let (None, class25::Class25Accuracy::Fitted { fit_tolerance, .. }) =
                                (plane_projection_gap, &map.accuracy)
                            {
                                plane_fit_tolerance =
                                    FitTolerance::try_new(fit_tolerance.get()).ok();
                            }
                        }
                        break;
                    }
                    curve_cache = lifted;
                    curve_range = source_range;
                    if ir.geometry.coedges[plane_coedges[0]].pcurves.is_empty() {
                        plane_pcurve = pcurve.map(|pcurve| {
                            (plane_coedges[0], pcurve, source_range, other.byte_offset())
                        });
                    }
                    break;
                }
                let mut matched_ends = 0;
                for (parameter, vertex_id, stored) in [
                    (
                        curve_range.endpoints()[0],
                        &ends.start,
                        start.position().get(),
                    ),
                    (curve_range.endpoints()[1], &ends.end, end.position().get()),
                ] {
                    let point = cadmpeg_ir::eval::finite_or_refusal(
                        cadmpeg_ir::eval::decode::outer_refusal(
                            cadmpeg_ir::eval::decode::nurbs_curve_point_at(
                                admission.context(),
                                &curve_cache,
                                parameter,
                            ),
                        )?,
                    )?;
                    let Some(point) = point else { break };
                    let point = point.get();
                    let miss = point.distance(stored);
                    let Some(&vertex_index) = vertex_indices.get(vertex_id) else {
                        break;
                    };
                    let existing = ir.geometry.vertices[vertex_index]
                        .tolerance
                        .map_or(ir.tolerances.linear.get(), |t| {
                            t.get().max(ir.tolerances.linear.get())
                        });
                    if miss > existing && !rounds_to_stored(point, stored) {
                        break;
                    }
                    matched_ends += 1;
                }
                if matched_ends != 2 {
                    continue;
                }
                plan = Some(Plan {
                    edge: edge_index,
                    curve: curve_index,
                    procedure: procedure_index,
                    face_pcurves: if add_face_pcurve {
                        vec![(face_coedges[0], line_geometry, interval, line.byte_offset())]
                    } else {
                        Vec::new()
                    },
                    curve_cache,
                    curve_range,
                    plane_pcurve,
                    plane_fit_tolerance,
                    plane_projection_gap,
                });
            }
        }
        if plan.is_none() {
            let candidates: Vec<_> = [reps.target, reps.source]
                .into_iter()
                .zip(ranges)
                .enumerate()
                .filter_map(|(slot, (rep, range))| {
                    let face_tag = occurrences.map(|occ| occ[slot].face_tag);
                    Some((rep?, range, face_tag))
                })
                .collect();
            plan = build_general_lifted_plan(
                &LiftedPlanContext {
                    ctx: admission.context(),
                    ir,
                    data,
                    sources,
                    journal,
                    a5_by_pos: &a5_by_pos,
                    a5_multi_extent_cache: &a5_multi_extent_cache,
                    jets: &jets,
                    planes: &planes,
                    cylinders: &cylinders,
                    cones: &cones,
                    spheres: &spheres,
                    tori: &tori,
                    loop_faces: &loop_faces,
                    faces: &faces,
                    surfaces: &surfaces,
                    edge_coedges: &edge_coedges,
                    vertex_indices: &vertex_indices,
                },
                LiftedEdgeTarget {
                    edge_index,
                    curve_index,
                    procedure_index,
                    start_pos: start.position().get(),
                    end_pos: end.position().get(),
                },
                &candidates,
                refusal,
            )?;
        }
        if let Some(plan) = plan {
            plans.push(plan);
        }
    }
    let count = plans.len();
    let pcurve_count = plans
        .iter()
        .map(|plan| plan.face_pcurves.len() + usize::from(plan.plane_pcurve.is_some()))
        .sum();
    for plan in plans {
        let curve = &mut ir.geometry.curves[plan.curve];
        let CurveGeometry::Procedural { cache, .. } = &mut curve.geometry else {
            continue;
        };
        *cache = Some(SolvedCurveGeometry::Nurbs(plan.curve_cache));
        annotations.derived(&curve.id, "geometry")?;
        ir.geometry.edges[plan.edge].set_param_range(Some(plan.curve_range));
        annotations.derived(&ir.geometry.edges[plan.edge].id, "carrier")?;
        let procedure = &mut ir.geometry.procedural_curves[plan.procedure];
        let ProceduralCurveDefinition::Intersection {
            context,
            discontinuity_flag,
            cache,
        } = procedure.definition()
        else {
            continue;
        };
        let Ok(context) = IntcurveSupportContext::try_new(
            context.sides().clone(),
            plan.curve_range.endpoints(),
            std::array::from_fn(|_| Vec::new()),
        ) else {
            continue;
        };
        procedure.replace_definition(ProceduralCurveDefinition::Intersection {
            context,
            discontinuity_flag: *discontinuity_flag,
            cache: *cache,
        });
        annotations.derived(&procedure.id, "definition")?;
        let face_pcurves = plan.face_pcurves.into_iter().map(|pcurve| {
            (
                pcurve,
                "consolidated_b18",
                "weighted_surface_curve",
                Some(true),
                None,
                None,
            )
        });
        let plane_pcurves = plan.plane_pcurve.into_iter().map(|pcurve| {
            (
                pcurve,
                if plan.plane_projection_gap.is_some() {
                    "consolidated_b18"
                } else {
                    "consolidated_b20"
                },
                if plan.plane_projection_gap.is_some() {
                    "projected_plane_curve"
                } else {
                    "weighted_plane_curve"
                },
                None,
                plan.plane_fit_tolerance,
                plan.plane_projection_gap,
            )
        });
        for (
            (coedge_index, geometry, range, source_pos),
            family,
            role,
            isoparametric,
            fit_tolerance,
            projection_gap,
        ) in face_pcurves.chain(plane_pcurves)
        {
            let id = crate::resource::compose_index_id(
                &cadmpeg_ir::identity_namespace!("catia", "consolidated", "weighted-pcurve"),
                ir.geometry.pcurves.len(),
                cadmpeg_ir::ids::PcurveId::mint,
            )?;
            crate::assemble::annotate(
                annotations,
                &id,
                family,
                cadmpeg_core::decode::u64_from_index(source_pos),
                role,
                cadmpeg_ir::Exactness::Derived,
            )?;
            if let Some(gap) = projection_gap {
                refusal.push_plane_pcurve_projected(source_pos, gap);
            } else if let Some(tolerance) = fit_tolerance {
                refusal.push_parameter_map_pcurve_accuracy(source_pos, tolerance.get());
            }
            admission.charge()?;
            ir.geometry.pcurves.push(Pcurve {
                id: id.clone(),
                geometry,
                metadata: PcurveMetadata::general(
                    None,
                    cadmpeg_ir::units::FiniteVector::new(range.endpoints()),
                    fit_tolerance,
                ),
                native_ref: None,
            });
            ir.geometry.coedges[coedge_index].pcurves.push(PcurveUse {
                pcurve: id,
                isoparametric,
                parameter_range: None,
            });
            annotations.derived(&ir.geometry.coedges[coedge_index].id, "pcurves")?;
        }
    }
    Ok(AttachResult {
        edges: count,
        pcurves: pcurve_count,
    })
}

#[cfg(test)]
mod tests {
    use cadmpeg_ir::math::Point3;

    use crate::families::consolidated::allocation::AllocationJournal;
    use crate::wire::bytes::AllocationReferenceEncoding;
    use crate::wire::records::consolidated_records;

    use super::{curve_record, endpoint_matches, unique_matching_coedge};

    #[test]
    fn endpoint_match_uses_the_stored_binary32_cell_or_declared_tolerance() {
        let stored = Point3::new(1_000_000.0, 0.0, 0.0);
        let rounded = Point3::new(1_000_000.02, 0.0, 0.0);
        let outside = Point3::new(1_000_000.08, 0.0, 0.0);
        assert!(endpoint_matches(rounded, stored, 1.0e-6));
        assert!(!endpoint_matches(outside, stored, 1.0e-6));
        assert!(endpoint_matches(outside, stored, 0.1));
    }

    #[test]
    fn unique_matching_coedge_handles_empty_and_ambiguous_matches_without_panicking() {
        assert_eq!(unique_matching_coedge(&[]), None);
        assert_eq!(unique_matching_coedge(&[7]), Some(7));
        assert_eq!(unique_matching_coedge(&[7, 9]), None);
    }

    #[test]
    fn curve_record_admits_class23_and_class25_maps() {
        let mut bytes = Vec::new();
        // Record 0: A_23 parameter map (family A, class 0x23, 2-byte payload).
        bytes.extend_from_slice(&[0xa5, 0x03, 0x23, 0x02, 0x00, 0x00, 0x00, 0x05, 0x82, 0x00]);
        // Record 1: B_25 parameter map (family B, class 0x25, 2-byte payload).
        bytes.extend_from_slice(&[0xb2, 0x03, 0x25, 0x02, 0x05, 0x82, 0x00]);
        // Record 2: B_5e edge node (family B, class 0x5e, 1-byte payload).
        bytes.extend_from_slice(&[0xb2, 0x03, 0x5e, 0x01, 0x05, 0x00]);
        let records = consolidated_records(&bytes);
        assert_eq!(records.len(), 3);
        let journal = AllocationJournal::new(&records);

        let rec23 = curve_record(
            &journal,
            2,
            2,
            AllocationReferenceEncoding::BackwardDistance,
        )
        .expect("class 0x23 map record should resolve");
        assert_eq!(rec23.class(), 0x23);

        let rec25 = curve_record(
            &journal,
            2,
            1,
            AllocationReferenceEncoding::BackwardDistance,
        )
        .expect("class 0x25 map record should resolve");
        assert_eq!(rec25.class(), 0x25);
    }
}
