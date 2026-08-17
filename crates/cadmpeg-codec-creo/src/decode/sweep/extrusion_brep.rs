// SPDX-License-Identifier: Apache-2.0
//! Resolved extrusion B-rep transfer.

use super::super::analytic::nurbs_intrinsic_parameter_range;
use super::super::feature_history::feature_allows_additive_linear_extrusion;
use super::super::native::annotate;
use super::super::sketch::{normalized, section_point_in_model};
use super::super::sketch_ids::model_sketch_id;
use super::super::sketch_transfer::feature_is_first_material_operation;
use super::super::uniqueness::{
    exactly_one, unique_feature_definition_for_transform, unique_feature_section_transform,
};
use super::extent::resolved_feature_extrusion_span;
use super::nurbs::{
    extrusion_brep_side_surface, oriented_sketch_nurbs_curve, placed_section_nurbs,
    translated_nurbs_curve,
};
use super::pcurves::add_extrusion_pcurve;
use super::profiles::{
    extrusion_cap_pcurve, extrusion_profile_signed_area, extrusion_side_uvs, line_pcurve,
    ordered_extrusion_profiles, oriented_arc_parameterization, resolved_sketch_profiles,
};
use crate::container::ContainerScan;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::{Curve, CurveGeometry, Surface, SurfaceGeometry};
use cadmpeg_ir::ids::{
    BodyId, CoedgeId, CurveId, EdgeId, FaceId, LoopId, PcurveId, PointId, RegionId, ShellId,
    SurfaceId, VertexId,
};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::sketches::{Sketch, SketchEntityId, SketchGeometry};
use cadmpeg_ir::topology::{
    Body, BodyKind, Coedge, Edge, Face, Loop as IrLoop, PcurveUse, Point, Region, Sense, Shell,
    Vertex,
};
use cadmpeg_ir::{AnnotationBuilder, Exactness};
use std::collections::BTreeSet;

pub(in super::super) fn sketch_profiles_cover_generated_extrusion_sides(
    scan: &ContainerScan,
    definition: &crate::feature::FeatureDefinition,
    feature_id: u32,
    sketch: &Sketch,
) -> bool {
    let expected_entities = scan
        .features
        .entity_tables
        .iter()
        .filter(|table| table.feature_id == Some(feature_id))
        .flat_map(|table| {
            table.entries.iter().filter(move |entry| {
                entry.class_id == 200 && table.surface_ids.contains(&entry.entity_id)
            })
        })
        .filter_map(|entry| {
            let external_id = entry.source_entity_id?;
            scan.surfaces
                .rows
                .iter()
                .any(|row| {
                    row.id == entry.entity_id
                        && row.feature_id == feature_id
                        && matches!(
                            row.kind,
                            crate::surface::SurfaceKind::Plane
                                | crate::surface::SurfaceKind::Cylinder
                                | crate::surface::SurfaceKind::Extrusion
                        )
                })
                .then(|| {
                    SketchEntityId(format!(
                        "creo:featdefs:sketch_entity#{}:{external_id}",
                        definition.id
                    ))
                })
        })
        .collect::<Vec<_>>();
    let expected = expected_entities.iter().cloned().collect::<BTreeSet<_>>();
    let profile_entities = sketch
        .profiles
        .iter()
        .flatten()
        .map(|entity_use| entity_use.entity.clone())
        .collect::<Vec<_>>();
    !expected.is_empty()
        && expected_entities.len() == expected.len()
        && profile_entities.len() == expected.len()
        && profile_entities.into_iter().collect::<BTreeSet<_>>() == expected
}

pub(in super::super) fn transfer_resolved_extrusion_breps(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) -> usize {
    let mut transferred = 0;
    for transform in &scan.features.section_transforms {
        if unique_feature_section_transform(
            &scan.features.section_transforms,
            transform.definition_id,
            transform.offset,
        )
        .is_none()
        {
            continue;
        }
        let Some(feature_id) = transform.feature_id else {
            continue;
        };
        if !feature_allows_additive_linear_extrusion(scan, feature_id)
            || !feature_is_first_material_operation(scan, feature_id)
        {
            continue;
        }
        let Some(definition) =
            unique_feature_definition_for_transform(&scan.features.definitions, transform)
        else {
            continue;
        };
        let sketch_id = model_sketch_id(scan, definition);
        let Some(span) = resolved_feature_extrusion_span(scan, ir, definition, transform) else {
            continue;
        };
        let length = span.upper - span.lower;
        let Some(sketch) = exactly_one(
            ir.model
                .sketches
                .iter()
                .filter(|sketch| sketch.id == sketch_id),
        ) else {
            continue;
        };
        if !sketch_profiles_cover_generated_extrusion_sides(scan, definition, feature_id, sketch) {
            continue;
        }
        let Some(profiles) = resolved_sketch_profiles(ir, &sketch_id, 1) else {
            continue;
        };
        let Some((profiles, outer_area)) = ordered_extrusion_profiles(profiles) else {
            continue;
        };
        if profiles.iter().flatten().any(|(geometry, _, start, end)| {
            matches!(geometry, SketchGeometry::Line { .. }) && start == end
        }) {
            continue;
        }
        if profiles
            .iter()
            .flatten()
            .any(|(geometry, reversed, start, end)| {
                extrusion_brep_side_surface(transform, geometry, *reversed, *start, *end, span)
                    .is_none()
            })
        {
            continue;
        }
        let forward_caps = outer_area > 0.0;

        let prefix = format!("creo:feature:extrusion#{feature_id}");
        let body_id = BodyId(format!("{prefix}:body"));
        if ir.model.bodies.iter().any(|body| body.id == body_id) {
            continue;
        }
        let region_id = RegionId(format!("{prefix}:region"));
        let shell_id = ShellId(format!("{prefix}:shell"));
        let bottom_surface = SurfaceId(format!("{prefix}:surface:bottom"));
        let top_surface = SurfaceId(format!("{prefix}:surface:top"));
        for (id, offset) in [(&bottom_surface, span.lower), (&top_surface, span.upper)] {
            annotate(
                annotations,
                id,
                "FeatDefs",
                transform.offset as u64,
                "extrusion_cap_plane",
                Exactness::Derived,
            );
            ir.model.surfaces.push(Surface {
                id: id.clone(),
                geometry: SurfaceGeometry::Plane {
                    origin: Point3::new(
                        transform.origin[0] + offset * transform.normal[0],
                        transform.origin[1] + offset * transform.normal[1],
                        transform.origin[2] + offset * transform.normal[2],
                    ),
                    normal: Vector3::new(
                        transform.normal[0],
                        transform.normal[1],
                        transform.normal[2],
                    ),
                    u_axis: Vector3::new(
                        transform.u_axis[0],
                        transform.u_axis[1],
                        transform.u_axis[2],
                    ),
                },
                source_object: None,
            });
        }

        let bottom_face = FaceId(format!("{prefix}:face:bottom"));
        let top_face = FaceId(format!("{prefix}:face:top"));
        let mut shell_faces = vec![bottom_face.clone(), top_face.clone()];
        let mut bottom_loops = Vec::new();
        let mut top_loops = Vec::new();
        for (profile_index, profile) in profiles.iter().enumerate() {
            let count = profile.len();
            let mut bottom_vertices = Vec::new();
            let mut top_vertices = Vec::new();
            for (index, (_, _, start, _)) in profile.iter().enumerate() {
                for (side, offset, arena) in [
                    ("bottom", span.lower, &mut bottom_vertices),
                    ("top", span.upper, &mut top_vertices),
                ] {
                    let position = section_point_in_model(transform, *start);
                    let point_id =
                        PointId(format!("{prefix}:point:{profile_index}:{index}:{side}"));
                    let vertex_id =
                        VertexId(format!("{prefix}:vertex:{profile_index}:{index}:{side}"));
                    ir.model.points.push(Point {
                        id: point_id.clone(),
                        position: Point3::new(
                            position[0] + offset * transform.normal[0],
                            position[1] + offset * transform.normal[1],
                            position[2] + offset * transform.normal[2],
                        ),
                        source_object: None,
                    });
                    ir.model.vertices.push(Vertex {
                        id: vertex_id.clone(),
                        point: point_id,
                        tolerance: None,
                    });
                    arena.push(vertex_id);
                }
            }

            let mut bottom_edges = Vec::new();
            let mut top_edges = Vec::new();
            let mut vertical_edges = Vec::new();
            for (index, (geometry, reversed, start, end)) in profile.iter().enumerate() {
                let next = (index + 1) % count;
                for (side, offset, vertices, arena) in [
                    ("bottom", span.lower, &bottom_vertices, &mut bottom_edges),
                    ("top", span.upper, &top_vertices, &mut top_edges),
                ] {
                    let curve_id =
                        CurveId(format!("{prefix}:curve:{profile_index}:{index}:{side}"));
                    let edge_id = EdgeId(format!("{prefix}:edge:{profile_index}:{index}:{side}"));
                    let curve = match geometry {
                        SketchGeometry::Line { .. } => {
                            let placed_start = section_point_in_model(transform, *start);
                            let placed_end = section_point_in_model(transform, *end);
                            let Some(direction) = normalized(std::array::from_fn(|axis| {
                                placed_end[axis] - placed_start[axis]
                            })) else {
                                continue;
                            };
                            CurveGeometry::Line {
                                origin: Point3::new(
                                    placed_start[0] + offset * transform.normal[0],
                                    placed_start[1] + offset * transform.normal[1],
                                    placed_start[2] + offset * transform.normal[2],
                                ),
                                direction: Vector3::new(direction[0], direction[1], direction[2]),
                            }
                        }
                        SketchGeometry::Arc { center, radius, .. }
                        | SketchGeometry::Circle { center, radius } => {
                            let center = section_point_in_model(transform, [center.u, center.v]);
                            let (axis_sign, _) = oriented_arc_parameterization(*reversed, 0.0, 0.0);
                            CurveGeometry::Circle {
                                center: Point3::new(
                                    center[0] + offset * transform.normal[0],
                                    center[1] + offset * transform.normal[1],
                                    center[2] + offset * transform.normal[2],
                                ),
                                axis: Vector3::new(
                                    axis_sign * transform.normal[0],
                                    axis_sign * transform.normal[1],
                                    axis_sign * transform.normal[2],
                                ),
                                ref_direction: Vector3::new(
                                    transform.u_axis[0],
                                    transform.u_axis[1],
                                    transform.u_axis[2],
                                ),
                                radius: radius.0,
                            }
                        }
                        SketchGeometry::Nurbs { .. } => {
                            let Some(nurbs) = oriented_sketch_nurbs_curve(geometry, *reversed)
                            else {
                                continue;
                            };
                            let placed = placed_section_nurbs(transform, &nurbs);
                            let translated = translated_nurbs_curve(
                                &placed,
                                [
                                    offset * transform.normal[0],
                                    offset * transform.normal[1],
                                    offset * transform.normal[2],
                                ],
                            );
                            CurveGeometry::Nurbs(translated)
                        }
                        _ => unreachable!("profile family checked above"),
                    };
                    ir.model.curves.push(Curve {
                        id: curve_id.clone(),
                        geometry: curve,
                        source_object: None,
                    });
                    let param_range = match geometry {
                        SketchGeometry::Line { .. } => {
                            Some([0.0, (end[0] - start[0]).hypot(end[1] - start[1])])
                        }
                        SketchGeometry::Arc {
                            start_angle,
                            end_angle,
                            ..
                        } => Some(
                            oriented_arc_parameterization(*reversed, start_angle.0, end_angle.0).1,
                        ),
                        SketchGeometry::Circle { .. } => Some(
                            oriented_arc_parameterization(*reversed, 0.0, std::f64::consts::TAU).1,
                        ),
                        SketchGeometry::Nurbs { .. } => {
                            oriented_sketch_nurbs_curve(geometry, *reversed)
                                .and_then(|nurbs| nurbs_intrinsic_parameter_range(&nurbs))
                        }
                        _ => None,
                    };
                    ir.model.edges.push(Edge {
                        id: edge_id.clone(),
                        curve: Some(curve_id),
                        start: vertices[index].clone(),
                        end: vertices[next].clone(),
                        param_range,
                        tolerance: None,
                    });
                    arena.push(edge_id);
                }
                let curve_id = CurveId(format!("{prefix}:curve:{profile_index}:{index}:vertical"));
                let edge_id = EdgeId(format!("{prefix}:edge:{profile_index}:{index}:vertical"));
                let origin = section_point_in_model(transform, *start);
                ir.model.curves.push(Curve {
                    id: curve_id.clone(),
                    geometry: CurveGeometry::Line {
                        origin: Point3::new(
                            origin[0] + span.lower * transform.normal[0],
                            origin[1] + span.lower * transform.normal[1],
                            origin[2] + span.lower * transform.normal[2],
                        ),
                        direction: Vector3::new(
                            transform.normal[0],
                            transform.normal[1],
                            transform.normal[2],
                        ),
                    },
                    source_object: None,
                });
                ir.model.edges.push(Edge {
                    id: edge_id.clone(),
                    curve: Some(curve_id),
                    start: bottom_vertices[index].clone(),
                    end: top_vertices[index].clone(),
                    param_range: Some([0.0, length]),
                    tolerance: None,
                });
                vertical_edges.push(edge_id);
            }

            let bottom_loop = LoopId(format!("{prefix}:loop:{profile_index}:bottom"));
            let top_loop = LoopId(format!("{prefix}:loop:{profile_index}:top"));
            bottom_loops.push(bottom_loop.clone());
            top_loops.push(top_loop.clone());
            let bottom_coedges = (0..count)
                .rev()
                .map(|index| {
                    CoedgeId(format!(
                        "{prefix}:coedge:{profile_index}:{index}:bottom-cap"
                    ))
                })
                .collect::<Vec<_>>();
            let top_coedges = (0..count)
                .map(|index| CoedgeId(format!("{prefix}:coedge:{profile_index}:{index}:top-cap")))
                .collect::<Vec<_>>();
            ir.model.loops.push(IrLoop {
                id: bottom_loop.clone(),
                face: bottom_face.clone(),
                boundary_role: if profile_index == 0 {
                    cadmpeg_ir::topology::LoopBoundaryRole::Outer
                } else {
                    cadmpeg_ir::topology::LoopBoundaryRole::Inner
                },
                coedges: bottom_coedges.clone(),
                vertex_uses: Vec::new(),
            });
            ir.model.loops.push(IrLoop {
                id: top_loop.clone(),
                face: top_face.clone(),
                boundary_role: if profile_index == 0 {
                    cadmpeg_ir::topology::LoopBoundaryRole::Outer
                } else {
                    cadmpeg_ir::topology::LoopBoundaryRole::Inner
                },
                coedges: top_coedges.clone(),
                vertex_uses: Vec::new(),
            });
            for ring_index in 0..count {
                let edge_index = count - 1 - ring_index;
                let id = bottom_coedges[ring_index].clone();
                let (geometry, reversed, start, end) = &profile[edge_index];
                let bottom_pcurve = add_extrusion_pcurve(
                    ir,
                    annotations,
                    PcurveId(format!(
                        "{prefix}:pcurve:{profile_index}:{edge_index}:bottom-cap"
                    )),
                    transform.offset,
                    extrusion_cap_pcurve(geometry, *reversed, *start, *end),
                );
                ir.model.coedges.push(Coedge {
                    id,
                    owner_loop: bottom_loop.clone(),
                    edge: bottom_edges[edge_index].clone(),
                    next: bottom_coedges[(ring_index + 1) % count].clone(),
                    previous: bottom_coedges[(ring_index + count - 1) % count].clone(),
                    radial_next: CoedgeId(format!(
                        "{prefix}:coedge:{profile_index}:{edge_index}:side-bottom"
                    )),
                    sense: Sense::Reversed,
                    pcurves: vec![PcurveUse {
                        pcurve: bottom_pcurve,
                        isoparametric: None,
                        parameter_range: None,
                    }],
                    use_curve: None,
                    use_curve_parameter_range: None,
                });
                let id = top_coedges[ring_index].clone();
                let (geometry, reversed, start, end) = &profile[ring_index];
                let top_pcurve = add_extrusion_pcurve(
                    ir,
                    annotations,
                    PcurveId(format!(
                        "{prefix}:pcurve:{profile_index}:{ring_index}:top-cap"
                    )),
                    transform.offset,
                    extrusion_cap_pcurve(geometry, *reversed, *start, *end),
                );
                ir.model.coedges.push(Coedge {
                    id,
                    owner_loop: top_loop.clone(),
                    edge: top_edges[ring_index].clone(),
                    next: top_coedges[(ring_index + 1) % count].clone(),
                    previous: top_coedges[(ring_index + count - 1) % count].clone(),
                    radial_next: CoedgeId(format!(
                        "{prefix}:coedge:{profile_index}:{ring_index}:side-top"
                    )),
                    sense: Sense::Forward,
                    pcurves: vec![PcurveUse {
                        pcurve: top_pcurve,
                        isoparametric: None,
                        parameter_range: None,
                    }],
                    use_curve: None,
                    use_curve_parameter_range: None,
                });
            }

            let forward_sides = extrusion_profile_signed_area(profile)
                .expect("validated extrusion profile has nonzero area")
                > 0.0;
            for (index, (geometry, _, start, _)) in profile.iter().enumerate() {
                let next = (index + 1) % count;
                let surface_id =
                    SurfaceId(format!("{prefix}:surface:{profile_index}:side:{index}"));
                let Some(surface_geometry) = extrusion_brep_side_surface(
                    transform,
                    geometry,
                    profile[index].1,
                    *start,
                    profile[index].3,
                    span,
                ) else {
                    break;
                };
                ir.model.surfaces.push(Surface {
                    id: surface_id.clone(),
                    geometry: surface_geometry,
                    source_object: None,
                });
                let face_id = FaceId(format!("{prefix}:face:{profile_index}:side:{index}"));
                let loop_id = LoopId(format!("{prefix}:loop:{profile_index}:side:{index}"));
                let coedges = [
                    CoedgeId(format!(
                        "{prefix}:coedge:{profile_index}:{index}:side-bottom"
                    )),
                    CoedgeId(format!(
                        "{prefix}:coedge:{profile_index}:{next}:side-vertical-out"
                    )),
                    CoedgeId(format!("{prefix}:coedge:{profile_index}:{index}:side-top")),
                    CoedgeId(format!(
                        "{prefix}:coedge:{profile_index}:{index}:side-vertical-in"
                    )),
                ];
                ir.model.loops.push(IrLoop {
                    id: loop_id.clone(),
                    face: face_id.clone(),
                    boundary_role: cadmpeg_ir::topology::LoopBoundaryRole::Outer,
                    coedges: coedges.to_vec(),
                    vertex_uses: Vec::new(),
                });
                let edge_uses = [
                    (bottom_edges[index].clone(), Sense::Forward),
                    (vertical_edges[next].clone(), Sense::Forward),
                    (top_edges[index].clone(), Sense::Reversed),
                    (vertical_edges[index].clone(), Sense::Reversed),
                ];
                let side_uvs =
                    extrusion_side_uvs(geometry, profile[index].1, *start, profile[index].3, span);
                for use_index in 0..4 {
                    let radial_next = match use_index {
                        0 => bottom_coedges[count - 1 - index].clone(),
                        1 => CoedgeId(format!(
                            "{prefix}:coedge:{profile_index}:{next}:side-vertical-in"
                        )),
                        2 => top_coedges[index].clone(),
                        3 => CoedgeId(format!(
                            "{prefix}:coedge:{profile_index}:{index}:side-vertical-out"
                        )),
                        _ => unreachable!(),
                    };
                    let pcurve = add_extrusion_pcurve(
                        ir,
                        annotations,
                        PcurveId(format!(
                            "{prefix}:pcurve:{profile_index}:{index}:side:{use_index}"
                        )),
                        transform.offset,
                        line_pcurve(side_uvs[use_index][0], side_uvs[use_index][1]),
                    );
                    ir.model.coedges.push(Coedge {
                        id: coedges[use_index].clone(),
                        owner_loop: loop_id.clone(),
                        edge: edge_uses[use_index].0.clone(),
                        next: coedges[(use_index + 1) % 4].clone(),
                        previous: coedges[(use_index + 3) % 4].clone(),
                        radial_next,
                        sense: edge_uses[use_index].1,
                        pcurves: vec![PcurveUse {
                            pcurve,
                            isoparametric: None,
                            parameter_range: None,
                        }],
                        use_curve: None,
                        use_curve_parameter_range: None,
                    });
                }
                ir.model.faces.push(Face {
                    id: face_id.clone(),
                    shell: shell_id.clone(),
                    surface: surface_id,
                    sense: if forward_sides {
                        Sense::Forward
                    } else {
                        Sense::Reversed
                    },
                    loops: vec![loop_id],
                    name: None,
                    color: None,
                    tolerance: None,
                });
                shell_faces.push(face_id);
            }
        }
        ir.model.faces.push(Face {
            id: bottom_face,
            shell: shell_id.clone(),
            surface: bottom_surface,
            sense: if forward_caps {
                Sense::Reversed
            } else {
                Sense::Forward
            },
            loops: bottom_loops,
            name: None,
            color: None,
            tolerance: None,
        });
        ir.model.faces.push(Face {
            id: top_face,
            shell: shell_id.clone(),
            surface: top_surface,
            sense: if forward_caps {
                Sense::Forward
            } else {
                Sense::Reversed
            },
            loops: top_loops,
            name: None,
            color: None,
            tolerance: None,
        });
        ir.model.shells.push(Shell {
            id: shell_id.clone(),
            region: region_id.clone(),
            faces: shell_faces,
            wire_edges: Vec::new(),
            free_vertices: Vec::new(),
        });
        ir.model.regions.push(Region {
            id: region_id.clone(),
            body: body_id.clone(),
            shells: vec![shell_id],
        });
        ir.model.bodies.push(Body {
            id: body_id,
            kind: BodyKind::Solid,
            regions: vec![region_id],
            transform: None,
            name: None,
            color: None,
            visible: None,
        });
        transferred += 1;
    }
    transferred
}
