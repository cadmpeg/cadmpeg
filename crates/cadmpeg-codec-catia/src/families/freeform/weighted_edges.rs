// SPDX-License-Identifier: Apache-2.0
//! Standard edge curves selected by consolidated allocation identities.

use std::collections::HashMap;

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::annotations::AnnotationStorage;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::nurbs::{NurbsCurve, SurfaceParameterAxis};
use cadmpeg_ir::geometry::pcurve::{LinePcurve, Pcurve, PcurveGeometry, PcurveMetadata};
use cadmpeg_ir::geometry::{
    CurveGeometry, IntcurveSupportContext, ProceduralCurveDefinition, SolvedCurveGeometry,
    SolvedSurfaceGeometry, SurfaceGeometry,
};
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::topology::{ParameterInterval, PcurveUse};
use cadmpeg_ir::AnnotationBuilder;

use super::{ConsolidatedCarrierChart, FamilyEntityAdmission};
use crate::families::b2::records::{
    b2_edge_nodes_from_records, b2_plane_carriers_from_records, b2_plane_geometry,
};
use crate::families::consolidated::a5_chart::{chart_side_payload, ChartAxis};
use crate::families::consolidated::allocation::AllocationJournal;
use crate::families::consolidated::owner_closure::OwnerClosureIndex;
use crate::families::consolidated::parameter_representation::{
    b18_support, class25_representations,
};
use crate::families::consolidated::records::class25::{self, Class25SourceRelation};
use crate::wire::bytes::AllocationReferenceEncoding;
use crate::wire::compact_unsigned::CompactUnsigned;
use crate::wire::records::{ConsolidatedFamily, ConsolidatedRecord};

const PLANE_OFFSET_TOLERANCE: f64 = 2.0e-3;
const PARAMETER_TOLERANCE: f64 = 1.0e-9;
const PLANE_NORMAL_TOLERANCE: f64 = 8.0 / 8_388_608.0;

struct Plan {
    edge: usize,
    curve: usize,
    procedure: usize,
    face_pcurves: Vec<(usize, PcurveGeometry, ParameterInterval, usize)>,
    curve_cache: NurbsCurve,
    curve_range: ParameterInterval,
    plane_pcurve: Option<(usize, PcurveGeometry, ParameterInterval, usize)>,
    vertex_misses: [(usize, f64); 2],
}

#[derive(Default)]
pub(in crate::families) struct AttachResult {
    pub(in crate::families) edges: usize,
    pub(in crate::families) pcurves: usize,
    pub(in crate::families) widened_vertices: usize,
    pub(in crate::families) max_vertex_widening: f64,
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
        | AllocationReferenceEncoding::TaggedU16 => {
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
    let record = match encoding {
        AllocationReferenceEncoding::WidthCoded => journal.resolve_weighted_distance(origin, value),
        AllocationReferenceEncoding::BackwardDistance => {
            journal.resolve_record_distance(origin, value)
        }
        AllocationReferenceEncoding::OwnedChild => journal.resolve_owned_child(origin, value),
        AllocationReferenceEncoding::Selector2
        | AllocationReferenceEncoding::TaggedU8
        | AllocationReferenceEncoding::TaggedU16 => None,
    }?;
    (record.family() == ConsolidatedFamily::B && record.class() == 0x25).then_some(record)
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

pub(in crate::families) fn attach(
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder<impl AnnotationStorage>,
    data: &[u8],
    records: &[ConsolidatedRecord],
    freeform_surfaces: &[crate::families::a5a8::records::FreeformSurface],
    refusal: &mut crate::nurbs::LaneRefusals,
    admission: &mut FamilyEntityAdmission<'_, '_>,
) -> Result<AttachResult, CodecError> {
    if freeform_surfaces.is_empty() {
        return Ok(AttachResult::default());
    }
    let journal = AllocationJournal::new(records);
    let mut nodes = HashMap::new();
    for (index, record) in records.iter().enumerate() {
        if record.family() != ConsolidatedFamily::B || record.class() != 0x5e {
            continue;
        }
        let Some(tag) = journal.absolute_address(index) else {
            continue;
        };
        let Some(node) = b2_edge_nodes_from_records(data, std::slice::from_ref(record)).next()
        else {
            continue;
        };
        nodes
            .entry(tag)
            .and_modify(|entry| *entry = None)
            .or_insert(Some((index, node)));
    }
    if nodes.is_empty() {
        return Ok(AttachResult::default());
    }
    let owners = OwnerClosureIndex::new(data, records, &journal);
    let jets: HashMap<_, _> =
        crate::wire::records::family_pcurves_from_records(data, records, ConsolidatedFamily::B)
            .into_iter()
            .map(|jet| (jet.pos, jet))
            .collect();
    let planes: HashMap<_, _> = b2_plane_carriers_from_records(data, records)
        .into_iter()
        .map(|plane| (plane.pos, plane))
        .collect();
    let curve_indices: HashMap<_, _> = ir
        .model
        .curves
        .iter()
        .enumerate()
        .map(|(i, c)| (c.id.clone(), i))
        .collect();
    let procedure_indices: HashMap<_, _> = ir
        .model
        .procedural_curves
        .iter()
        .enumerate()
        .map(|(index, procedure)| (procedure.id.clone(), index))
        .collect();
    let points: HashMap<_, _> = ir.model.points.iter().map(|p| (p.id.clone(), p)).collect();
    let vertices: HashMap<_, _> = ir
        .model
        .vertices
        .iter()
        .filter_map(|v| Some((v.id.clone(), *points.get(&v.point)?)))
        .collect();
    let vertex_indices: HashMap<_, _> = ir
        .model
        .vertices
        .iter()
        .enumerate()
        .map(|(i, vertex)| (vertex.id.clone(), i))
        .collect();
    let loop_faces: HashMap<_, _> = ir
        .model
        .loops
        .iter()
        .map(|l| (l.id.clone(), l.face.clone()))
        .collect();
    let faces: HashMap<_, _> = ir.model.faces.iter().map(|f| (f.id.clone(), f)).collect();
    let surfaces: HashMap<_, _> = ir
        .model
        .surfaces
        .iter()
        .map(|s| (s.id.clone(), s))
        .collect();
    let mut edge_coedges: HashMap<_, Vec<usize>> = HashMap::new();
    for (index, coedge) in ir.model.coedges.iter().enumerate() {
        edge_coedges
            .entry(coedge.edge.clone())
            .or_default()
            .push(index);
    }
    let mut plans = Vec::new();
    for (edge_index, edge) in ir.model.edges.iter().enumerate() {
        let Some(curve_index) = edge.curve().and_then(|id| curve_indices.get(id)).copied() else {
            continue;
        };
        let curve = &ir.model.curves[curve_index];
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
            ir.model.procedural_curves[procedure_index].definition()
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
        let Some(Some((node_index, node))) = nodes.get(&tag) else {
            continue;
        };
        let Some(start) = vertices.get(&edge.start) else {
            continue;
        };
        let Some(end) = vertices.get(&edge.end) else {
            continue;
        };
        let Some(start_tag) = source_tag(start.source_object.as_ref(), "vertex") else {
            continue;
        };
        let Some(end_tag) = source_tag(end.source_object.as_ref(), "vertex") else {
            continue;
        };
        let Some(a) = endpoint_address(
            &journal,
            *node_index,
            node.start_vertex_ref,
            node.reference_encodings[1],
        ) else {
            continue;
        };
        let Some(b) = endpoint_address(
            &journal,
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
            &journal,
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
        let Some(map) = map_record
            .payload()
            .and_then(|range| data.get(range))
            .and_then(class25::parse)
        else {
            continue;
        };
        let reps = class25_representations(&journal, map_index, &map);
        let [Some(first), Some(second)] = [reps.target, reps.source] else {
            continue;
        };
        let mut representation_indices = Vec::new();
        let mut carrier_indices = Vec::new();
        for representation in [first, second] {
            let Some(index) = journal.index(
                representation.source_index(),
                representation.source_range().start,
            ) else {
                break;
            };
            let Some(payload) = representation.payload().and_then(|range| data.get(range)) else {
                break;
            };
            let mut at = 0;
            let Some(operand) = CompactUnsigned::read(payload, &mut at) else {
                break;
            };
            let Some(carrier) = journal.resolve_weighted_distance(index, operand.value()) else {
                break;
            };
            let Some(carrier_index) =
                journal.index(carrier.source_index(), carrier.source_range().start)
            else {
                break;
            };
            representation_indices.push(index);
            carrier_indices.push(carrier_index);
        }
        let (Ok(representation_indices), Ok(carrier_indices)) = (
            <[usize; 2]>::try_from(representation_indices),
            <[usize; 2]>::try_from(carrier_indices),
        ) else {
            continue;
        };
        let face_tags: Vec<_> = edge_coedges
            .get(&edge.id)
            .into_iter()
            .flatten()
            .filter_map(|index| {
                let coedge = &ir.model.coedges[*index];
                let surface = loop_faces
                    .get(&coedge.owner_loop)
                    .and_then(|face| faces.get(face))
                    .and_then(|face| surfaces.get(&face.surface))?;
                source_tag(surface.source_object.as_ref(), "carrier")
            })
            .collect();
        let Ok(face_tags) = <[u64; 2]>::try_from(face_tags) else {
            continue;
        };
        let Some(occurrences) = owners.bind(
            *node_index,
            representation_indices,
            carrier_indices,
            face_tags,
        ) else {
            continue;
        };
        let ranges = [
            Some(map.target_range.endpoints()),
            match &map.source {
                Class25SourceRelation::Affine { range, .. }
                | Class25SourceRelation::Sampled { range, .. } => Some(range.endpoints()),
                Class25SourceRelation::Absent => None,
            },
        ];
        let mut plan: Option<Plan> = None;
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
            let Some(line_index) = journal.index(line.source_index(), line.source_range().start)
            else {
                continue;
            };
            let Some(carrier) = b18_support(data, &journal, line_index) else {
                continue;
            };
            if carrier.family() != ConsolidatedFamily::A || carrier.class() != 0x34 {
                continue;
            }
            let Some(surface) = freeform_surfaces
                .iter()
                .find(|s| s.pos == carrier.byte_offset())
            else {
                continue;
            };
            let face_coedges: Vec<_> = edge_coedges
                .get(&edge.id)
                .into_iter()
                .flatten()
                .copied()
                .filter(|i| {
                    let c = &ir.model.coedges[*i];
                    loop_faces
                        .get(&c.owner_loop)
                        .and_then(|f| faces.get(f))
                        .and_then(|f| surfaces.get(&f.surface))
                        .is_some_and(|s| {
                            source_tag(s.source_object.as_ref(), "carrier")
                                == Some(occurrence.face_tag)
                                && s.geometry
                                    == SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(
                                        surface.geometry.clone(),
                                    ))
                        })
                })
                .collect();
            if face_coedges.len() != 1 {
                continue;
            }
            let add_face_pcurve = ir.model.coedges[face_coedges[0]].pcurves.is_empty();
            let Some(&start_index) = vertex_indices.get(&edge.start) else {
                continue;
            };
            let Some(&end_index) = vertex_indices.get(&edge.end) else {
                continue;
            };
            let endpoint_tolerances = [start_index, end_index].map(|index| {
                ir.model.vertices[index]
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
                &surface.geometry,
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
                let Some(support) = journal.resolve_weighted_distance(other_index, jet.support_id)
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
                        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane)) => Some(plane),
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
                        let c = &ir.model.coedges[*i];
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
                let coedge = &ir.model.coedges[plane_coedges[0]];
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
                let geometry = CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(lifted.clone()));
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
                curve_cache = lifted;
                curve_range = source_range;
                if ir.model.coedges[plane_coedges[0]].pcurves.is_empty() {
                    plane_pcurve = pcurve.map(|pcurve| {
                        (plane_coedges[0], pcurve, source_range, other.byte_offset())
                    });
                }
                break;
            }
            let mut endpoint_misses = Vec::new();
            for (parameter, vertex_id, stored) in [
                (
                    curve_range.endpoints()[0],
                    &edge.start,
                    start.position().get(),
                ),
                (curve_range.endpoints()[1], &edge.end, end.position().get()),
            ] {
                let point =
                    cadmpeg_ir::eval::finite_or_refusal(cadmpeg_ir::eval::decode::outer_refusal(
                        cadmpeg_ir::eval::decode::nurbs_curve_point_at(
                            admission.context(),
                            &curve_cache,
                            parameter,
                        ),
                    )?)?;
                let Some(point) = point else { break };
                let point = point.get();
                let miss = point.distance(stored);
                let Some(&vertex_index) = vertex_indices.get(vertex_id) else {
                    break;
                };
                let existing = ir.model.vertices[vertex_index]
                    .tolerance
                    .map_or(ir.tolerances.linear.get(), |t| {
                        t.get().max(ir.tolerances.linear.get())
                    });
                if miss > existing && !rounds_to_stored(point, stored) {
                    break;
                }
                endpoint_misses.push((vertex_index, miss));
            }
            let Ok(vertex_misses) = <[(usize, f64); 2]>::try_from(endpoint_misses) else {
                continue;
            };
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
                vertex_misses,
            });
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
    let mut vertex_misses = HashMap::<usize, f64>::new();
    for plan in plans {
        for (vertex, miss) in plan.vertex_misses {
            vertex_misses
                .entry(vertex)
                .and_modify(|old| *old = old.max(miss))
                .or_insert(miss);
        }
        let curve = &mut ir.model.curves[plan.curve];
        let CurveGeometry::Procedural { cache, .. } = &mut curve.geometry else {
            continue;
        };
        *cache = Some(SolvedCurveGeometry::Nurbs(plan.curve_cache));
        annotations.derived(&curve.id, "geometry")?;
        ir.model.edges[plan.edge].set_param_range(Some(plan.curve_range));
        annotations.derived(&ir.model.edges[plan.edge].id, "carrier")?;
        let procedure = &mut ir.model.procedural_curves[plan.procedure];
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
            )
        });
        let plane_pcurves = plan
            .plane_pcurve
            .into_iter()
            .map(|pcurve| (pcurve, "consolidated_b20", "weighted_plane_curve", None));
        for ((coedge_index, geometry, range, source_pos), family, role, isoparametric) in
            face_pcurves.chain(plane_pcurves)
        {
            let id = crate::resource::compose_index_id(
                &cadmpeg_ir::identity_namespace!("catia", "consolidated", "weighted-pcurve"),
                ir.model.pcurves.len(),
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
            admission.charge()?;
            ir.model.pcurves.push(Pcurve {
                id: id.clone(),
                geometry,
                metadata: PcurveMetadata::general(
                    None,
                    cadmpeg_ir::units::FiniteVector::new(range.endpoints()),
                    None,
                ),
            });
            ir.model.coedges[coedge_index].pcurves.push(PcurveUse {
                pcurve: id,
                isoparametric,
                parameter_range: None,
            });
            annotations.derived(&ir.model.coedges[coedge_index].id, "pcurves")?;
        }
    }
    let mut widened_vertices = 0;
    let mut max_vertex_widening = 0.0_f64;
    for (index, miss) in vertex_misses {
        let vertex = &mut ir.model.vertices[index];
        let previous = vertex
            .tolerance
            .map_or(0.0, cadmpeg_ir::scalar::PositiveReal::get);
        if miss <= previous.max(ir.tolerances.linear.get()) {
            continue;
        }
        let Some(tolerance) = cadmpeg_ir::scalar::PositiveReal::new(miss.next_up()) else {
            continue;
        };
        vertex.tolerance = Some(tolerance);
        annotations.derived(&vertex.id, "tolerance")?;
        widened_vertices += 1;
        max_vertex_widening = max_vertex_widening.max(tolerance.get() - previous);
    }
    Ok(AttachResult {
        edges: count,
        pcurves: pcurve_count,
        widened_vertices,
        max_vertex_widening,
    })
}

#[cfg(test)]
mod tests {
    use cadmpeg_ir::math::Point3;

    use super::endpoint_matches;

    #[test]
    fn endpoint_match_uses_the_stored_binary32_cell_or_declared_tolerance() {
        let stored = Point3::new(1_000_000.0, 0.0, 0.0);
        let rounded = Point3::new(1_000_000.02, 0.0, 0.0);
        let outside = Point3::new(1_000_000.08, 0.0, 0.0);
        assert!(endpoint_matches(rounded, stored, 1.0e-6));
        assert!(!endpoint_matches(outside, stored, 1.0e-6));
        assert!(endpoint_matches(outside, stored, 0.1));
    }
}
