// SPDX-License-Identifier: Apache-2.0
//! Circular extrusion B-rep transfer.

use super::super::feature_history::draft::feature_allows_additive_linear_extrusion;
use super::super::holes::sweep::circular_sweep_geometry;
use super::super::sketch::intersect::section_point_in_model;
use super::super::sketch_ids::model_sketch_id;
use super::super::uniqueness::{
    exactly_one_by, unique_feature_definition_for_transform, unique_feature_section_transform,
};
use super::extent::resolved_feature_extrusion_span;
use super::pcurves::{add_extrusion_pcurve, PcurveAdmission};
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

fn circular_identity<I>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    feature_id: u32,
    suffix: impl std::fmt::Display,
) -> Result<I, cadmpeg_core::CodecError>
where
    I: TryFrom<String, Error = cadmpeg_ir::ids::IdentityError>,
{
    crate::identity::compose_checked(
        ctx,
        &crate::identity::FEATURE_EXTRUSION,
        format_args!("{feature_id}:{suffix}"),
        "creo circular extrusion identity",
    )
}

fn circular_item<T>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    value: T,
    operation: &'static str,
) -> Result<Vec<T>, cadmpeg_core::CodecError> {
    let mut values = Vec::new();
    ctx.reserve_vec(&mut values, 1, operation)?;
    values.push(value);
    Ok(values)
}

fn copy_circular_pcurve(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    geometry: &cadmpeg_ir::geometry::pcurve::PcurveGeometry,
) -> Result<cadmpeg_ir::geometry::pcurve::PcurveGeometry, cadmpeg_core::CodecError> {
    let cadmpeg_ir::geometry::pcurve::PcurveGeometry::Nurbs { nurbs } = geometry else {
        return Err(cadmpeg_core::CodecError::malformed(
            "circular cap pcurve is not NURBS",
        ));
    };
    Ok(cadmpeg_ir::geometry::pcurve::PcurveGeometry::Nurbs {
        nurbs: super::nurbs::copy_pcurve_nurbs(
            ctx,
            nurbs,
            "creo circular cap pcurve knot copy",
            "creo circular cap pcurve pole copy",
        )?,
    })
}

use crate::lane_refusal::JoinedLaneRecords;

pub(in super::super) fn transfer_resolved_circular_extrusion_breps(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
    source_carriers: &mut crate::decode::source_carriers::SourceUnitCarriers,
) -> Result<usize, cadmpeg_core::CodecError> {
    let mut transferred = 0;
    for transform in ctx.admit_iter(&scan.features.section_transforms, "creo sweep transform scan")? {
        if unique_feature_section_transform(
            ctx,
            &scan.features.section_transforms,
            transform.definition_id,
            transform.offset,
        )?
        .is_none()
        {
            continue;
        }
        let Some(feature_id) = transform.feature_id else {
            continue;
        };
        if !feature_allows_additive_linear_extrusion(ctx, scan, feature_id)?
            || !feature_is_first_material_operation(ctx, scan, feature_id)?
        {
            continue;
        }
        let Some(definition) =
            unique_feature_definition_for_transform(ctx, &scan.features.definitions, transform)?
        else {
            continue;
        };
        let Some(sketch_id) = model_sketch_id(ctx, scan, definition)? else {
            continue;
        };
        let Some((section_center, radius)) = resolved_circular_extrusion_profile(
            ctx,
            scan,
            ir,
            source_carriers,
            transform,
            feature_id,
            &sketch_id,
        )?
        else {
            continue;
        };
        let Some(span) =
            resolved_feature_extrusion_span(ctx, scan, ir, source_carriers, definition, transform)?
        else {
            continue;
        };
        let body_id: BodyId = circular_identity(ctx, feature_id, "body")?;
        if ctx.any_by(&ir.model.bodies, |body| ctx.equal(body.id.as_str(), body_id.as_str(), "creo model identity comparison"), "creo model identity scan")? {
            continue;
        }
        let region_id: RegionId = circular_identity(ctx, feature_id, "region")?;
        let shell_id: ShellId = circular_identity(ctx, feature_id, "shell")?;
        // Both caps state the same full-turn circle in section parameters, so
        // the cap pcurve is stated once and before the first record of this
        // body reaches the model. A refused lane here leaves no partial body
        // behind, so the model carries the absence of this one extrusion.
        let mut cap_geometry_storage = ctx.reserve_scoped(0, "creo circular cap probe")?;
        let cap_geometry = {
            let mut refusal = crate::lane_refusal::LaneRefusals::new();
            let (cap_record, _cap_record_reservation) = ctx.format_scoped(
                format_args!("extrusion feature {feature_id} cap"),
                "creo circular cap record",
            )?;
            let cap = cap_geometry_storage.with_storage(|| circular_pcurve(
                ctx,
                section_center,
                radius,
                0.0,
                std::f64::consts::TAU,
                &cap_record,
                &mut refusal,
            ))?;
            let records = refusal.take_records_checked()?;
            match cap {
                Some(cap) if records.is_empty() => cap,
                _ => {
                    let note = if records.is_empty() {
                        ctx.format_retained(
                            format_args!("Extrusion body {body_id} states no cap pcurve; its B-rep was skipped."),
                            "creo circular extrusion rejection text",
                        )?
                    } else {
                        ctx.format_retained(
                            format_args!("Extrusion body {body_id} states no cap pcurve; its B-rep was skipped: {}", JoinedLaneRecords(&records)),
                            "creo circular extrusion rejection text",
                        )?
                    };
                    ctx.reserve_vec(losses, 1, "creo circular extrusion losses")?;
                    losses.push(crate::loss::CreoLossCode::ExtrusionBodyRejected.note(note));
                    continue;
                }
            }
        };
        let center = section_point_in_model(transform, section_center);
        let seam =
            std::array::from_fn::<_, 3, _>(|axis| center[axis] + radius * transform.u_axis()[axis]);
        let side_surface_geometry = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
            CylinderSurface::try_new(
                Point3::from(center),
                transform.normal_vector(),
                transform.u_axis_vector(),
                radius,
            )
            .map_err(cadmpeg_core::CodecError::malformed)?,
        ));
        let side_surface: SurfaceId = circular_identity(ctx, feature_id, "surface:side")?;
        let sides = [("bottom", span.lower()), ("top", span.upper())];
        let mut face_ids = Vec::new();
        let (mut cap_coedges, mut cap_coedge_storage) = ctx.temporary_vec(0, "creo circular cap coedge IDs")?;
        let mut side_coedges = Vec::new();
        for (side_index, (side, offset)) in sides.into_iter().enumerate() {
            let cap_surface: SurfaceId =
                circular_identity(ctx, feature_id, format_args!("surface:{side}"))?;
            let cap_face: FaceId = circular_identity(ctx, feature_id, format_args!("face:{side}"))?;
            let cap_loop: LoopId = circular_identity(ctx, feature_id, format_args!("loop:{side}"))?;
            let curve_id: CurveId =
                circular_identity(ctx, feature_id, format_args!("curve:{side}"))?;
            let edge_id: EdgeId = circular_identity(ctx, feature_id, format_args!("edge:{side}"))?;
            let point_id: PointId =
                circular_identity(ctx, feature_id, format_args!("point:{side}"))?;
            let vertex_id: VertexId =
                circular_identity(ctx, feature_id, format_args!("vertex:{side}"))?;
            let cap_coedge: CoedgeId =
                circular_identity(ctx, feature_id, format_args!("coedge:{side}:cap"))?;
            let side_coedge: CoedgeId =
                circular_identity(ctx, feature_id, format_args!("coedge:{side}:side"))?;
            let cap_surface_geometry = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
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
            ));
            let cap_pcurve = add_extrusion_pcurve(
                ctx,
                ir,
                annotations,
                PcurveAdmission::Pending(source_carriers, &cap_surface_geometry),
                circular_identity::<PcurveId>(ctx, feature_id, format_args!("pcurve:{side}:cap"))?,
                transform.offset,
                copy_circular_pcurve(ctx, &cap_geometry)?,
            )?;
            let side_pcurve = add_extrusion_pcurve(
                ctx,
                ir,
                annotations,
                PcurveAdmission::Pending(source_carriers, &side_surface_geometry),
                circular_identity::<PcurveId>(ctx, feature_id, format_args!("pcurve:{side}:side"))?,
                transform.offset,
                line_pcurve([0.0, offset], [std::f64::consts::TAU, offset]).ok_or_else(|| {
                    cadmpeg_core::CodecError::malformed("extrusion pcurve geometry is invalid")
                })?,
            )?;
            ctx.charge_entities(1, "admit Creo model surfaces")?;
            source_carriers.admit_surface(
                ctx,
                ir,
                Surface {
                    id: cap_surface
                        .try_clone_for_decode(ctx, "creo circular extrusion identity copy")?,
                    geometry: cap_surface_geometry,
                    source_object: None,
                },
            )?;
            ctx.charge_entities(1, "admit Creo model curves")?;
            source_carriers.admit_curve(
                ctx,
                ir,
                Curve {
                    id: curve_id
                        .try_clone_for_decode(ctx, "creo circular extrusion identity copy")?,
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
                },
            )?;
            let finite_position = cadmpeg_ir::features::FinitePoint3::new(Point3::new(
                seam[0] + offset * transform.normal()[0],
                seam[1] + offset * transform.normal()[1],
                seam[2] + offset * transform.normal()[2],
            ))
            .ok_or(Point::NON_FINITE_POSITION)
            .map_err(cadmpeg_core::CodecError::malformed)?;
            ctx.charge_entities(1, "admit Creo model points")?;
            source_carriers.admit_point(
                ctx,
                ir,
                Point::new(
                    point_id.try_clone_for_decode(ctx, "creo circular extrusion identity copy")?,
                    finite_position,
                    None,
                ),
            )?;
            ctx.charge_entities(1, "admit Creo model vertices")?;
            source_carriers.admit_vertex(
                ctx,
                ir,
                Vertex {
                    id: vertex_id
                        .try_clone_for_decode(ctx, "creo circular extrusion identity copy")?,
                    point: point_id,
                    tolerance: None,
                },
            )?;
            ctx.charge_entities(1, "admit Creo model edges")?;
            source_carriers.admit_edge(
                ctx,
                ir,
                Edge {
                    id: edge_id
                        .try_clone_for_decode(ctx, "creo circular extrusion identity copy")?,
                    carrier: cadmpeg_ir::topology::EdgeCarrier::new(
                        Some(curve_id),
                        Some([0.0, std::f64::consts::TAU]),
                    )
                    .map_err(cadmpeg_core::CodecError::malformed)?,
                    start: vertex_id
                        .try_clone_for_decode(ctx, "creo circular extrusion identity copy")?,
                    end: vertex_id,
                    tolerance: None,
                },
            )?;
            ctx.charge_entities(1, "admit Creo model loops")?;
            ctx.reserve_vec(
                &mut ir.model.loops,
                1,
                "creo model circular extrusion loops",
            )?;
            ir.model.loops.push(IrLoop {
                id: cap_loop.try_clone_for_decode(ctx, "creo circular extrusion identity copy")?,
                face: cap_face
                    .try_clone_for_decode(ctx, "creo circular extrusion identity copy")?,
                boundary: cadmpeg_ir::topology::LoopBoundary::Ring(
                    cadmpeg_ir::topology::LoopRing::new(
                        ctx,
                        circular_item(
                            ctx,
                            cap_coedge.try_clone_for_decode(
                                ctx,
                                "creo circular extrusion identity copy",
                            )?,
                            "creo circular cap ring coedges",
                        )?,
                        Vec::new(),
                    )
                    .map_err(cadmpeg_core::CodecError::from)?
                    .map_err(cadmpeg_core::CodecError::malformed)?,
                ),
            });
            ctx.charge_entities(1, "admit Creo model coedges")?;
            source_carriers.admit_coedge(
                ctx,
                ir,
                Coedge {
                    id: cap_coedge
                        .try_clone_for_decode(ctx, "creo circular extrusion identity copy")?,
                    owner_loop: cap_loop
                        .try_clone_for_decode(ctx, "creo circular extrusion identity copy")?,
                    edge: edge_id
                        .try_clone_for_decode(ctx, "creo circular extrusion identity copy")?,
                    radial_next: side_coedge
                        .try_clone_for_decode(ctx, "creo circular extrusion identity copy")?,
                    sense: if side_index == 0 {
                        Sense::Reversed
                    } else {
                        Sense::Forward
                    },
                    pcurves: circular_item(
                        ctx,
                        PcurveUse {
                            pcurve: cap_pcurve,
                            isoparametric: None,
                            parameter_range: None,
                        },
                        "creo circular cap coedge pcurves",
                    )?,
                    use_curve: None,
                },
            )?;
            ctx.charge_entities(1, "admit Creo model faces")?;
            source_carriers.admit_face(
                ctx,
                ir,
                Face {
                    id: cap_face
                        .try_clone_for_decode(ctx, "creo circular extrusion identity copy")?,
                    shell: shell_id
                        .try_clone_for_decode(ctx, "creo circular extrusion identity copy")?,
                    surface: cap_surface,
                    sense: if side_index == 0 {
                        Sense::Reversed
                    } else {
                        Sense::Forward
                    },
                    loops: cadmpeg_ir::topology::FaceLoops::unspecified(circular_item(
                        ctx,
                        cap_loop,
                        "creo circular cap face loops",
                    )?),
                    name: None,
                    color: None,
                    tolerance: None,
                },
            )?;
            ctx.reserve_vec(&mut face_ids, 1, "creo circular shell face IDs")?;
            face_ids.push(cap_face);
            ctx.reserve_scoped_vec(&mut cap_coedge_storage, &mut cap_coedges, 1, "creo circular cap coedge IDs")?;
            cap_coedges.push(cap_coedge);
            ctx.reserve_vec(&mut side_coedges, 1, "creo circular side coedge rows")?;
            side_coedges.push((side_coedge, edge_id, side_pcurve));
        }
        let side_face: FaceId = circular_identity(ctx, feature_id, "face:side")?;
        let mut side_loops = Vec::new();
        ctx.charge_entities(1, "admit Creo model surfaces")?;
        source_carriers.admit_surface(
            ctx,
            ir,
            Surface {
                id: side_surface
                    .try_clone_for_decode(ctx, "creo circular extrusion identity copy")?,
                geometry: side_surface_geometry,
                source_object: None,
            },
        )?;
        for (side_index, ((side, _), (coedge, edge, pcurve))) in
            sides.into_iter().zip(side_coedges).enumerate()
        {
            let loop_id: LoopId =
                circular_identity(ctx, feature_id, format_args!("loop:side:{side}"))?;
            ctx.charge_entities(1, "admit Creo model loops")?;
            ctx.reserve_vec(
                &mut ir.model.loops,
                1,
                "creo model circular extrusion loops",
            )?;
            ir.model.loops.push(IrLoop {
                id: loop_id.try_clone_for_decode(ctx, "creo circular extrusion identity copy")?,
                face: side_face
                    .try_clone_for_decode(ctx, "creo circular extrusion identity copy")?,
                boundary: cadmpeg_ir::topology::LoopBoundary::Ring(
                    cadmpeg_ir::topology::LoopRing::new(
                        ctx,
                        circular_item(
                            ctx,
                            coedge.try_clone_for_decode(
                                ctx,
                                "creo circular extrusion identity copy",
                            )?,
                            "creo circular side ring coedges",
                        )?,
                        Vec::new(),
                    )
                    .map_err(cadmpeg_core::CodecError::from)?
                    .map_err(cadmpeg_core::CodecError::malformed)?,
                ),
            });
            ctx.charge_entities(1, "admit Creo model coedges")?;
            source_carriers.admit_coedge(
                ctx,
                ir,
                Coedge {
                    id: coedge
                        .try_clone_for_decode(ctx, "creo circular extrusion identity copy")?,
                    owner_loop: loop_id
                        .try_clone_for_decode(ctx, "creo circular extrusion identity copy")?,
                    edge,
                    radial_next: cap_coedges[side_index]
                        .try_clone_for_decode(ctx, "creo circular extrusion identity copy")?,
                    sense: if side_index == 0 {
                        Sense::Forward
                    } else {
                        Sense::Reversed
                    },
                    pcurves: circular_item(
                        ctx,
                        PcurveUse {
                            pcurve,
                            isoparametric: None,
                            parameter_range: None,
                        },
                        "creo circular side coedge pcurves",
                    )?,
                    use_curve: None,
                },
            )?;
            ctx.reserve_vec(&mut side_loops, 1, "creo circular side face loops")?;
            side_loops.push(loop_id);
        }
        ctx.charge_entities(1, "admit Creo model faces")?;
        source_carriers.admit_face(
            ctx,
            ir,
            Face {
                id: side_face.try_clone_for_decode(ctx, "creo circular extrusion identity copy")?,
                shell: shell_id
                    .try_clone_for_decode(ctx, "creo circular extrusion identity copy")?,
                surface: side_surface,
                sense: Sense::Forward,
                loops: cadmpeg_ir::topology::FaceLoops::unspecified(side_loops),
                name: None,
                color: None,
                tolerance: None,
            },
        )?;
        ctx.reserve_vec(&mut face_ids, 1, "creo circular shell face IDs")?;
        face_ids.push(side_face);
        ctx.charge_entities(1, "admit Creo model shells")?;
        ctx.reserve_vec(
            &mut ir.model.shells,
            1,
            "creo model circular extrusion shells",
        )?;
        ir.model.shells.push(
            match Shell::new(
                shell_id.try_clone_for_decode(ctx, "creo circular extrusion identity copy")?,
                region_id.try_clone_for_decode(ctx, "creo circular extrusion identity copy")?,
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
        ctx.charge_entities(1, "admit Creo model regions")?;
        ctx.reserve_vec(
            &mut ir.model.regions,
            1,
            "creo model circular extrusion regions",
        )?;
        ir.model.regions.push(Region {
            id: region_id.try_clone_for_decode(ctx, "creo circular extrusion identity copy")?,
            body: body_id.try_clone_for_decode(ctx, "creo circular extrusion identity copy")?,
            shells: circular_item(ctx, shell_id, "creo circular region shells")?,
        });
        ctx.charge_entities(1, "admit Creo model bodies")?;
        source_carriers.admit_body(
            ctx,
            ir,
            Body {
                id: body_id,
                kind: BodyKind::Solid,
                regions: circular_item(ctx, region_id, "creo circular body regions")?,
                transform: None,
                name: None,
                color: None,
                visible: None,
            },
        )?;
        transferred += 1;
    }
    Ok(transferred)
}

fn resolved_circular_extrusion_profile(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &CadIr,
    source_carriers: &crate::decode::source_carriers::SourceUnitCarriers,
    transform: &crate::placement::FeatureSectionTransform,
    feature_id: u32,
    sketch_id: &SketchId,
) -> Result<Option<([f64; 2], f64)>, cadmpeg_core::CodecError> {
    if let Some(sketch) = exactly_one_by(ctx, &ir.model.sketches, |sketch| ctx.equal(sketch.id.as_str(), sketch_id.as_str(), "creo circular profile sketch identity"), "creo circular profile sketch scan")? {
        if let [profile] = sketch.profiles.as_slice() {
            if let [entity_use] = profile.as_slice() {
                if let Some(SketchGeometryDefinition::Circle { center, radius }) = exactly_one_by(ctx, &ir.model.sketch_entities, |entity| Ok(ctx.equal(entity.id().as_str(), entity_use.entity.as_str(), "creo circular profile entity identity")? && ctx.equal(entity.sketch.as_str(), sketch_id.as_str(), "creo circular profile sketch identity")?), "creo circular profile entity scan")?.map(|entity| source_carriers.sketch_geometry(entity).definition()) {
                    return Ok(Some(([center.u, center.v], radius.get())));
                }
            }
        }
    }
    let Some(sweep) = circular_sweep_geometry(ctx, scan, feature_id)? else {
        return Ok(None);
    };
    if sweep
        .section_definition_id
        .is_some_and(|definition_id| definition_id != transform.definition_id)
    {
        return Ok(None);
    }
    Ok(circular_section_profile_from_cylinder(
        transform,
        &sweep.geometry,
    ))
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

#[cfg(test)]
mod tests {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    use cadmpeg_ir::document::CadIr;
    use cadmpeg_ir::geometry::pcurve::PcurveGeometry;
    use cadmpeg_ir::ids::BodyId;
    use cadmpeg_ir::math::Point2;
    use cadmpeg_ir::sketches::{
        Sketch, SketchEntity, SketchEntityId, SketchEntityUse, SketchGeometry,
        SketchGeometryDefinition, SketchId, SketchPlacement, SketchProfiles,
    };

    #[test]
    fn circular_extrusion_identity_refuses_below_retained_limit() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let error =
            super::circular_identity::<BodyId>(&ctx, 7, "body").expect_err("identity refused");
        assert!(matches!(error, CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::RetainedBytes
                && resource.operation == "creo circular extrusion identity"));
        let arena = DecodeArena::new();
        let policy = DecodePolicy::service();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        assert_eq!(
            super::circular_identity::<BodyId>(&ctx, 7, "body").expect("identity"),
            BodyId::compose(
                &crate::identity::FEATURE_EXTRUSION,
                cadmpeg_ir::ids::IdentityKey::from(7).colon(cadmpeg_ir::identity_key!("body"))
            ),
        );
    }

    #[test]
    fn circular_cap_copy_refuses_each_nested_collection_limit() {
        let arena = DecodeArena::new();
        let policy = DecodePolicy::service();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let geometry = super::circular_pcurve(
            &ctx,
            [0.0, 0.0],
            1.0,
            0.0,
            std::f64::consts::TAU,
            &"cap",
            &mut crate::lane_refusal::LaneRefusals::new(),
        )
        .expect("service resources")
        .expect("cap pcurve");
        let PcurveGeometry::Nurbs { nurbs } = &geometry else {
            panic!("circular cap is NURBS")
        };
        for (limit, operation) in [
            (0, "creo circular cap pcurve knot copy"),
            (nurbs.knots().len(), "creo circular cap pcurve pole copy"),
        ] {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_collection_items = cadmpeg_core::decode::u64_from_index(limit);
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            let error = super::copy_circular_pcurve(&ctx, &geometry).expect_err("copy refused");
            assert!(matches!(error, CodecError::ResourceLimit(resource)
                if resource.dimension == ResourceDimension::CollectionItems
                    && resource.operation == operation));
        }
        assert_eq!(
            super::copy_circular_pcurve(&ctx, &geometry).expect("copy"),
            geometry
        );
    }

    #[test]
    fn circular_extrusion_profile_uses_source_sketch_geometry_after_admission() {
        let sketch_id = SketchId::mint("creo:test:sketch#1").expect("identity grammar");
        let entity_id =
            SketchEntityId::mint("creo:test:sketch_entity#1").expect("identity grammar");
        let mut ir = CadIr::empty();
        ir.model.sketches.push(Sketch {
            id: sketch_id.clone(),
            name: None,
            configuration: None,
            visible: None,
            placement: SketchPlacement::Unresolved {},
            profiles: SketchProfiles::try_from(vec![vec![SketchEntityUse {
                entity: entity_id.clone(),
                reversed: false,
            }]])
            .expect("valid profile"),
            native_ref: None,
        });
        let mut carriers = crate::decode::source_carriers::SourceUnitCarriers::new(
            cadmpeg_ir::scalar::PositiveReal::new(25.4),
        );
        crate::decode::with_test_decode_ctx(|ctx| {
            carriers.admit_sketch_entities(
                ctx,
                &mut ir,
                vec![SketchEntity::new(
                    entity_id,
                    sketch_id.clone(),
                    SketchGeometry::try_from(SketchGeometryDefinition::Circle {
                        center: Point2::new(1.0, 0.0),
                        radius: cadmpeg_ir::scalar::Length::new(2.0).expect("finite radius"),
                    })
                    .expect("source circle"),
                )],
            )
        })
        .expect("millimeter admission");
        let scan = crate::test_support::empty_container_scan();
        let transform = crate::placement::FeatureSectionTransform::new(
            1,
            Some(1),
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            0,
        )
        .expect("section transform");
        assert_eq!(
            crate::decode::with_test_decode_ctx(|ctx| super::resolved_circular_extrusion_profile(
                ctx, &scan, &ir, &carriers, &transform, 1, &sketch_id,
            ))
            .expect("service resources"),
            Some(([1.0, 0.0], 2.0))
        );
    }
}
