// SPDX-License-Identifier: Apache-2.0
//! Native spherical UV jets selected by complete source owner occurrences.

use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use cadmpeg_core::CodecError;
use cadmpeg_ir::annotations::AnnotationStorage;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::pcurve::{Pcurve, PcurveGeometry, PcurveMetadata, PcurveNurbsPoles};
use cadmpeg_ir::geometry::{DirectedParameterRange, SurfaceGeometry};
use cadmpeg_ir::ids::PcurveId;
use cadmpeg_ir::index::{ModelIndex, StandardIndex};
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::topology::{Coedge, PcurveUse, Sense, Vertex};
use cadmpeg_ir::{AnnotationBuilder, Exactness};

use crate::families::b2::records::{
    b2_edge_nodes_from_records, b2_sphere_geometry, b2_spheres_from_records,
};
use crate::families::consolidated::allocation::AllocationJournal;
use crate::families::consolidated::edge_identity::{endpoint_address, source_tag};
use crate::families::consolidated::owner_closure::OwnerClosureIndex;
use crate::families::FamilyEntityAdmission;
use crate::wire::records::{family_pcurves_from_records, ConsolidatedFamily, ConsolidatedRecord};

const OPERATION: &str = "transfer source-owned spherical pcurves";
const STREAM: &str = "consolidated_a20";
const TAG: &str = "source_owned_sphere_jet";
const GENERATED_TAG: &str = "derived_surface_parameter_curve";

#[derive(Default)]
pub(in crate::families) struct AttachResult {
    pub(in crate::families) pcurves: usize,
    pub(in crate::families) unresolved_source_uses: usize,
    pub(in crate::families) unresolved_identity_collisions: usize,
    pub(in crate::families) unresolved_existing_uses: usize,
}

#[derive(Clone)]
struct Plan {
    coedge: usize,
    pcurve: Pcurve,
    directed: DirectedParameterRange,
    source_offset: u64,
    use_slots: Vec<PcurveUse>,
    replaced_pcurve: Option<PcurveId>,
}

fn replaceable(coedge: &Coedge, annotations: &cadmpeg_ir::Annotations) -> bool {
    coedge.pcurves.len() <= 1
        && (coedge.pcurves.is_empty()
            || annotations
                .exactness()
                .get(coedge.id.as_str())
                .is_some_and(|note| {
                    note.fields().get("pcurves").copied() == Some(Exactness::Derived)
                }))
        && coedge.pcurves.iter().all(|usage| {
            usage.isoparametric.is_none()
                && annotations
                    .provenance
                    .get(usage.pcurve.as_str())
                    .is_some_and(|p| {
                        p.tag.as_deref() == Some(GENERATED_TAG)
                            && p.stream() == "catia:MainDataStream+SurfacicReps"
                    })
                && annotations
                    .exactness()
                    .get(usage.pcurve.as_str())
                    .is_some_and(|note| {
                        note.entity() == Exactness::Derived
                            && note.fields().get("geometry").copied() == Some(Exactness::Derived)
                    })
        })
}

fn endpoint_matches(point: Point3, stored: Point3, tolerance: f64) -> bool {
    if point.distance(stored) <= tolerance {
        return true;
    }
    let mut outside = [0.0; 3];
    for (distance, (actual, expected)) in outside.iter_mut().zip([
        (point.x, stored.x),
        (point.y, stored.y),
        (point.z, stored.z),
    ]) {
        let Some(rounded) = cadmpeg_core::convert::f32_from_f64(expected) else {
            return false;
        };
        if f64::from(rounded) != expected {
            return false;
        }
        let lower = f64::from(rounded.next_down()).midpoint(expected);
        let upper = f64::from(rounded.next_up()).midpoint(expected);
        if !lower.is_finite() || !upper.is_finite() {
            return false;
        }
        *distance = (lower - actual).max(actual - upper).max(0.0);
    }
    outside[0].hypot(outside[1]).hypot(outside[2]) <= tolerance
}

fn directed_range(
    ctx: &DecodeContext<'_>,
    geometry: &PcurveGeometry,
    surface: &SurfaceGeometry,
    range: [f64; 2],
    endpoints: [(Point3, f64); 2],
    sense: Sense,
) -> Result<Option<DirectedParameterRange>, CodecError> {
    let mut lifted = [None; 2];
    for (at, parameter) in range.into_iter().enumerate() {
        let Some(uv) =
            cadmpeg_ir::eval::finite_or_refusal(cadmpeg_ir::eval::decode::outer_refusal(
                cadmpeg_ir::eval::decode::pcurve_uv(ctx, geometry, parameter),
            )?)?
        else {
            return Ok(None);
        };
        lifted[at] = cadmpeg_ir::eval::finite_or_refusal(cadmpeg_ir::eval::decode::outer_refusal(
            cadmpeg_ir::eval::decode::surface_point(ctx, surface, uv.get().u, uv.get().v),
        )?)?
        .map(cadmpeg_ir::features::FinitePoint3::get);
    }
    let [Some(first), Some(second)] = lifted else {
        return Ok(None);
    };
    let forward = endpoint_matches(first, endpoints[0].0, endpoints[0].1)
        && endpoint_matches(second, endpoints[1].0, endpoints[1].1);
    let reversed = endpoint_matches(second, endpoints[0].0, endpoints[0].1)
        && endpoint_matches(first, endpoints[1].0, endpoints[1].1);
    if forward == reversed {
        return Ok(None);
    }
    let mut directed = range;
    if reversed ^ (sense == Sense::Reversed) {
        directed.reverse();
    }
    Ok(Some(
        DirectedParameterRange::new(directed).map_err(CodecError::malformed)?,
    ))
}

fn vertex_point(index: &ModelIndex<'_>, vertex: &Vertex, linear: f64) -> Option<(Point3, f64)> {
    let point = index
        .points(vertex.point.as_str(), StandardIndex)?
        .position()
        .get();
    let tolerance = vertex.tolerance.map_or(linear, |t| t.get().max(linear));
    Some((point, tolerance))
}

fn retained_size(
    plan: &Plan,
    annotations: &cadmpeg_ir::Annotations,
    coedge: &Coedge,
) -> Result<usize, CodecError> {
    let PcurveGeometry::Nurbs { nurbs } = &plan.pcurve.geometry else {
        return Err(CodecError::malformed(
            "source jet lost its NURBS representation",
        ));
    };
    let mut bytes = std::mem::size_of::<Pcurve>();
    let mut add = |count: usize, width: usize| -> Result<(), CodecError> {
        bytes = count
            .checked_mul(width)
            .and_then(|n| bytes.checked_add(n))
            .ok_or_else(|| CodecError::malformed("source pcurve retention exceeds its lane"))?;
        Ok(())
    };
    let id_length = plan.pcurve.id.as_str().len();
    add(4, id_length)?;
    add(nurbs.knots().as_slice().len(), std::mem::size_of::<f64>())?;
    match nurbs.pole_rows() {
        PcurveNurbsPoles::Polynomial { points } => add(
            points.len(),
            std::mem::size_of::<cadmpeg_ir::units::FinitePoint2>(),
        )?,
        PcurveNurbsPoles::Rational { points } => add(
            points.len(),
            std::mem::size_of::<
                cadmpeg_ir::geometry::pcurve::WeightedPole2<cadmpeg_ir::units::FinitePoint2>,
            >(),
        )?,
    }
    add(
        1,
        std::mem::size_of::<(String, cadmpeg_ir::AnnotationProvenance)>(),
    )?;
    add(
        1,
        std::mem::size_of::<cadmpeg_ir::StreamName>()
            + std::mem::size_of::<usize>() * 2
            + "catia:".len()
            + STREAM.len()
            + TAG.len(),
    )?;
    add(
        1,
        std::mem::size_of::<(String, cadmpeg_ir::annotations::ExactnessNote)>(),
    )?;
    add(
        1,
        std::mem::size_of::<(cadmpeg_ir::annotations::FieldName, Exactness)>() + "geometry".len(),
    )?;
    if coedge.pcurves.is_empty() {
        add(1, std::mem::size_of::<PcurveUse>())?;
    }
    let old = annotations.exactness().get(coedge.id.as_str());
    if old.is_none() {
        add(
            1,
            coedge.id.as_str().len()
                + std::mem::size_of::<(String, cadmpeg_ir::annotations::ExactnessNote)>(),
        )?;
    }
    if old.is_none_or(|note| !note.fields().contains_key("pcurves")) {
        add(
            1,
            std::mem::size_of::<(cadmpeg_ir::annotations::FieldName, Exactness)>()
                + "pcurves".len(),
        )?;
    }
    Ok(bytes)
}

pub(in crate::families) fn attach(
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder<impl AnnotationStorage>,
    data: &[u8],
    records: &[ConsolidatedRecord],
    refusal: &mut crate::nurbs::LaneRefusals,
    admission: &mut FamilyEntityAdmission<'_, '_>,
) -> Result<AttachResult, CodecError> {
    let mut result = AttachResult::default();
    if ir.model.coedges.is_empty()
        || !records
            .iter()
            .any(|r| r.family() == ConsolidatedFamily::A && r.class() == 0x20)
    {
        return Ok(result);
    }
    let ctx = admission.context();
    let journal = AllocationJournal::new(records);
    let owners = OwnerClosureIndex::new(data, records, &journal);
    let count = records
        .iter()
        .filter(|r| r.family() == ConsolidatedFamily::B && r.class() == 0x5e)
        .count();
    let mut nodes = ctx.alloc_filled(count, None, OPERATION)?;
    for (slot, (at, record)) in nodes.iter_mut().zip(
        records
            .iter()
            .enumerate()
            .filter(|(_, r)| r.family() == ConsolidatedFamily::B && r.class() == 0x5e),
    ) {
        if let (Some(tag), Some(node)) = (
            journal.absolute_address(at),
            b2_edge_nodes_from_records(data, std::slice::from_ref(record)).next(),
        ) {
            *slot = Some((tag, at, node));
        }
    }
    nodes.sort_unstable_by_key(|node| node.as_ref().map(|(tag, _, _)| *tag));
    let mut coedges =
        ctx.alloc_filled(ir.model.coedges.len(), (0, &ir.model.coedges[0]), OPERATION)?;
    for (slot, value) in coedges.iter_mut().zip(ir.model.coedges.iter().enumerate()) {
        *slot = value;
    }
    coedges.sort_unstable_by(|a, b| a.1.edge.cmp(&b.1.edge));
    let mut plans = ctx.alloc_filled(ir.model.coedges.len(), None, OPERATION)?;
    let mut used = 0;
    {
        let model = ModelIndex::new_model_only(ir);
        for edge in &ir.model.edges {
            let Some(curve) = edge
                .curve()
                .and_then(|id| model.curves(id.as_str(), StandardIndex))
            else {
                continue;
            };
            let Some(tag) = source_tag(curve.source_object.as_ref(), "edge-support") else {
                continue;
            };
            let lower = nodes.partition_point(|node| node.as_ref().map(|x| x.0) < Some(tag));
            let upper = nodes.partition_point(|node| node.as_ref().map(|x| x.0) <= Some(tag));

            if upper.checked_sub(lower) != Some(1) {
                continue;
            }
            let Some((_, node_index, node)) = &nodes[lower] else {
                continue;
            };
            let Some(start) = model.vertices(edge.start.as_str(), StandardIndex) else {
                continue;
            };
            let Some(end) = model.vertices(edge.end.as_str(), StandardIndex) else {
                continue;
            };
            let (Some(a), Some(b), Some(start_tag), Some(end_tag)) = (
                endpoint_address(
                    &journal,
                    *node_index,
                    node.start_vertex_ref,
                    node.reference_encodings[1],
                ),
                endpoint_address(
                    &journal,
                    *node_index,
                    node.end_vertex_ref,
                    node.reference_encodings[2],
                ),
                model
                    .points(start.point.as_str(), StandardIndex)
                    .and_then(|p| source_tag(p.source_object.as_ref(), "vertex")),
                model
                    .points(end.point.as_str(), StandardIndex)
                    .and_then(|p| source_tag(p.source_object.as_ref(), "vertex")),
            ) else {
                continue;
            };

            if !((a == start_tag && b == end_tag) || (a == end_tag && b == start_tag)) {
                continue;
            }
            let first = coedges.partition_point(|(_, c)| c.edge < edge.id);
            let last = coedges.partition_point(|(_, c)| c.edge <= edge.id);

            let Ok(sides) = <&[(usize, &Coedge); 2]>::try_from(&coedges[first..last]) else {
                continue;
            };
            let side_surfaces = sides.map(|(_, coedge)| {
                let owner = model.loops(coedge.owner_loop.as_str(), StandardIndex)?;
                let face = model.faces(owner.face.as_str(), StandardIndex)?;
                model.surfaces(face.surface.as_str(), StandardIndex)
            });
            let [Some(first_surface), Some(second_surface)] = side_surfaces else {
                continue;
            };
            let (Some(first_tag), Some(second_tag)) = (
                source_tag(first_surface.source_object.as_ref(), "carrier"),
                source_tag(second_surface.source_object.as_ref(), "carrier"),
            ) else {
                continue;
            };

            let Some(occurrences) = owners.edge_occurrences(*node_index, [first_tag, second_tag])
            else {
                continue;
            };

            let (Some(first_point), Some(second_point)) = (
                vertex_point(&model, start, ir.tolerances.linear.get()),
                vertex_point(&model, end, ir.tolerances.linear.get()),
            ) else {
                continue;
            };
            for occurrence in occurrences {
                let side = usize::from(occurrence.face_tag == second_tag);
                let (coedge_index, coedge) = sides[side];
                let Some(representation) = journal.record(occurrence.representation_index) else {
                    continue;
                };

                if representation.family() != ConsolidatedFamily::A
                    || representation.class() != 0x20
                {
                    continue;
                }
                let Some(carrier) = journal.record(occurrence.carrier_index) else {
                    continue;
                };
                if carrier.family() != ConsolidatedFamily::B || carrier.class() != 0x2a {
                    continue;
                }
                let Some(sphere) =
                    b2_spheres_from_records(data, std::slice::from_ref(carrier)).next()
                else {
                    result.unresolved_source_uses += 1;
                    continue;
                };
                let surface = if side == 0 {
                    first_surface
                } else {
                    second_surface
                };

                if surface.geometry != b2_sphere_geometry(&sphere) {
                    result.unresolved_source_uses += 1;
                    continue;
                }
                let Some(jet) = family_pcurves_from_records(
                    data,
                    std::slice::from_ref(representation),
                    ConsolidatedFamily::A,
                )
                .into_iter()
                .next() else {
                    result.unresolved_source_uses += 1;
                    continue;
                };
                let Some(geometry) = super::consolidated_jet_pcurve(
                    &jet,
                    &super::ConsolidatedCarrierChart::Sphere {
                        radius: sphere.radius.get(),
                    },
                    refusal,
                ) else {
                    result.unresolved_source_uses += 1;
                    continue;
                };
                let range = jet.range.endpoints();
                let Some(fit) = source_map::fit_tolerance(
                    data,
                    &journal,
                    *node_index,
                    node,
                    source_map::Selection {
                        representations: occurrences.map(|owner| owner.representation_index),
                        selected: occurrence.representation_index,
                        range,
                    },
                ) else {
                    result.unresolved_source_uses += 1;
                    continue;
                };
                let endpoints =
                    [first_point, second_point].map(|(point, tolerance)| (point, tolerance + fit));
                if endpoints
                    .iter()
                    .any(|(_, tolerance)| !tolerance.is_finite())
                {
                    result.unresolved_source_uses += 1;
                    continue;
                }

                let Some(directed) = directed_range(
                    ctx,
                    &geometry,
                    &surface.geometry,
                    range,
                    endpoints,
                    coedge.sense,
                )?
                else {
                    result.unresolved_source_uses += 1;
                    continue;
                };

                let id = PcurveId::mint(format!(
                    "catia:consolidated:owner-pcurve#{}:{}:{}",
                    representation.source_index(),
                    representation.source_range().start,
                    occurrence.owner_index
                ))
                .map_err(CodecError::malformed)?;
                let pcurve = Pcurve {
                    id: id.clone(),
                    geometry,
                    metadata: PcurveMetadata::general(
                        None,
                        cadmpeg_ir::units::FiniteVector::new(range),
                        None,
                    ),
                };
                if matches!(coedge.pcurves.as_slice(), [usage]
                    if usage.pcurve == id && usage.isoparametric.is_none()
                    && usage.parameter_range == Some(directed))
                    && model.pcurves(id.as_str(), StandardIndex) == Some(&pcurve)
                {
                    continue;
                }
                if model.contains(id.as_str(), StandardIndex)
                    || annotations
                        .annotations()
                        .provenance
                        .contains_key(id.as_str())
                    || annotations
                        .annotations()
                        .exactness()
                        .contains_key(id.as_str())
                    || plans[..used]
                        .iter()
                        .flatten()
                        .any(|p: &Plan| p.pcurve.id == id)
                {
                    result.unresolved_identity_collisions += 1;
                    continue;
                }

                if !replaceable(coedge, annotations.annotations()) {
                    result.unresolved_existing_uses += 1;
                    continue;
                }
                let replaced_pcurve = coedge.pcurves.first().map(|usage| usage.pcurve.clone());
                if replaced_pcurve.as_ref().is_some_and(|replaced| {
                    ir.model
                        .coedges
                        .iter()
                        .flat_map(|candidate| &candidate.pcurves)
                        .filter(|usage| usage.pcurve == *replaced)
                        .count()
                        != 1
                }) {
                    result.unresolved_existing_uses += 1;
                    continue;
                }
                let use_slots = if coedge.pcurves.is_empty() {
                    ctx.alloc_filled(
                        1,
                        PcurveUse {
                            pcurve: id,
                            isoparametric: None,
                            parameter_range: Some(directed),
                        },
                        OPERATION,
                    )?
                } else {
                    Vec::new()
                };
                plans[used] = Some(Plan {
                    coedge: coedge_index,
                    pcurve,
                    directed,
                    source_offset: u64_from_index(representation.byte_offset()),
                    use_slots,
                    replaced_pcurve,
                });
                used += 1;
            }
        }
    }
    for plan in plans[..used].iter().flatten() {
        ctx.retain_bytes(
            u64_from_index(retained_size(
                plan,
                annotations.annotations(),
                &ir.model.coedges[plan.coedge],
            )?),
            OPERATION,
        )?;
    }
    for _ in 0..used {
        admission.charge()?;
    }
    for plan in plans.into_iter().take(used).flatten() {
        if let Some(replaced_pcurve) = plan.replaced_pcurve {
            ir.model
                .pcurves
                .retain(|pcurve| pcurve.id != replaced_pcurve);
            annotations.remove_entity(replaced_pcurve.as_str());
        }
        crate::assemble::annotate(
            annotations,
            &plan.pcurve.id,
            STREAM,
            plan.source_offset,
            TAG,
            Exactness::Derived,
        )?;
        annotations.derived(&plan.pcurve.id, "geometry")?;
        let coedge = &mut ir.model.coedges[plan.coedge];
        if coedge.pcurves.is_empty() {
            coedge.pcurves = plan.use_slots;
        } else {
            coedge.pcurves[0] = PcurveUse {
                pcurve: plan.pcurve.id.clone(),
                isoparametric: None,
                parameter_range: Some(plan.directed),
            };
        }
        annotations.derived(&coedge.id, "pcurves")?;
        ir.model.pcurves.push(plan.pcurve);
    }
    result.pcurves = used;
    Ok(result)
}

mod source_map;

#[cfg(test)]
mod tests;
