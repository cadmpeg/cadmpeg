// SPDX-License-Identifier: Apache-2.0
//! Resolved revolution B-rep transfer.

use super::super::feature_history::axes::revolution_axis_for_transfer;
use super::super::sketch::intersect::section_point_in_model;
use super::super::sketch_ids::model_sketch_id;
use super::super::uniqueness::{
    unique_feature_definition_for_transform, unique_feature_section_transform,
};
use super::pcurves::{
    add_extrusion_pcurve, revolution_face_sense, revolution_profile_boundary_pcurve,
    revolved_brep_surface, PcurveAdmission, RevolutionBoundary,
};
use super::profiles::{extrusion_profile_signed_area, resolved_sketch_profiles};
use super::surfaces::revolved_section_circle;
use crate::container::ContainerScan;
use crate::decode::sketch_transfer::recipe::{
    current_additive_feature_recipe, feature_is_first_material_operation,
    feature_revolution_extent, unique_feature_revolution_extent,
};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::{Curve, Surface};
use cadmpeg_ir::ids::{
    BodyId, CoedgeId, CurveId, EdgeId, FaceId, LoopId, PcurveId, PointId, RegionId, ShellId,
    SurfaceId, VertexId,
};
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::topology::{
    Body, BodyKind, Coedge, Edge, Face, Loop as IrLoop, PcurveUse, Point, Region, Sense, Shell,
    Vertex,
};
use cadmpeg_ir::AnnotationBuilder;

fn revolution_identity<I>(
    feature_id: u32,
    suffix: impl std::fmt::Display,
) -> Result<I, cadmpeg_core::CodecError>
where
    I: TryFrom<String, Error = cadmpeg_ir::ids::IdentityError>,
{
    crate::identity::compose_checked(
        &crate::identity::FEATURE_REVOLUTION,
        format_args!("{feature_id}:{suffix}"),
    )
}

struct JoinedLaneRecords<'a>(&'a [String]);

impl std::fmt::Display for JoinedLaneRecords<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for (index, record) in self.0.iter().enumerate() {
            if index != 0 {
                formatter.write_str("; ")?;
            }
            formatter.write_str(record)?;
        }
        Ok(())
    }
}

fn push_revolution_loss(
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
    feature_id: u32,
    stem: &'static str,
    records: &[String],
) {
    let message = if records.is_empty() {
        format!("Revolution feature {feature_id} {stem}.")
    } else {
        format!(
            "Revolution feature {feature_id} {stem}: {}",
            JoinedLaneRecords(records)
        )
    };
    losses.push(crate::loss::CreoLossCode::BrepTransferIncomplete.note(message));
}

pub(in super::super) fn transfer_resolved_revolution_breps(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
    source_carriers: &mut crate::decode::source_carriers::SourceUnitCarriers,
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
        if current_additive_feature_recipe(&scan.features.operations, feature_id)
            != Some(crate::feature::operations::FeatureRecipeKind::Revolve)
            || !feature_is_first_material_operation(scan, feature_id)
            || unique_feature_revolution_extent(&scan.features.revolution_extents, feature_id)
                .is_none()
        {
            continue;
        }
        let Some(definition) =
            unique_feature_definition_for_transform(&scan.features.definitions, transform)
        else {
            continue;
        };
        let extent = feature_revolution_extent(scan, feature_id);
        let Some(axis) = revolution_axis_for_transfer(
            ctx,
            scan,
            ir,
            source_carriers,
            feature_id,
            (definition, transform),
            extent.as_ref(),
        )?
        else {
            continue;
        };
        let Some(sketch_id) = model_sketch_id(scan, definition) else {
            continue;
        };
        let Some(mut profiles) = resolved_sketch_profiles(ctx, ir, source_carriers, &sketch_id, 2)?
        else {
            continue;
        };
        let [profile] = profiles.as_mut_slice() else {
            continue;
        };
        let Some(area) = extrusion_profile_signed_area(ctx, profile)? else {
            continue;
        };
        let vertex_curves = profile
            .iter()
            .map(|entity| revolved_section_circle(transform, entity.start(), &axis))
            .collect::<Vec<_>>();
        let mut refusal = crate::lane_refusal::LaneRefusals::new();
        let mut surfaces = Vec::new();
        let mut complete = true;
        for (index, entity) in profile.iter().enumerate() {
            let Some(geometry) = entity.geometry().to_sketch(ctx)? else {
                complete = false;
                break;
            };
            let Some(surface) = revolved_brep_surface(
                ctx,
                transform,
                &geometry,
                entity.reversed(),
                &axis,
                &format_args!("revolution feature {feature_id} profile segment {index}"),
                &mut refusal,
            )?
            else {
                complete = false;
                break;
            };
            surfaces.push(surface);
        }
        let surface_geometries = complete.then_some(surfaces);
        let Some(surface_geometries) = surface_geometries else {
            let records = refusal.take_records();
            push_revolution_loss(
                losses,
                feature_id,
                "states no revolved surface; its B-rep was skipped",
                &records,
            );
            continue;
        };
        let mut boundary_rows = Vec::new();
        let mut complete = true;
        for (index, segment) in profile.iter().enumerate() {
            let next = (index + 1) % profile.len();
            if vertex_curves[index].is_none() && vertex_curves[next].is_none() {
                complete = false;
                break;
            }
            let mut segment_boundaries = Vec::new();
            for (section_point, present, boundary) in [
                (
                    segment.start(),
                    vertex_curves[index].is_some(),
                    RevolutionBoundary::Start,
                ),
                (
                    segment.end(),
                    vertex_curves[next].is_some(),
                    RevolutionBoundary::End,
                ),
            ] {
                if !present {
                    continue;
                }
                let record = format!(
                    "revolution feature {feature_id} profile segment {index} boundary {}",
                    boundary.key()
                );
                let mut diagnostics =
                    crate::lane_refusal::LaneRefusalContext::new(&record, &mut refusal);
                let Some(row) = PrevalidatedRevolutionBoundary::new(
                    ctx,
                    transform,
                    segment,
                    &surface_geometries[index],
                    &axis,
                    (section_point, boundary),
                    &mut diagnostics,
                )?
                else {
                    complete = false;
                    break;
                };
                segment_boundaries.push(row);
            }
            if !complete {
                break;
            }
            boundary_rows.push(segment_boundaries);
        }
        let boundaries = complete.then_some(boundary_rows);
        let Some(boundaries) = boundaries else {
            let records = refusal.take_records();
            push_revolution_loss(
                losses,
                feature_id,
                "has an unresolved boundary pcurve; its B-rep was skipped",
                &records,
            );
            continue;
        };
        let mut face_senses = Vec::new();
        let mut complete = true;
        for (index, (segment, surface)) in profile.iter().zip(&surface_geometries).enumerate() {
            let Some(sense) = revolution_face_sense(
                ctx,
                transform,
                segment,
                surface,
                &axis,
                area.get(),
                (
                    &format_args!(
                        "revolution feature {feature_id} profile segment {index} face sense"
                    ),
                    &mut refusal,
                ),
            )?
            else {
                complete = false;
                break;
            };
            face_senses.push(sense);
        }
        let face_senses = complete.then_some(face_senses);
        let Some(face_senses) = face_senses else {
            let records = refusal.take_records();
            push_revolution_loss(
                losses,
                feature_id,
                "states no face sense; its B-rep was skipped",
                &records,
            );
            continue;
        };
        let body_id: BodyId = revolution_identity(feature_id, "body")?;
        if ir.model.bodies.iter().any(|body| body.id == body_id) {
            continue;
        }
        let region_id: RegionId = revolution_identity(feature_id, "region")?;
        let shell_id: ShellId = revolution_identity(feature_id, "shell")?;
        let count = profile.len();
        let mut edges = ctx.alloc_filled(count, None, "creo revolution profile edges")?;
        for (index, (entity, curve_geometry)) in profile.iter().zip(vertex_curves).enumerate() {
            let Some(curve_geometry) = curve_geometry else {
                continue;
            };
            let Ok(curve_geometry) = cadmpeg_ir::geometry::CurveGeometry::try_from(curve_geometry)
            else {
                continue;
            };
            let curve_id: CurveId =
                revolution_identity(feature_id, format_args!("curve:vertex:{index}"))?;
            let point_id: PointId =
                revolution_identity(feature_id, format_args!("point:vertex:{index}"))?;
            let vertex_id: VertexId =
                revolution_identity(feature_id, format_args!("vertex:{index}"))?;
            let edge_id: EdgeId =
                revolution_identity(feature_id, format_args!("edge:vertex:{index}"))?;
            let position = section_point_in_model(transform, entity.start());
            ctx.charge_entities(1, "admit Creo model curves")?;
            source_carriers.admit_curve(
                ctx,
                ir,
                Curve {
                    parameter_range: None,
                    id: curve_id.try_clone_for_decode(ctx, "creo revolution identity copy")?,
                    geometry: curve_geometry,
                    source_object: None,
                },
            )?;
            let finite_position = cadmpeg_ir::features::FinitePoint3::new(Point3::from(position))
                .ok_or(Point::NON_FINITE_POSITION)
                .map_err(cadmpeg_core::CodecError::malformed)?;
            ctx.charge_entities(1, "admit Creo model points")?;
            source_carriers.admit_point(
                ir,
                Point::new(
                    point_id.try_clone_for_decode(ctx, "creo revolution identity copy")?,
                    finite_position,
                    None,
                ),
            )?;
            ctx.charge_entities(1, "admit Creo model vertices")?;
            source_carriers.admit_vertex(
                ir,
                Vertex {
                    id: vertex_id.try_clone_for_decode(ctx, "creo revolution identity copy")?,
                    point: point_id,
                    tolerance: None,
                },
            )?;
            ctx.charge_entities(1, "admit Creo model edges")?;
            source_carriers.admit_edge(
                ctx,
                ir,
                Edge {
                    id: edge_id.try_clone_for_decode(ctx, "creo revolution identity copy")?,
                    carrier: cadmpeg_ir::topology::EdgeCarrier::new(
                        Some(curve_id),
                        Some([0.0, std::f64::consts::TAU]),
                    )
                    .map_err(cadmpeg_core::CodecError::malformed)?,
                    start: vertex_id.try_clone_for_decode(ctx, "creo revolution identity copy")?,
                    end: vertex_id,
                    tolerance: None,
                },
            )?;
            edges[index] = Some(edge_id);
        }
        let mut faces = Vec::new();
        for (index, ((surface_geometry, face_sense), boundaries)) in surface_geometries
            .into_iter()
            .zip(face_senses)
            .zip(boundaries)
            .enumerate()
        {
            let next = (index + 1) % count;
            let surface_id: SurfaceId =
                revolution_identity(feature_id, format_args!("surface:{index}"))?;
            let face_id: FaceId = revolution_identity(feature_id, format_args!("face:{index}"))?;
            ctx.charge_entities(1, "admit Creo model surfaces")?;
            source_carriers.admit_surface(
                ctx,
                ir,
                Surface {
                    id: surface_id.try_clone_for_decode(ctx, "creo revolution identity copy")?,
                    geometry: surface_geometry,
                    source_object: None,
                },
            )?;
            let mut loops = Vec::new();
            for PrevalidatedRevolutionBoundary {
                boundary,
                geometry: pcurve_geometry,
            } in boundaries
            {
                let (vertex_index, sense) = match boundary {
                    RevolutionBoundary::Start => (index, Sense::Reversed),
                    RevolutionBoundary::End => (next, Sense::Forward),
                };
                let Some(edge_id) = edges[vertex_index]
                    .as_ref()
                    .map(|edge| edge.try_clone_for_decode(ctx, "creo revolution identity copy"))
                    .transpose()?
                else {
                    continue;
                };
                let boundary_key = boundary.key();
                let loop_id: LoopId =
                    revolution_identity(feature_id, format_args!("loop:{index}:{boundary_key}"))?;
                let coedge_id: CoedgeId =
                    revolution_identity(feature_id, format_args!("coedge:{index}:{boundary_key}"))?;
                let radial_index = match boundary {
                    RevolutionBoundary::Start => (index + count - 1) % count,
                    RevolutionBoundary::End => next,
                };
                let radial_boundary = boundary.opposite().key();
                let pcurve = add_extrusion_pcurve(
                    ctx,
                    ir,
                    annotations,
                    PcurveAdmission::Existing(source_carriers, &surface_id),
                    revolution_identity::<PcurveId>(
                        feature_id,
                        format_args!("pcurve:{index}:{boundary_key}"),
                    )?,
                    transform.offset,
                    pcurve_geometry,
                )?;
                let ring_coedges =
                    vec![coedge_id.try_clone_for_decode(ctx, "creo revolution identity copy")?];
                ctx.charge_entities(1, "admit Creo model loops")?;
                ir.model.loops.push(IrLoop {
                    id: loop_id.try_clone_for_decode(ctx, "creo revolution identity copy")?,
                    face: face_id.try_clone_for_decode(ctx, "creo revolution identity copy")?,
                    boundary: cadmpeg_ir::topology::LoopBoundary::Ring(
                        cadmpeg_ir::topology::LoopRing::new(ctx, ring_coedges, Vec::new())
                            .map_err(cadmpeg_core::CodecError::from)?
                            .map_err(cadmpeg_core::CodecError::malformed)?,
                    ),
                });
                let pcurve_uses = vec![PcurveUse {
                    pcurve,
                    isoparametric: None,
                    parameter_range: None,
                }];
                ctx.charge_entities(1, "admit Creo model coedges")?;
                source_carriers.admit_coedge(
                    ctx,
                    ir,
                    Coedge {
                        id: coedge_id.try_clone_for_decode(ctx, "creo revolution identity copy")?,
                        owner_loop: loop_id
                            .try_clone_for_decode(ctx, "creo revolution identity copy")?,
                        edge: edge_id,
                        radial_next: revolution_identity::<CoedgeId>(
                            feature_id,
                            format_args!("coedge:{radial_index}:{radial_boundary}"),
                        )?,
                        sense,
                        pcurves: pcurve_uses,
                        use_curve: None,
                    },
                )?;
                loops.push(loop_id);
            }
            ctx.charge_entities(1, "admit Creo model faces")?;
            source_carriers.admit_face(
                ir,
                Face {
                    id: face_id.try_clone_for_decode(ctx, "creo revolution identity copy")?,
                    shell: shell_id.try_clone_for_decode(ctx, "creo revolution identity copy")?,
                    surface: surface_id,
                    sense: face_sense,
                    loops: cadmpeg_ir::topology::FaceLoops::unspecified(loops),
                    name: None,
                    color: None,
                    tolerance: None,
                },
            )?;
            faces.push(face_id);
        }
        let Ok(shell) = Shell::new(
            shell_id.try_clone_for_decode(ctx, "creo revolution identity copy")?,
            region_id.try_clone_for_decode(ctx, "creo revolution identity copy")?,
            faces,
            Vec::new(),
            Vec::new(),
        ) else {
            continue;
        };
        ctx.charge_entities(1, "admit Creo model shells")?;
        ir.model.shells.push(shell);
        let region_shells = vec![shell_id];
        ctx.charge_entities(1, "admit Creo model regions")?;
        ir.model.regions.push(Region {
            id: region_id.try_clone_for_decode(ctx, "creo revolution identity copy")?,
            body: body_id.try_clone_for_decode(ctx, "creo revolution identity copy")?,
            shells: region_shells,
        });
        let body_regions = vec![region_id];
        ctx.charge_entities(1, "admit Creo model bodies")?;
        source_carriers.admit_body(
            ir,
            Body {
                id: body_id,
                kind: BodyKind::Solid,
                regions: body_regions,
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

struct PrevalidatedRevolutionBoundary {
    boundary: RevolutionBoundary,
    geometry: cadmpeg_ir::geometry::pcurve::PcurveGeometry,
}

impl PrevalidatedRevolutionBoundary {
    fn new(
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        transform: &crate::placement::FeatureSectionTransform,
        segment: &super::profiles::ProfileEntity,
        surface: &cadmpeg_ir::geometry::SurfaceGeometry,
        axis: &cadmpeg_ir::features::RevolutionAxis,
        boundary_point: ([f64; 2], RevolutionBoundary),
        diagnostics: &mut crate::lane_refusal::LaneRefusalContext<'_, '_>,
    ) -> Result<Option<Self>, cadmpeg_core::CodecError> {
        let (section_point, boundary) = boundary_point;

        let Some(geometry) = revolution_profile_boundary_pcurve(
            ctx,
            transform,
            segment,
            surface,
            axis,
            (section_point, boundary),
            diagnostics,
        )?
        else {
            return Ok(None);
        };
        Ok(Some(Self { boundary, geometry }))
    }
}

#[cfg(test)]
mod tests;
