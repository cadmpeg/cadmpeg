// SPDX-License-Identifier: Apache-2.0
//! Hole, split, round, and positional cylinders and cones.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::{Surface, SurfaceGeometry};
use cadmpeg_ir::ids::SurfaceId;
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::{AnnotationBuilder, Exactness, SourceObjectAssociation};

use crate::container::ContainerScan;

use super::super::analytic::{dot, is_axis_aligned, PlaneEquation};
use super::super::feature_history::{
    agreed_feature_affected_ids, agreed_feature_replay_geometry_ids, has_feature_affected_ids,
    section_sweep_allows_linear_extrusion, slot_fillet_cylinder,
};
use super::super::holes::{
    circular_sweep_geometry, counterbore_patch_geometries,
    cylinder_from_complementary_outline_bounds, simple_hole_geometry,
};
use super::super::native::annotate;
use super::super::sketch::normalized;
use super::super::sketch_transfer::{feature_recipe, feature_section_sweep_semantics_conflict};

pub(in super::super) fn rowless_round_cylinder_pairs(
    round_feature_ids: &BTreeSet<u32>,
    tables: &[crate::feature::FeatureEntityTable],
    rows: &[crate::surface::SurfaceRow],
) -> Vec<(u32, u32, usize)> {
    tables
        .iter()
        .filter_map(|table| {
            let feature_id = table.feature_id?;
            round_feature_ids.contains(&feature_id).then_some(())?;
            let [first, second, rowless, cylinder] = table.entry_ids.as_slice() else {
                return None;
            };
            rows.iter().any(|row| row.id == *first).then_some(())?;
            rows.iter().any(|row| row.id == *second).then_some(())?;
            (!rows.iter().any(|row| row.id == *rowless)).then_some(())?;
            rows.iter()
                .any(|row| {
                    row.id == *cylinder
                        && row.feature_id == feature_id
                        && row.kind == crate::surface::SurfaceKind::Cylinder
                })
                .then_some(())?;
            Some((*rowless, *cylinder, table.offset))
        })
        .collect()
}

pub(in super::super) fn transfer_constrained_slot_fillet_cylinders(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) -> usize {
    let round_feature_ids = scan
        .features
        .rows
        .iter()
        .filter(|row| row.root_schema_class == Some(913))
        .map(|row| row.feature_id)
        .collect::<BTreeSet<_>>();
    let mut transferred = 0;
    for feature_id in round_feature_ids {
        let named = agreed_feature_affected_ids(
            &scan.features.affected_ids,
            feature_id,
            crate::feature::AffectedIdKind::Geometry,
        );
        let named_present = has_feature_affected_ids(
            &scan.features.affected_ids,
            feature_id,
            crate::feature::AffectedIdKind::Geometry,
        );
        let replay =
            agreed_feature_replay_geometry_ids(&scan.features.replay_affected_ids, feature_id);
        let affected = match (named, replay) {
            (Some(ids), _) => ids,
            (None, Some(ids)) if !named_present => ids,
            _ => continue,
        };
        let Some((cap_ids, support_ids)) = affected.split_at_checked(2) else {
            continue;
        };
        if support_ids.len() < 4 {
            continue;
        }
        let planes = affected
            .iter()
            .filter_map(|id| {
                let surface_id = SurfaceId(format!("creo:visibgeom:surface#{id}"));
                let surface = ir
                    .model
                    .surfaces
                    .iter()
                    .find(|surface| surface.id == surface_id)?;
                match surface.geometry {
                    SurfaceGeometry::Plane { origin, normal, .. } => Some(PlaneEquation {
                        origin: [origin.x, origin.y, origin.z],
                        normal: [normal.x, normal.y, normal.z],
                    }),
                    _ => None,
                }
            })
            .collect::<Vec<_>>();
        if planes.len() != affected.len() {
            continue;
        }
        let cap_planes: [PlaneEquation; 2] = planes[..cap_ids.len()].try_into().expect("two caps");
        let Some(cylinder) = slot_fillet_cylinder(cap_planes, &planes[cap_ids.len()..]) else {
            continue;
        };
        let unresolved_rows = scan
            .surfaces
            .rows
            .iter()
            .filter(|row| {
                row.feature_id == feature_id
                    && row.kind == crate::surface::SurfaceKind::Cylinder
                    && !ir.model.surfaces.iter().any(|surface| {
                        surface.id == SurfaceId(format!("creo:visibgeom:surface#{}", row.id))
                    })
            })
            .collect::<Vec<_>>();
        let [row] = unresolved_rows.as_slice() else {
            continue;
        };
        let id = SurfaceId(format!("creo:visibgeom:surface#{}", row.id));
        annotate(
            annotations,
            &id,
            "AllFeatur",
            row.offset as u64,
            "constrained_slot_fillet_cylinder",
            Exactness::Derived,
        );
        ir.model.surfaces.push(Surface {
            id,
            geometry: SurfaceGeometry::Cylinder {
                origin: Point3::new(cylinder.origin[0], cylinder.origin[1], cylinder.origin[2]),
                axis: Vector3::new(cylinder.axis[0], cylinder.axis[1], cylinder.axis[2]),
                ref_direction: Vector3::new(
                    cylinder.ref_direction[0],
                    cylinder.ref_direction[1],
                    cylinder.ref_direction[2],
                ),
                radius: cylinder.radius,
            },
            source_object: Some(SourceObjectAssociation {
                format: "creo".to_string(),
                object_id: format!("AllFeatur:{}:{}", feature_id, row.id),
                name: None,
                color: None,
                visible: None,
                layer: None,
                instance_path: Vec::new(),
            }),
        });
        transferred += 1;
    }
    transferred
}

pub(in super::super) fn transfer_rowless_round_cylinders(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) -> usize {
    let round_feature_ids = scan
        .features
        .rows
        .iter()
        .filter(|row| row.root_schema_class == Some(913))
        .map(|row| row.feature_id)
        .collect::<BTreeSet<_>>();
    let mut transferred = 0;
    for (rowless_id, sibling_id, offset) in rowless_round_cylinder_pairs(
        &round_feature_ids,
        &scan.features.entity_tables,
        &scan.surfaces.rows,
    ) {
        let sibling = SurfaceId(format!("creo:visibgeom:surface#{sibling_id}"));
        let Some(SurfaceGeometry::Cylinder {
            origin,
            axis,
            ref_direction,
            radius,
        }) = ir
            .model
            .surfaces
            .iter()
            .find(|surface| surface.id == sibling)
            .map(|surface| &surface.geometry)
        else {
            continue;
        };
        let id = SurfaceId(format!("creo:visibgeom:surface#{rowless_id}"));
        if ir.model.surfaces.iter().any(|surface| surface.id == id) {
            continue;
        }
        annotate(
            annotations,
            &id,
            "AllFeatur",
            offset as u64,
            "round_rowless_sibling_cylinder",
            Exactness::Derived,
        );
        ir.model.surfaces.push(Surface {
            id,
            geometry: SurfaceGeometry::Cylinder {
                origin: *origin,
                axis: *axis,
                ref_direction: *ref_direction,
                radius: *radius,
            },
            source_object: Some(SourceObjectAssociation {
                format: "creo".to_string(),
                object_id: format!("AllFeatur:{rowless_id}"),
                name: None,
                color: None,
                visible: None,
                layer: None,
                instance_path: Vec::new(),
            }),
        });
        transferred += 1;
    }
    transferred
}

pub(in super::super) fn transfer_hole_cylinders(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) -> usize {
    let hole_feature_ids = scan
        .features
        .rows
        .iter()
        .filter(|row| row.root_schema_class == Some(911))
        .map(|row| row.feature_id)
        .collect::<BTreeSet<_>>();
    let mut transferred = 0;
    for feature_id in hole_feature_ids {
        let cylinders = if let Some(hole) = simple_hole_geometry(scan, feature_id) {
            hole.cylinder_ids
                .into_iter()
                .map(|id| (id, hole.geometry.clone()))
                .collect::<Vec<_>>()
        } else {
            counterbore_patch_geometries(scan, ir, feature_id).unwrap_or_default()
        };
        for (cylinder_id, geometry) in cylinders {
            let row = crate::surface::unique_surface_row(&scan.surfaces.rows, cylinder_id)
                .expect("validated cylinder row");
            let id = SurfaceId(format!("creo:visibgeom:surface#{cylinder_id}"));
            if ir.model.surfaces.iter().any(|surface| surface.id == id) {
                continue;
            }
            annotate(
                annotations,
                &id,
                "AllFeatur",
                row.offset as u64,
                "hole_cap_outline_cylinder",
                Exactness::Derived,
            );
            ir.model.surfaces.push(Surface {
                id,
                geometry,
                source_object: Some(SourceObjectAssociation {
                    format: "creo".to_string(),
                    object_id: format!("VisibGeom:{cylinder_id}"),
                    name: None,
                    color: None,
                    visible: None,
                    layer: None,
                    instance_path: Vec::new(),
                }),
            });
            transferred += 1;
        }
    }
    transferred
}

pub(in super::super) fn transfer_split_outline_cylinders(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) -> usize {
    let rows = scan
        .surfaces
        .rows
        .iter()
        .map(|row| (row.id, row))
        .collect::<BTreeMap<_, _>>();
    let mut cylinders_by_plane = BTreeMap::<(u32, u32), BTreeSet<u32>>::new();
    for edge in crate::topology::uniquely_identified_rows(&scan.curves.topology_rows) {
        if edge.type_byte != 0 {
            continue;
        }
        let [left, right] = edge.faces;
        let pair = match (rows.get(&left), rows.get(&right)) {
            (Some(plane), Some(cylinder))
                if plane.kind == crate::surface::SurfaceKind::Plane
                    && cylinder.kind == crate::surface::SurfaceKind::Cylinder =>
            {
                Some(((left, cylinder.feature_id), right))
            }
            (Some(cylinder), Some(plane))
                if plane.kind == crate::surface::SurfaceKind::Plane
                    && cylinder.kind == crate::surface::SurfaceKind::Cylinder =>
            {
                Some(((right, cylinder.feature_id), left))
            }
            _ => None,
        };
        if let Some((plane_and_feature, cylinder)) = pair {
            cylinders_by_plane
                .entry(plane_and_feature)
                .or_default()
                .insert(cylinder);
        }
    }

    let mut transferred = 0;
    for ((plane_id, _), cylinder_ids) in cylinders_by_plane {
        let cylinder_ids = cylinder_ids.into_iter().collect::<Vec<_>>();
        let [first_id, second_id] = cylinder_ids.as_slice() else {
            continue;
        };
        let Some(first) =
            crate::surface::unique_surface_parameter(&scan.surfaces.parameters, *first_id)
        else {
            continue;
        };
        let Some(second) =
            crate::surface::unique_surface_parameter(&scan.surfaces.parameters, *second_id)
        else {
            continue;
        };
        let Some(bounds) = first
            .split_cylinder_outline_bounds
            .zip(second.split_cylinder_outline_bounds)
            .map(|(first, second)| [first, second])
        else {
            continue;
        };
        let plane_id = SurfaceId(format!("creo:visibgeom:surface#{plane_id}"));
        let Some(plane) = ir
            .model
            .surfaces
            .iter()
            .find(|surface| surface.id == plane_id)
        else {
            continue;
        };
        let Some(geometry) = cylinder_from_complementary_outline_bounds(&plane.geometry, bounds)
        else {
            continue;
        };
        for cylinder_id in [*first_id, *second_id] {
            let id = SurfaceId(format!("creo:visibgeom:surface#{cylinder_id}"));
            if ir.model.surfaces.iter().any(|surface| surface.id == id) {
                continue;
            }
            let row = rows[&cylinder_id];
            annotate(
                annotations,
                &id,
                "VisibGeom",
                row.offset as u64,
                "split_outline_cylinder",
                Exactness::Derived,
            );
            ir.model.surfaces.push(Surface {
                id,
                geometry: geometry.clone(),
                source_object: Some(SourceObjectAssociation {
                    format: "creo".to_string(),
                    object_id: format!("VisibGeom:{cylinder_id}"),
                    name: None,
                    color: None,
                    visible: None,
                    layer: None,
                    instance_path: Vec::new(),
                }),
            });
            transferred += 1;
        }
    }
    transferred
}

pub(in super::super) fn transfer_positional_cylinders(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) -> usize {
    let mut transferred = 0;
    for record in &scan.surfaces.parameters {
        if crate::surface::unique_surface_parameter(&scan.surfaces.parameters, record.surface_id)
            != Some(record)
        {
            continue;
        }
        let Some(row) = crate::surface::unique_surface_row(&scan.surfaces.rows, record.surface_id)
            .filter(|row| row.kind == crate::surface::SurfaceKind::Cylinder)
        else {
            continue;
        };
        let reference_bound_frame = || {
            let entity_ids = scan
                .features
                .entity_tables
                .iter()
                .filter(|table| table.feature_id == Some(row.feature_id))
                .flat_map(|table| table.entry_ids.iter().copied())
                .collect::<BTreeSet<_>>();
            let circles = scan
                .references
                .circles
                .iter()
                .filter(|circle| entity_ids.contains(&circle.entity_id))
                .collect::<Vec<_>>();
            let generated_cylinder_count = scan
                .surfaces
                .rows
                .iter()
                .filter(|candidate| {
                    candidate.feature_id == row.feature_id
                        && candidate.kind == crate::surface::SurfaceKind::Cylinder
                })
                .count();
            if generated_cylinder_count == 1 {
                if let Some(frame) = reference_circle_pair_cylinder_frame(&circles) {
                    return Some((frame, "reference_circle_pair_cylinder_frame"));
                }
            }
            let envelope = record.type24_scalar_frame_round_envelope(row.type_byte)?;
            reference_cap_bound_round_frame(envelope, &circles)
                .map(|frame| (frame, "round_reference_cap_cylinder_frame"))
        };
        let (frame, mechanism) = match record.positional_cylinder_frame {
            Some(frame) => (frame, "positional_cylinder_frame"),
            None => {
                let Some(frame) = reference_bound_frame() else {
                    continue;
                };
                frame
            }
        };
        let id = SurfaceId(format!("creo:visibgeom:surface#{}", record.surface_id));
        if ir.model.surfaces.iter().any(|surface| surface.id == id) {
            continue;
        }
        annotate(
            annotations,
            &id,
            "VisibGeom",
            row.offset as u64,
            mechanism,
            Exactness::Derived,
        );
        ir.model.surfaces.push(Surface {
            id,
            geometry: SurfaceGeometry::Cylinder {
                origin: Point3::new(frame.origin[0], frame.origin[1], frame.origin[2]),
                axis: Vector3::new(frame.axis[0], frame.axis[1], frame.axis[2]),
                ref_direction: Vector3::new(
                    frame.ref_direction[0],
                    frame.ref_direction[1],
                    frame.ref_direction[2],
                ),
                radius: frame.radius,
            },
            source_object: Some(SourceObjectAssociation {
                format: "creo".to_string(),
                object_id: format!("VisibGeom:{}", record.surface_id),
                name: None,
                color: None,
                visible: None,
                layer: None,
                instance_path: Vec::new(),
            }),
        });
        transferred += 1;
    }
    transferred
}

pub(in super::super) fn reference_circle_pair_cylinder_frame(
    circles: &[&crate::reference::ReferenceCircle],
) -> Option<crate::surface::PositionalCylinderFrame> {
    let [first, second] = circles else {
        return None;
    };
    (first.radius.is_finite()
        && first.radius > 0.0
        && first.center_stored
        && second.center_stored
        && second.radius.is_finite())
    .then_some(())?;
    let radius = first.radius;
    let radius_scale = radius.max(second.radius).max(1.0);
    ((second.radius - radius).abs() <= 1e-9 * radius_scale).then_some(())?;
    let scale = first
        .center
        .iter()
        .chain(&second.center)
        .map(|value| value.abs())
        .fold(radius_scale, f64::max);
    let first_axis = normalized(first.axis)?;
    let second_axis = normalized(second.axis)?;
    ((dot(first_axis, second_axis).abs() - 1.0).abs() <= 1e-9).then_some(())?;
    let displacement: [f64; 3] =
        std::array::from_fn(|index| second.center[index] - first.center[index]);
    let length = dot(displacement, displacement).sqrt();
    (length.is_finite() && length > 1e-9 * scale).then_some(())?;
    let center_direction = displacement.map(|value| value / length);
    ((dot(center_direction, first_axis).abs() - 1.0).abs() <= 1e-9
        && (dot(center_direction, second_axis).abs() - 1.0).abs() <= 1e-9)
        .then_some(())?;
    let validated_radial = |circle: &crate::reference::ReferenceCircle, axis| {
        let vector: [f64; 3] =
            std::array::from_fn(|index| circle.start[index] - circle.center[index]);
        let length = dot(vector, vector).sqrt();
        ((length - radius).abs() <= 1e-9 * radius_scale
            && dot(axis, vector).abs() <= 1e-9 * radius_scale)
            .then_some((vector, length))
    };
    let (radial, radial_length) = validated_radial(first, first_axis)?;
    validated_radial(second, second_axis)?;
    Some(crate::surface::PositionalCylinderFrame {
        origin: first.center,
        axis: first_axis,
        ref_direction: radial.map(|value| value / radial_length),
        radius,
        length: Some(length),
    })
    .filter(crate::surface::PositionalCylinderFrame::is_valid)
}

pub(in super::super) fn reference_cap_bound_round_frame(
    envelope: crate::surface::Type24RoundEnvelope,
    circles: &[&crate::reference::ReferenceCircle],
) -> Option<crate::surface::PositionalCylinderFrame> {
    let [first, second] = envelope.extent_endpoints;
    let scale = first
        .iter()
        .chain(&second)
        .copied()
        .map(f64::abs)
        .fold(envelope.diameter.max(1.0), f64::max);
    let tolerance = 1.0e-9 * scale;
    let point_matches = |actual: [f64; 3], expected: [f64; 3]| {
        actual
            .iter()
            .zip(expected)
            .all(|(actual, expected)| (actual - expected).abs() <= tolerance)
    };
    let mut candidates = Vec::new();
    for axis_index in 0..3 {
        let radial_indices = (0..3)
            .filter(|index| *index != axis_index)
            .collect::<Vec<_>>();
        if radial_indices.iter().any(|index| {
            ((second[*index] - first[*index]).abs() - envelope.diameter).abs() > tolerance
        }) || (second[axis_index] - first[axis_index]).abs() <= tolerance
        {
            continue;
        }
        let cap_pair = |coordinate: f64, crossed: bool| {
            let mut first_corner = first;
            let mut second_corner = second;
            first_corner[axis_index] = coordinate;
            second_corner[axis_index] = coordinate;
            if crossed {
                first_corner[radial_indices[1]] = second[radial_indices[1]];
                second_corner[radial_indices[1]] = first[radial_indices[1]];
            }
            circles.iter().any(|circle| {
                circle.axis.iter().enumerate().all(|(index, component)| {
                    if index == axis_index {
                        (component.abs() - 1.0).abs() <= 1.0e-9
                    } else {
                        component.abs() <= 1.0e-9
                    }
                }) && ((point_matches(circle.start, first_corner)
                    && point_matches(circle.end, second_corner))
                    || (point_matches(circle.end, first_corner)
                        && point_matches(circle.start, second_corner)))
            })
        };
        if ![false, true].into_iter().any(|crossed| {
            cap_pair(first[axis_index], crossed) && cap_pair(second[axis_index], crossed)
        }) {
            continue;
        }
        let mut origin = first;
        for index in &radial_indices {
            origin[*index] = first[*index].midpoint(second[*index]);
        }
        let mut axis = [0.0; 3];
        axis[axis_index] = (second[axis_index] - first[axis_index]).signum();
        let mut ref_direction = [0.0; 3];
        let reference_index = radial_indices[0];
        ref_direction[reference_index] =
            (second[reference_index] - first[reference_index]).signum();
        candidates.push(crate::surface::PositionalCylinderFrame {
            origin,
            axis,
            ref_direction,
            radius: envelope.diameter / 2.0,
            length: Some((second[axis_index] - first[axis_index]).abs()),
        });
    }
    let [frame] = candidates.as_slice() else {
        return None;
    };
    Some(*frame).filter(crate::surface::PositionalCylinderFrame::is_valid)
}

pub(in super::super) fn transfer_positional_cones(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) -> usize {
    let mut transferred = 0;
    for record in &scan.surfaces.parameters {
        let Some(frame) = record.positional_cone_frame else {
            continue;
        };
        if crate::surface::unique_surface_parameter(&scan.surfaces.parameters, record.surface_id)
            != Some(record)
        {
            continue;
        }
        let Some(row) = crate::surface::unique_surface_row(&scan.surfaces.rows, record.surface_id)
            .filter(|row| row.kind == crate::surface::SurfaceKind::Cone)
        else {
            continue;
        };
        let id = SurfaceId(format!("creo:visibgeom:surface#{}", record.surface_id));
        if ir.model.surfaces.iter().any(|surface| surface.id == id) {
            continue;
        }
        annotate(
            annotations,
            &id,
            "VisibGeom",
            row.offset as u64,
            "positional_cone_frame",
            Exactness::Derived,
        );
        ir.model.surfaces.push(Surface {
            id,
            geometry: SurfaceGeometry::Cone {
                origin: Point3::new(frame.apex[0], frame.apex[1], frame.apex[2]),
                axis: Vector3::new(frame.axis[0], frame.axis[1], frame.axis[2]),
                ref_direction: Vector3::new(
                    frame.ref_direction[0],
                    frame.ref_direction[1],
                    frame.ref_direction[2],
                ),
                radius: 0.0,
                ratio: 1.0,
                half_angle: frame.half_angle,
            },
            source_object: Some(SourceObjectAssociation {
                format: "creo".to_string(),
                object_id: format!("VisibGeom:{}", record.surface_id),
                name: None,
                color: None,
                visible: None,
                layer: None,
                instance_path: Vec::new(),
            }),
        });
        transferred += 1;
    }
    transferred
}

pub(in super::super) fn transfer_circular_sweep_cylinders(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) -> usize {
    let sweep_feature_ids = scan
        .features
        .rows
        .iter()
        .filter(|row| {
            row.root_schema_class == Some(917)
                && !feature_section_sweep_semantics_conflict(scan, row.feature_id)
                && section_sweep_allows_linear_extrusion(917, feature_recipe(scan, row.feature_id))
        })
        .map(|row| row.feature_id)
        .collect::<BTreeSet<_>>();
    let mut transferred = 0;
    for feature_id in sweep_feature_ids {
        let Some(sweep) = circular_sweep_geometry(scan, feature_id) else {
            continue;
        };
        for cylinder_id in &sweep.cylinder_ids {
            let row = crate::surface::unique_surface_row(&scan.surfaces.rows, *cylinder_id)
                .expect("validated cylinder row");
            let id = SurfaceId(format!("creo:visibgeom:surface#{cylinder_id}"));
            if ir.model.surfaces.iter().any(|surface| surface.id == id) {
                continue;
            }
            annotate(
                annotations,
                &id,
                "AllFeatur",
                row.offset as u64,
                "circular_sweep_cap_outline_cylinder",
                Exactness::Derived,
            );
            ir.model.surfaces.push(Surface {
                id,
                geometry: sweep.geometry.clone(),
                source_object: Some(SourceObjectAssociation {
                    format: "creo".to_string(),
                    object_id: format!("VisibGeom:{cylinder_id}"),
                    name: None,
                    color: None,
                    visible: None,
                    layer: None,
                    instance_path: Vec::new(),
                }),
            });
            transferred += 1;
        }
    }
    transferred
}

pub(in super::super) fn transfer_cross_section_planes(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) -> usize {
    let mut transferred = 0;
    for frame in &scan.planes.cross_section_local_systems {
        let (Some(origin), Some(normal), Some(u_axis)) = (frame.origin, frame.normal, frame.u_axis)
        else {
            continue;
        };
        if is_axis_aligned(normal) {
            continue;
        }
        let id = SurfaceId(format!(
            "creo:cross_section_geometry:surface#{}",
            frame.surface_id
        ));
        if ir.model.surfaces.iter().any(|surface| surface.id == id) {
            continue;
        }
        annotate(
            annotations,
            &id,
            "Xsections",
            frame.offset as u64,
            "cross_section_plane_local_system",
            Exactness::Derived,
        );
        ir.model.surfaces.push(Surface {
            id,
            geometry: SurfaceGeometry::Plane {
                origin: Point3::new(origin[0], origin[1], origin[2]),
                normal: Vector3::new(normal[0], normal[1], normal[2]),
                u_axis: Vector3::new(u_axis[0], u_axis[1], u_axis[2]),
            },
            source_object: Some(SourceObjectAssociation {
                format: "creo".to_string(),
                object_id: format!("Xsections:{}", frame.surface_id),
                name: None,
                color: None,
                visible: None,
                layer: None,
                instance_path: Vec::new(),
            }),
        });
        transferred += 1;
    }
    for plane in &scan.planes.cross_section_outlines {
        let id = SurfaceId(format!(
            "creo:cross_section_geometry:surface#{}",
            plane.surface_id
        ));
        if ir.model.surfaces.iter().any(|surface| surface.id == id) {
            continue;
        }
        annotate(
            annotations,
            &id,
            "Xsections",
            plane.offset as u64,
            "cross_section_plane_outline_held_coordinate",
            Exactness::Derived,
        );
        ir.model.surfaces.push(Surface {
            id,
            geometry: SurfaceGeometry::Plane {
                origin: Point3::new(plane.origin[0], plane.origin[1], plane.origin[2]),
                normal: Vector3::new(plane.normal[0], plane.normal[1], plane.normal[2]),
                u_axis: Vector3::new(plane.u_axis[0], plane.u_axis[1], plane.u_axis[2]),
            },
            source_object: Some(SourceObjectAssociation {
                format: "creo".to_string(),
                object_id: format!("Xsections:{}", plane.surface_id),
                name: None,
                color: None,
                visible: None,
                layer: None,
                instance_path: Vec::new(),
            }),
        });
        transferred += 1;
    }
    transferred
}
