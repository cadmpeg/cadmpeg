// SPDX-License-Identifier: Apache-2.0
//! Circular extrusion B-rep transfer.

use super::super::feature_history::draft::feature_allows_additive_linear_extrusion;
use super::super::holes::sweep::circular_sweep_geometry;
use super::super::sketch::intersect::section_point_in_model;
use super::super::sketch_ids::model_sketch_id;
use super::super::uniqueness::{
    exactly_one, unique_feature_definition_for_transform, unique_feature_section_transform,
};
use super::extent::resolved_feature_extrusion_span;
use super::pcurves::add_extrusion_pcurve;
use super::profiles::{circular_pcurve, line_pcurve};
use crate::container::ContainerScan;
use crate::decode::sketch_transfer::recipe::feature_is_first_material_operation;
use crate::vecmath::dot;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::analytic::CylinderSurface;
use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, Surface, SurfaceGeometry,
};
use cadmpeg_ir::ids::{
    BodyId, CoedgeId, CurveId, EdgeId, FaceId, LoopId, PcurveId, PointId, RegionId, ShellId,
    SurfaceId, VertexId,
};
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::sketches::{SketchGeometryDefinition, SketchId};
use cadmpeg_ir::topology::{
    Body, BodyKind, Coedge, Edge, Face, Loop as IrLoop, PcurveUse, Point, Region, Sense, Shell,
    Vertex,
};
use cadmpeg_ir::AnnotationBuilder;

const EPS_AXIS_ALIGNMENT: f64 = 1.0e-9;

pub(in super::super) fn transfer_resolved_circular_extrusion_breps(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
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
        let Some((section_center, radius)) =
            resolved_circular_extrusion_profile(scan, ir, transform, feature_id, &sketch_id)
        else {
            continue;
        };
        let Some(span) = resolved_feature_extrusion_span(scan, ir, definition, transform) else {
            continue;
        };
        let feature_key = cadmpeg_ir::ids::IdentityKey::from(feature_id);
        let body_id = BodyId::compose(
            &crate::identity::FEATURE_EXTRUSION,
            feature_key.clone().colon(cadmpeg_ir::identity_key!("body")),
        );
        if ir.model.bodies.iter().any(|body| body.id == body_id) {
            continue;
        }
        let region_id = RegionId::compose(
            &crate::identity::FEATURE_EXTRUSION,
            feature_key
                .clone()
                .colon(cadmpeg_ir::identity_key!("region")),
        );
        let shell_id = ShellId::compose(
            &crate::identity::FEATURE_EXTRUSION,
            feature_key
                .clone()
                .colon(cadmpeg_ir::identity_key!("shell")),
        );
        // Both caps state the same full-turn circle in section parameters, so
        // the cap pcurve is stated once and before the first record of this
        // body reaches the model. A refused lane here leaves no partial body
        // behind, so the model carries the absence of this one extrusion.
        let cap_geometry = {
            let mut refusal = crate::lane_refusal::LaneRefusals::new();
            let cap_record = format!("extrusion feature {feature_id} cap");
            let cap = circular_pcurve(
                section_center,
                radius,
                0.0,
                std::f64::consts::TAU,
                &cap_record,
                &mut refusal,
            );
            let records = refusal.take_records();
            match cap {
                Some(cap) if records.is_empty() => cap,
                _ => {
                    let detail = if records.is_empty() {
                        ".".to_owned()
                    } else {
                        format!(": {}", records.join("; "))
                    };
                    losses.push(
                        crate::loss::CreoLossCode::ExtrusionBodyRejected.note(format!(
                            "Extrusion body {body_id} states no cap pcurve; its B-rep was \
                         skipped{detail}"
                        )),
                    );
                    continue;
                }
            }
        };
        let center = section_point_in_model(transform, section_center);
        let seam =
            std::array::from_fn::<_, 3, _>(|axis| center[axis] + radius * transform.u_axis()[axis]);
        let sides = [("bottom", span.lower()), ("top", span.upper())];
        let mut face_ids = Vec::new();
        let mut cap_coedges = Vec::new();
        let mut side_coedges = Vec::new();
        for (side_index, (side, offset)) in sides.into_iter().enumerate() {
            let side_key = match side {
                "bottom" => cadmpeg_ir::identity_key!("bottom"),
                "top" => cadmpeg_ir::identity_key!("top"),
                _ => continue,
            };
            let cap_surface = SurfaceId::compose(
                &crate::identity::FEATURE_EXTRUSION,
                feature_key
                    .clone()
                    .colon(cadmpeg_ir::identity_key!("surface"))
                    .colon(&side_key),
            );
            let cap_face = FaceId::compose(
                &crate::identity::FEATURE_EXTRUSION,
                feature_key
                    .clone()
                    .colon(cadmpeg_ir::identity_key!("face"))
                    .colon(&side_key),
            );
            let cap_loop = LoopId::compose(
                &crate::identity::FEATURE_EXTRUSION,
                feature_key
                    .clone()
                    .colon(cadmpeg_ir::identity_key!("loop"))
                    .colon(&side_key),
            );
            let curve_id = CurveId::compose(
                &crate::identity::FEATURE_EXTRUSION,
                feature_key
                    .clone()
                    .colon(cadmpeg_ir::identity_key!("curve"))
                    .colon(&side_key),
            );
            let edge_id = EdgeId::compose(
                &crate::identity::FEATURE_EXTRUSION,
                feature_key
                    .clone()
                    .colon(cadmpeg_ir::identity_key!("edge"))
                    .colon(&side_key),
            );
            let point_id = PointId::compose(
                &crate::identity::FEATURE_EXTRUSION,
                feature_key
                    .clone()
                    .colon(cadmpeg_ir::identity_key!("point"))
                    .colon(&side_key),
            );
            let vertex_id = VertexId::compose(
                &crate::identity::FEATURE_EXTRUSION,
                feature_key
                    .clone()
                    .colon(cadmpeg_ir::identity_key!("vertex"))
                    .colon(&side_key),
            );
            let cap_coedge = CoedgeId::compose(
                &crate::identity::FEATURE_EXTRUSION,
                feature_key
                    .clone()
                    .colon(cadmpeg_ir::identity_key!("coedge"))
                    .colon(&side_key)
                    .colon(cadmpeg_ir::identity_key!("cap")),
            );
            let side_coedge = CoedgeId::compose(
                &crate::identity::FEATURE_EXTRUSION,
                feature_key
                    .clone()
                    .colon(cadmpeg_ir::identity_key!("coedge"))
                    .colon(&side_key)
                    .colon(cadmpeg_ir::identity_key!("side")),
            );
            let cap_pcurve = add_extrusion_pcurve(
                ir,
                annotations,
                PcurveId::compose(
                    &crate::identity::FEATURE_EXTRUSION,
                    feature_key
                        .clone()
                        .colon(cadmpeg_ir::identity_key!("pcurve"))
                        .colon(&side_key)
                        .colon(cadmpeg_ir::identity_key!("cap")),
                ),
                transform.offset,
                cap_geometry.clone(),
            )?;
            let side_pcurve = add_extrusion_pcurve(
                ir,
                annotations,
                PcurveId::compose(
                    &crate::identity::FEATURE_EXTRUSION,
                    feature_key
                        .clone()
                        .colon(cadmpeg_ir::identity_key!("pcurve"))
                        .colon(&side_key)
                        .colon(cadmpeg_ir::identity_key!("side")),
                ),
                transform.offset,
                line_pcurve([0.0, offset], [std::f64::consts::TAU, offset]).ok_or_else(|| {
                    cadmpeg_core::CodecError::malformed("extrusion pcurve geometry is invalid")
                })?,
            )?;
            ir.model.surfaces.push(Surface {
                id: cap_surface.clone(),
                geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
                    cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                        Point3::new(
                            transform.origin()[0] + offset * transform.normal()[0],
                            transform.origin()[1] + offset * transform.normal()[1],
                            transform.origin()[2] + offset * transform.normal()[2],
                        ),
                        transform.normal_vector(),
                        transform.u_axis_vector(),
                    )
                    .map_err(cadmpeg_core::CodecError::malformed)?,
                )),
                source_object: None,
            });
            ir.model.curves.push(Curve {
                id: curve_id.clone(),
                geometry: CurveGeometry::Solved(SolvedCurveGeometry::Circle(
                    cadmpeg_ir::geometry::analytic::CircleCurve::try_new(
                        Point3::new(
                            center[0] + offset * transform.normal()[0],
                            center[1] + offset * transform.normal()[1],
                            center[2] + offset * transform.normal()[2],
                        ),
                        transform.normal_vector(),
                        transform.u_axis_vector(),
                        radius,
                    )
                    .map_err(cadmpeg_core::CodecError::malformed)?,
                )),
                source_object: None,
            });
            ir.model.points.push(
                Point::new(
                    point_id.clone(),
                    Point3::new(
                        seam[0] + offset * transform.normal()[0],
                        seam[1] + offset * transform.normal()[1],
                        seam[2] + offset * transform.normal()[2],
                    ),
                    None,
                )
                .map_err(cadmpeg_core::CodecError::malformed)?,
            );
            ir.model.vertices.push(Vertex {
                id: vertex_id.clone(),
                point: point_id,
                tolerance: None,
            });
            ir.model.edges.push(Edge {
                id: edge_id.clone(),
                carrier: cadmpeg_ir::topology::EdgeCarrier::new(
                    Some(curve_id),
                    Some([0.0, std::f64::consts::TAU]),
                )
                .map_err(cadmpeg_core::CodecError::malformed)?,
                start: vertex_id.clone(),
                end: vertex_id,
                tolerance: None,
            });
            ir.model.loops.push(IrLoop {
                id: cap_loop.clone(),
                face: cap_face.clone(),
                boundary: cadmpeg_ir::topology::LoopBoundary::Ring(
                    cadmpeg_ir::topology::LoopRing::single(cap_coedge.clone()),
                ),
            });
            ir.model.coedges.push(Coedge {
                id: cap_coedge.clone(),
                owner_loop: cap_loop.clone(),
                edge: edge_id.clone(),
                radial_next: side_coedge.clone(),
                sense: if side_index == 0 {
                    Sense::Reversed
                } else {
                    Sense::Forward
                },
                pcurves: vec![PcurveUse {
                    pcurve: cap_pcurve,
                    isoparametric: None,
                    parameter_range: None,
                }],
                use_curve: None,
            });
            ir.model.faces.push(Face {
                id: cap_face.clone(),
                shell: shell_id.clone(),
                surface: cap_surface,
                sense: if side_index == 0 {
                    Sense::Reversed
                } else {
                    Sense::Forward
                },
                loops: cadmpeg_ir::topology::FaceLoops::unspecified(vec![cap_loop]),
                name: None,
                color: None,
                tolerance: None,
            });
            face_ids.push(cap_face);
            cap_coedges.push(cap_coedge);
            side_coedges.push((side_coedge, edge_id, side_pcurve));
        }
        let side_surface = SurfaceId::compose(
            &crate::identity::FEATURE_EXTRUSION,
            feature_key
                .clone()
                .colon(cadmpeg_ir::identity_key!("surface"))
                .colon(cadmpeg_ir::identity_key!("side")),
        );
        let side_face = FaceId::compose(
            &crate::identity::FEATURE_EXTRUSION,
            feature_key
                .clone()
                .colon(cadmpeg_ir::identity_key!("face"))
                .colon(cadmpeg_ir::identity_key!("side")),
        );
        let mut side_loops = Vec::new();
        ir.model.surfaces.push(Surface {
            id: side_surface.clone(),
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
                cadmpeg_ir::geometry::analytic::CylinderSurface::try_new(
                    Point3::from(center),
                    transform.normal_vector(),
                    transform.u_axis_vector(),
                    radius,
                )
                .map_err(cadmpeg_core::CodecError::malformed)?,
            )),
            source_object: None,
        });
        for (side_index, ((side, _), (coedge, edge, pcurve))) in
            sides.into_iter().zip(side_coedges).enumerate()
        {
            let side_key = match side {
                "bottom" => cadmpeg_ir::identity_key!("bottom"),
                "top" => cadmpeg_ir::identity_key!("top"),
                _ => continue,
            };
            let loop_id = LoopId::compose(
                &crate::identity::FEATURE_EXTRUSION,
                feature_key
                    .clone()
                    .colon(cadmpeg_ir::identity_key!("loop"))
                    .colon(cadmpeg_ir::identity_key!("side"))
                    .colon(&side_key),
            );
            ir.model.loops.push(IrLoop {
                id: loop_id.clone(),
                face: side_face.clone(),
                boundary: cadmpeg_ir::topology::LoopBoundary::Ring(
                    cadmpeg_ir::topology::LoopRing::single(coedge.clone()),
                ),
            });
            ir.model.coedges.push(Coedge {
                id: coedge.clone(),
                owner_loop: loop_id.clone(),
                edge,
                radial_next: cap_coedges[side_index].clone(),
                sense: if side_index == 0 {
                    Sense::Forward
                } else {
                    Sense::Reversed
                },
                pcurves: vec![PcurveUse {
                    pcurve,
                    isoparametric: None,
                    parameter_range: None,
                }],
                use_curve: None,
            });
            side_loops.push(loop_id);
        }
        ir.model.faces.push(Face {
            id: side_face.clone(),
            shell: shell_id.clone(),
            surface: side_surface,
            sense: Sense::Forward,
            loops: cadmpeg_ir::topology::FaceLoops::unspecified(side_loops),
            name: None,
            color: None,
            tolerance: None,
        });
        face_ids.push(side_face);
        ir.model.shells.push(
            match Shell::new(
                shell_id.clone(),
                region_id.clone(),
                face_ids,
                Vec::new(),
                Vec::new(),
            ) {
                Ok(shell) => shell,
                Err(_) => {
                    continue;
                }
            },
        );
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

fn resolved_circular_extrusion_profile(
    scan: &ContainerScan,
    ir: &CadIr,
    transform: &crate::placement::FeatureSectionTransform,
    feature_id: u32,
    sketch_id: &SketchId,
) -> Option<([f64; 2], f64)> {
    if let Some(sketch) = exactly_one(
        ir.model
            .sketches
            .iter()
            .filter(|sketch| sketch.id == *sketch_id),
    ) {
        if let [profile] = sketch.profiles.as_slice() {
            if let [entity_use] = profile.as_slice() {
                if let Some(SketchGeometryDefinition::Circle { center, radius }) =
                    exactly_one(ir.model.sketch_entities.iter().filter(|entity| {
                        entity.id() == &entity_use.entity && entity.sketch == *sketch_id
                    }))
                    .map(|entity| entity.geometry.definition())
                {
                    return Some(([center.u, center.v], radius.get()));
                }
            }
        }
    }
    let sweep = circular_sweep_geometry(scan, feature_id)?;
    sweep
        .section_definition_id
        .is_none_or(|definition_id| definition_id == transform.definition_id)
        .then_some(())?;
    circular_section_profile_from_cylinder(transform, &sweep.geometry)
}

pub(in super::super) fn circular_section_profile_from_cylinder(
    transform: &crate::placement::FeatureSectionTransform,
    geometry: &CylinderSurface,
) -> Option<([f64; 2], f64)> {
    let origin = geometry.origin().get();
    let axis = geometry.frame().axis().as_raw();
    let radius = geometry.radius().get();
    (dot([axis.x, axis.y, axis.z], transform.normal()).abs() >= 1.0 - EPS_AXIS_ALIGNMENT)
        .then_some(())?;
    let delta = [
        origin.x - transform.origin()[0],
        origin.y - transform.origin()[1],
        origin.z - transform.origin()[2],
    ];
    Some((
        [
            dot(delta, transform.u_axis()),
            dot(delta, transform.v_axis()),
        ],
        radius,
    ))
}
