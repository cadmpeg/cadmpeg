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
use cadmpeg_ir::report::{DecodeReport, LossNote, Severity};
use cadmpeg_ir::topology::{Body, BodyKind, Edge, Point, Region, Shell, Vertex};
use cadmpeg_ir::units::Units;
use cadmpeg_ir::AnnotationBuilder;
use cadmpeg_ir::Exactness;

use crate::assemble::{annotate, link_payload_carriers, preserve_raw_payload, source_meta};
use crate::container::{self, ContainerScan};
use crate::families::FamilyOutput;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct WireTransferCounts {
    bodies: usize,
    loops: usize,
    edges: usize,
    vertices: usize,
    points: usize,
}

fn finite_point(point: Point3) -> bool {
    [point.x, point.y, point.z].into_iter().all(f64::is_finite)
}

/// Transfer closed face-local boundary wires without assigning unresolved
/// source physical-edge or body identities.
fn transfer_closed_wire_loops(
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    support_runs: &[crate::families::zero_entity::records::ZeroEntitySupportRun],
    support_curve_ids: &HashMap<u32, CurveId>,
) -> WireTransferCounts {
    const CLOSURE_TOLERANCE: f64 = 2e-3;
    let mut counts = WireTransferCounts::default();

    for run in support_runs {
        let Some(face) = run.face.as_ref() else {
            continue;
        };
        let supports_by_ordinal = run
            .supports
            .iter()
            .map(|support| (support.record_ordinal, support))
            .collect::<HashMap<_, _>>();

        for loop_record in &face.loops {
            let member_count = loop_record.support_record_ordinals.len();
            if member_count == 0
                || loop_record.forward_senses.len() != member_count
                || loop_record.oriented_model_endpoints.len() != member_count
            {
                continue;
            }

            let members = loop_record
                .support_record_ordinals
                .iter()
                .zip(&loop_record.oriented_model_endpoints)
                .map(|(record_ordinal, endpoints)| {
                    let support = supports_by_ordinal.get(record_ordinal)?;
                    let curve = support_curve_ids.get(record_ordinal)?.clone();
                    support.model_endpoints?;
                    Some((support, curve, *endpoints))
                })
                .collect::<Option<Vec<_>>>();
            let Some(members) = members else {
                continue;
            };
            if members
                .iter()
                .any(|(_, _, [start, end])| !finite_point(*start) || !finite_point(*end))
            {
                continue;
            }
            if !members.iter().enumerate().all(|(index, (_, _, [_, end]))| {
                let next_start = members[(index + 1) % member_count].2[0];
                end.distance(next_start) <= CLOSURE_TOLERANCE
            }) {
                continue;
            }

            let identity = format!(
                "{}-{}-{}",
                run.carrier_record_ordinal, face.record_ordinal, loop_record.record_ordinal
            );
            let body_id = BodyId(format!("catia:zero-entity:wire-body#{identity}"));
            let region_id = RegionId(format!("catia:zero-entity:wire-region#{identity}"));
            let shell_id = ShellId(format!("catia:zero-entity:wire-shell#{identity}"));
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

            let mut vertex_ids = Vec::with_capacity(member_count);
            for (index, (_, _, [start, _])) in members.iter().enumerate() {
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
                    tolerance: Some(CLOSURE_TOLERANCE),
                });
                vertex_ids.push(vertex_id);
                counts.points += 1;
                counts.vertices += 1;
            }

            let mut edge_ids = Vec::with_capacity(member_count);
            for (index, (support, curve, _)) in members.iter().enumerate() {
                let edge_id = EdgeId(format!("catia:zero-entity:wire-edge#{identity}-{index}"));
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
                    curve: Some(curve.clone()),
                    start: vertex_ids[index].clone(),
                    end: vertex_ids[(index + 1) % member_count].clone(),
                    param_range: None,
                    tolerance: Some(CLOSURE_TOLERANCE),
                });
                edge_ids.push(edge_id);
                counts.edges += 1;
            }

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
            counts.loops += 1;
        }
    }

    counts
}

pub(crate) fn try_decode_zero_entity(
    _ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
) -> Option<FamilyOutput> {
    let surfaces = crate::families::zero_entity::records::zero_entity_surfaces(&scan.data);
    if surfaces.is_empty() {
        return None;
    }
    let support_runs = crate::families::zero_entity::records::zero_entity_support_runs(&scan.data);
    let ownership_root =
        crate::families::zero_entity::records::zero_entity_ownership_root(&scan.data);

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

    let wire_counts =
        transfer_closed_wire_loops(&mut ir, &mut annotations, &support_runs, &support_curve_ids);

    link_payload_carriers(&ir, &mut unknowns, &mut annotations);
    let coverage = BTreeMap::from([
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
    let topology_message = if wire_counts.loops == 0 {
        if ownership_root.is_some() {
            "Zero-entity loop members bind their face-local support occurrences and the terminal ownership root binds the complete face roster through one shell and body, but support-to-oriented-use, oriented-use-to-incidence, and physical endpoint bindings remain unresolved; no neutral topology was transferred."
        } else {
            "Zero-entity loop members bind their face-local support occurrences, but support-to-oriented-use, oriented-use-to-incidence, physical endpoint, and body/shell bindings remain unresolved; no neutral topology was transferred."
        }
    } else {
        "Complete zero-entity face-local loops with exact model carriers and closed endpoint tapes were emitted as independent wire bodies; support-to-oriented-use, oriented-use-to-incidence, physical edge identity, and source body/shell bindings remain unresolved."
    };
    Some(FamilyOutput {
        ir,
        report: DecodeReport {
            format: "catia".to_string(),
            container_only: false,
            geometry_transferred: true,
            coverage,
            transfer_ledger: cadmpeg_ir::report::TransferLedger::default(),
            losses: vec![LossNote {
                code: cadmpeg_ir::report::LossKind::TopologyNotTransferred,
                severity: Severity::Blocking,
                message: topology_message.to_string(),
                provenance: None,
            }],
            notes: container::summarize(scan).notes,
        },
        annotations: annotations.build(),
        unknowns,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadmpeg_ir::geometry::{Curve, CurveGeometry};
    use cadmpeg_ir::math::Vector3;

    fn support(
        record_ordinal: u32,
        pos: usize,
        endpoints: [Point3; 2],
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
            model_parameters: None,
            model_midpoint: None,
            model_endpoints: Some(endpoints),
        }
    }

    #[test]
    fn closed_face_local_loop_transfers_as_independent_wire() {
        let first = Point3::new(0.0, 0.0, 0.0);
        let corner = Point3::new(1.0, 0.0, 0.0);
        let mut ir = CadIr::empty(Units::default());
        for (index, direction) in [Vector3::new(1.0, 0.0, 0.0), Vector3::new(-1.0, 0.0, 0.0)]
            .into_iter()
            .enumerate()
        {
            ir.model.curves.push(Curve {
                id: CurveId(format!("catia:test:curve#{index}")),
                geometry: CurveGeometry::Line {
                    origin: if index == 0 { first } else { corner },
                    direction,
                },
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
                    support(4, 30, [first, corner]),
                    support(5, 40, [corner, first]),
                ],
            },
        ];
        let support_curve_ids = HashMap::from([
            (4, CurveId("catia:test:curve#0".to_string())),
            (5, CurveId("catia:test:curve#1".to_string())),
        ]);
        let mut annotations = AnnotationBuilder::new();

        let counts = transfer_closed_wire_loops(
            &mut ir,
            &mut annotations,
            &support_runs,
            &support_curve_ids,
        );

        assert_eq!(counts.bodies, 1);
        assert_eq!(counts.loops, 1);
        assert_eq!(counts.edges, 2);
        assert_eq!(counts.vertices, 2);
        assert_eq!(counts.points, 2);
        assert!(matches!(ir.model.bodies[0].kind, BodyKind::Wire));
        assert_eq!(ir.model.shells[0].wire_edges.len(), 2);
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

        let counts =
            transfer_closed_wire_loops(&mut ir, &mut annotations, &support_runs, &HashMap::new());

        assert_eq!(counts, WireTransferCounts::default());
        assert!(ir.model.bodies.is_empty());
    }
}
