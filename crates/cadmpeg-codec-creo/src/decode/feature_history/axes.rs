// SPDX-License-Identifier: Apache-2.0
//! Revolution axes, section profile refs, and geometry-generator features.

use super::super::sketch::coordinates::resolved_section_points;
use super::super::sketch::intersect::section_point_in_model;
use super::super::uniqueness::{exactly_one, unique_feature_profile_definition};
use crate::container::ContainerScan;
use crate::vecmath::normalize;
use crate::vecmath::unit_length;
use crate::vecmath::{cross, dot};
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{
    AngularTermination, FeatureId as IrFeatureId, PlanarProfileRef, ProfileRef, RevolutionAxis,
    RevolveExtent,
};
use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
use cadmpeg_ir::math::{Point3, Vector3};
use std::collections::{BTreeMap, BTreeSet};

const EPS_FULL_TURN: f64 = 1.0e-12;
const EPS_DIRECTION_COMPONENT: f64 = 1.0e-12;
const EPS_AXIS_ALIGNMENT: f64 = 1.0e-10;
const EPS_AXIS_OFFSET: f64 = 1.0e-9;

pub(in super::super) fn resolved_revolution_axis(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
    transform: &crate::placement::FeatureSectionTransform,
) -> Result<Option<RevolutionAxis>, cadmpeg_core::CodecError> {
    let Some(segments) = definition
        .variables
        .as_ref()
        .and(definition.segments.as_ref())
        .filter(|segments| segments.is_complete())
    else {
        return Ok(None);
    };
    let points = resolved_section_points(ctx, definition)?;
    let mut candidates = segments
        .rows
        .ordinary()
        .filter(|segment| {
            matches!(
                segment.kind,
                crate::feature::definitions::FeatureSegmentKind::Line(_)
            )
        })
        .filter_map(|segment| {
            let start = points.get(&segment.point_ids()[0])?;
            let end = points.get(&segment.point_ids()[1])?;
            if start[0] != 0.0 || end[0] != 0.0 || start == end {
                return None;
            }
            let start = section_point_in_model(transform, *start);
            let end = section_point_in_model(transform, *end);
            let direction = normalize(std::array::from_fn(|axis| end[axis] - start[axis]))?;
            Some(RevolutionAxis {
                origin: cadmpeg_ir::features::FinitePoint3::new(Point3::from(start))?,
                direction: cadmpeg_ir::features::FeatureDirection3::new(Vector3::from(direction))?,
                reference: None,
            })
        });
    let Some(axis) = candidates.next() else {
        return Ok(None);
    };
    if candidates.next().is_some() {
        return Ok(None);
    }
    Ok(Some(axis))
}

pub(in super::super) fn full_turn_revolution_carrier_axis(
    scan: &ContainerScan,
    ir: &CadIr,
    source_carriers: &crate::decode::source_carriers::SourceUnitCarriers,
    feature_id: u32,
    extent: Option<&RevolveExtent>,
) -> Option<RevolutionAxis> {
    let Some(RevolveExtent::OneSided {
        termination: AngularTermination::Angle { angle },
    }) = extent
    else {
        return None;
    };
    let angle = angle.get();

    if (angle.abs() - std::f64::consts::TAU).abs() > EPS_FULL_TURN {
        return None;
    }

    let rows = scan
        .surfaces
        .rows
        .iter()
        .filter(|row| row.feature_id == feature_id);
    let mut axes = Vec::new();
    let mut plane_normals = Vec::new();
    let mut sphere_centers = Vec::new();
    let mut saw_row = false;
    for row in rows {
        saw_row = true;
        if crate::surface::unique_surface_row(&scan.surfaces.rows, row.id) != Some(row) {
            return None;
        }
        let mut surfaces = ir.model.surfaces.iter().filter(|surface| {
            crate::identity::matches_numbered_identity(
                surface.id.as_str(),
                "creo:visibgeom:surface#",
                row.id,
            )
        });
        let surface = surfaces.next().filter(|_| surfaces.next().is_none())?;
        match source_carriers.surface_geometry(surface) {
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(cylinder_surface)) => {
                let origin = cylinder_surface.origin().get();
                axes.push((origin, *cylinder_surface.frame().axis()));
            }
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(cone_surface)) => {
                let origin = cone_surface.origin().get();
                axes.push((origin, *cone_surface.frame().axis()));
            }
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Torus(torus_surface)) => {
                let center = torus_surface.center().get();
                axes.push((center, *torus_surface.frame().axis()));
            }
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane_surface)) => {
                plane_normals.push(*plane_surface.frame().axis());
            }
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(sphere_surface)) => {
                let center = sphere_surface.center().get();
                sphere_centers.push(center);
            }
            _ => return None,
        }
    }
    if !saw_row {
        return None;
    }
    let [(first_origin, first_direction), rest @ ..] = axes.as_slice() else {
        return None;
    };
    let mut direction = unit_length(*first_direction);
    if direction
        .iter()
        .find(|component| component.abs() > EPS_DIRECTION_COMPONENT)
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
        let candidate_direction = unit_length(*candidate_direction);
        if !matches!(
            ((dot(direction, candidate_direction).abs() - 1.0).abs())
                .partial_cmp(&(EPS_AXIS_ALIGNMENT)),
            Some(std::cmp::Ordering::Less | std::cmp::Ordering::Equal)
        ) {
            return None;
        }
        let displacement = [
            candidate_origin.x - origin[0],
            candidate_origin.y - origin[1],
            candidate_origin.z - origin[2],
        ];
        let radial = cross(displacement, direction);
        if !matches!(
            (dot(radial, radial).sqrt()).partial_cmp(&(EPS_AXIS_OFFSET * scale)),
            Some(std::cmp::Ordering::Less | std::cmp::Ordering::Equal)
        ) {
            return None;
        }
    }
    for normal in plane_normals {
        let normal = unit_length(normal);
        if !matches!(
            ((dot(direction, normal).abs() - 1.0).abs()).partial_cmp(&(EPS_AXIS_ALIGNMENT)),
            Some(std::cmp::Ordering::Less | std::cmp::Ordering::Equal)
        ) {
            return None;
        }
    }
    for center in sphere_centers {
        let displacement = [
            center.x - origin[0],
            center.y - origin[1],
            center.z - origin[2],
        ];
        let radial = cross(displacement, direction);
        if !matches!(
            (dot(radial, radial).sqrt()).partial_cmp(&(EPS_AXIS_OFFSET * scale)),
            Some(std::cmp::Ordering::Less | std::cmp::Ordering::Equal)
        ) {
            return None;
        }
    }
    cadmpeg_ir::features::FinitePoint3::new(Point3::from(origin))
        .zip(cadmpeg_ir::features::FeatureDirection3::new(Vector3::from(
            direction,
        )))
        .map(|(origin, direction)| RevolutionAxis {
            origin,
            direction,
            reference: None,
        })
}

#[cfg(test)]
mod full_turn_carrier_tests {
    use super::full_turn_revolution_carrier_axis;
    use cadmpeg_ir::document::CadIr;
    use cadmpeg_ir::features::{AngularTermination, RevolveExtent};
    use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, Surface, SurfaceGeometry};
    use cadmpeg_ir::ids::SurfaceId;
    use cadmpeg_ir::math::{Point3, Vector3};

    #[derive(Clone, Copy)]
    enum ExtraCarrier {
        None,
        Plane,
        Sphere,
    }

    fn fixture(
        extra: ExtraCarrier,
    ) -> (
        crate::container::ContainerScan<'static>,
        CadIr,
        RevolveExtent,
    ) {
        let mut scan = crate::test_support::empty_container_scan();
        let mut ir = CadIr::empty();
        let mut add = |id, kind, geometry| {
            scan.surfaces.rows.push(crate::surface::SurfaceRow {
                id,
                kind,
                feature_id: 7,
                reversed: false,
                boundary_type: crate::surface::BoundaryType::Code00,
                next_surface: 0,
                offset: usize::try_from(id).expect("fixture index fits usize"),
            });
            ir.model.surfaces.push(Surface {
                id: SurfaceId::mint(format!("creo:visibgeom:surface#{id}")).expect("surface ID"),
                geometry: SurfaceGeometry::Solved(geometry),
                source_object: None,
            });
        };
        add(
            31,
            crate::surface::SurfaceKind::Cylinder,
            SolvedSurfaceGeometry::Cylinder(
                cadmpeg_ir::geometry::analytic::CylinderSurface::try_new(
                    Point3::new(2.0, 0.0, 0.0),
                    Vector3::new(0.0, 0.0, 1.0),
                    Vector3::new(1.0, 0.0, 0.0),
                    1.0,
                )
                .expect("cylinder fixture"),
            ),
        );
        match extra {
            ExtraCarrier::None => {}
            ExtraCarrier::Plane => add(
                32,
                crate::surface::SurfaceKind::Plane,
                SolvedSurfaceGeometry::Plane(
                    cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                        Point3::new(0.0, 0.0, 0.0),
                        Vector3::new(0.0, 0.0, 1.0),
                        Vector3::new(1.0, 0.0, 0.0),
                    )
                    .expect("plane fixture"),
                ),
            ),
            ExtraCarrier::Sphere => add(
                32,
                crate::surface::SurfaceKind::TorusOrSphere,
                SolvedSurfaceGeometry::Sphere(
                    cadmpeg_ir::geometry::analytic::SphereSurface::try_new(
                        Point3::new(2.0, 0.0, 0.0),
                        Vector3::new(0.0, 0.0, 1.0),
                        Vector3::new(1.0, 0.0, 0.0),
                        1.0,
                    )
                    .expect("sphere fixture"),
                ),
            ),
        }
        let extent = RevolveExtent::OneSided {
            termination: AngularTermination::Angle {
                angle: cadmpeg_ir::scalar::PositiveAngle::new(std::f64::consts::TAU)
                    .expect("full-turn extent"),
            },
        };
        (scan, ir, extent)
    }

    fn assert_axis_resolves(extra: ExtraCarrier) {
        let (scan, ir, extent) = fixture(extra);
        let source_carriers = crate::decode::source_carriers::SourceUnitCarriers::default();
        assert!(
            full_turn_revolution_carrier_axis(&scan, &ir, &source_carriers, 7, Some(&extent))
                .is_some()
        );
    }

    #[test]
    fn full_turn_carrier_axis_resolves_from_a_cylinder() {
        assert_axis_resolves(ExtraCarrier::None);
    }

    #[test]
    fn full_turn_carrier_axis_resolves_with_a_perpendicular_plane() {
        assert_axis_resolves(ExtraCarrier::Plane);
    }

    #[test]
    fn full_turn_carrier_axis_resolves_with_a_coaxial_sphere() {
        assert_axis_resolves(ExtraCarrier::Sphere);
    }
}

pub(in super::super) fn revolution_axis_for_transfer(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &CadIr,
    source_carriers: &crate::decode::source_carriers::SourceUnitCarriers,
    feature_id: u32,
    section: (
        &crate::feature::definitions::FeatureDefinition,
        &crate::placement::FeatureSectionTransform,
    ),
    extent: Option<&RevolveExtent>,
) -> Result<Option<RevolutionAxis>, cadmpeg_core::CodecError> {
    let (definition, transform) = section;
    if let Some(axis) = resolved_revolution_axis(ctx, definition, transform)? {
        Ok(Some(axis))
    } else {
        Ok(full_turn_revolution_carrier_axis(
            scan,
            ir,
            source_carriers,
            feature_id,
            extent,
        ))
    }
}

pub(super) fn feature_revolution_axis_for_transfer(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &CadIr,
    source_carriers: &crate::decode::source_carriers::SourceUnitCarriers,
    feature_id: u32,
    extent: Option<&RevolveExtent>,
) -> Result<Option<RevolutionAxis>, cadmpeg_core::CodecError> {
    let definition = unique_feature_profile_definition(
        &scan.features.definitions,
        &scan.features.section_transforms,
        feature_id,
    );
    let mut transforms = scan
        .features
        .section_transforms
        .iter()
        .filter(|transform| transform.feature_id == Some(feature_id));
    let transform = transforms.next().filter(|_| transforms.next().is_none());
    let axis = match definition.zip(transform) {
        Some((definition, transform)) => revolution_axis_for_transfer(
            ctx,
            scan,
            ir,
            source_carriers,
            feature_id,
            (definition, transform),
            extent,
        )?,
        None => None,
    };
    if let Some(axis) = axis {
        Ok(Some(axis))
    } else {
        Ok(full_turn_revolution_carrier_axis(
            scan,
            ir,
            source_carriers,
            feature_id,
            extent,
        ))
    }
}

pub(in super::super) fn section_profile_ref(ir: &CadIr, native_ref: String) -> ProfileRef {
    let native_scope = native_ref.strip_prefix("creo:featdefs:sketch#");
    let Some(sketch) = exactly_one(
        ir.model
            .sketches
            .iter()
            .filter(|sketch| match native_scope {
                Some(scope) => sketch.id.as_str().strip_prefix("creo:model:sketch#") == Some(scope),
                None => sketch.id.as_str() == native_ref,
            }),
    ) else {
        return ProfileRef::Planar(PlanarProfileRef::Native(native_ref));
    };
    if sketch.profiles.is_empty() {
        ProfileRef::Planar(PlanarProfileRef::Native(native_ref))
    } else {
        ProfileRef::Planar(PlanarProfileRef::Sketch(sketch.id.clone()))
    }
}

pub(in super::super) fn unresolved_feature_profile_ref(feature_id: u32) -> ProfileRef {
    ProfileRef::Planar(PlanarProfileRef::Unresolved(format!(
        "creo:model:feature#{feature_id}"
    )))
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
    let mut operation_feature_ids = BTreeSet::new();
    for operation in &scan.features.operations {
        operation_feature_ids.insert(operation.feature_id);
    }
    let mut row_feature_ids = BTreeSet::new();
    for row in &scan.features.rows {
        row_feature_ids.insert(row.feature_id);
    }
    let mut datum_feature_ids = BTreeSet::new();
    for datum in &scan.planes.datums {
        datum_feature_ids.insert(datum.feature_id);
    }
    let mut generators = BTreeMap::<u32, GeometryGeneratorFeature>::new();
    for row in &scan.surfaces.rows {
        if row.feature_id == 0 {
            continue;
        }
        let generator = match generators.entry(row.feature_id) {
            std::collections::btree_map::Entry::Occupied(entry) => entry.into_mut(),
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(GeometryGeneratorFeature {
                    feature_id: row.feature_id,
                    offset: row.offset,
                    surface_ids: Vec::new(),
                    curve_ids: Vec::new(),
                })
            }
        };
        generator.offset = generator.offset.min(row.offset);
        generator.surface_ids.push(row.id);
    }
    for row in &scan.curves.topology_rows {
        if row.feature_id == 0 {
            continue;
        }
        let generator = match generators.entry(row.feature_id) {
            std::collections::btree_map::Entry::Occupied(entry) => entry.into_mut(),
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(GeometryGeneratorFeature {
                    feature_id: row.feature_id,
                    offset: row.offset,
                    surface_ids: Vec::new(),
                    curve_ids: Vec::new(),
                })
            }
        };
        generator.offset = generator.offset.min(row.offset);
        generator.curve_ids.push(row.id);
    }
    let mut output = Vec::new();
    for generator in generators.into_values() {
        if operation_feature_ids.contains(&generator.feature_id)
            || row_feature_ids.contains(&generator.feature_id)
            || datum_feature_ids.contains(&generator.feature_id)
        {
            continue;
        }
        output.push(generator);
    }
    output.as_mut_slice().sort_by_key(|left| left.offset);
    output
}

/// Return the feature identities that the model-transfer pass will emit.
///
/// Feature definitions are built while the transfer pass is still walking
/// source order. A generated face or edge can therefore name a valid
/// row-backed producer that has not been inserted into `ir.model.features`
/// yet. Derive the complete emitted identity set from the scan instead of
/// using the construction-time prefix of the IR.
pub(in super::super) fn model_feature_ids(
    scan: &ContainerScan,
) -> Result<BTreeSet<IrFeatureId>, CodecError> {
    let mut ids = BTreeSet::new();
    let mut numeric_ids = BTreeSet::new();
    for feature_id in scan
        .features
        .operations
        .iter()
        .map(|operation| operation.feature_id)
        .chain(scan.features.rows.iter().map(|row| row.feature_id))
        .chain(scan.planes.datums.iter().map(|datum| datum.feature_id))
        .chain(
            geometry_generator_features(scan)
                .into_iter()
                .map(|generator| generator.feature_id),
        )
    {
        if numeric_ids.contains(&feature_id) {
            continue;
        }
        numeric_ids.insert(feature_id);
        let text = format!("creo:model:feature#{feature_id}");
        let id = IrFeatureId::mint(text)
            .map_err(|_| CodecError::Malformed("constructed Creo feature ID is invalid".into()))?;
        ids.insert(id);
    }
    Ok(ids)
}

#[cfg(test)]
mod profile_ref_tests {
    use super::unresolved_feature_profile_ref;

    #[test]
    fn unresolved_section_profile_names_the_model_feature() {
        let profile = unresolved_feature_profile_ref(50);
        assert_eq!(
            profile,
            cadmpeg_ir::features::ProfileRef::Planar(
                cadmpeg_ir::features::PlanarProfileRef::Unresolved(
                    "creo:model:feature#50".to_owned()
                )
            )
        );
    }
}
