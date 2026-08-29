// SPDX-License-Identifier: Apache-2.0
//! Zero-entity decode route for independently complete geometry carriers.

use std::collections::{BTreeMap, HashMap};

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, IntcurveSupportContext, IntcurveSupportSide, PcurveGeometry,
    ProceduralCurve, ProceduralCurveDefinition, Surface, SurfaceCurveFamily,
};
use cadmpeg_ir::ids::{
    BodyId, CurveId, EdgeId, PointId, ProceduralCurveId, RegionId, ShellId, SurfaceId, VertexId,
};
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::report::DecodeReport;
use cadmpeg_ir::topology::{Body, BodyKind, Edge, Point, Region, Shell, Vertex};
use cadmpeg_ir::units::Units;
use cadmpeg_ir::AnnotationBuilder;
use cadmpeg_ir::Exactness;

use crate::assemble::{
    annotate, link_payload_carriers, neutral_model_is_admissible, preserve_raw_payload, source_meta,
};
use crate::container::{self, ContainerScan};
use crate::families::FamilyOutput;
use crate::loss::CatiaLossCode;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct WireTransferCounts {
    bodies: usize,
    owned_bodies: usize,
    loops: usize,
    edges: usize,
    vertices: usize,
    points: usize,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct WireCurveOrientation {
    reversed: bool,
    source_range: [f64; 2],
    edge_range: [f64; 2],
}

#[derive(Debug, Clone)]
struct WireSourceProcedural {
    construction_id: ProceduralCurveId,
    definition: ProceduralCurveDefinition,
    cache_fit_tolerance: Option<f64>,
}

type ClosedWireMember<'a> = (
    &'a crate::families::zero_entity::records::ZeroEntitySupportOccurrence,
    CurveId,
    [Point3; 2],
    bool,
    Option<[f64; 2]>,
);

const ZERO_ENTITY_WIRE_TOLERANCE: f64 = 2e-3;

fn finite_point(point: Point3) -> bool {
    [point.x, point.y, point.z].into_iter().all(f64::is_finite)
}

fn closed_wire_loop_members<'a>(
    run: &'a crate::families::zero_entity::records::ZeroEntitySupportRun,
    loop_record: &'a crate::families::zero_entity::records::ZeroEntityLoop,
    support_curve_ids: &'a HashMap<u32, CurveId>,
) -> Option<Vec<ClosedWireMember<'a>>> {
    let member_count = loop_record.support_record_ordinals.len();
    if member_count == 0
        || loop_record.forward_senses.len() != member_count
        || loop_record.oriented_model_endpoints.len() != member_count
    {
        return None;
    }
    let supports_by_ordinal = run
        .supports
        .iter()
        .map(|support| (support.record_ordinal, support))
        .collect::<HashMap<_, _>>();
    let members = loop_record
        .support_record_ordinals
        .iter()
        .zip(&loop_record.oriented_model_endpoints)
        .zip(&loop_record.forward_senses)
        .map(|((record_ordinal, endpoints), forward)| {
            let support = *supports_by_ordinal.get(record_ordinal)?;
            let curve = support_curve_ids.get(record_ordinal)?.clone();
            support.model_endpoints?;
            let parameter_range = support.model_parameters.filter(|parameters| {
                parameters.iter().all(|value| value.is_finite()) && parameters[0] != parameters[1]
            });
            Some((support, curve, *endpoints, *forward, parameter_range))
        })
        .collect::<Option<Vec<_>>>();
    let members = members?;
    if members
        .iter()
        .any(|(_, _, [start, end], _, _)| !finite_point(*start) || !finite_point(*end))
    {
        return None;
    }
    members
        .iter()
        .enumerate()
        .all(|(index, (_, _, [_, end], _, _))| {
            let next_start = members[(index + 1) % member_count].2[0];
            end.distance(next_start) <= ZERO_ENTITY_WIRE_TOLERANCE
        })
        .then_some(members)
}

fn append_oriented_wire_curve(
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    curve_id: CurveId,
    geometry: CurveGeometry,
    source_pos: usize,
    procedural: Option<(ProceduralCurveDefinition, Option<f64>)>,
) {
    let geometry = if let Some((definition, cache_fit_tolerance)) = procedural {
        let construction_id = ProceduralCurveId(format!("{}-construction", curve_id.0));
        annotate(
            annotations,
            &construction_id,
            "zero_entity_a9_03",
            source_pos as u64,
            "oriented_support_model_curve_construction",
            Exactness::Derived,
        );
        annotations
            .derived(&construction_id, "curve")
            .derived(&construction_id, "definition");
        ir.model.procedural_curves.push(ProceduralCurve {
            id: construction_id.clone(),
            curve: curve_id.clone(),
            definition,
            cache_fit_tolerance,
        });
        CurveGeometry::Procedural {
            construction: construction_id,
        }
    } else {
        geometry
    };
    annotate(
        annotations,
        &curve_id,
        "zero_entity_a9_03",
        source_pos as u64,
        "oriented_support_model_curve",
        Exactness::Derived,
    );
    annotations.derived(&curve_id, "geometry");
    ir.model.curves.push(Curve {
        id: curve_id,
        geometry,
        source_object: None,
    });
}

fn source_wire_procedural(
    ir: &CadIr,
    curve_id: &CurveId,
    geometry: &CurveGeometry,
) -> Option<WireSourceProcedural> {
    let CurveGeometry::Procedural { construction } = geometry else {
        return None;
    };
    ir.model
        .procedural_curves
        .iter()
        .find(|candidate| candidate.id == *construction && candidate.curve == *curve_id)
        .map(|candidate| WireSourceProcedural {
            construction_id: candidate.id.clone(),
            definition: candidate.definition.clone(),
            cache_fit_tolerance: candidate.cache_fit_tolerance,
        })
}

/// Transfer closed face-local boundary wires without assigning unresolved
/// source physical-edge or face identities. A complete ownership root groups
/// the wires under its one source shell and body.
fn transfer_closed_wire_loops(
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    support_runs: &[crate::families::zero_entity::records::ZeroEntitySupportRun],
    support_curve_ids: &HashMap<u32, CurveId>,
    ownership_root: Option<&crate::families::zero_entity::records::ZeroEntityOwnershipRoot>,
) -> WireTransferCounts {
    let mut counts = WireTransferCounts::default();
    let root_owns_support_runs = ownership_root.is_some_and(|root| {
        root.face_slots.len() == support_runs.len()
            && support_runs.iter().all(|run| run.face.is_some())
    });
    let mut owned_edge_ids = Vec::new();
    let mut source_curve_geometries = HashMap::<CurveId, Option<CurveGeometry>>::new();
    let mut source_curve_procedurals = HashMap::<CurveId, Option<WireSourceProcedural>>::new();
    let mut source_curve_orientations = HashMap::<CurveId, WireCurveOrientation>::new();

    for run in support_runs {
        let Some(face) = run.face.as_ref() else {
            continue;
        };

        for loop_record in &face.loops {
            let Some(members) = closed_wire_loop_members(run, loop_record, support_curve_ids)
            else {
                continue;
            };
            let member_count = members.len();

            let identity = format!(
                "{}-{}-{}",
                run.carrier_record_ordinal, face.record_ordinal, loop_record.record_ordinal
            );
            let body_id = BodyId(format!("catia:zero-entity:wire-body#{identity}"));
            let region_id = RegionId(format!("catia:zero-entity:wire-region#{identity}"));
            let shell_id = ShellId(format!("catia:zero-entity:wire-shell#{identity}"));
            if !root_owns_support_runs {
                annotate(
                    annotations,
                    &body_id,
                    "zero_entity_a9_03",
                    loop_record.pos as u64,
                    "standalone_wire_owner",
                    Exactness::Inferred,
                );
                annotate(
                    annotations,
                    &region_id,
                    "zero_entity_a9_03",
                    loop_record.pos as u64,
                    "standalone_wire_region",
                    Exactness::Inferred,
                );
                annotate(
                    annotations,
                    &shell_id,
                    "zero_entity_a9_03",
                    loop_record.pos as u64,
                    "standalone_wire_shell",
                    Exactness::Inferred,
                );
            }

            let mut vertex_ids = Vec::with_capacity(member_count);
            for (index, (_, _, [start, _], _, _)) in members.iter().enumerate() {
                let point_id = PointId(format!("catia:zero-entity:wire-point#{identity}-{index}"));
                let vertex_id =
                    VertexId(format!("catia:zero-entity:wire-vertex#{identity}-{index}"));
                annotate(
                    annotations,
                    &point_id,
                    "zero_entity_a9_03",
                    loop_record.pos as u64,
                    "standalone_wire_point",
                    Exactness::Derived,
                );
                annotate(
                    annotations,
                    &vertex_id,
                    "zero_entity_a9_03",
                    loop_record.pos as u64,
                    "standalone_wire_vertex",
                    Exactness::Derived,
                );
                annotations
                    .derived(&point_id, "position")
                    .derived(&vertex_id, "point");
                ir.model.points.push(Point {
                    id: point_id.clone(),
                    position: *start,
                    source_object: None,
                });
                ir.model.vertices.push(Vertex {
                    id: vertex_id.clone(),
                    point: point_id,
                    tolerance: Some(ZERO_ENTITY_WIRE_TOLERANCE),
                });
                vertex_ids.push(vertex_id);
                counts.points += 1;
                counts.vertices += 1;
            }

            let mut edge_ids = Vec::with_capacity(member_count);
            for (index, (support, curve, _, forward, parameter_range)) in members.iter().enumerate()
            {
                let edge_id = EdgeId(format!("catia:zero-entity:wire-edge#{identity}-{index}"));
                let (curve_id, param_range) = if let Some(parameters) = *parameter_range {
                    let oriented_range = if *forward {
                        parameters
                    } else {
                        [parameters[1], parameters[0]]
                    };
                    let reversed = oriented_range[0] > oriented_range[1];
                    let raw_source_range = if reversed {
                        [oriented_range[1], oriented_range[0]]
                    } else {
                        oriented_range
                    };
                    let source_geometry = source_curve_geometries
                        .entry(curve.clone())
                        .or_insert_with(|| {
                            ir.model
                                .curves
                                .iter()
                                .find(|candidate| candidate.id == curve.clone())
                                .map(|candidate| candidate.geometry.clone())
                        })
                        .clone();
                    let canonical_source_range =
                        source_geometry
                            .as_ref()
                            .map_or(Some(raw_source_range), |geometry| {
                                crate::nurbs::canonical_model_curve_range(
                                    geometry,
                                    raw_source_range,
                                )
                            });
                    let source_range = canonical_source_range.unwrap_or(raw_source_range);
                    let existing = source_curve_orientations.get(curve).copied();
                    if canonical_source_range.is_none() {
                        (curve.clone(), None)
                    } else if existing.is_some_and(|orientation| {
                        orientation.reversed == reversed && orientation.source_range == source_range
                    }) {
                        (
                            curve.clone(),
                            Some(existing.expect("checked above").edge_range),
                        )
                    } else if !reversed && existing.is_some_and(|orientation| !orientation.reversed)
                    {
                        (curve.clone(), Some(source_range))
                    } else if existing.is_none() && !reversed {
                        let procedural_available =
                            if matches!(&source_geometry, Some(CurveGeometry::Procedural { .. })) {
                                source_curve_procedurals
                                    .entry(curve.clone())
                                    .or_insert_with(|| {
                                        source_geometry.as_ref().and_then(|geometry| {
                                            source_wire_procedural(ir, curve, geometry)
                                        })
                                    })
                                    .is_some()
                            } else {
                                true
                            };
                        if source_geometry.is_none() || !procedural_available {
                            (curve.clone(), None)
                        } else {
                            let orientation = WireCurveOrientation {
                                reversed,
                                source_range,
                                edge_range: source_range,
                            };
                            source_curve_orientations.insert(curve.clone(), orientation);
                            (curve.clone(), Some(source_range))
                        }
                    } else if let Some(geometry) = source_geometry {
                        let source_procedural = source_curve_procedurals
                            .entry(curve.clone())
                            .or_insert_with(|| source_wire_procedural(ir, curve, &geometry))
                            .clone();
                        let oriented = if reversed {
                            match source_procedural.as_ref() {
                                Some(procedural) => crate::nurbs::reverse_helix_definition(
                                    &procedural.definition,
                                    source_range,
                                )
                                .map(|(definition, edge_range)| {
                                    (
                                        geometry.clone(),
                                        edge_range,
                                        Some((definition, procedural.cache_fit_tolerance)),
                                    )
                                }),
                                None => {
                                    crate::nurbs::reverse_curve_geometry(&geometry, source_range)
                                        .map(|(geometry, edge_range)| (geometry, edge_range, None))
                                }
                            }
                        } else {
                            Some((
                                geometry,
                                source_range,
                                source_procedural.as_ref().map(|procedural| {
                                    (
                                        procedural.definition.clone(),
                                        procedural.cache_fit_tolerance,
                                    )
                                }),
                            ))
                        };
                        if let Some((geometry, edge_range, procedural)) = oriented {
                            if existing.is_none() {
                                let carrier_updated = if reversed {
                                    if let (Some((definition, _)), Some(source_procedural)) =
                                        (procedural.as_ref(), source_procedural.as_ref())
                                    {
                                        ir.model
                                            .procedural_curves
                                            .iter_mut()
                                            .find(|candidate| {
                                                candidate.id == source_procedural.construction_id
                                                    && candidate.curve == *curve
                                            })
                                            .map(|candidate| {
                                                candidate.definition = definition.clone();
                                            })
                                            .is_some()
                                    } else {
                                        ir.model
                                            .curves
                                            .iter_mut()
                                            .find(|candidate| candidate.id == *curve)
                                            .map(|candidate| candidate.geometry = geometry.clone())
                                            .is_some()
                                    }
                                } else {
                                    true
                                };
                                if carrier_updated {
                                    source_curve_orientations.insert(
                                        curve.clone(),
                                        WireCurveOrientation {
                                            reversed,
                                            source_range,
                                            edge_range,
                                        },
                                    );
                                    (curve.clone(), Some(edge_range))
                                } else {
                                    (curve.clone(), None)
                                }
                            } else {
                                let oriented_curve_id = CurveId(format!(
                                    "catia:zero-entity:wire-curve#{identity}-{index}"
                                ));
                                append_oriented_wire_curve(
                                    ir,
                                    annotations,
                                    oriented_curve_id.clone(),
                                    geometry,
                                    support.pos,
                                    procedural,
                                );
                                (oriented_curve_id, Some(edge_range))
                            }
                        } else {
                            (curve.clone(), None)
                        }
                    } else {
                        (curve.clone(), None)
                    }
                } else {
                    (curve.clone(), None)
                };
                let param_range = param_range.and_then(|range| {
                    ir.model
                        .curves
                        .iter()
                        .find(|candidate| candidate.id == curve_id)
                        .and_then(|candidate| {
                            crate::nurbs::canonical_model_curve_range(&candidate.geometry, range)
                        })
                });
                annotate(
                    annotations,
                    &edge_id,
                    "zero_entity_a9_03",
                    support.pos as u64,
                    "standalone_wire_edge",
                    Exactness::Derived,
                );
                annotations
                    .derived(&edge_id, "curve")
                    .derived(&edge_id, "start")
                    .derived(&edge_id, "end");
                ir.model.edges.push(Edge {
                    id: edge_id.clone(),
                    curve: Some(curve_id),
                    start: vertex_ids[index].clone(),
                    end: vertex_ids[(index + 1) % member_count].clone(),
                    param_range,
                    tolerance: Some(ZERO_ENTITY_WIRE_TOLERANCE),
                });
                if param_range.is_some() {
                    annotations.derived(&edge_id, "param_range");
                }
                edge_ids.push(edge_id);
                counts.edges += 1;
            }

            if root_owns_support_runs {
                owned_edge_ids.extend(edge_ids);
            } else {
                ir.model.bodies.push(Body {
                    id: body_id.clone(),
                    kind: BodyKind::Wire,
                    regions: vec![region_id.clone()],
                    transform: None,
                    name: None,
                    color: None,
                    visible: None,
                });
                ir.model.regions.push(Region {
                    id: region_id.clone(),
                    body: body_id,
                    shells: vec![shell_id.clone()],
                });
                ir.model.shells.push(Shell {
                    id: shell_id,
                    region: region_id,
                    faces: Vec::new(),
                    wire_edges: edge_ids,
                    free_vertices: Vec::new(),
                });
                counts.bodies += 1;
            }
            counts.loops += 1;
        }
    }

    if root_owns_support_runs && counts.loops != 0 {
        let Some(root) = ownership_root else {
            return counts;
        };
        let identity = root.body_record_ordinal;
        let body_id = BodyId(format!("catia:zero-entity:owned-wire-body#{identity}"));
        let region_id = RegionId(format!("catia:zero-entity:owned-wire-region#{identity}"));
        let shell_id = ShellId(format!("catia:zero-entity:owned-wire-shell#{identity}"));
        annotate(
            annotations,
            &body_id,
            "zero_entity_a9_03",
            root.body_pos as u64,
            "owned_wire_body",
            Exactness::Derived,
        );
        annotate(
            annotations,
            &region_id,
            "zero_entity_a9_03",
            root.shell_pos as u64,
            "owned_wire_region",
            Exactness::Derived,
        );
        annotate(
            annotations,
            &shell_id,
            "zero_entity_a9_03",
            root.shell_pos as u64,
            "owned_wire_shell",
            Exactness::Derived,
        );
        ir.model.bodies.push(Body {
            id: body_id.clone(),
            kind: BodyKind::Wire,
            regions: vec![region_id.clone()],
            transform: None,
            name: None,
            color: None,
            visible: None,
        });
        ir.model.regions.push(Region {
            id: region_id.clone(),
            body: body_id,
            shells: vec![shell_id.clone()],
        });
        ir.model.shells.push(Shell {
            id: shell_id,
            region: region_id,
            faces: Vec::new(),
            wire_edges: owned_edge_ids,
            free_vertices: Vec::new(),
        });
        counts.bodies += 1;
        counts.owned_bodies += 1;
    }

    counts
}

pub(crate) fn try_decode_zero_entity(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
) -> Option<FamilyOutput> {
    let preamble = container::outer_preamble_range(&scan.data)?;
    let surfaces = crate::families::zero_entity::records::zero_entity_surfaces_in_range(
        &scan.data,
        preamble.clone(),
    );
    if surfaces.is_empty() {
        return None;
    }
    let support_runs = crate::families::zero_entity::records::zero_entity_support_runs_in_range(
        &scan.data,
        preamble.clone(),
    );
    let ownership_root = crate::families::zero_entity::records::zero_entity_ownership_root_in_range(
        &scan.data, preamble,
    );

    let mut ir = CadIr::empty(Units::default());
    let mut annotations = AnnotationBuilder::new();
    let mut unknowns = Vec::new();
    ir.source = Some(source_meta(scan));
    preserve_raw_payload(
        &mut unknowns,
        &mut annotations,
        scan,
        "catia:payload:unknown#zero-entity",
    );

    let mut surface_ids_by_position = HashMap::new();
    for (index, surface) in surfaces.into_iter().enumerate() {
        let id = SurfaceId(format!("catia:zero-entity:surf#{index}"));
        annotate(
            &mut annotations,
            &id,
            "zero_entity_a9_03",
            surface.pos as u64,
            "analytic_surface",
            Exactness::ByteExact,
        );
        ir.model.surfaces.push(Surface {
            id: id.clone(),
            geometry: surface.geometry,
            source_object: None,
        });
        surface_ids_by_position.insert(surface.pos, id);
    }

    let mut transferred_support_curves = 0usize;
    let mut transferred_parametric_surface_curves = 0usize;
    let mut support_curve_ids = HashMap::new();
    for run in &support_runs {
        let Some(surface) = surface_ids_by_position.get(&run.carrier_pos).cloned() else {
            continue;
        };
        for support in &run.supports {
            let curve_id = CurveId(format!(
                "catia:zero-entity:support-curve#{}",
                support.record_ordinal
            ));
            if let Some(geometry) = support.model_curve.clone() {
                annotate(
                    &mut annotations,
                    &curve_id,
                    "zero_entity_a9_03",
                    support.pos as u64,
                    "support_model_curve",
                    Exactness::Derived,
                );
                annotations.derived(&curve_id, "geometry");
                ir.model.curves.push(Curve {
                    id: curve_id.clone(),
                    geometry,
                    source_object: None,
                });
                support_curve_ids.insert(support.record_ordinal, curve_id);
                transferred_support_curves += 1;
                continue;
            }

            let (definition, role) =
                if let Some(definition) = support.model_curve_construction.clone() {
                    (definition, "support_model_curve_construction")
                } else {
                    let Some(pcurve) = support.pcurve.clone() else {
                        continue;
                    };
                    let Some(surface_geometry) = ir
                        .model
                        .surfaces
                        .iter()
                        .find(|candidate| candidate.id == surface)
                        .map(|candidate| &candidate.geometry)
                    else {
                        continue;
                    };
                    let Some(pcurve) =
                        crate::families::zero_entity::records::zero_entity_neutral_pcurve(
                            surface_geometry,
                            &pcurve,
                        )
                    else {
                        continue;
                    };
                    let PcurveGeometry::Nurbs {
                        degree,
                        knots,
                        control_points,
                        ..
                    } = &pcurve
                    else {
                        continue;
                    };
                    let Some(parameter_range) = usize::try_from(*degree)
                        .ok()
                        .and_then(|degree| {
                            Some([*knots.get(degree)?, *knots.get(control_points.len())?])
                        })
                        .filter(|range| {
                            range.iter().all(|value| value.is_finite()) && range[0] < range[1]
                        })
                    else {
                        continue;
                    };
                    transferred_parametric_surface_curves += 1;
                    (
                        ProceduralCurveDefinition::SurfaceCurve {
                            family: SurfaceCurveFamily::Parametric,
                            context: IntcurveSupportContext {
                                sides: [
                                    IntcurveSupportSide {
                                        surface: Some(surface.clone()),
                                        pcurve: Some(pcurve),
                                        pcurve_parameter_range: None,
                                    },
                                    IntcurveSupportSide {
                                        surface: None,
                                        pcurve: None,
                                        pcurve_parameter_range: None,
                                    },
                                ],
                                parameter_range,
                                discontinuities: std::array::from_fn(|_| Vec::new()),
                            },
                            tail: None,
                        },
                        "parametric_surface_curve",
                    )
                };
            let construction_id = ProceduralCurveId(format!(
                "catia:zero-entity:support-curve-construction#{}",
                support.record_ordinal
            ));
            annotate(
                &mut annotations,
                &curve_id,
                "zero_entity_a9_03",
                support.pos as u64,
                role,
                Exactness::Derived,
            );
            annotations
                .derived(&curve_id, "geometry")
                .derived(&construction_id, "curve")
                .derived(&construction_id, "definition");
            ir.model.curves.push(Curve {
                id: curve_id.clone(),
                geometry: CurveGeometry::Procedural {
                    construction: construction_id.clone(),
                },
                source_object: None,
            });
            ir.model.procedural_curves.push(ProceduralCurve {
                id: construction_id,
                curve: curve_id.clone(),
                definition,
                cache_fit_tolerance: None,
            });
            support_curve_ids.insert(support.record_ordinal, curve_id);
            transferred_support_curves += 1;
        }
    }

    let topology_counts = {
        let mut candidate_ir = ir.clone();
        let mut candidate_annotations = annotations.clone();
        let topology_budget = ctx.work_budget(
            crate::families::zero_entity::topology::MAX_ZERO_ENTITY_TOPOLOGY_OPERATIONS as u64,
        );
        let counts = crate::families::zero_entity::topology_transfer::transfer_closed_face_topology(
            &mut candidate_ir,
            &mut candidate_annotations,
            &support_runs,
            &surface_ids_by_position,
            &support_curve_ids,
            ownership_root.as_ref(),
            &topology_budget,
        );
        match counts {
            Some(counts) if neutral_model_is_admissible(&mut candidate_ir, &unknowns) => {
                ir = candidate_ir;
                annotations = candidate_annotations;
                Some(counts)
            }
            _ => None,
        }
    };
    let wire_counts = if topology_counts.is_some() {
        WireTransferCounts::default()
    } else {
        transfer_closed_wire_loops(
            &mut ir,
            &mut annotations,
            &support_runs,
            &support_curve_ids,
            ownership_root.as_ref(),
        )
    };

    link_payload_carriers(&ir, &mut unknowns, &mut annotations);
    let mut coverage = BTreeMap::from([
        (
            "transferred_zero_entity_support_curve_count".to_string(),
            transferred_support_curves,
        ),
        (
            "transferred_zero_entity_parametric_surface_curve_count".to_string(),
            transferred_parametric_surface_curves,
        ),
        (
            crate::coverage::TRANSFERRED_ZERO_ENTITY_WIRE_BODY_COUNT
                .0
                .to_string(),
            wire_counts.bodies,
        ),
        (
            crate::coverage::TRANSFERRED_ZERO_ENTITY_OWNED_WIRE_BODY_COUNT
                .0
                .to_string(),
            wire_counts.owned_bodies,
        ),
        (
            crate::coverage::TRANSFERRED_ZERO_ENTITY_WIRE_LOOP_COUNT
                .0
                .to_string(),
            wire_counts.loops,
        ),
        (
            crate::coverage::TRANSFERRED_ZERO_ENTITY_WIRE_EDGE_COUNT
                .0
                .to_string(),
            wire_counts.edges,
        ),
        (
            crate::coverage::TRANSFERRED_ZERO_ENTITY_WIRE_VERTEX_COUNT
                .0
                .to_string(),
            wire_counts.vertices,
        ),
        (
            crate::coverage::TRANSFERRED_ZERO_ENTITY_WIRE_POINT_COUNT
                .0
                .to_string(),
            wire_counts.points,
        ),
    ]);
    if let Some(counts) = topology_counts {
        coverage.extend([
            (
                crate::coverage::TRANSFERRED_ZERO_ENTITY_TOPOLOGY_BODY_COUNT
                    .0
                    .to_string(),
                counts.bodies,
            ),
            (
                crate::coverage::TRANSFERRED_ZERO_ENTITY_TOPOLOGY_FACE_COUNT
                    .0
                    .to_string(),
                counts.faces,
            ),
            (
                crate::coverage::TRANSFERRED_ZERO_ENTITY_TOPOLOGY_LOOP_COUNT
                    .0
                    .to_string(),
                counts.loops,
            ),
            (
                crate::coverage::TRANSFERRED_ZERO_ENTITY_TOPOLOGY_COEDGE_COUNT
                    .0
                    .to_string(),
                counts.coedges,
            ),
            (
                crate::coverage::TRANSFERRED_ZERO_ENTITY_TOPOLOGY_EDGE_COUNT
                    .0
                    .to_string(),
                counts.edges,
            ),
            (
                crate::coverage::TRANSFERRED_ZERO_ENTITY_TOPOLOGY_VERTEX_COUNT
                    .0
                    .to_string(),
                counts.vertices,
            ),
            (
                crate::coverage::TRANSFERRED_ZERO_ENTITY_TOPOLOGY_POINT_COUNT
                    .0
                    .to_string(),
                counts.points,
            ),
            (
                crate::coverage::TRANSFERRED_ZERO_ENTITY_TOPOLOGY_PCURVE_COUNT
                    .0
                    .to_string(),
                counts.pcurves,
            ),
        ]);
    }
    let topology_message = if topology_counts.is_some() && ownership_root.is_some() {
        "Complete zero-entity radial support pairs and endpoint loci lower into connected neutral faces, loops, coedges, edges, vertices, p-curves, and a body/region/shell hierarchy bound to the complete native ownership root; source allocation identities and native physical-edge identity remain retained as native records."
    } else if topology_counts.is_some() {
        "Complete zero-entity radial support pairs and endpoint loci lower into connected neutral faces, loops, coedges, edges, vertices, and p-curves under a deterministic derived body/region/shell hierarchy; native ownership allocation and physical-edge identities remain retained as native records."
    } else if wire_counts.loops == 0 {
        if ownership_root.is_some() {
            "Zero-entity loop members bind their face-local support occurrences and the terminal ownership root binds the complete face roster through one shell and body, but support-to-oriented-use, oriented-use-to-incidence, and physical endpoint bindings remain unresolved; no neutral topology was transferred."
        } else {
            "Zero-entity loop members bind their face-local support occurrences, but support-to-oriented-use, oriented-use-to-incidence, physical endpoint, and body/shell bindings remain unresolved; no neutral topology was transferred."
        }
    } else if wire_counts.owned_bodies != 0 {
        "Complete zero-entity face-local loops with exact model carriers and closed endpoint tapes were emitted under one derived wire container bound to the complete source shell and body roster; support-to-oriented-use, oriented-use-to-incidence, physical edge identity, and face topology remain unresolved."
    } else {
        "Complete zero-entity face-local loops with exact model carriers and closed endpoint tapes were emitted as independent wire bodies; support-to-oriented-use, oriented-use-to-incidence, physical edge identity, and source body/shell bindings remain unresolved."
    };
    let topology_loss = if topology_counts.is_some() {
        CatiaLossCode::TopologyZeroEntityGaugeSubstituted
    } else if wire_counts.loops == 0 {
        CatiaLossCode::TopologyZeroEntityNotTransferred
    } else {
        CatiaLossCode::TopologyZeroEntityFaceUnresolved
    };
    Some(FamilyOutput {
        ir,
        report: DecodeReport::unclassified(
            "catia",
            false,
            true,
            coverage,
            vec![topology_loss.note(topology_message)],
            container::summarize(scan).notes,
            cadmpeg_ir::report::TransferLedger::default(),
        ),
        annotations: annotations.build(),
        unknowns,
        standard_face_population: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadmpeg_ir::geometry::{Curve, CurveGeometry, NurbsCurve, ProceduralCurve};
    use cadmpeg_ir::math::Vector3;

    fn support(
        record_ordinal: u32,
        pos: usize,
        endpoints: [Point3; 2],
    ) -> crate::families::zero_entity::records::ZeroEntitySupportOccurrence {
        support_with_parameters(record_ordinal, pos, endpoints, Some([0.0, 1.0]))
    }

    fn support_with_parameters(
        record_ordinal: u32,
        pos: usize,
        endpoints: [Point3; 2],
        model_parameters: Option<[f64; 2]>,
    ) -> crate::families::zero_entity::records::ZeroEntitySupportOccurrence {
        crate::families::zero_entity::records::ZeroEntitySupportOccurrence {
            pos,
            record_ordinal,
            tag: [0x21, 0x71],
            face_local_slot: record_ordinal,
            uv_endpoints: None,
            pcurve: None,
            model_curve: None,
            model_curve_construction: None,
            model_parameters,
            model_midpoint: None,
            model_endpoints: Some(endpoints),
        }
    }

    #[test]
    fn closed_wire_clamps_nurbs_range_at_the_domain_boundary() {
        let first = Point3::new(0.0, 0.0, 0.0);
        let corner = Point3::new(1.0, 0.0, 0.0);
        let mut ir = CadIr::empty(Units::default());
        ir.model.curves.push(Curve {
            id: CurveId("catia:test:nurbs#0".to_string()),
            geometry: CurveGeometry::Nurbs(NurbsCurve {
                degree: 1,
                knots: vec![0.0, 0.0, 1.0, 1.0],
                control_points: vec![first, corner],
                weights: None,
                periodic: false,
            }),
            source_object: None,
        });
        ir.model.curves.push(Curve {
            id: CurveId("catia:test:line#1".to_string()),
            geometry: CurveGeometry::Line {
                origin: corner,
                direction: Vector3::new(-1.0, 0.0, 0.0),
            },
            source_object: None,
        });
        let support_runs = vec![
            crate::families::zero_entity::records::ZeroEntitySupportRun {
                carrier_pos: 0,
                carrier_record_ordinal: 1,
                face: Some(crate::families::zero_entity::records::ZeroEntityFace {
                    pos: 10,
                    record_ordinal: 2,
                    tag: [0x5f, 0x0c],
                    allocations: vec![8],
                    loop_terminals: vec![1],
                    loops: vec![crate::families::zero_entity::records::ZeroEntityLoop {
                        pos: 20,
                        record_ordinal: 3,
                        tag: [0x62, 0x14],
                        member_ids: vec![7, 6],
                        typed_references: vec![1, 2],
                        support_record_ordinals: vec![4, 5],
                        terminal_id: 8,
                        gap: 1,
                        loop_class: 0x41,
                        forward_senses: vec![true, true],
                        oriented_model_endpoints: vec![[first, corner], [corner, first]],
                    }],
                    terminal_control: 0x05,
                }),
                supports: vec![
                    support_with_parameters(
                        4,
                        30,
                        [first, corner],
                        Some([-1.0e-12, 1.0 + 1.0e-12]),
                    ),
                    support(5, 40, [corner, first]),
                ],
            },
        ];
        let support_curve_ids = HashMap::from([
            (4, CurveId("catia:test:nurbs#0".to_string())),
            (5, CurveId("catia:test:line#1".to_string())),
        ]);
        let mut annotations = AnnotationBuilder::new();

        let counts = transfer_closed_wire_loops(
            &mut ir,
            &mut annotations,
            &support_runs,
            &support_curve_ids,
            None,
        );

        assert_eq!(counts.edges, 2);
        assert_eq!(ir.model.edges[0].param_range, Some([0.0, 1.0]));
    }

    #[test]
    fn closed_face_local_loop_uses_ownership_root_with_untransferred_sibling() {
        let first = Point3::new(0.0, 0.0, 0.0);
        let corner = Point3::new(1.0, 0.0, 0.0);
        let mut ir = CadIr::empty(Units::default());
        for (index, (origin, direction)) in [
            (corner, Vector3::new(-1.0, 0.0, 0.0)),
            (first, Vector3::new(1.0, 0.0, 0.0)),
        ]
        .into_iter()
        .enumerate()
        {
            ir.model.curves.push(Curve {
                id: CurveId(format!("catia:test:curve#{index}")),
                geometry: CurveGeometry::Line { origin, direction },
                source_object: None,
            });
        }
        let support_runs = vec![
            crate::families::zero_entity::records::ZeroEntitySupportRun {
                carrier_pos: 0,
                carrier_record_ordinal: 1,
                face: Some(crate::families::zero_entity::records::ZeroEntityFace {
                    pos: 10,
                    record_ordinal: 2,
                    tag: [0x5f, 0x0c],
                    allocations: vec![9, 8],
                    loop_terminals: vec![1, 1, 1],
                    loops: vec![
                        crate::families::zero_entity::records::ZeroEntityLoop {
                            pos: 20,
                            record_ordinal: 3,
                            tag: [0x62, 0x14],
                            member_ids: vec![7, 6],
                            typed_references: vec![1, 2],
                            support_record_ordinals: vec![4, 5],
                            terminal_id: 8,
                            gap: 1,
                            loop_class: 0x41,
                            forward_senses: vec![true, false],
                            oriented_model_endpoints: vec![[first, corner], [corner, first]],
                        },
                        crate::families::zero_entity::records::ZeroEntityLoop {
                            pos: 25,
                            record_ordinal: 6,
                            tag: [0x62, 0x14],
                            member_ids: vec![9, 8],
                            typed_references: vec![4, 5],
                            support_record_ordinals: vec![5, 4],
                            terminal_id: 9,
                            gap: 1,
                            loop_class: 0x41,
                            forward_senses: vec![true, false],
                            oriented_model_endpoints: vec![[first, corner], [corner, first]],
                        },
                        crate::families::zero_entity::records::ZeroEntityLoop {
                            pos: 30,
                            record_ordinal: 7,
                            tag: [0x62, 0x14],
                            member_ids: vec![10],
                            typed_references: vec![3],
                            support_record_ordinals: vec![99],
                            terminal_id: 10,
                            gap: 1,
                            loop_class: 0x50,
                            forward_senses: vec![true],
                            oriented_model_endpoints: vec![[first, corner]],
                        },
                    ],
                    terminal_control: 0x05,
                }),
                supports: vec![
                    support_with_parameters(4, 30, [first, corner], Some([1.0, 0.0])),
                    support(5, 40, [first, corner]),
                ],
            },
        ];
        let support_curve_ids = HashMap::from([
            (4, CurveId("catia:test:curve#0".to_string())),
            (5, CurveId("catia:test:curve#1".to_string())),
        ]);
        let ownership_root = crate::families::zero_entity::records::ZeroEntityOwnershipRoot {
            face_roster_pos: 50,
            face_roster_record_ordinal: 6,
            face_slots: vec![1],
            shell_pos: 60,
            shell_record_ordinal: 7,
            body_pos: 70,
            body_record_ordinal: 8,
        };
        let mut annotations = AnnotationBuilder::new();

        let counts = transfer_closed_wire_loops(
            &mut ir,
            &mut annotations,
            &support_runs,
            &support_curve_ids,
            Some(&ownership_root),
        );

        assert_eq!(counts.bodies, 1);
        assert_eq!(counts.owned_bodies, 1);
        assert_eq!(counts.loops, 2);
        assert_eq!(counts.edges, 4);
        assert_eq!(counts.vertices, 4);
        assert_eq!(counts.points, 4);
        assert_eq!(
            ir.model.bodies[0].id,
            BodyId("catia:zero-entity:owned-wire-body#8".to_string())
        );
        assert!(matches!(ir.model.bodies[0].kind, BodyKind::Wire));
        assert_eq!(ir.model.shells[0].wire_edges.len(), 4);
        assert_eq!(ir.model.edges[0].param_range, Some([0.0, 1.0]));
        assert_eq!(ir.model.edges[1].param_range, Some([0.0, 1.0]));
        assert_eq!(
            ir.model.edges[1].curve,
            Some(CurveId("catia:test:curve#1".to_string()))
        );
        assert!(matches!(
            ir.model
                .curves
                .iter()
                .find(|curve| curve.id == CurveId("catia:test:curve#1".to_string()))
                .map(|curve| &curve.geometry),
            Some(CurveGeometry::Line { origin, direction })
                if *origin == corner && *direction == Vector3::new(-1.0, 0.0, 0.0)
        ));
        assert_eq!(
            ir.model.edges[2].curve,
            Some(CurveId("catia:zero-entity:wire-curve#1-2-6-0".to_string()))
        );
        assert_eq!(
            ir.model.edges[3].curve,
            Some(CurveId("catia:zero-entity:wire-curve#1-2-6-1".to_string()))
        );
        assert!(crate::assemble::neutral_model_is_admissible(&mut ir, &[]));
    }

    #[test]
    fn closed_wire_canonicalizes_periodic_model_range() {
        let start_angle: f64 = 0.25;
        let end_angle: f64 = 1.25;
        let first = Point3::new(start_angle.cos(), start_angle.sin(), 0.0);
        let corner = Point3::new(end_angle.cos(), end_angle.sin(), 0.0);
        let chord = first.distance(corner);
        let mut ir = CadIr::empty(Units::default());
        ir.model.curves.push(Curve {
            id: CurveId("catia:test:circle#0".to_string()),
            geometry: CurveGeometry::Circle {
                center: Point3::new(0.0, 0.0, 0.0),
                axis: Vector3::new(0.0, 0.0, 1.0),
                ref_direction: Vector3::new(1.0, 0.0, 0.0),
                radius: 1.0,
            },
            source_object: None,
        });
        ir.model.curves.push(Curve {
            id: CurveId("catia:test:line#1".to_string()),
            geometry: CurveGeometry::Line {
                origin: corner,
                direction: first.vector_from(corner).scale(1.0 / chord),
            },
            source_object: None,
        });
        let support_runs = vec![
            crate::families::zero_entity::records::ZeroEntitySupportRun {
                carrier_pos: 0,
                carrier_record_ordinal: 1,
                face: Some(crate::families::zero_entity::records::ZeroEntityFace {
                    pos: 10,
                    record_ordinal: 2,
                    tag: [0x5f, 0x0c],
                    allocations: vec![8],
                    loop_terminals: vec![1],
                    loops: vec![crate::families::zero_entity::records::ZeroEntityLoop {
                        pos: 20,
                        record_ordinal: 3,
                        tag: [0x62, 0x14],
                        member_ids: vec![7, 6],
                        typed_references: vec![1, 2],
                        support_record_ordinals: vec![4, 5],
                        terminal_id: 8,
                        gap: 1,
                        loop_class: 0x41,
                        forward_senses: vec![true, true],
                        oriented_model_endpoints: vec![[first, corner], [corner, first]],
                    }],
                    terminal_control: 0x05,
                }),
                supports: vec![
                    support_with_parameters(
                        4,
                        30,
                        [first, corner],
                        Some([
                            std::f64::consts::TAU + start_angle,
                            std::f64::consts::TAU + end_angle,
                        ]),
                    ),
                    support_with_parameters(5, 40, [corner, first], Some([0.0, chord])),
                ],
            },
        ];
        let support_curve_ids = HashMap::from([
            (4, CurveId("catia:test:circle#0".to_string())),
            (5, CurveId("catia:test:line#1".to_string())),
        ]);
        let mut annotations = AnnotationBuilder::new();

        let counts = transfer_closed_wire_loops(
            &mut ir,
            &mut annotations,
            &support_runs,
            &support_curve_ids,
            None,
        );

        assert_eq!(counts.loops, 1);
        assert_eq!(
            ir.model.edges[0].param_range,
            Some([start_angle, end_angle])
        );
        assert_eq!(ir.model.edges[1].param_range, Some([0.0, chord]));
        assert!(crate::assemble::neutral_model_is_admissible(&mut ir, &[]));
    }

    #[test]
    fn closed_wire_reverses_helix_construction_and_clones_mixed_orientation() {
        let first = Point3::new(0.0, 0.0, 0.0);
        let corner = Point3::new(1.0, 0.0, 0.0);
        let curve_id = CurveId("catia:test:helix-curve#0".to_string());
        let construction_id = ProceduralCurveId("catia:test:helix-construction#0".to_string());
        let definition = ProceduralCurveDefinition::Helix {
            angle_range: [0.0, 1.0],
            center: Point3::new(0.0, 0.0, 0.0),
            major: Vector3::new(1.0, 0.0, 0.0),
            minor: Vector3::new(0.0, 1.0, 0.0),
            pitch: Vector3::new(0.0, 0.0, 1.0),
            apex_factor: 0.2,
            axis: Vector3::new(0.0, 0.0, 1.0),
        };
        let mut ir = CadIr::empty(Units::default());
        ir.model.curves.push(Curve {
            id: curve_id.clone(),
            geometry: CurveGeometry::Procedural {
                construction: construction_id.clone(),
            },
            source_object: None,
        });
        ir.model.procedural_curves.push(ProceduralCurve {
            id: construction_id.clone(),
            curve: curve_id.clone(),
            definition: definition.clone(),
            cache_fit_tolerance: None,
        });
        let support_runs = vec![
            crate::families::zero_entity::records::ZeroEntitySupportRun {
                carrier_pos: 0,
                carrier_record_ordinal: 1,
                face: Some(crate::families::zero_entity::records::ZeroEntityFace {
                    pos: 10,
                    record_ordinal: 2,
                    tag: [0x5f, 0x0c],
                    allocations: vec![8],
                    loop_terminals: vec![1],
                    loops: vec![crate::families::zero_entity::records::ZeroEntityLoop {
                        pos: 20,
                        record_ordinal: 3,
                        tag: [0x62, 0x14],
                        member_ids: vec![7, 6],
                        typed_references: vec![1, 2],
                        support_record_ordinals: vec![4, 4],
                        terminal_id: 8,
                        gap: 1,
                        loop_class: 0x41,
                        forward_senses: vec![true, false],
                        oriented_model_endpoints: vec![[first, corner], [corner, first]],
                    }],
                    terminal_control: 0x05,
                }),
                supports: vec![support_with_parameters(
                    4,
                    30,
                    [first, corner],
                    Some([1.0, 0.0]),
                )],
            },
        ];
        let support_curve_ids = HashMap::from([(4, curve_id.clone())]);
        let mut annotations = AnnotationBuilder::new();

        let counts = transfer_closed_wire_loops(
            &mut ir,
            &mut annotations,
            &support_runs,
            &support_curve_ids,
            None,
        );

        assert_eq!(counts.bodies, 1);
        assert_eq!(counts.loops, 1);
        assert_eq!(counts.edges, 2);
        assert_eq!(ir.model.edges[0].param_range, Some([0.0, 1.0]));
        assert_eq!(ir.model.edges[1].param_range, Some([0.0, 1.0]));
        assert_eq!(ir.model.edges[0].curve, Some(curve_id.clone()));
        let derived_curve_id = CurveId("catia:zero-entity:wire-curve#1-2-3-1".to_string());
        assert_eq!(ir.model.edges[1].curve, Some(derived_curve_id.clone()));
        let source_definition = ir
            .model
            .procedural_curves
            .iter()
            .find(|construction| construction.id == construction_id)
            .expect("source construction")
            .definition
            .clone();
        assert_ne!(source_definition, definition);
        let derived_construction = match &ir
            .model
            .curves
            .iter()
            .find(|curve| curve.id == derived_curve_id)
            .expect("derived curve")
            .geometry
        {
            CurveGeometry::Procedural { construction } => construction.clone(),
            _ => panic!("derived curve remains procedural"),
        };
        assert_ne!(derived_construction, construction_id);
        assert_eq!(
            ir.model
                .procedural_curves
                .iter()
                .find(|construction| construction.id == derived_construction)
                .expect("derived construction")
                .definition,
            definition
        );
        assert!(crate::assemble::neutral_model_is_admissible(&mut ir, &[]));
    }

    #[test]
    fn incomplete_face_local_loop_is_not_emitted_as_wire() {
        let first = Point3::new(0.0, 0.0, 0.0);
        let second = Point3::new(1.0, 0.0, 0.0);
        let support_runs = vec![
            crate::families::zero_entity::records::ZeroEntitySupportRun {
                carrier_pos: 0,
                carrier_record_ordinal: 1,
                face: Some(crate::families::zero_entity::records::ZeroEntityFace {
                    pos: 10,
                    record_ordinal: 2,
                    tag: [0x5f, 0x0c],
                    allocations: vec![9, 8],
                    loop_terminals: vec![1],
                    loops: vec![crate::families::zero_entity::records::ZeroEntityLoop {
                        pos: 20,
                        record_ordinal: 3,
                        tag: [0x62, 0x14],
                        member_ids: vec![7],
                        typed_references: vec![1],
                        support_record_ordinals: vec![4],
                        terminal_id: 8,
                        gap: 1,
                        loop_class: 0x41,
                        forward_senses: vec![true],
                        oriented_model_endpoints: vec![[first, second]],
                    }],
                    terminal_control: 0x05,
                }),
                supports: vec![support(4, 30, [first, second])],
            },
        ];
        let mut ir = CadIr::empty(Units::default());
        let mut annotations = AnnotationBuilder::new();

        let counts = transfer_closed_wire_loops(
            &mut ir,
            &mut annotations,
            &support_runs,
            &HashMap::new(),
            None,
        );

        assert_eq!(counts, WireTransferCounts::default());
        assert!(ir.model.bodies.is_empty());
    }
}
