// SPDX-License-Identifier: Apache-2.0
//! Revolution axes, section profile refs, and geometry-generator features.

use super::super::sketch::coordinates::resolved_section_points;
use super::super::sketch::intersect::section_point_in_model;
use super::super::uniqueness::unique_feature_profile_definition;
use crate::container::ContainerScan;
use crate::vecmath::normalize;
use crate::vecmath::unit_length;
use crate::vecmath::{cross, dot};
use cadmpeg_core::decode::DecodeContext;
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
    let mut scratch = ctx.reserve_scoped(0, "creo revolution axis point index")?;
    let points = scratch.with_storage(|| resolved_section_points(ctx, definition))?;
    let mut axis = None;
    let mut rows = segments.rows.as_slice().iter();
    while let Some(row) =
        ctx.next_charged(&mut rows, "creo revolution axis section segment rows")?
    {
        let crate::feature::segment_rows::SegmentRow::Ordinary(segment) = row else {
            continue;
        };
        if !matches!(
            segment.kind,
            crate::feature::definitions::FeatureSegmentKind::Line(_)
        ) {
            continue;
        }
        let (Some(start), Some(end)) = (
            ctx.get_btree_map(
                &points,
                &segment.point_ids()[0],
                "creo revolution axis start point",
            )?,
            ctx.get_btree_map(
                &points,
                &segment.point_ids()[1],
                "creo revolution axis end point",
            )?,
        ) else {
            continue;
        };
        if start[0] != 0.0 || end[0] != 0.0 || start == end {
            continue;
        }
        let start = section_point_in_model(transform, *start);
        let end = section_point_in_model(transform, *end);
        let Some(direction) = normalize(std::array::from_fn(|axis| end[axis] - start[axis])) else {
            continue;
        };
        let (Some(origin), Some(direction)) = (
            cadmpeg_ir::features::FinitePoint3::new(Point3::from(start)),
            cadmpeg_ir::features::FeatureDirection3::new(Vector3::from(direction)),
        ) else {
            continue;
        };
        if axis
            .replace(RevolutionAxis {
                origin,
                direction,
                reference: None,
            })
            .is_some()
        {
            return Ok(None);
        }
    }
    Ok(axis)
}

pub(in super::super) fn full_turn_revolution_carrier_axis(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &CadIr,
    source_carriers: &crate::decode::source_carriers::SourceUnitCarriers,
    feature_id: u32,
    extent: Option<&RevolveExtent>,
) -> Result<Option<RevolutionAxis>, CodecError> {
    let Some(RevolveExtent::OneSided {
        termination: AngularTermination::Angle { angle },
    }) = extent
    else {
        return Ok(None);
    };
    let angle = angle.get();

    if (angle.abs() - std::f64::consts::TAU).abs() > EPS_FULL_TURN {
        return Ok(None);
    }

    let mut scratch = ctx.reserve_scoped(0, "creo full-turn revolution evidence")?;
    let mut rows = scan.surfaces.rows.iter();
    let mut axes = Vec::new();
    let mut plane_normals = Vec::new();
    let mut sphere_centers = Vec::new();
    let mut saw_row = false;
    while let Some(row) = ctx.next_charged(&mut rows, "creo full-turn revolution surface rows")? {
        if row.feature_id != feature_id {
            continue;
        }
        saw_row = true;
        if crate::surface::unique_surface_row(&scan.surfaces.rows, row.id) != Some(row) {
            return Ok(None);
        }
        let Some(surface) = crate::decode::uniqueness::exactly_one_by(
            ctx,
            &ir.model.surfaces,
            |surface| {
                Ok(crate::identity::matches_numbered_identity(
                    surface.id.as_str(),
                    "creo:visibgeom:surface#",
                    row.id,
                ))
            },
            "creo full-turn model surfaces",
        )?
        else {
            return Ok(None);
        };
        match source_carriers.surface_geometry(surface)? {
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(cylinder_surface)) => {
                let origin = cylinder_surface.origin().get();
                scratch.with_storage(|| {
                    ctx.reserve_vec(&mut axes, 1, "creo full-turn revolution carrier axes")
                })?;
                axes.push((origin, *cylinder_surface.frame().axis()));
            }
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(cone_surface)) => {
                let origin = cone_surface.origin().get();
                scratch.with_storage(|| {
                    ctx.reserve_vec(&mut axes, 1, "creo full-turn revolution carrier axes")
                })?;
                axes.push((origin, *cone_surface.frame().axis()));
            }
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Torus(torus_surface)) => {
                let center = torus_surface.center().get();
                scratch.with_storage(|| {
                    ctx.reserve_vec(&mut axes, 1, "creo full-turn revolution carrier axes")
                })?;
                axes.push((center, *torus_surface.frame().axis()));
            }
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane_surface)) => {
                scratch.with_storage(|| {
                    ctx.reserve_vec(
                        &mut plane_normals,
                        1,
                        "creo full-turn revolution plane normals",
                    )
                })?;
                plane_normals.push(*plane_surface.frame().axis());
            }
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(sphere_surface)) => {
                let center = sphere_surface.center().get();
                scratch.with_storage(|| {
                    ctx.reserve_vec(
                        &mut sphere_centers,
                        1,
                        "creo full-turn revolution sphere centers",
                    )
                })?;
                sphere_centers.push(center);
            }
            _ => return Ok(None),
        }
    }
    if !saw_row {
        return Ok(None);
    }
    let [(first_origin, first_direction), rest @ ..] = axes.as_slice() else {
        return Ok(None);
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
        .iter()
        .copied()
        .chain(
            ctx.admit_iter(rest, "creo full-turn revolution axis scale rest")?
                .flat_map(|(origin, _)| [origin.x, origin.y, origin.z]),
        )
        .chain(
            ctx.admit_iter(
                &sphere_centers,
                "creo full-turn revolution axis scale sphere centers",
            )?
            .flat_map(|center| [center.x, center.y, center.z]),
        )
        .map(f64::abs)
        .fold(1.0, f64::max);
    let mut items = rest.iter();
    while let Some((candidate_origin, candidate_direction)) =
        ctx.next_charged(&mut items, "creo full-turn revolution remaining axes")?
    {
        let candidate_direction = unit_length(*candidate_direction);
        if !matches!(
            ((dot(direction, candidate_direction).abs() - 1.0).abs())
                .partial_cmp(&(EPS_AXIS_ALIGNMENT)),
            Some(std::cmp::Ordering::Less | std::cmp::Ordering::Equal)
        ) {
            return Ok(None);
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
            return Ok(None);
        }
    }
    let mut normal_iter = plane_normals.iter();
    while let Some(normal) =
        ctx.next_charged(&mut normal_iter, "creo full-turn revolution plane normals")?
    {
        let normal = unit_length(*normal);
        if !matches!(
            ((dot(direction, normal).abs() - 1.0).abs()).partial_cmp(&(EPS_AXIS_ALIGNMENT)),
            Some(std::cmp::Ordering::Less | std::cmp::Ordering::Equal)
        ) {
            return Ok(None);
        }
    }
    let mut center_iter = sphere_centers.iter();
    while let Some(center) =
        ctx.next_charged(&mut center_iter, "creo full-turn revolution sphere centers")?
    {
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
            return Ok(None);
        }
    }
    Ok(
        cadmpeg_ir::features::FinitePoint3::new(Point3::from(origin))
            .zip(cadmpeg_ir::features::FeatureDirection3::new(Vector3::from(
                direction,
            )))
            .map(|(origin, direction)| RevolutionAxis {
                origin,
                direction,
                reference: None,
            }),
    )
}

#[cfg(test)]
mod full_turn_carrier_allocation_tests {
    use super::full_turn_revolution_carrier_axis;
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
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

    fn assert_limit(extra: ExtraCarrier, operation: &'static str) {
        let (scan, ir, extent) = fixture(extra);
        let source_carriers = crate::decode::source_carriers::SourceUnitCarriers::default();
        let arena = DecodeArena::new();
        let policy = DecodePolicy::service();
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");
        assert!(full_turn_revolution_carrier_axis(
            &ctx,
            &scan,
            &ir,
            &source_carriers,
            7,
            Some(&extent)
        )
        .expect("service profile admits carrier evidence")
        .is_some());
        let error = crate::test_support::last_refusal_at(
            &[],
            ResourceDimension::CollectionItems,
            operation,
            |ctx| {
                full_turn_revolution_carrier_axis(
                    ctx,
                    &scan,
                    &ir,
                    &source_carriers,
                    7,
                    Some(&extent),
                )
            },
        );
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::CollectionItems
                && resource.operation == operation),
            "{error:?}"
        );
    }

    #[test]
    fn full_turn_carrier_axes_refuse_collection_limit() {
        assert_limit(ExtraCarrier::None, "creo full-turn revolution carrier axes");
    }

    #[test]
    fn full_turn_carrier_plane_normals_refuse_collection_limit() {
        assert_limit(
            ExtraCarrier::Plane,
            "creo full-turn revolution plane normals",
        );
    }

    #[test]
    fn full_turn_carrier_sphere_centers_refuse_collection_limit() {
        assert_limit(
            ExtraCarrier::Sphere,
            "creo full-turn revolution sphere centers",
        );
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
        full_turn_revolution_carrier_axis(ctx, scan, ir, source_carriers, feature_id, extent)
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
        ctx,
        &scan.features.definitions,
        &scan.features.section_transforms,
        feature_id,
    )?;
    let transform = crate::decode::uniqueness::exactly_one_by(
        ctx,
        &scan.features.section_transforms,
        |transform| Ok(transform.feature_id == Some(feature_id)),
        "creo revolution feature transform lookup",
    )?;
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
        full_turn_revolution_carrier_axis(ctx, scan, ir, source_carriers, feature_id, extent)
    }
}

pub(in super::super) fn section_profile_ref(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    native_ref: String,
) -> Result<ProfileRef, CodecError> {
    let native_scope = native_ref.strip_prefix("creo:featdefs:sketch#");
    let matching_sketch = crate::decode::uniqueness::exactly_one_by(
        ctx,
        &ir.model.sketches,
        |sketch| match native_scope {
            Some(scope) => ctx.equal(
                &sketch.id.as_str().strip_prefix("creo:model:sketch#"),
                &Some(scope),
                "creo section profile sketch identity comparison",
            ),
            None => ctx.equal(
                sketch.id.as_str(),
                native_ref.as_str(),
                "creo section profile sketch identity comparison",
            ),
        },
        "creo section profile sketch lookup",
    )?;
    let Some(sketch) = matching_sketch else {
        return Ok(ProfileRef::Planar(PlanarProfileRef::Native(native_ref)));
    };
    if sketch.profiles.is_empty() {
        Ok(ProfileRef::Planar(PlanarProfileRef::Native(native_ref)))
    } else {
        Ok(ProfileRef::Planar(PlanarProfileRef::Sketch(
            sketch
                .id
                .try_clone_for_decode(ctx, "creo section profile sketch identity")?,
        )))
    }
}

pub(in super::super) fn unresolved_feature_profile_ref(
    ctx: &DecodeContext<'_>,
    feature_id: u32,
    operation: &'static str,
) -> Result<ProfileRef, CodecError> {
    Ok(ProfileRef::Planar(PlanarProfileRef::Unresolved(
        ctx.format_retained(format_args!("creo:model:feature#{feature_id}"), operation)?,
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
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
) -> Result<Vec<GeometryGeneratorFeature>, CodecError> {
    let mut lookup_storage = ctx.reserve_scoped(0, "Creo generator exclusion lookup")?;
    let mut operation_feature_ids = std::collections::HashSet::new();
    for operation in ctx.admit_iter(&scan.features.operations, "creo generator operation rows")? {
        lookup_storage.with_storage(|| {
            ctx.insert_hash_set(
                &mut operation_feature_ids,
                operation.feature_id,
                "creo generator operation feature nodes",
            )
        })?;
    }
    let mut row_feature_ids = std::collections::HashSet::new();
    for row in ctx.admit_iter(&scan.features.rows, "creo generator feature rows")? {
        lookup_storage.with_storage(|| {
            ctx.insert_hash_set(
                &mut row_feature_ids,
                row.feature_id,
                "creo generator row feature nodes",
            )
        })?;
    }
    let mut datum_feature_ids = std::collections::HashSet::new();
    for datum in ctx.admit_iter(&scan.planes.datums, "creo generator datum rows")? {
        lookup_storage.with_storage(|| {
            ctx.insert_hash_set(
                &mut datum_feature_ids,
                datum.feature_id,
                "creo generator datum feature nodes",
            )
        })?;
    }
    let mut map_storage = ctx.reserve_scoped(0, "Creo generator map storage")?;
    let mut generators = BTreeMap::<u32, GeometryGeneratorFeature>::new();
    for row in ctx.admit_iter(&*scan.surfaces.rows, "creo generator surface rows")? {
        if row.feature_id == 0
            || operation_feature_ids.contains(&row.feature_id)
            || row_feature_ids.contains(&row.feature_id)
            || datum_feature_ids.contains(&row.feature_id)
        {
            continue;
        }
        let generator = match map_storage.with_storage(|| {
            ctx.entry_btree_map(
                &mut generators,
                row.feature_id,
                "creo generator feature map nodes",
            )
        })? {
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
        ctx.reserve_vec(&mut generator.surface_ids, 1, "creo generator surface IDs")?;
        generator.surface_ids.push(row.id);
    }
    for row in ctx.admit_iter(&scan.curves.topology_rows, "creo generator curve rows")? {
        if row.feature_id == 0
            || operation_feature_ids.contains(&row.feature_id)
            || row_feature_ids.contains(&row.feature_id)
            || datum_feature_ids.contains(&row.feature_id)
        {
            continue;
        }
        let generator = match map_storage.with_storage(|| {
            ctx.entry_btree_map(
                &mut generators,
                row.feature_id,
                "creo generator feature map nodes",
            )
        })? {
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
        ctx.reserve_vec(&mut generator.curve_ids, 1, "creo generator curve IDs")?;
        generator.curve_ids.push(row.id);
    }
    let mut output = Vec::new();
    for (_, generator) in ctx.admit_iter(generators, "creo generator feature output rows")? {
        ctx.reserve_vec(&mut output, 1, "creo geometry generator features")?;
        output.push(generator);
    }
    ctx.stable_sort_by(
        output.as_mut_slice(),
        |value| &value.offset,
        Ord::cmp,
        "creo geometry generator features output ordering",
    )?;
    Ok(output)
}

/// Return the feature identities that the model-transfer pass will emit.
///
/// Feature definitions are built while the transfer pass is still walking
/// source order. A generated face or edge can therefore name a valid
/// row-backed producer that has not been inserted into `ir.model.features`
/// yet. Derive the complete emitted identity set from the scan instead of
/// using the construction-time prefix of the IR.
pub(in super::super) fn model_feature_ids(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
) -> Result<BTreeSet<IrFeatureId>, CodecError> {
    let mut ids = BTreeSet::new();
    let mut numeric_storage = ctx.reserve_scoped(0, "Creo model numeric identity lookup")?;
    let mut numeric_ids = BTreeSet::new();
    let geometry_generators =
        numeric_storage.with_storage(|| geometry_generator_features(ctx, scan))?;
    for feature_id in ctx
        .admit_iter(
            &scan.features.operations,
            "creo emitted operation feature IDs",
        )?
        .map(|operation| operation.feature_id)
        .chain(
            ctx.admit_iter(&scan.features.rows, "creo emitted feature row IDs")?
                .map(|row| row.feature_id),
        )
        .chain(
            ctx.admit_iter(&scan.planes.datums, "creo emitted datum feature IDs")?
                .map(|datum| datum.feature_id),
        )
        .chain(
            ctx.admit_iter(&geometry_generators, "creo emitted generator feature IDs")?
                .map(|generator| generator.feature_id),
        )
    {
        if ctx.contains_btree_set(
            &numeric_ids,
            &feature_id,
            "creo numeric feature identity lookup",
        )? {
            continue;
        }
        numeric_storage.with_storage(|| {
            ctx.insert_btree_set(
                &mut numeric_ids,
                feature_id,
                "creo model feature numeric identity nodes",
            )
        })?;
        let text = ctx.format_retained(
            format_args!("creo:model:feature#{feature_id}"),
            "creo model feature identity text",
        )?;
        let id = IrFeatureId::mint(text)
            .map_err(|_| CodecError::Malformed("constructed Creo feature ID is invalid".into()))?;
        ctx.insert_btree_set(&mut ids, id, "creo model feature identity nodes")?;
    }
    Ok(ids)
}

#[cfg(test)]
mod allocation_tests {
    use super::{geometry_generator_features, model_feature_ids, unresolved_feature_profile_ref};
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use std::collections::BTreeSet;

    fn generator_scan() -> crate::container::ContainerScan<'static> {
        let mut scan = crate::test_support::empty_container_scan();
        scan.surfaces.rows.push(crate::surface::SurfaceRow {
            id: 61,
            kind: crate::surface::SurfaceKind::Plane,
            feature_id: 50,
            reversed: false,
            boundary_type: crate::surface::BoundaryType::Code00,
            next_surface: 0,
            offset: 200,
        });
        scan.curves
            .topology_rows
            .push(crate::curve::CurveTopologyRow {
                id: 59,
                type_byte: 8,
                feature_id: 50,
                directions: [1, 0xf6],
                faces: [std::num::NonZeroU32::new(61), std::num::NonZeroU32::new(62)],
                next_edges: [59, 59],
                offset: 100,
            });
        scan
    }

    fn generator_limit_error(model_ids: bool, operation: &'static str) {
        let scan = generator_scan();
        let error = crate::test_support::last_refusal_at(
            &[],
            cadmpeg_core::decode::ResourceDimension::CollectionItems,
            operation,
            |ctx| {
                if model_ids {
                    model_feature_ids(ctx, &scan).map(|_| ())
                } else {
                    geometry_generator_features(ctx, &scan).map(|_| ())
                }
            },
        );
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::CollectionItems
                && resource.operation == operation),
            "{error:?}"
        );
    }

    fn feature_set_limit_error(operation: &'static str) {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = crate::test_support::allocation_limit_at(
            cadmpeg_core::decode::ResourceDimension::CollectionItems,
            Some(operation),
            |cap| {
                let arena = DecodeArena::new();
                let mut policy = policy;
                policy.limits.max_collection_items = cap;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
                    .expect("empty root is admitted");
                let mut ids = BTreeSet::new();
                ctx.insert_btree_set(&mut ids, 50, operation).map(|_| ())
            },
        );
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");
        let mut ids = BTreeSet::new();
        let error = ctx
            .insert_btree_set(&mut ids, 50, operation)
            .expect_err("one source feature exceeds the collection limit");
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::CollectionItems
                && resource.operation == operation),
            "{error:?}"
        );
    }

    #[test]
    fn generator_operation_feature_nodes_refuse_collection_limit() {
        feature_set_limit_error("creo generator operation feature nodes");
    }

    #[test]
    fn generator_row_feature_nodes_refuse_collection_limit() {
        feature_set_limit_error("creo generator row feature nodes");
    }

    #[test]
    fn generator_datum_feature_nodes_refuse_collection_limit() {
        feature_set_limit_error("creo generator datum feature nodes");
    }

    #[test]
    fn generator_feature_map_nodes_refuse_collection_limit() {
        generator_limit_error(false, "creo generator feature map nodes");
    }

    #[test]
    fn generator_surface_ids_refuse_collection_limit() {
        generator_limit_error(false, "creo generator surface IDs");
    }

    #[test]
    fn generator_curve_ids_refuse_collection_limit() {
        generator_limit_error(false, "creo generator curve IDs");
    }

    #[test]
    fn geometry_generator_features_refuse_collection_limit() {
        generator_limit_error(false, "creo geometry generator features");
    }

    #[test]
    fn model_feature_numeric_identity_nodes_refuse_collection_limit() {
        generator_limit_error(true, "creo model feature numeric identity nodes");
    }

    #[test]
    fn model_feature_identity_nodes_refuse_collection_limit() {
        generator_limit_error(true, "creo model feature identity nodes");
    }

    #[test]
    fn model_feature_identity_text_refuses_retained_limit() {
        let scan = generator_scan();
        let error = crate::test_support::last_refusal_at(
            &[],
            ResourceDimension::RetainedBytes,
            "creo model feature identity text",
            |ctx| model_feature_ids(ctx, &scan),
        );
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::RetainedBytes
                && resource.operation == "creo model feature identity text"),
            "{error:?}"
        );
    }

    #[test]
    fn unresolved_section_profile_identity_refuses_retained_bytes() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = crate::test_support::allocation_limit_at(
            cadmpeg_core::decode::ResourceDimension::RetainedBytes,
            Some("creo unresolved section profile identity"),
            |cap| {
                let arena = DecodeArena::new();
                let mut policy = policy;
                policy.limits.max_retained_bytes = cap;
                let (ctx, _) =
                    DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
                unresolved_feature_profile_ref(&ctx, 50, "creo unresolved section profile identity")
                    .map(|_| ())
            },
        );
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
        let error =
            unresolved_feature_profile_ref(&ctx, 50, "creo unresolved section profile identity")
                .expect_err("feature identity exceeds cap");
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::RetainedBytes
                && resource.operation == "creo unresolved section profile identity")
        );
        let profile = crate::decode::with_test_decode_ctx(|ctx| {
            unresolved_feature_profile_ref(ctx, 50, "creo unresolved section profile identity")
        })
        .expect("service profile admitted");
        assert_eq!(
            profile,
            cadmpeg_ir::features::ProfileRef::Planar(
                cadmpeg_ir::features::PlanarProfileRef::Unresolved(
                    "creo:model:feature#50".to_owned()
                )
            )
        );
    }

    #[test]
    fn unresolved_named_profile_identity_refuses_retained_bytes() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = crate::test_support::allocation_limit_at(
            cadmpeg_core::decode::ResourceDimension::RetainedBytes,
            Some("creo unresolved named profile identity"),
            |cap| {
                let arena = DecodeArena::new();
                let mut policy = policy;
                policy.limits.max_retained_bytes = cap;
                let (ctx, _) =
                    DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
                unresolved_feature_profile_ref(&ctx, 50, "creo unresolved named profile identity")
                    .map(|_| ())
            },
        );
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
        let error =
            unresolved_feature_profile_ref(&ctx, 50, "creo unresolved named profile identity")
                .expect_err("feature identity exceeds cap");
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::RetainedBytes
                && resource.operation == "creo unresolved named profile identity")
        );
    }

    #[test]
    fn model_feature_identity_validation_refuses_at_work_boundary() {
        let scan = generator_scan();
        let ids = crate::test_support::assert_work_boundaries(
            &["creo model feature identity validation"],
            |ctx| model_feature_ids(ctx, &scan),
        );
        assert_eq!(
            ids,
            BTreeSet::from([
                cadmpeg_ir::features::FeatureId::mint("creo:model:feature#50",)
                    .expect("fixture feature identity")
            ]),
        );
    }
}
