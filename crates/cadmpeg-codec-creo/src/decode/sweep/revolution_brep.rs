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
    revolved_brep_surface, RevolutionBoundary,
};
use super::profiles::{extrusion_profile_signed_area, resolved_sketch_profiles};
use super::surfaces::revolved_section_circle;
use crate::container::ContainerScan;
use crate::decode::sketch_transfer::recipe::{
    current_additive_feature_recipe, feature_is_first_material_operation,
    feature_revolution_extent, unique_feature_revolution_extent,
};
use cadmpeg_core::decode::alloc_filled;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::{Curve, Surface};
use cadmpeg_ir::ids::{
    BodyId, CoedgeId, CurveId, EdgeId, FaceId, IdentityKey, LoopId, PcurveId, PointId, RegionId,
    ShellId, SurfaceId, VertexId,
};
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::topology::{
    Body, BodyKind, Coedge, Edge, Face, Loop as IrLoop, PcurveUse, Point, Region, Sense, Shell,
    Vertex,
};
use cadmpeg_ir::AnnotationBuilder;

pub(in super::super) fn transfer_resolved_revolution_breps(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    losses: &mut Vec<cadmpeg_ir::report::LossNote>,
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
            != Some(crate::feature::FeatureRecipeKind::Revolve)
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
            scan,
            ir,
            feature_id,
            definition,
            transform,
            extent.as_ref(),
        ) else {
            continue;
        };
        let Some(sketch_id) = model_sketch_id(scan, definition) else {
            continue;
        };
        let Some(mut profiles) = resolved_sketch_profiles(ir, &sketch_id, 2) else {
            continue;
        };
        let [profile] = profiles.as_mut_slice() else {
            continue;
        };
        let Some(area) = extrusion_profile_signed_area(profile) else {
            continue;
        };
        let vertex_curves = profile
            .iter()
            .map(|entity| revolved_section_circle(transform, entity.start(), &axis))
            .collect::<Vec<_>>();
        let mut refusal = crate::lane_refusal::LaneRefusals::new();
        let refusal = &mut refusal;
        let surface_geometries = profile
            .iter()
            .enumerate()
            .map(|(index, entity)| {
                let geometry = entity.geometry();
                let reversed = entity.reversed();

                revolved_brep_surface(
                    transform,
                    &geometry.to_sketch()?,
                    reversed,
                    &axis,
                    &format!("revolution feature {feature_id} profile segment {index}"),
                    refusal,
                )
            })
            .collect::<Option<Vec<_>>>();
        let Some(surface_geometries) = surface_geometries else {
            let records = refusal.take_records();
            losses.push(
                crate::loss::CreoLossCode::BrepTransferIncomplete.note(if records.is_empty() {
                    format!(
                        "Revolution feature {feature_id} states no revolved surface; its B-rep was skipped."
                    )
                } else {
                    format!(
                        "Revolution feature {feature_id} states no revolved surface; its B-rep was skipped: {}",
                        records.join("; ")
                    )
                }),
            );
            continue;
        };
        let boundaries = profile
            .iter()
            .enumerate()
            .map(|(index, segment)| {
                let next = (index + 1) % profile.len();
                if vertex_curves[index].is_none() && vertex_curves[next].is_none() {
                    return None;
                }
                [
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
                ]
                .into_iter()
                .filter(|(_, present, _)| *present)
                .map(|(section_point, _, boundary)| {
                    let record = format!(
                        "revolution feature {feature_id} profile segment {index} boundary {}",
                        boundary.key()
                    );
                    let mut diagnostics =
                        crate::lane_refusal::LaneRefusalContext::new(&record, refusal);
                    PrevalidatedRevolutionBoundary::new(
                        transform,
                        segment,
                        &surface_geometries[index],
                        &axis,
                        section_point,
                        boundary,
                        &mut diagnostics,
                    )
                })
                .collect::<Option<Vec<_>>>()
            })
            .collect::<Option<Vec<_>>>();
        let Some(boundaries) = boundaries else {
            let records = refusal.take_records();
            losses.push(
                crate::loss::CreoLossCode::BrepTransferIncomplete.note(if records.is_empty() {
                    format!(
                        "Revolution feature {feature_id} has an unresolved boundary pcurve; its B-rep was skipped."
                    )
                } else {
                    format!(
                        "Revolution feature {feature_id} has an unresolved boundary pcurve; its B-rep was skipped: {}",
                        records.join("; ")
                    )
                }),
            );
            continue;
        };
        let face_senses = profile
            .iter()
            .zip(&surface_geometries)
            .enumerate()
            .map(|(index, (segment, surface))| {
                revolution_face_sense(
                    transform,
                    segment,
                    surface,
                    &axis,
                    area,
                    &format!("revolution feature {feature_id} profile segment {index} face sense"),
                    refusal,
                )
            })
            .collect::<Option<Vec<_>>>();
        let Some(face_senses) = face_senses else {
            let records = refusal.take_records();
            losses.push(
                crate::loss::CreoLossCode::BrepTransferIncomplete.note(if records.is_empty() {
                    format!(
                        "Revolution feature {feature_id} states no face sense; its B-rep was skipped."
                    )
                } else {
                    format!(
                        "Revolution feature {feature_id} states no face sense; its B-rep was skipped: {}",
                        records.join("; ")
                    )
                }),
            );
            continue;
        };
        let feature_key = IdentityKey::from(feature_id);
        macro_rules! revolution_id {
            ($id:ident, $key:expr) => {
                $id::compose(
                    &crate::identity::FEATURE_REVOLUTION,
                    feature_key.clone().colon($key),
                )
            };
        }
        let body_id = revolution_id!(BodyId, cadmpeg_ir::identity_key!("body"));
        if ir.model.bodies.iter().any(|body| body.id == body_id) {
            continue;
        }
        let region_id = revolution_id!(RegionId, cadmpeg_ir::identity_key!("region"));
        let shell_id = revolution_id!(ShellId, cadmpeg_ir::identity_key!("shell"));
        let count = profile.len();
        let Ok(mut edges) = alloc_filled(count, None, "creo revolution profile edges") else {
            continue;
        };
        for (index, (entity, curve_geometry)) in profile.iter().zip(vertex_curves).enumerate() {
            let Some(curve_geometry) = curve_geometry else {
                continue;
            };
            let Ok(curve_geometry) = cadmpeg_ir::geometry::CurveGeometry::try_from(curve_geometry)
            else {
                continue;
            };
            let curve_id = revolution_id!(
                CurveId,
                cadmpeg_ir::identity_key!("curve")
                    .colon(cadmpeg_ir::identity_key!("vertex"))
                    .colon(index)
            );
            let point_id = revolution_id!(
                PointId,
                cadmpeg_ir::identity_key!("point")
                    .colon(cadmpeg_ir::identity_key!("vertex"))
                    .colon(index)
            );
            let vertex_id =
                revolution_id!(VertexId, cadmpeg_ir::identity_key!("vertex").colon(index));
            let edge_id = revolution_id!(
                EdgeId,
                cadmpeg_ir::identity_key!("edge")
                    .colon(cadmpeg_ir::identity_key!("vertex"))
                    .colon(index)
            );
            let position = section_point_in_model(transform, entity.start());
            ir.model.curves.push(Curve {
                id: curve_id.clone(),
                geometry: curve_geometry,
                source_object: None,
            });
            ir.model.points.push(
                Point::new(
                    point_id.clone(),
                    Point3::new(position[0], position[1], position[2]),
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
            let surface_id =
                revolution_id!(SurfaceId, cadmpeg_ir::identity_key!("surface").colon(index));
            let face_id = revolution_id!(FaceId, cadmpeg_ir::identity_key!("face").colon(index));
            ir.model.surfaces.push(Surface {
                id: surface_id.clone(),
                geometry: surface_geometry.clone(),
                source_object: None,
            });
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
                let Some(edge_id) = edges[vertex_index].clone() else {
                    continue;
                };
                let boundary_key = boundary.key();
                let loop_id = revolution_id!(
                    LoopId,
                    cadmpeg_ir::identity_key!("loop").colon(index).colon(
                        IdentityKey::try_new(boundary_key)
                            .map_err(cadmpeg_core::CodecError::malformed,)?
                    )
                );
                let coedge_id = revolution_id!(
                    CoedgeId,
                    cadmpeg_ir::identity_key!("coedge").colon(index).colon(
                        IdentityKey::try_new(boundary_key)
                            .map_err(cadmpeg_core::CodecError::malformed,)?
                    )
                );
                let radial_index = match boundary {
                    RevolutionBoundary::Start => (index + count - 1) % count,
                    RevolutionBoundary::End => next,
                };
                let radial_boundary = boundary.opposite().key();
                let pcurve = add_extrusion_pcurve(
                    ir,
                    annotations,
                    revolution_id!(
                        PcurveId,
                        cadmpeg_ir::identity_key!("pcurve").colon(index).colon(
                            IdentityKey::try_new(boundary_key)
                                .map_err(cadmpeg_core::CodecError::malformed,)?
                        )
                    ),
                    transform.offset,
                    pcurve_geometry,
                )?;
                ir.model.loops.push(IrLoop {
                    id: loop_id.clone(),
                    face: face_id.clone(),
                    boundary: cadmpeg_ir::topology::LoopBoundary::Ring(
                        cadmpeg_ir::topology::LoopRing::new(vec![coedge_id.clone()], Vec::new())
                            .map_err(cadmpeg_core::CodecError::malformed)?,
                    ),
                });
                ir.model.coedges.push(Coedge {
                    id: coedge_id.clone(),
                    owner_loop: loop_id.clone(),
                    edge: edge_id,
                    radial_next: revolution_id!(
                        CoedgeId,
                        cadmpeg_ir::identity_key!("coedge")
                            .colon(radial_index)
                            .colon(
                                IdentityKey::try_new(radial_boundary)
                                    .map_err(cadmpeg_core::CodecError::malformed,)?
                            )
                    ),
                    sense,
                    pcurves: vec![PcurveUse {
                        pcurve,
                        isoparametric: None,
                        parameter_range: None,
                    }],
                    use_curve: None,
                });
                loops.push(loop_id);
            }
            ir.model.faces.push(Face {
                id: face_id.clone(),
                shell: shell_id.clone(),
                surface: surface_id,
                sense: face_sense,
                loops: cadmpeg_ir::topology::FaceLoops::unspecified(loops),
                name: None,
                color: None,
                tolerance: None,
            });
            faces.push(face_id);
        }
        ir.model.shells.push(
            match Shell::new(
                shell_id.clone(),
                region_id.clone(),
                faces,
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

struct PrevalidatedRevolutionBoundary {
    boundary: RevolutionBoundary,
    geometry: cadmpeg_ir::geometry::PcurveGeometry,
}

impl PrevalidatedRevolutionBoundary {
    fn new(
        transform: &crate::placement::FeatureSectionTransform,
        segment: &super::profiles::ProfileEntity,
        surface: &cadmpeg_ir::geometry::SurfaceGeometry,
        axis: &cadmpeg_ir::features::RevolutionAxis,
        section_point: [f64; 2],
        boundary: RevolutionBoundary,
        diagnostics: &mut crate::lane_refusal::LaneRefusalContext<'_, '_>,
    ) -> Option<Self> {
        Some(Self {
            boundary,
            geometry: revolution_profile_boundary_pcurve(
                transform,
                segment,
                surface,
                axis,
                section_point,
                boundary,
                diagnostics,
            )?,
        })
    }
}

#[cfg(test)]
mod tests;
