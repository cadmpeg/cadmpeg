// SPDX-License-Identifier: Apache-2.0
//! Native B-rep, prototype surfaces, positional solids, and carrier intersections.

pub(super) mod brep;
pub(super) mod cylinders;
pub(super) mod intersection_candidates;
pub(super) mod intersection_resolve;
pub(super) mod intersections;
pub(super) mod nurbs_boundaries;
mod model_ids;
mod native_ids;
pub(super) mod positional;
pub(super) mod prototypes;
pub(super) mod transfer_curves;

use crate::axis::{Axis, Sign};

use std::collections::BTreeMap;

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, Surface, SurfaceGeometry,
};
use cadmpeg_ir::ids::{CurveId, OccurrenceId, ProductDefinitionId, SurfaceId};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::products::{
    Occurrence, OccurrenceParent, ProductDefinition, ProductDefinitionKind, PrototypeReference,
};
use cadmpeg_ir::transform::Transform;
use cadmpeg_ir::{AnnotationBuilder, Exactness, SourceObjectAssociation};

use crate::container::ContainerScan;

use super::native::annotate;

/// Builds the apex cone the positional and legacy carrier routes all state.
///
/// The apex is the frame origin, the radius there is zero and the ratio is one, so the half
/// angle alone states the taper.
fn apex_cone(
    frame: &crate::surface::PositionalFrame,
    half_angle: crate::surface::ApexConeHalfAngle,
) -> cadmpeg_ir::geometry::analytic::ConeSurface {
    cadmpeg_ir::geometry::analytic::ConeSurface::new(
        frame.finite_origin(),
        frame.orthonormal_frame(),
        cadmpeg_ir::scalar::NonNegativeLength::ZERO,
        cadmpeg_ir::scalar::PositiveReal::ONE,
        half_angle.get().into(),
    )
}

/// Select the IR surface namespace and its canonical prefix for a native ID.
///
/// Visible geometry, non-visible geometry, and active-datum rows share the
/// compact native identifier space used by topology links. Keep their source
/// namespaces distinct when only one namespace owns the identifier; visible
/// geometry remains the default for the existing and rowless feature-carrier
/// paths.
fn native_surface_namespace(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    surface_id: u32,
) -> Result<(cadmpeg_ir::ids::IdentityNamespace, &'static str), cadmpeg_core::CodecError> {
    let visible_present = scan.surfaces.rows.contains_id(surface_id);
    let nonvisible_present = !visible_present && scan.surfaces.nonvisible_rows.contains_id(surface_id);
    let active_datum_present = !visible_present && !nonvisible_present && ctx.any_by(
        &scan.planes.datum_cylinders, |cylinder| Ok(cylinder.id == surface_id),
        "creo datum surface namespace search")?;
    Ok(if visible_present {
        (
            crate::identity::VISIBGEOM_SURFACE,
            "creo:visibgeom:surface#",
        )
    } else if nonvisible_present {
        (
            crate::identity::NOVISGEOM_SURFACE,
            "creo:novisgeom:surface#",
        )
    } else if active_datum_present {
        (crate::identity::ACTDATUM_SURFACE, "creo:actdatums:surface#")
    } else {
        (
            crate::identity::VISIBGEOM_SURFACE,
            "creo:visibgeom:surface#",
        )
    })
}

/// Construct the selected surface identity for a native topology identifier.
pub(super) fn native_surface_id(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    surface_id: u32,
) -> Result<SurfaceId, cadmpeg_core::CodecError> {
    crate::identity::compose_checked(
        ctx,
        &native_surface_namespace(ctx, scan, surface_id)?.0,
        surface_id,
        "creo native surface identity",
    )
}

/// Compare a selected native surface identity without constructing one.
pub(super) fn matches_native_surface_id(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    surface_id: u32,
    candidate: &SurfaceId,
) -> Result<bool, cadmpeg_core::CodecError> {
    Ok(crate::identity::matches_numbered_identity(
        candidate.as_str(),
        native_surface_namespace(ctx, scan, surface_id)?.1,
        surface_id,
    ))
}

/// Return a native surface row only when its compact identifier is unique
/// across visible and non-visible geometry namespaces.
pub(super) fn unique_native_surface_row<'a>(
    scan: &'a ContainerScan<'_>,
    surface_id: u32,
) -> Option<&'a crate::surface::SurfaceRow> {
    let visible = &scan.surfaces.rows;
    let nonvisible = &scan.surfaces.nonvisible_rows;
    match (
        visible.contains_id(surface_id),
        nonvisible.contains_id(surface_id),
    ) {
        (true, false) => visible.unique(surface_id),
        (false, true) => nonvisible.unique(surface_id),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        matches_native_surface_id, native_surface_id, native_surface_namespace,
        transfer_part_product,
    };
    use crate::surface::{SurfaceKind, SurfaceRow};
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    fn named_scan() -> crate::container::ContainerScan<'static> {
        let mut scan = crate::test_support::empty_container_scan();
        scan.framing.model_name = Some(crate::container::ModelName {
            name: "wheel".into(),
            offset: 0,
        });
        scan
    }

    fn limited_product(
        scan: &crate::container::ContainerScan<'_>,
        collection_limit: u64,
        retained_limit: u64,
        with_body: bool,
    ) -> Result<bool, cadmpeg_core::CodecError> {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = collection_limit;
        policy.limits.max_retained_bytes = retained_limit;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
        let mut ir = cadmpeg_ir::document::CadIr::empty();
        if with_body {
            ir.model.bodies.push(cadmpeg_ir::topology::Body {
                id: cadmpeg_ir::ids::BodyId::mint("creo:test:body#1").expect("identity grammar"),
                kind: cadmpeg_ir::topology::BodyKind::Solid,
                regions: Vec::new(),
                transform: None,
                name: None,
                color: None,
                visible: None,
            });
        }
        transfer_part_product(
            &ctx,
            scan,
            &mut ir,
            &mut cadmpeg_ir::AnnotationBuilder::new(),
            &crate::decode::source_carriers::SourceUnitCarriers::default(),
        )
    }

    #[test]
    fn part_product_refuses_before_model_vector_growth() {
        let error = limited_product(&named_scan(),crate::test_support::allocation_limit_at(cadmpeg_core::decode::ResourceDimension::CollectionItems, Some("creo model product definitions"), |cap| limited_product(&named_scan(),cap, u64::MAX, false)), u64::MAX, false)
            .expect_err("product exceeds the configured resource limit");
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::CollectionItems
                && resource.operation == "creo model product definitions")
        );
    }

    #[test]
    fn part_product_name_copies_refuse_before_each_retained_growth() {
        // Each copy boundary includes the preceding annotation backing nodes.
        for operation in [
            "creo product definition reference",
            "creo product source name",
            "creo product label",
            "creo product part number",
            "creo occurrence name",
        ] {
            let error = cadmpeg_test_support::refusal::resource_limit_at(
                ResourceDimension::RetainedBytes,
                operation,
                |cap| limited_product(&named_scan(), u64::MAX, cap, false),
            );
            assert!(
                matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
                if resource.dimension == ResourceDimension::RetainedBytes
                    && resource.operation == operation),
                "{operation}: {error:?}"
            );
        }
    }

    #[test]
    fn part_product_identities_refuse_before_allocation() {
        let error = limited_product(&named_scan(), u64::MAX,crate::test_support::allocation_limit_at(cadmpeg_core::decode::ResourceDimension::RetainedBytes, Some("creo occurrence identity"), |cap| limited_product(&named_scan(), u64::MAX,cap, false)), false)
            .expect_err("product exceeds the configured resource limit");
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::RetainedBytes
                && resource.operation == "creo occurrence identity")
        );

        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = 0;
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root admitted");
        let error = transfer_part_product(
            &ctx,
            &named_scan(),
            &mut cadmpeg_ir::document::CadIr::empty(),
            &mut cadmpeg_ir::AnnotationBuilder::new(),
            &crate::decode::source_carriers::SourceUnitCarriers::default(),
        )
        .expect_err("product identity exceeds temporary-byte limit");
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::MaterializedBytes
                && resource.operation == "creo product identity")
        );
    }

    #[test]
    fn part_product_refuses_before_body_reference_rows_and_ids() {
        let error = limited_product(&named_scan(),crate::test_support::allocation_limit_at(cadmpeg_core::decode::ResourceDimension::CollectionItems, Some("creo product body references"), |cap| limited_product(&named_scan(),cap, u64::MAX, true)), u64::MAX, true)
            .expect_err("product exceeds the configured resource limit");
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::CollectionItems
                && resource.operation == "creo product body references"),
            "{error:?}"
        );
        // Admit annotation and body-reference slots before refusing the copied body identity.
        let error = cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::RetainedBytes,
            "creo product body IDs",
            |cap| limited_product(&named_scan(), u64::MAX, cap, true),
        );
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::RetainedBytes
                && resource.operation == "creo product body IDs"),
            "{error:?}"
        );
    }

    #[test]
    fn part_product_transfer_preserves_typed_product_and_body_ids() {
        let body_ids = [
            cadmpeg_ir::ids::BodyId::mint("creo:test:body#1").expect("valid ASCII body identity"),
            cadmpeg_ir::ids::BodyId::mint("creo:test:body#support-café")
                .expect("valid Unicode body identity"),
        ];
        let mut ir = cadmpeg_ir::document::CadIr::empty();
        for id in &body_ids {
            ir.model.bodies.push(cadmpeg_ir::topology::Body {
                id: id.clone(),
                kind: cadmpeg_ir::topology::BodyKind::Solid,
                regions: Vec::new(),
                transform: None,
                name: None,
                color: None,
                visible: None,
            });
        }
        let mut annotations = cadmpeg_ir::AnnotationBuilder::new();
        let source_carriers = crate::decode::source_carriers::SourceUnitCarriers::default();

        let transferred = crate::decode::with_test_decode_ctx(|ctx| {
            transfer_part_product(
                ctx,
                &named_scan(),
                &mut ir,
                &mut annotations,
                &source_carriers,
            )
        })
        .expect("service part product transfer");

        assert!(transferred);
        assert_eq!(ir.model.product_definitions.len(), 1);
        let product = &ir.model.product_definitions[0];
        assert_eq!(product.id.as_str(), "creo:model:product_definition#root");
        assert_eq!(
            product
                .bodies
                .iter()
                .map(cadmpeg_ir::ids::BodyId::as_str)
                .collect::<Vec<_>>(),
            ["creo:test:body#1", "creo:test:body#support-café"]
        );
        assert_eq!(ir.model.occurrences.len(), 1);
        assert_eq!(
            ir.model.occurrences[0].id.as_str(),
            "creo:model:occurrence#root"
        );
        assert!(matches!(
            &ir.model.occurrences[0].prototype,
            cadmpeg_ir::products::PrototypeReference::Local { definition }
                if definition.as_str() == product.id.as_str()
        ));
    }

    #[test]
    fn part_product_identity_retention_refuses_before_occurrence_transfer() {
        let product_id_len = cadmpeg_core::decode::u64_from_index(
            cadmpeg_ir::ids::ProductDefinitionId::compose(
                &crate::identity::MODEL_PRODUCT_DEFINITION,
                cadmpeg_ir::identity_key!("root"),
            )
            .as_str()
            .len(),
        );
        // Identity retention follows annotation nodes and product-vector capacity.
        let error = cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::RetainedBytes,
            "creo product identity",
            |cap| limited_product(&named_scan(), u64::MAX, cap, false),
        );
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::RetainedBytes && limit.operation == "creo product identity"
                && limit.additional == product_id_len)
        );
    }

    #[test]
    fn native_surface_id_preserves_nonvisible_namespace() {
        let mut scan = crate::test_support::empty_container_scan();
        scan.surfaces.nonvisible_rows.push(SurfaceRow {
            id: 17,
            kind: SurfaceKind::Plane,
            feature_id: 1,
            reversed: false,
            boundary_type: crate::surface::BoundaryType::Code00,
            next_surface: 0,
            offset: 0,
        });

        let native = crate::decode::with_test_decode_ctx(|ctx| native_surface_id(ctx, &scan, 17))
            .expect("service native surface identity admitted");
        assert_eq!(native.as_str(), "creo:novisgeom:surface#17");
        let prefix =
            crate::decode::with_test_decode_ctx(|ctx| native_surface_namespace(ctx, &scan, 17))
                .expect("surface namespace")
                .1;
        assert!(crate::identity::matches_numbered_identity(
            native.as_str(),
            prefix,
            17,
        ));
        assert!(
            crate::decode::with_test_decode_ctx(|ctx| matches_native_surface_id(
                ctx, &scan, 17, &native
            ))
            .expect("surface match")
        );
        let visible = cadmpeg_ir::ids::SurfaceId::compose(&crate::identity::VISIBGEOM_SURFACE, 17);
        assert!(!crate::identity::matches_numbered_identity(
            visible.as_str(),
            prefix,
            17,
        ));
        assert!(
            !crate::decode::with_test_decode_ctx(|ctx| matches_native_surface_id(
                ctx, &scan, 17, &visible
            ))
            .expect("surface match")
        );
    }

    #[test]
    fn native_surface_id_refuses_retained_limit() {
        let scan = named_scan();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root admitted");
        let error = native_surface_id(&ctx, &scan, 17).expect_err("surface ID refused");
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::RetainedBytes
                && resource.operation == "creo native surface identity")
        );
    }


}

pub(super) fn transfer_part_product(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    source_carriers: &crate::decode::source_carriers::SourceUnitCarriers,
) -> Result<bool, cadmpeg_core::CodecError> {
    let Some(model_name) = scan.framing.model_name.as_ref() else {
        return Ok(false);
    };
    let model_name_offset = model_name.offset;
    let model_name = &model_name.name;
    let (product_id, product_id_reservation) =
        crate::identity::compose_scoped::<ProductDefinitionId>(
            ctx,
            &crate::identity::MODEL_PRODUCT_DEFINITION,
            "root",
            "creo product identity",
        )?;
    let occurrence_id = crate::identity::compose_checked::<OccurrenceId>(
        ctx,
        &crate::identity::MODEL_OCCURRENCE,
        "root",
        "creo occurrence identity",
    )?;
    annotate(
        ctx,
        annotations,
        &product_id,
        "archive_header",
        cadmpeg_core::decode::u64_from_index(model_name_offset),
        "part_product",
        Exactness::Derived,
    )?;
    annotate(
        ctx,
        annotations,
        &occurrence_id,
        "archive_header",
        cadmpeg_core::decode::u64_from_index(model_name_offset),
        "part_product_occurrence",
        Exactness::Derived,
    )?;
    ctx.charge_entities(1, "admit Creo model product_definitions")?;
    let mut bodies = Vec::new();
    ctx.reserve_vec(
        &mut bodies,
        ir.model.bodies.len(),
        "creo product body references",
    )?;
    for body in ctx.admit_iter(
        &ir.model.bodies,
        "creo transfer part product bodies traversal",
    )? {
        bodies.push(body.id.try_clone_for_decode(ctx, "creo product body IDs")?);
    }
    let product_ref = product_id.try_clone_for_decode(ctx, "creo product definition reference")?;
    let source_name = ctx.copy_retained_text(model_name, "creo product source name")?;
    let label = ctx.copy_retained_text(model_name, "creo product label")?;
    let part_number = ctx.copy_retained_text(model_name, "creo product part number")?;
    ctx.reserve_vec(
        &mut ir.model.product_definitions,
        1,
        "creo model product definitions",
    )?;
    ir.model.product_definitions.push(ProductDefinition {
        id: product_ref,
        kind: ProductDefinitionKind::Part,
        source_name: Some(source_name),
        label: Some(label),
        description: None,
        part_number: Some(part_number),
        bom_properties: BTreeMap::default(),
        bodies,
        native_ref: None,
    });
    ctx.charge_entities(1, "admit Creo model occurrences")?;
    let occurrence_name = ctx.copy_retained_text(model_name, "creo occurrence name")?;
    product_id_reservation.commit()?;
    source_carriers.admit_occurrence(
        ctx,
        ir,
        Occurrence {
            id: occurrence_id,
            prototype: PrototypeReference::Local {
                definition: product_id,
            },
            parent: OccurrenceParent::Root {},
            ordinal: 0,
            transform: Transform::identity(),
            linked_prototype: None,
            scale: [cadmpeg_ir::scalar::FiniteReal::ONE; 3],
            name: Some(occurrence_name),
            visible: None,
            link: None,
            native_ref: None,
        },
    )?;
    Ok(true)
}

pub(super) fn fc05_model_frame(
    axis_index: Axis,
    axis_ordinate: f64,
    center_row_frame: [f64; 2],
    reference_row_frame: [f64; 2],
    axis_sign: Sign,
) -> ([f64; 3], [f64; 3], [f64; 3]) {
    let [first, second] = center_row_frame;
    let [reference_x, reference_z] = reference_row_frame;
    let axis_sign = axis_sign.scale();
    match axis_index {
        Axis::X => (
            [axis_ordinate, second, first],
            [axis_sign, 0.0, 0.0],
            [0.0, reference_z, reference_x],
        ),
        Axis::Y => (
            [first, axis_ordinate, second],
            [0.0, axis_sign, 0.0],
            [reference_x, 0.0, reference_z],
        ),
        Axis::Z => (
            [second, first, axis_ordinate],
            [0.0, 0.0, axis_sign],
            [reference_z, reference_x, 0.0],
        ),
    }
}

const EPS_FC05_CAP_FRAME: f64 = 1.0e-9;

#[derive(Clone, Copy)]
pub(super) struct Fc05CapPairFrame {
    /// Model-space origin of the native cylinder parameterization (`v = 0`).
    pub(super) origin: [f64; 3],
    pub(super) ref_direction: [f64; 3],
    axis_index: Axis,
    axis_sign: Sign,
}

impl Fc05CapPairFrame {
    /// The signed unit vector of the cylinder axis.
    pub(super) fn unit_vector(self) -> [f64; 3] {
        let mut axis = [0.0; 3];
        axis[self.axis_index.index()] = self.axis_sign.scale();
        axis
    }
}

/// Resolve one cap-pair cylinder in model space from its two placed cap planes.
///
/// The cap ordinates and the cap-plane origins must describe one translation
/// along the cap normal. This is the same bounded witness used by B-rep
/// transfer and is also available to analytic plane-branch selection.
pub(super) fn fc05_cap_pair_model_frame(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    pair: &crate::curve::Fc05CylinderCapPair,
) -> Result<Option<Fc05CapPairFrame>, cadmpeg_core::CodecError> {
    if pair.cap_edges.len() < 2 { return Ok(None); }
    let outlines = native_ids::UniqueRows::new(ctx, &scan.planes.outlines,
        |plane| Some(plane.surface_id), "creo cap pair outline index")?;
    let mut placed_caps = pair.cap_edges.iter();
    let Some(first) = ctx.next_charged(&mut placed_caps, "creo cap pair placed edge traversal")? else { return Ok(None); };
    let Some(first_cap) = outlines.unique(first.cap_plane_id) else { return Ok(None); };
    let first_ordinate = first.cap_ordinate_row_frame;
    let Some(last) = ctx.next_charged(&mut placed_caps, "creo cap pair placed edge traversal")? else { return Ok(None); };
    let Some(mut last_cap) = outlines.unique(last.cap_plane_id) else { return Ok(None); };
    let mut last_ordinate = last.cap_ordinate_row_frame;
    let Some(axis_index) = Axis::ALL
        .into_iter()
        .find(|axis| first_cap.normal()[axis.index()].abs() > 1.0 - EPS_FC05_CAP_FRAME)
    else {
        return Ok(None);
    };
    if last_cap.normal != first_cap.normal {
        return Ok(None);
    }
    let offsets = |plane: &crate::surface::OutlinePlane, ordinate: f64| {
        let origin = first_cap.origin[axis_index.index()];
        let current = plane.origin[axis_index.index()];
        [(current - ordinate - (origin - first_ordinate)).abs(),
            (current + ordinate - (origin + first_ordinate)).abs()]
    };
    let mut disagreement = offsets(last_cap, last_ordinate);
    while let Some(edge) = ctx.next_charged(&mut placed_caps, "creo cap pair placed edge traversal")? {
        let Some(plane) = outlines.unique(edge.cap_plane_id) else { return Ok(None); };
        if plane.normal != first_cap.normal { return Ok(None); }
        last_cap = plane;
        last_ordinate = edge.cap_ordinate_row_frame;
        let current = offsets(plane, last_ordinate);
        disagreement = std::array::from_fn(|index| disagreement[index].max(current[index]));
    }
    let row_span = last_ordinate - first_ordinate;
    let model_span = last_cap.origin[axis_index.index()] - first_cap.origin[axis_index.index()];
    let span_scale = row_span.abs().max(model_span.abs()).max(1.0);
    if !row_span.is_finite()
        || !model_span.is_finite()
        || row_span.abs() <= EPS_FC05_CAP_FRAME
        || (row_span.abs() - model_span.abs()).abs() > EPS_FC05_CAP_FRAME * span_scale
    {
        return Ok(None);
    }
    let axis_sign = if (model_span / row_span).is_sign_negative() {
        Sign::Negative
    } else {
        Sign::Positive
    };
    let axis_origin = first_cap.origin[axis_index.index()] - axis_sign.scale() * first_ordinate;
    let disagreement = match axis_sign { Sign::Positive => disagreement[0], Sign::Negative => disagreement[1] };
    if disagreement > EPS_FC05_CAP_FRAME {
        // A cap pair whose row-frame and model-space spans do not agree does
        // not establish a unit parameter-axis transform. Retain the circles
        // for their independent carrier evidence, but do not invent a chart.
        return Ok(None);
    }
    let (origin, _, ref_direction) = fc05_model_frame(
        axis_index,
        axis_origin,
        pair.center_row_frame,
        pair.reference_direction_row_frame,
        axis_sign,
    );
    Ok(Some(Fc05CapPairFrame {
        origin,
        ref_direction,
        axis_index,
        axis_sign,
    }))
}

pub(super) fn transfer_fc05_cap_circles(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    source_carriers: &mut crate::decode::source_carriers::SourceUnitCarriers,
) -> Result<(), cadmpeg_core::CodecError> {
    let mut curves_index = model_ids::ModelIdentityIndex::new(ctx)?;
    let mut surfaces_index = model_ids::ModelIdentityIndex::new(ctx)?;
    if scan.curves.fc05_circles.is_empty() { return Ok(()); }
    let topologies = native_ids::UniqueRows::new(ctx, &scan.curves.topology_rows,
        |row| Some(row.id), "creo cap circle topology index")?;
    let outlines = native_ids::UniqueRows::new(ctx, &scan.planes.outlines,
        |plane| Some(plane.surface_id), "creo cap circle outline index")?;
    for circle in ctx.admit_iter(
        &scan.curves.fc05_circles,
        "creo transfer fc05 cap circles fc05 circles traversal",
    )? {
        let Some(topology) = topologies.unique(circle.curve_id) else { continue; };
        let cap_plane = crate::decode::uniqueness::exactly_one(
            topology.bounded_face_ids().filter_map(|face| {
                crate::surface::unique_surface_row(&scan.surfaces.rows, face)
                    .filter(|row| row.kind == crate::surface::SurfaceKind::Plane)?;
                outlines.unique(face)
            }),
        );
        let cylinder =
            crate::decode::uniqueness::exactly_one(topology.bounded_face_ids().filter(|face| {
                crate::surface::unique_surface_row(&scan.surfaces.rows, *face)
                    .is_some_and(|row| row.kind == crate::surface::SurfaceKind::Cylinder)
            }));
        let (Some(cap), Some(cylinder_id), Some(_)) =
            (cap_plane, cylinder, circle.cap_ordinate_row_frame)
        else {
            continue;
        };
        let Some(axis_index) = Axis::ALL
            .into_iter()
            .find(|axis| cap.normal()[axis.index()].abs() > 1.0 - EPS_FC05_CAP_FRAME)
        else {
            continue;
        };
        let [first, second] = circle.center_row_frame;
        let pair_frame = match ctx.find_by(
            &scan.curves.fc05_cylinder_cap_pairs,
            |pair| Ok(pair.surface_id == cylinder_id),
            "creo circle cap pair search",
        )? {
            Some(pair) => fc05_cap_pair_model_frame(ctx, scan, pair)?,
            None => None,
        };
        let (reference, circle_axis_sign) = match circle.angle_parameter {
            crate::curve::Fc05AngleParameterRelation::Inconsistent => (
                circle.sample_direction_row_frame.get(),
                Sign::of_component(cap.normal()[axis_index.index()]),
            ),
            crate::curve::Fc05AngleParameterRelation::Consistent {
                sense,
                reference_direction_row_frame,
            } => (reference_direction_row_frame, Sign::from(sense).reversed()),
        };
        let axis_sign = pair_frame.map_or(circle_axis_sign, |frame| frame.axis_sign);
        let legacy_frame = fc05_model_frame(
            axis_index,
            cap.origin[axis_index.index()],
            [first, second],
            reference,
            axis_sign,
        );
        let witness = crate::decode::analytic::planes::fc05_cylinder_model_witness(
            ctx,
            scan,
            cylinder_id,
            crate::decode::analytic::equations::CylinderEquation {
                origin: legacy_frame.0,
                axis: legacy_frame.1,
                ref_direction: legacy_frame.2,
                radius: circle.radius_mm,
            },
        )?;
        let mut surface_origin = witness.origin;
        if let Some(frame) = pair_frame {
            surface_origin[axis_index.index()] = frame.origin[axis_index.index()];
        }
        let (center, axis, ref_direction) = (witness.origin, witness.axis, witness.ref_direction);
        let (id, id_storage) = crate::identity::compose_scoped::<CurveId>(
            ctx,
            &crate::identity::VISIBGEOM_CURVE,
            circle.curve_id,
            "creo FC05 cap circle identity",
        )?;
        let identity_present = curves_index.lookup(ctx, &ir.model.curves, |record| record.id.as_str(), id.as_str())?.is_some();
        if !identity_present {
            let Ok(circle_curve) = cadmpeg_ir::geometry::analytic::CircleCurve::try_new(
                Point3::from(center),
                Vector3::from(axis),
                Vector3::from(ref_direction),
                circle.radius_mm,
            ) else {
                continue;
            };
            id_storage.commit()?;
        annotate(
                ctx,
                annotations,
                &id,
                "VisibGeom",
                cadmpeg_core::decode::u64_from_index(circle.offset),
                "fc05_cap_circle",
                Exactness::Derived,
            )?;
            ctx.charge_entities(1, "admit Creo model curves")?;
            source_carriers.admit_curve(
                ctx,
                ir,
                Curve {
                    id,
                    geometry: CurveGeometry::Solved(SolvedCurveGeometry::Circle(circle_curve)),
                    source_object: Some(SourceObjectAssociation {
                        format: cadmpeg_ir::CodecFormat::Creo,
                        object_id: cadmpeg_core::text::NonBlankString::for_decode(
                            ctx,
                            ctx.format_retained(
                                format_args!("VisibGeom:{}", circle.curve_id),
                                "creo FC05 cap circle source object ID",
                            )?,
                            "validate nonblank text",
                        )?
                        .ok_or_else(|| {
                            cadmpeg_core::CodecError::malformed(
                                "source object_id must not be empty",
                            )
                        })?,
                        name: None,
                        color: None,
                        visible: None,
                        layer: None,
                        instance_path: Vec::new(),
                    }),
                },
            )?;
        }
        let (surface_id, surface_id_storage) = crate::identity::compose_scoped::<SurfaceId>(
            ctx,
            &crate::identity::VISIBGEOM_SURFACE,
            cylinder_id,
            "creo FC05 axis cylinder identity",
        )?;
        let identity_present = surfaces_index.lookup(ctx, &ir.model.surfaces, |record| record.id.as_str(), surface_id.as_str())?.is_some();
        if identity_present {
            continue;
        }
        let Ok(cylinder_surface) = cadmpeg_ir::geometry::analytic::CylinderSurface::try_new(
            Point3::from(surface_origin),
            Vector3::from(axis),
            Vector3::from(ref_direction),
            circle.radius_mm,
        ) else {
            continue;
        };
        surface_id_storage.commit()?;
        annotate(
            ctx,
            annotations,
            &surface_id,
            "VisibGeom",
            cadmpeg_core::decode::u64_from_index(circle.offset),
            "fc05_axis_cylinder",
            Exactness::Derived,
        )?;
        ctx.charge_entities(1, "admit Creo model surfaces")?;
        source_carriers.admit_surface(
            ctx,
            ir,
            Surface {
                id: surface_id,
                geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
                    cylinder_surface,
                )),
                source_object: Some(SourceObjectAssociation {
                    format: cadmpeg_ir::CodecFormat::Creo,
                    object_id: cadmpeg_core::text::NonBlankString::for_decode(
                        ctx,
                        ctx.format_retained(
                            format_args!("VisibGeom:{cylinder_id}"),
                            "creo FC05 axis cylinder source object ID",
                        )?,
                        "validate nonblank text",
                    )?
                    .ok_or_else(|| {
                        cadmpeg_core::CodecError::malformed("source object_id must not be empty")
                    })?,
                    name: None,
                    color: None,
                    visible: None,
                    layer: None,
                    instance_path: Vec::new(),
                }),
            },
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod fc05_tests;
