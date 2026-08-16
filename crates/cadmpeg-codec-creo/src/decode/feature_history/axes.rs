// SPDX-License-Identifier: Apache-2.0
//! Revolution axes, section profile refs, and geometry-generator features.

use super::super::analytic::{cross, dot};
use super::super::sketch::{normalized, resolved_section_points, section_point_in_model};
use super::super::uniqueness::unique_feature_profile_definition;
use crate::container::ContainerScan;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{
    Angle, FeatureId as IrFeatureId, ProfileRef, RevolutionAxis, RevolveExtent, Termination,
};
use cadmpeg_ir::geometry::SurfaceGeometry;
use cadmpeg_ir::ids::SurfaceId;
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::sketches::SketchId;
use std::collections::{BTreeMap, BTreeSet};

pub(in super::super) fn resolved_revolution_axis(
    definition: &crate::feature::FeatureDefinition,
    transform: &crate::placement::FeatureSectionTransform,
) -> Option<RevolutionAxis> {
    definition.variables.as_ref()?;
    let segments = definition.segments.as_ref()?;
    segments.is_complete().then_some(())?;
    let points = resolved_section_points(definition);
    let candidates = segments
        .rows
        .iter()
        .filter(|segment| segment.kind == crate::feature::FeatureSegmentKind::Line)
        .filter_map(|segment| {
            let start = points.get(&segment.point_ids[0])?;
            let end = points.get(&segment.point_ids[1])?;
            if start[0] != 0.0 || end[0] != 0.0 || start == end {
                return None;
            }
            let start = section_point_in_model(transform, *start);
            let end = section_point_in_model(transform, *end);
            let direction = normalized(std::array::from_fn(|axis| end[axis] - start[axis]))?;
            Some(RevolutionAxis {
                origin: Point3::new(start[0], start[1], start[2]),
                direction: Vector3::new(direction[0], direction[1], direction[2]),
            })
        })
        .collect::<Vec<_>>();
    let [axis] = candidates.as_slice() else {
        return None;
    };
    Some(*axis)
}

pub(in super::super) fn full_turn_revolution_carrier_axis(
    scan: &ContainerScan,
    ir: &CadIr,
    feature_id: u32,
    extent: Option<&RevolveExtent>,
) -> Option<RevolutionAxis> {
    let Some(RevolveExtent::OneSided {
        termination: Termination::Angle {
            angle: Angle(angle),
        },
    }) = extent
    else {
        return None;
    };
    if (angle.abs() - std::f64::consts::TAU).abs() > 1e-12 {
        return None;
    }

    let rows = scan
        .surfaces
        .rows
        .iter()
        .filter(|row| row.feature_id == feature_id)
        .collect::<Vec<_>>();
    (!rows.is_empty()).then_some(())?;
    let mut axes = Vec::new();
    let mut plane_normals = Vec::new();
    let mut sphere_centers = Vec::new();
    for row in rows {
        (crate::surface::unique_surface_row(&scan.surfaces.rows, row.id) == Some(row))
            .then_some(())?;
        let id = SurfaceId(format!("creo:visibgeom:surface#{}", row.id));
        let surfaces = ir
            .model
            .surfaces
            .iter()
            .filter(|surface| surface.id == id)
            .collect::<Vec<_>>();
        let [surface] = surfaces.as_slice() else {
            return None;
        };
        match surface.geometry {
            SurfaceGeometry::Cylinder { origin, axis, .. }
            | SurfaceGeometry::Cone { origin, axis, .. } => {
                axes.push((origin, axis));
            }
            SurfaceGeometry::Torus { center, axis, .. } => {
                axes.push((center, axis));
            }
            SurfaceGeometry::Plane { normal, .. } => plane_normals.push(normal),
            SurfaceGeometry::Sphere { center, .. } => sphere_centers.push(center),
            _ => return None,
        }
    }
    let [(first_origin, first_direction), rest @ ..] = axes.as_slice() else {
        return None;
    };
    let mut direction = normalized([first_direction.x, first_direction.y, first_direction.z])?;
    if direction
        .iter()
        .find(|component| component.abs() > 1e-12)
        .is_some_and(|component| component.is_sign_negative())
    {
        direction = direction.map(|component| -component);
    }
    let first_origin = [first_origin.x, first_origin.y, first_origin.z];
    let axial = dot(first_origin, direction);
    let origin: [f64; 3] = std::array::from_fn(|axis| first_origin[axis] - axial * direction[axis]);
    let scale = first_origin
        .into_iter()
        .chain(
            rest.iter()
                .flat_map(|(origin, _)| [origin.x, origin.y, origin.z]),
        )
        .chain(
            sphere_centers
                .iter()
                .flat_map(|center| [center.x, center.y, center.z]),
        )
        .map(f64::abs)
        .fold(1.0, f64::max);
    for (candidate_origin, candidate_direction) in rest {
        let candidate_direction = normalized([
            candidate_direction.x,
            candidate_direction.y,
            candidate_direction.z,
        ])?;
        ((dot(direction, candidate_direction).abs() - 1.0).abs() <= 1e-10).then_some(())?;
        let displacement = [
            candidate_origin.x - origin[0],
            candidate_origin.y - origin[1],
            candidate_origin.z - origin[2],
        ];
        let radial = cross(displacement, direction);
        (dot(radial, radial).sqrt() <= 1e-9 * scale).then_some(())?;
    }
    for normal in plane_normals {
        let normal = normalized([normal.x, normal.y, normal.z])?;
        ((dot(direction, normal).abs() - 1.0).abs() <= 1e-10).then_some(())?;
    }
    for center in sphere_centers {
        let displacement = [
            center.x - origin[0],
            center.y - origin[1],
            center.z - origin[2],
        ];
        let radial = cross(displacement, direction);
        (dot(radial, radial).sqrt() <= 1e-9 * scale).then_some(())?;
    }
    Some(RevolutionAxis {
        origin: Point3::new(origin[0], origin[1], origin[2]),
        direction: Vector3::new(direction[0], direction[1], direction[2]),
    })
}

pub(in super::super) fn revolution_axis_for_transfer(
    scan: &ContainerScan,
    ir: &CadIr,
    feature_id: u32,
    definition: &crate::feature::FeatureDefinition,
    transform: &crate::placement::FeatureSectionTransform,
    extent: Option<&RevolveExtent>,
) -> Option<RevolutionAxis> {
    resolved_revolution_axis(definition, transform)
        .or_else(|| full_turn_revolution_carrier_axis(scan, ir, feature_id, extent))
}

pub(in super::super) fn feature_revolution_axis_for_transfer(
    scan: &ContainerScan,
    ir: &CadIr,
    feature_id: u32,
    extent: Option<&RevolveExtent>,
) -> Option<RevolutionAxis> {
    let definition = unique_feature_profile_definition(
        &scan.features.definitions,
        &scan.features.section_transforms,
        feature_id,
    );
    let transforms = scan
        .features
        .section_transforms
        .iter()
        .filter(|transform| transform.feature_id == Some(feature_id))
        .collect::<Vec<_>>();
    let transform = match transforms.as_slice() {
        [transform] => Some(*transform),
        _ => None,
    };
    definition
        .zip(transform)
        .and_then(|(definition, transform)| {
            revolution_axis_for_transfer(scan, ir, feature_id, definition, transform, extent)
        })
        .or_else(|| full_turn_revolution_carrier_axis(scan, ir, feature_id, extent))
}

pub(in super::super) fn section_profile_ref(ir: &CadIr, native_ref: String) -> ProfileRef {
    let sketch_id = SketchId(native_ref.replacen("creo:featdefs:sketch#", "creo:model:sketch#", 1));
    let Some(sketch) = ir
        .model
        .sketches
        .iter()
        .find(|sketch| sketch.id == sketch_id)
    else {
        return ProfileRef::Native(native_ref);
    };
    if sketch.profiles.is_empty() {
        ProfileRef::Native(native_ref)
    } else {
        ProfileRef::Sketch(sketch_id)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in super::super) struct GeometryGeneratorFeature {
    pub(in super::super) feature_id: u32,
    pub(in super::super) offset: usize,
    pub(in super::super) surface_ids: Vec<u32>,
    pub(in super::super) curve_ids: Vec<u32>,
}

pub(in super::super) fn geometry_generator_features(
    scan: &ContainerScan,
) -> Vec<GeometryGeneratorFeature> {
    let operation_feature_ids = scan
        .features
        .operations
        .iter()
        .map(|operation| operation.feature_id)
        .collect::<BTreeSet<_>>();
    let row_feature_ids = scan
        .features
        .rows
        .iter()
        .map(|row| row.feature_id)
        .collect::<BTreeSet<_>>();
    let datum_feature_ids = scan
        .planes
        .datums
        .iter()
        .map(|datum| datum.feature_id)
        .collect::<BTreeSet<_>>();
    let mut generators = BTreeMap::<u32, GeometryGeneratorFeature>::new();
    for row in &scan.surfaces.rows {
        if row.feature_id == 0 {
            continue;
        }
        let generator =
            generators
                .entry(row.feature_id)
                .or_insert_with(|| GeometryGeneratorFeature {
                    feature_id: row.feature_id,
                    offset: row.offset,
                    surface_ids: Vec::new(),
                    curve_ids: Vec::new(),
                });
        generator.offset = generator.offset.min(row.offset);
        generator.surface_ids.push(row.id);
    }
    for row in &scan.curves.topology_rows {
        if row.feature_id == 0 {
            continue;
        }
        let generator =
            generators
                .entry(row.feature_id)
                .or_insert_with(|| GeometryGeneratorFeature {
                    feature_id: row.feature_id,
                    offset: row.offset,
                    surface_ids: Vec::new(),
                    curve_ids: Vec::new(),
                });
        generator.offset = generator.offset.min(row.offset);
        generator.curve_ids.push(row.id);
    }
    let mut generators = generators
        .into_values()
        .filter(|generator| {
            !operation_feature_ids.contains(&generator.feature_id)
                && !row_feature_ids.contains(&generator.feature_id)
                && !datum_feature_ids.contains(&generator.feature_id)
        })
        .collect::<Vec<_>>();
    generators.sort_by_key(|generator| generator.offset);
    generators
}

/// Return the feature identities that the model-transfer pass will emit.
///
/// Feature definitions are built while the transfer pass is still walking
/// source order. A generated face or edge can therefore name a valid
/// row-backed producer that has not been inserted into `ir.model.features`
/// yet. Derive the complete emitted identity set from the scan instead of
/// using the construction-time prefix of the IR.
pub(in super::super) fn model_feature_ids(scan: &ContainerScan) -> BTreeSet<IrFeatureId> {
    let mut ids = scan
        .features
        .operations
        .iter()
        .map(|operation| operation.feature_id)
        .chain(scan.features.rows.iter().map(|row| row.feature_id))
        .chain(scan.planes.datums.iter().map(|datum| datum.feature_id))
        .map(|feature_id| IrFeatureId(format!("creo:model:feature#{feature_id}")))
        .collect::<BTreeSet<_>>();
    ids.extend(
        geometry_generator_features(scan)
            .into_iter()
            .map(|generator| IrFeatureId(format!("creo:model:feature#{}", generator.feature_id))),
    );
    ids
}
