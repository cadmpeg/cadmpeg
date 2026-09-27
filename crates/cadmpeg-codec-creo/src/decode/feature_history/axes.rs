// SPDX-License-Identifier: Apache-2.0
//! Revolution axes, section profile refs, and geometry-generator features.

use super::super::sketch::coordinates::resolved_section_points;
use super::super::sketch::intersect::section_point_in_model;
use super::super::uniqueness::{exactly_one, unique_feature_profile_definition};
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
        .filter(|row| row.feature_id == feature_id)
        .collect::<Vec<_>>();
    (!rows.is_empty()).then_some(())?;
    let mut axes = Vec::new();
    let mut plane_normals = Vec::new();
    let mut sphere_centers = Vec::new();
    for row in rows {
        (crate::surface::unique_surface_row(&scan.surfaces.rows, row.id) == Some(row))
            .then_some(())?;
        let surfaces = ir
            .model
            .surfaces
            .iter()
            .filter(|surface| {
                crate::identity::matches_numbered_identity(
                    surface.id.as_str(),
                    "creo:visibgeom:surface#",
                    row.id,
                )
            })
            .collect::<Vec<_>>();
        let [surface] = surfaces.as_slice() else {
            return None;
        };
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
        ((dot(direction, candidate_direction).abs() - 1.0).abs() <= EPS_AXIS_ALIGNMENT)
            .then_some(())?;
        let displacement = [
            candidate_origin.x - origin[0],
            candidate_origin.y - origin[1],
            candidate_origin.z - origin[2],
        ];
        let radial = cross(displacement, direction);
        (dot(radial, radial).sqrt() <= EPS_AXIS_OFFSET * scale).then_some(())?;
    }
    for normal in plane_normals {
        let normal = unit_length(normal);
        ((dot(direction, normal).abs() - 1.0).abs() <= EPS_AXIS_ALIGNMENT).then_some(())?;
    }
    for center in sphere_centers {
        let displacement = [
            center.x - origin[0],
            center.y - origin[1],
            center.z - origin[2],
        ];
        let radial = cross(displacement, direction);
        (dot(radial, radial).sqrt() <= EPS_AXIS_OFFSET * scale).then_some(())?;
    }
    Some(RevolutionAxis {
        origin: cadmpeg_ir::features::FinitePoint3::new(Point3::from(origin))?,
        direction: cadmpeg_ir::features::FeatureDirection3::new(Vector3::from(direction))?,
        reference: None,
    })
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
    Ok(
        resolved_revolution_axis(ctx, definition, transform)?.or_else(|| {
            full_turn_revolution_carrier_axis(scan, ir, source_carriers, feature_id, extent)
        }),
    )
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
    Ok(axis.or_else(|| {
        full_turn_revolution_carrier_axis(scan, ir, source_carriers, feature_id, extent)
    }))
}

pub(in super::super) fn section_profile_ref(ir: &CadIr, native_ref: String) -> ProfileRef {
    let sketch_id = native_ref.replacen("creo:featdefs:sketch#", "creo:model:sketch#", 1);
    let Some(sketch) = exactly_one(
        ir.model
            .sketches
            .iter()
            .filter(|sketch| sketch.id.as_str() == sketch_id),
    ) else {
        return ProfileRef::Planar(PlanarProfileRef::Native(native_ref));
    };
    if sketch.profiles.is_empty() {
        ProfileRef::Planar(PlanarProfileRef::Native(native_ref))
    } else {
        ProfileRef::Planar(PlanarProfileRef::Sketch(sketch.id.clone()))
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
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
) -> Result<Vec<GeometryGeneratorFeature>, CodecError> {
    let mut operation_feature_ids = BTreeSet::new();
    for operation in &scan.features.operations {
        insert_numeric_feature_id(ctx, &mut operation_feature_ids, operation.feature_id,
            "creo generator operation feature nodes")?;
    }
    let mut row_feature_ids = BTreeSet::new();
    for row in &scan.features.rows {
        insert_numeric_feature_id(ctx, &mut row_feature_ids, row.feature_id,
            "creo generator row feature nodes")?;
    }
    let mut datum_feature_ids = BTreeSet::new();
    for datum in &scan.planes.datums {
        insert_numeric_feature_id(ctx, &mut datum_feature_ids, datum.feature_id,
            "creo generator datum feature nodes")?;
    }
    let mut generators = BTreeMap::<u32, GeometryGeneratorFeature>::new();
    for row in &scan.surfaces.rows {
        if row.feature_id == 0 {
            continue;
        }
        let generator = match generators.entry(row.feature_id) {
            std::collections::btree_map::Entry::Occupied(entry) => entry.into_mut(),
            std::collections::btree_map::Entry::Vacant(entry) => {
                ctx.charge_collection_items(1, "creo generator feature map nodes")?;
                entry.insert(GeometryGeneratorFeature {
                    feature_id: row.feature_id,
                    offset: row.offset,
                    surface_ids: Vec::new(),
                    curve_ids: Vec::new(),
                })
            }
        };
        generator.offset = generator.offset.min(row.offset);
        ctx.try_reserve_items(&mut generator.surface_ids, 1, "creo generator surface IDs")?;
        generator.surface_ids.push(row.id);
    }
    for row in &scan.curves.topology_rows {
        if row.feature_id == 0 {
            continue;
        }
        let generator = match generators.entry(row.feature_id) {
            std::collections::btree_map::Entry::Occupied(entry) => entry.into_mut(),
            std::collections::btree_map::Entry::Vacant(entry) => {
                ctx.charge_collection_items(1, "creo generator feature map nodes")?;
                entry.insert(GeometryGeneratorFeature {
                    feature_id: row.feature_id,
                    offset: row.offset,
                    surface_ids: Vec::new(),
                    curve_ids: Vec::new(),
                })
            }
        };
        generator.offset = generator.offset.min(row.offset);
        ctx.try_reserve_items(&mut generator.curve_ids, 1, "creo generator curve IDs")?;
        generator.curve_ids.push(row.id);
    }
    let mut output = Vec::new();
    for generator in generators.into_values() {
        if operation_feature_ids.contains(&generator.feature_id)
            || row_feature_ids.contains(&generator.feature_id)
            || datum_feature_ids.contains(&generator.feature_id) {
            continue;
        }
        ctx.try_reserve_items(&mut output, 1, "creo geometry generator features")?;
        output.push(generator);
    }
    output.sort_by_key(|generator| generator.offset);
    Ok(output)
}

fn insert_numeric_feature_id(
    ctx: &DecodeContext<'_>,
    ids: &mut BTreeSet<u32>,
    id: u32,
    operation: &'static str,
) -> Result<(), CodecError> {
    if !ids.contains(&id) {
        ctx.charge_collection_items(1, operation)?;
        ids.insert(id);
    }
    Ok(())
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
    let mut numeric_ids = BTreeSet::new();
    for feature_id in scan
        .features
        .operations
        .iter()
        .map(|operation| operation.feature_id)
        .chain(scan.features.rows.iter().map(|row| row.feature_id))
        .chain(scan.planes.datums.iter().map(|datum| datum.feature_id))
        .chain(geometry_generator_features(ctx, scan)?.into_iter().map(|generator| generator.feature_id))
    {
        if numeric_ids.contains(&feature_id) {
            continue;
        }
        ctx.charge_collection_items(1, "creo model feature numeric identity nodes")?;
        numeric_ids.insert(feature_id);
        let text = ctx.format_retained(
            format_args!("creo:model:feature#{feature_id}"),
            "creo model feature identity text",
        )?;
        let id = IrFeatureId::mint(text)
            .map_err(|_| CodecError::Malformed("constructed Creo feature ID is invalid".into()))?;
        ctx.charge_collection_items(1, "creo model feature identity nodes")?;
        ids.insert(id);
    }
    Ok(ids)
}

#[cfg(test)]
mod allocation_tests {
    use super::{geometry_generator_features, insert_numeric_feature_id, model_feature_ids};
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use std::collections::BTreeSet;

    fn generator_scan() -> crate::container::ContainerScan<'static> {
        let mut scan = crate::container::scan_bytes_ok(Vec::new());
        scan.surfaces.rows.push(crate::surface::SurfaceRow {
            id: 61,
            kind: crate::surface::SurfaceKind::Plane,
            feature_id: 50,
            reversed: false,
            boundary_type: crate::surface::BoundaryType::Code00,
            next_surface: 0,
            offset: 200,
        });
        scan.curves.topology_rows.push(crate::curve::CurveTopologyRow {
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

    fn generator_limit_error(limit: u64, model_ids: bool, operation: &'static str) {
        let scan = generator_scan();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("empty root is admitted");
        let error = if model_ids {
            model_feature_ids(&ctx, &scan).map(|_| ())
        } else {
            geometry_generator_features(&ctx, &scan).map(|_| ())
        }
        .expect_err("one generator exceeds the collection limit");
        assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::CollectionItems
                && resource.operation == operation), "{error:?}");
    }

    fn feature_set_limit_error(operation: &'static str) {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("empty root is admitted");
        let mut ids = BTreeSet::new();
        let error = insert_numeric_feature_id(&ctx, &mut ids, 50, operation)
            .expect_err("one source feature exceeds the collection limit");
        assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::CollectionItems
                && resource.operation == operation), "{error:?}");
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
        generator_limit_error(0, false, "creo generator feature map nodes");
    }

    #[test]
    fn generator_surface_ids_refuse_collection_limit() {
        generator_limit_error(1, false, "creo generator surface IDs");
    }

    #[test]
    fn generator_curve_ids_refuse_collection_limit() {
        generator_limit_error(2, false, "creo generator curve IDs");
    }

    #[test]
    fn geometry_generator_features_refuse_collection_limit() {
        generator_limit_error(3, false, "creo geometry generator features");
    }

    #[test]
    fn model_feature_numeric_identity_nodes_refuse_collection_limit() {
        generator_limit_error(4, true, "creo model feature numeric identity nodes");
    }

    #[test]
    fn model_feature_identity_nodes_refuse_collection_limit() {
        generator_limit_error(5, true, "creo model feature identity nodes");
    }

    #[test]
    fn model_feature_identity_text_refuses_retained_limit() {
        let scan = generator_scan();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = "creo:model:feature#50".len() as u64 - 1;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("empty root is admitted");
        let error = model_feature_ids(&ctx, &scan)
            .expect_err("one feature ID exceeds the retained limit");
        assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::RetainedBytes
                && resource.operation == "creo model feature identity text"), "{error:?}");
    }
}
