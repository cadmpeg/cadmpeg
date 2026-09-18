// SPDX-License-Identifier: Apache-2.0
//! Resolved extrusion B-rep transfer.

use super::super::feature_history::draft::feature_allows_additive_linear_extrusion;
use super::super::feature_history::link::generated_profile_entry_is_admissible;
use super::super::native::annotate;
use super::super::sketch::section_point_in_model;
use super::super::sketch_ids::model_sketch_id;
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
    extrusion_cap_pcurve, extrusion_side_uvs, line_pcurve, ordered_extrusion_profiles,
    oriented_arc_parameterization, resolved_sketch_profiles, ProfileGeometry,
};
use crate::container::ContainerScan;
use crate::decode::analytic::edges::nurbs_intrinsic_parameter_range;
use crate::decode::sketch_transfer::recipe::feature_is_first_material_operation;
use crate::vecmath::normalize;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, Surface, SurfaceGeometry,
};
use cadmpeg_ir::ids::{
    BodyId, CoedgeId, CurveId, EdgeId, FaceId, IdentityKey, LoopId, PcurveId, PointId, RegionId,
    ShellId, SurfaceId, VertexId,
};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::sketches::{Sketch, SketchEntityId};
use cadmpeg_ir::topology::{
    Body, BodyKind, Coedge, Edge, Face, Loop as IrLoop, PcurveUse, Point, Region, Sense, Shell,
    Vertex,
};
use cadmpeg_ir::{AnnotationBuilder, Exactness};
use std::collections::BTreeSet;

const GENERATED_EXTRUSION_SIDE_KINDS: &[crate::surface::SurfaceKind] = &[
    crate::surface::SurfaceKind::Plane,
    crate::surface::SurfaceKind::Cylinder,
    crate::surface::SurfaceKind::Extrusion(crate::surface::ExtrusionVariant::Linear),
];

pub(in super::super) fn sketch_profiles_cover_generated_extrusion_sides(
    scan: &ContainerScan,
    definition: &crate::feature::FeatureDefinition,
    feature_id: u32,
    sketch: &Sketch,
) -> bool {
    let profile_entities = sketch
        .profiles
        .iter()
        .flatten()
        .map(|entity_use| entity_use.entity.clone())
        .collect::<Vec<_>>();
    let profile_entity_set = profile_entities.iter().cloned().collect::<BTreeSet<_>>();
    let expected_entities = scan
        .features
        .entity_tables
        .iter()
        .filter(|table| table.feature_id == feature_id)
        .flat_map(|table| {
            table.entries.iter().filter_map(|entry| {
                let external_id = entry.source_entity_id()?;
                let entity = SketchEntityId::compose(
                    &crate::identity::FEATDEFS_SKETCH_ENTITY,
                    IdentityKey::from(definition.identity.id()).colon(external_id),
                );
                (profile_entity_set.contains(&entity)
                    && generated_profile_entry_is_admissible(
                        feature_id,
                        table,
                        entry,
                        GENERATED_EXTRUSION_SIDE_KINDS,
                        &scan.surfaces.rows,
                    ))
                .then_some(entity)
            })
        })
        .collect::<Vec<_>>();
    let expected_entity_set = expected_entities.iter().cloned().collect::<BTreeSet<_>>();
    !expected_entities.is_empty()
        && expected_entities.len() == expected_entity_set.len()
        && profile_entities.len() == expected_entity_set.len()
        && profile_entity_set == expected_entity_set
}

pub(in super::super) fn transfer_resolved_extrusion_breps(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    diagnostics: &mut crate::decode::surfaces::BrepTransferDiagnostics,
) -> Result<usize, cadmpeg_core::CodecError> {
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
        let Some(sketch_id) = model_sketch_id(scan, definition) else {
            continue;
        };
        let Some(span) = resolved_feature_extrusion_span(scan, ir, definition, transform) else {
            continue;
        };
        let feature_key = IdentityKey::from(feature_id);
        macro_rules! extrusion_id {
            ($id:ident, $key:expr) => {
                $id::compose(
                    &crate::identity::FEATURE_EXTRUSION,
                    feature_key.clone().colon($key),
                )
            };
        }
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
        let Some(profiles) = ordered_extrusion_profiles(profiles) else {
            continue;
        };
        let body_id = extrusion_id!(BodyId, cadmpeg_ir::identity_key!("body"));
        if ir.model.bodies.iter().any(|body| body.id == body_id) {
            continue;
        }
        let mut refusal = crate::lane_refusal::LaneRefusals::new();
        let unprojectable = profiles
            .iter()
            .flat_map(super::profiles::ValidatedProfile::entities)
            .enumerate()
            .any(|(entity_index, entity)| {
                let geometry = entity.geometry();
                let reversed = entity.reversed();
                let start = entity.start();
                let end = entity.end();
                let record =
                    format!("extrusion feature {feature_id} profile entity {entity_index}");

                geometry
                    .to_sketch()
                    .and_then(|sketch_geometry| {
                        let mut diagnostics =
                            crate::lane_refusal::LaneRefusalContext::new(&record, &mut refusal);
                        extrusion_brep_side_surface(
                            transform,
                            &sketch_geometry,
                            reversed,
                            start,
                            end,
                            span,
                            &mut diagnostics,
                        )
                    })
                    .is_none()
            });
        let records = refusal.take_records();
        if !records.is_empty() {
            // The probe states every side before the first record of this body
            // reaches the model, so a refused lane leaves no partial body and
            // the model carries the absence of this one extrusion.
            diagnostics.rejected_extrusion_bodies.push((
                body_id.clone(),
                format!("refused extrusion side lanes: {}", records.join("; ")),
            ));
            continue;
        }
        if unprojectable {
            continue;
        }
        let forward_caps = profiles[0].area() > 0.0;

        let region_id = extrusion_id!(RegionId, cadmpeg_ir::identity_key!("region"));
        let shell_id = extrusion_id!(ShellId, cadmpeg_ir::identity_key!("shell"));
        let bottom_face = extrusion_id!(
            FaceId,
            cadmpeg_ir::identity_key!("face").colon(cadmpeg_ir::identity_key!("bottom"))
        );
        let top_face = extrusion_id!(
            FaceId,
            cadmpeg_ir::identity_key!("face").colon(cadmpeg_ir::identity_key!("top"))
        );
        let mut shell_faces = vec![bottom_face.clone(), top_face.clone()];
        for (profile_index, profile) in profiles.iter().enumerate() {
            for index in 0..profile.entities().len() {
                shell_faces.push(extrusion_id!(
                    FaceId,
                    cadmpeg_ir::identity_key!("face")
                        .colon(profile_index)
                        .colon(cadmpeg_ir::identity_key!("side"))
                        .colon(index)
                ));
            }
        }
        let shell = match Shell::new(
            shell_id.clone(),
            region_id.clone(),
            shell_faces,
            Vec::new(),
            Vec::new(),
        ) {
            Ok(shell) => shell,
            Err(error) => {
                diagnostics
                    .rejected_extrusion_bodies
                    .push((body_id, error.to_string()));
                continue;
            }
        };
        let bottom_surface = extrusion_id!(
            SurfaceId,
            cadmpeg_ir::identity_key!("surface").colon(cadmpeg_ir::identity_key!("bottom"))
        );
        let top_surface = extrusion_id!(
            SurfaceId,
            cadmpeg_ir::identity_key!("surface").colon(cadmpeg_ir::identity_key!("top"))
        );
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
                geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
                    cadmpeg_ir::geometry::PlaneSurface::try_new(
                        Point3::new(
                            transform.origin()[0] + offset * transform.normal()[0],
                            transform.origin()[1] + offset * transform.normal()[1],
                            transform.origin()[2] + offset * transform.normal()[2],
                        ),
                        Vector3::new(
                            transform.normal()[0],
                            transform.normal()[1],
                            transform.normal()[2],
                        ),
                        Vector3::new(
                            transform.u_axis()[0],
                            transform.u_axis()[1],
                            transform.u_axis()[2],
                        ),
                    )
                    .map_err(cadmpeg_core::CodecError::malformed)?,
                )),
                source_object: None,
            });
        }

        let mut bottom_loops = Vec::new();
        let mut top_loops = Vec::new();
        for (profile_index, validated) in profiles.iter().enumerate() {
            let profile = validated.entities();
            let count = profile.len();
            let mut bottom_vertices = Vec::new();
            let mut top_vertices = Vec::new();
            for (index, entity) in profile.iter().enumerate() {
                let start = entity.start();

                for (side, offset, arena) in [
                    ("bottom", span.lower, &mut bottom_vertices),
                    ("top", span.upper, &mut top_vertices),
                ] {
                    let position = section_point_in_model(transform, start);
                    let side_key = match side {
                        "bottom" => cadmpeg_ir::identity_key!("bottom"),
                        "top" => cadmpeg_ir::identity_key!("top"),
                        _ => continue,
                    };
                    let point_id = extrusion_id!(
                        PointId,
                        cadmpeg_ir::identity_key!("point")
                            .colon(profile_index)
                            .colon(index)
                            .colon(&side_key)
                    );
                    let vertex_id = extrusion_id!(
                        VertexId,
                        cadmpeg_ir::identity_key!("vertex")
                            .colon(profile_index)
                            .colon(index)
                            .colon(&side_key)
                    );
                    ir.model.points.push(Point {
                        id: point_id.clone(),
                        position: Point3::new(
                            position[0] + offset * transform.normal()[0],
                            position[1] + offset * transform.normal()[1],
                            position[2] + offset * transform.normal()[2],
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
            for (index, entity) in profile.iter().enumerate() {
                let geometry = entity.geometry();
                let Some(sketch_geometry) = geometry.to_sketch() else {
                    continue;
                };
                let reversed = entity.reversed();
                let start = entity.start();
                let end = entity.end();

                let next = (index + 1) % count;
                for (side, offset, vertices, arena) in [
                    ("bottom", span.lower, &bottom_vertices, &mut bottom_edges),
                    ("top", span.upper, &top_vertices, &mut top_edges),
                ] {
                    let side_key = match side {
                        "bottom" => cadmpeg_ir::identity_key!("bottom"),
                        "top" => cadmpeg_ir::identity_key!("top"),
                        _ => continue,
                    };
                    let curve_id = extrusion_id!(
                        CurveId,
                        cadmpeg_ir::identity_key!("curve")
                            .colon(profile_index)
                            .colon(index)
                            .colon(&side_key)
                    );
                    let edge_id = extrusion_id!(
                        EdgeId,
                        cadmpeg_ir::identity_key!("edge")
                            .colon(profile_index)
                            .colon(index)
                            .colon(&side_key)
                    );
                    let curve = match geometry {
                        ProfileGeometry::Line { .. } => {
                            let placed_start = section_point_in_model(transform, start);
                            let placed_end = section_point_in_model(transform, end);
                            let Some(direction) = normalize(std::array::from_fn(|axis| {
                                placed_end[axis] - placed_start[axis]
                            })) else {
                                continue;
                            };
                            CurveGeometry::Solved(SolvedCurveGeometry::Line(
                                cadmpeg_ir::geometry::LineCurve::try_new(
                                    Point3::new(
                                        placed_start[0] + offset * transform.normal()[0],
                                        placed_start[1] + offset * transform.normal()[1],
                                        placed_start[2] + offset * transform.normal()[2],
                                    ),
                                    Vector3::new(direction[0], direction[1], direction[2]),
                                )
                                .map_err(cadmpeg_core::CodecError::malformed)?,
                            ))
                        }
                        ProfileGeometry::Arc { center, radius, .. }
                        | ProfileGeometry::Circle { center, radius } => {
                            let center = section_point_in_model(transform, [center.u, center.v]);
                            let (axis_sign, _) = oriented_arc_parameterization(reversed, 0.0, 0.0);
                            CurveGeometry::Solved(SolvedCurveGeometry::Circle(
                                cadmpeg_ir::geometry::CircleCurve::try_new(
                                    Point3::new(
                                        center[0] + offset * transform.normal()[0],
                                        center[1] + offset * transform.normal()[1],
                                        center[2] + offset * transform.normal()[2],
                                    ),
                                    Vector3::new(
                                        axis_sign * transform.normal()[0],
                                        axis_sign * transform.normal()[1],
                                        axis_sign * transform.normal()[2],
                                    ),
                                    Vector3::new(
                                        transform.u_axis()[0],
                                        transform.u_axis()[1],
                                        transform.u_axis()[2],
                                    ),
                                    radius.get(),
                                )
                                .map_err(cadmpeg_core::CodecError::malformed)?,
                            ))
                        }
                        ProfileGeometry::Nurbs { .. } => {
                            let Some(nurbs) =
                                oriented_sketch_nurbs_curve(&sketch_geometry, reversed)
                            else {
                                continue;
                            };
                            let Some(placed) = placed_section_nurbs(transform, &nurbs) else {
                                continue;
                            };
                            let Some(translated) = translated_nurbs_curve(
                                &placed,
                                [
                                    offset * transform.normal()[0],
                                    offset * transform.normal()[1],
                                    offset * transform.normal()[2],
                                ],
                            ) else {
                                continue;
                            };
                            CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(translated))
                        }
                    };
                    ir.model.curves.push(Curve {
                        id: curve_id.clone(),
                        geometry: curve,
                        source_object: None,
                    });
                    let param_range = match geometry {
                        ProfileGeometry::Line { .. } => {
                            Some([0.0, (end[0] - start[0]).hypot(end[1] - start[1])])
                        }
                        ProfileGeometry::Arc {
                            start_angle,
                            end_angle,
                            ..
                        } => Some(
                            oriented_arc_parameterization(
                                reversed,
                                start_angle.get(),
                                end_angle.get(),
                            )
                            .1,
                        ),
                        ProfileGeometry::Circle { .. } => Some(
                            oriented_arc_parameterization(reversed, 0.0, std::f64::consts::TAU).1,
                        ),
                        ProfileGeometry::Nurbs { .. } => {
                            oriented_sketch_nurbs_curve(&sketch_geometry, reversed)
                                .and_then(|nurbs| nurbs_intrinsic_parameter_range(&nurbs))
                        }
                    };
                    ir.model.edges.push(Edge {
                        id: edge_id.clone(),
                        carrier: cadmpeg_ir::topology::EdgeCarrier::new(
                            Some(curve_id),
                            param_range,
                        )
                        .map_err(cadmpeg_core::CodecError::malformed)?,
                        start: vertices[index].clone(),
                        end: vertices[next].clone(),
                        tolerance: None,
                    });
                    arena.push(edge_id);
                }
                let curve_id = extrusion_id!(
                    CurveId,
                    cadmpeg_ir::identity_key!("curve")
                        .colon(profile_index)
                        .colon(index)
                        .colon(cadmpeg_ir::identity_key!("vertical"))
                );
                let edge_id = extrusion_id!(
                    EdgeId,
                    cadmpeg_ir::identity_key!("edge")
                        .colon(profile_index)
                        .colon(index)
                        .colon(cadmpeg_ir::identity_key!("vertical"))
                );
                let origin = section_point_in_model(transform, start);
                ir.model.curves.push(Curve {
                    id: curve_id.clone(),
                    geometry: CurveGeometry::Solved(SolvedCurveGeometry::Line(
                        cadmpeg_ir::geometry::LineCurve::try_new(
                            Point3::new(
                                origin[0] + span.lower * transform.normal()[0],
                                origin[1] + span.lower * transform.normal()[1],
                                origin[2] + span.lower * transform.normal()[2],
                            ),
                            Vector3::new(
                                transform.normal()[0],
                                transform.normal()[1],
                                transform.normal()[2],
                            ),
                        )
                        .map_err(cadmpeg_core::CodecError::malformed)?,
                    )),
                    source_object: None,
                });
                ir.model.edges.push(Edge {
                    id: edge_id.clone(),
                    carrier: cadmpeg_ir::topology::EdgeCarrier::new(
                        Some(curve_id),
                        Some([0.0, length]),
                    )
                    .map_err(cadmpeg_core::CodecError::malformed)?,
                    start: bottom_vertices[index].clone(),
                    end: top_vertices[index].clone(),
                    tolerance: None,
                });
                vertical_edges.push(edge_id);
            }

            let bottom_loop = extrusion_id!(
                LoopId,
                cadmpeg_ir::identity_key!("loop")
                    .colon(profile_index)
                    .colon(cadmpeg_ir::identity_key!("bottom"))
            );
            let top_loop = extrusion_id!(
                LoopId,
                cadmpeg_ir::identity_key!("loop")
                    .colon(profile_index)
                    .colon(cadmpeg_ir::identity_key!("top"))
            );
            bottom_loops.push(bottom_loop.clone());
            top_loops.push(top_loop.clone());
            let bottom_coedges = (0..count)
                .rev()
                .map(|index| {
                    extrusion_id!(
                        CoedgeId,
                        cadmpeg_ir::identity_key!("coedge")
                            .colon(profile_index)
                            .colon(index)
                            .colon(cadmpeg_ir::identity_key!("bottom-cap"))
                    )
                })
                .collect::<Vec<_>>();
            let top_coedges = (0..count)
                .map(|index| {
                    extrusion_id!(
                        CoedgeId,
                        cadmpeg_ir::identity_key!("coedge")
                            .colon(profile_index)
                            .colon(index)
                            .colon(cadmpeg_ir::identity_key!("top-cap"))
                    )
                })
                .collect::<Vec<_>>();
            ir.model.loops.push(IrLoop {
                id: bottom_loop.clone(),
                face: bottom_face.clone(),
                boundary: cadmpeg_ir::topology::LoopBoundary::Ring(
                    cadmpeg_ir::topology::LoopRing::new(bottom_coedges.clone(), Vec::new())
                        .map_err(cadmpeg_core::CodecError::malformed)?,
                ),
            });
            ir.model.loops.push(IrLoop {
                id: top_loop.clone(),
                face: top_face.clone(),
                boundary: cadmpeg_ir::topology::LoopBoundary::Ring(
                    cadmpeg_ir::topology::LoopRing::new(top_coedges.clone(), Vec::new())
                        .map_err(cadmpeg_core::CodecError::malformed)?,
                ),
            });
            for ring_index in 0..count {
                let edge_index = count - 1 - ring_index;
                let id = bottom_coedges[ring_index].clone();
                let entity = &profile[edge_index];
                let geometry = entity.geometry();
                let Some(sketch_geometry) = geometry.to_sketch() else {
                    continue;
                };
                let reversed = entity.reversed();
                let start = entity.start();
                let end = entity.end();

                let bottom_pcurve = add_extrusion_pcurve(
                    ir,
                    annotations,
                    extrusion_id!(
                        PcurveId,
                        cadmpeg_ir::identity_key!("pcurve")
                            .colon(profile_index)
                            .colon(edge_index)
                            .colon(cadmpeg_ir::identity_key!("bottom-cap"))
                    ),
                    transform.offset,
                    {
                        let mut refusal = crate::lane_refusal::LaneRefusals::new();
                        let record = format!(
                            "extrusion feature {feature_id} profile {profile_index} bottom cap \
                             at entity {edge_index}"
                        );
                        let cap = extrusion_cap_pcurve(
                            &sketch_geometry,
                            reversed,
                            start,
                            end,
                            &record,
                            &mut refusal,
                        );
                        let records = refusal.take_records();
                        if !records.is_empty() {
                            // The shell of this body already declares this cap
                            // face, so the model cannot omit the pcurve.
                            return Err(cadmpeg_core::CodecError::malformed(format!(
                                "Refused lanes on {record}: {}",
                                records.join("; ")
                            )));
                        }
                        cap.ok_or_else(|| {
                            cadmpeg_core::CodecError::malformed(format!(
                                "{record} states no pcurve geometry"
                            ))
                        })?
                    },
                )?;
                ir.model.coedges.push(Coedge {
                    id,
                    owner_loop: bottom_loop.clone(),
                    edge: bottom_edges[edge_index].clone(),
                    radial_next: extrusion_id!(
                        CoedgeId,
                        cadmpeg_ir::identity_key!("coedge")
                            .colon(profile_index)
                            .colon(edge_index)
                            .colon(cadmpeg_ir::identity_key!("side-bottom"))
                    ),
                    sense: Sense::Reversed,
                    pcurves: vec![PcurveUse {
                        pcurve: bottom_pcurve,
                        isoparametric: None,
                        parameter_range: None,
                    }],
                    use_curve: None,
                });
                let id = top_coedges[ring_index].clone();
                let entity = &profile[ring_index];
                let geometry = entity.geometry();
                let Some(sketch_geometry) = geometry.to_sketch() else {
                    continue;
                };
                let reversed = entity.reversed();
                let start = entity.start();
                let end = entity.end();

                let top_pcurve = add_extrusion_pcurve(
                    ir,
                    annotations,
                    extrusion_id!(
                        PcurveId,
                        cadmpeg_ir::identity_key!("pcurve")
                            .colon(profile_index)
                            .colon(ring_index)
                            .colon(cadmpeg_ir::identity_key!("top-cap"))
                    ),
                    transform.offset,
                    {
                        let mut refusal = crate::lane_refusal::LaneRefusals::new();
                        let record = format!(
                            "extrusion feature {feature_id} profile {profile_index} top cap \
                             at entity {ring_index}"
                        );
                        let cap = extrusion_cap_pcurve(
                            &sketch_geometry,
                            reversed,
                            start,
                            end,
                            &record,
                            &mut refusal,
                        );
                        let records = refusal.take_records();
                        if !records.is_empty() {
                            // The shell of this body already declares this cap
                            // face, so the model cannot omit the pcurve.
                            return Err(cadmpeg_core::CodecError::malformed(format!(
                                "Refused lanes on {record}: {}",
                                records.join("; ")
                            )));
                        }
                        cap.ok_or_else(|| {
                            cadmpeg_core::CodecError::malformed(format!(
                                "{record} states no pcurve geometry"
                            ))
                        })?
                    },
                )?;
                ir.model.coedges.push(Coedge {
                    id,
                    owner_loop: top_loop.clone(),
                    edge: top_edges[ring_index].clone(),
                    radial_next: extrusion_id!(
                        CoedgeId,
                        cadmpeg_ir::identity_key!("coedge")
                            .colon(profile_index)
                            .colon(ring_index)
                            .colon(cadmpeg_ir::identity_key!("side-top"))
                    ),
                    sense: Sense::Forward,
                    pcurves: vec![PcurveUse {
                        pcurve: top_pcurve,
                        isoparametric: None,
                        parameter_range: None,
                    }],
                    use_curve: None,
                });
            }

            let forward_sides = validated.area() > 0.0;
            for (index, entity) in profile.iter().enumerate() {
                let geometry = entity.geometry();
                let Some(sketch_geometry) = geometry.to_sketch() else {
                    continue;
                };
                let start = entity.start();

                let next = (index + 1) % count;
                let surface_id = extrusion_id!(
                    SurfaceId,
                    cadmpeg_ir::identity_key!("surface")
                        .colon(profile_index)
                        .colon(cadmpeg_ir::identity_key!("side"))
                        .colon(index)
                );
                let mut refusal = crate::lane_refusal::LaneRefusals::new();
                let record =
                    format!("extrusion feature {feature_id} profile {profile_index} side {index}");
                let mut diagnostics =
                    crate::lane_refusal::LaneRefusalContext::new(&record, &mut refusal);
                let surface_geometry = extrusion_brep_side_surface(
                    transform,
                    &sketch_geometry,
                    profile[index].reversed(),
                    start,
                    profile[index].end(),
                    span,
                    &mut diagnostics,
                );
                let records = refusal.take_records();
                if !records.is_empty() {
                    // The shell of this body already declares this side face,
                    // so the model cannot omit the surface.
                    return Err(cadmpeg_core::CodecError::malformed(format!(
                        "Refused lanes on {record}: {}",
                        records.join("; ")
                    )));
                }
                let Some(surface_geometry) = surface_geometry else {
                    break;
                };
                ir.model.surfaces.push(Surface {
                    id: surface_id.clone(),
                    geometry: surface_geometry,
                    source_object: None,
                });
                let face_id = extrusion_id!(
                    FaceId,
                    cadmpeg_ir::identity_key!("face")
                        .colon(profile_index)
                        .colon(cadmpeg_ir::identity_key!("side"))
                        .colon(index)
                );
                let loop_id = extrusion_id!(
                    LoopId,
                    cadmpeg_ir::identity_key!("loop")
                        .colon(profile_index)
                        .colon(cadmpeg_ir::identity_key!("side"))
                        .colon(index)
                );
                let coedges = [
                    extrusion_id!(
                        CoedgeId,
                        cadmpeg_ir::identity_key!("coedge")
                            .colon(profile_index)
                            .colon(index)
                            .colon(cadmpeg_ir::identity_key!("side-bottom"))
                    ),
                    extrusion_id!(
                        CoedgeId,
                        cadmpeg_ir::identity_key!("coedge")
                            .colon(profile_index)
                            .colon(next)
                            .colon(cadmpeg_ir::identity_key!("side-vertical-out"))
                    ),
                    extrusion_id!(
                        CoedgeId,
                        cadmpeg_ir::identity_key!("coedge")
                            .colon(profile_index)
                            .colon(index)
                            .colon(cadmpeg_ir::identity_key!("side-top"))
                    ),
                    extrusion_id!(
                        CoedgeId,
                        cadmpeg_ir::identity_key!("coedge")
                            .colon(profile_index)
                            .colon(index)
                            .colon(cadmpeg_ir::identity_key!("side-vertical-in"))
                    ),
                ];
                ir.model.loops.push(IrLoop {
                    id: loop_id.clone(),
                    face: face_id.clone(),
                    boundary: cadmpeg_ir::topology::LoopBoundary::Ring(
                        cadmpeg_ir::topology::LoopRing::new(coedges.to_vec(), Vec::new())
                            .map_err(cadmpeg_core::CodecError::malformed)?,
                    ),
                });
                let edge_uses = [
                    (bottom_edges[index].clone(), Sense::Forward),
                    (vertical_edges[next].clone(), Sense::Forward),
                    (top_edges[index].clone(), Sense::Reversed),
                    (vertical_edges[index].clone(), Sense::Reversed),
                ];
                let side_uvs = extrusion_side_uvs(
                    &sketch_geometry,
                    profile[index].reversed(),
                    start,
                    profile[index].end(),
                    span,
                );
                for use_index in 0..4 {
                    let radial_next = match use_index {
                        0 => bottom_coedges[count - 1 - index].clone(),
                        1 => extrusion_id!(
                            CoedgeId,
                            cadmpeg_ir::identity_key!("coedge")
                                .colon(profile_index)
                                .colon(next)
                                .colon(cadmpeg_ir::identity_key!("side-vertical-in"))
                        ),
                        2 => top_coedges[index].clone(),
                        3 => extrusion_id!(
                            CoedgeId,
                            cadmpeg_ir::identity_key!("coedge")
                                .colon(profile_index)
                                .colon(index)
                                .colon(cadmpeg_ir::identity_key!("side-vertical-out"))
                        ),
                        _ => continue,
                    };
                    let pcurve = add_extrusion_pcurve(
                        ir,
                        annotations,
                        extrusion_id!(
                            PcurveId,
                            cadmpeg_ir::identity_key!("pcurve")
                                .colon(profile_index)
                                .colon(index)
                                .colon(cadmpeg_ir::identity_key!("side"))
                                .colon(use_index)
                        ),
                        transform.offset,
                        line_pcurve(side_uvs[use_index][0], side_uvs[use_index][1]).ok_or_else(
                            || {
                                cadmpeg_core::CodecError::malformed(
                                    "extrusion pcurve geometry is invalid",
                                )
                            },
                        )?,
                    )?;
                    ir.model.coedges.push(Coedge {
                        id: coedges[use_index].clone(),
                        owner_loop: loop_id.clone(),
                        edge: edge_uses[use_index].0.clone(),
                        radial_next,
                        sense: edge_uses[use_index].1,
                        pcurves: vec![PcurveUse {
                            pcurve,
                            isoparametric: None,
                            parameter_range: None,
                        }],
                        use_curve: None,
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
                    loops: cadmpeg_ir::topology::FaceLoops::unspecified(vec![loop_id]),
                    name: None,
                    color: None,
                    tolerance: None,
                });
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
            loops: cadmpeg_ir::topology::FaceLoops::unspecified(bottom_loops),
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
            loops: cadmpeg_ir::topology::FaceLoops::unspecified(top_loops),
            name: None,
            color: None,
            tolerance: None,
        });
        ir.model.shells.push(shell);
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
    Ok(transferred)
}

#[cfg(test)]
mod tests;
