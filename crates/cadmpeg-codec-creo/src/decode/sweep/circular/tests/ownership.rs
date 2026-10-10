// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::sketches::{
    Sketch, SketchEntity, SketchEntityId, SketchEntityUse, SketchGeometry,
    SketchGeometryDefinition, SketchId, SketchPlacement, SketchProfiles,
};
use cadmpeg_ir::topology::Sense;
use cadmpeg_ir::AnnotationBuilder;

fn circular_scan() -> crate::container::ContainerScan<'static> {
    let mut scan = crate::test_support::empty_container_scan();
    scan.features
        .definitions
        .push(crate::feature::definitions::FeatureDefinition {
            identity: crate::feature::definitions::DefinitionIdentity::Parsed {
                schema_id: std::num::NonZeroU32::new(40),
                owner_feature_id: Some(40),
            },
            body: Vec::new(),
            parameter_frames: Vec::new(),
            outlines: Vec::new(),
            variables: None,
            segments: None,
            trim_entities: None,
            trim_vertices: None,
            order_table: None,
            section_3d: Some(crate::feature::definitions::FeatureSection3d {
                sketch_plane_entity_id: None,
                sketch_plane_flip: None,
                reference_planes: crate::feature::definitions::ReferencePlanes::Named(Vec::new()),
                reference_plane_datum_geometry_id: None,
                orientation: crate::feature::definitions::FeatureSectionOrientation::default(),
                dimension_ids: Vec::new(),
                offset: 0,
            }),
            dimensions: None,
            relations: None,
            saved_section: None,
            offset: 0,
        });
    scan.features.section_transforms.push(
        crate::placement::FeatureSectionTransform::new(
            40,
            Some(40),
            [0.0; 3],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            0,
        )
        .expect("section frame"),
    );
    scan.features
        .operations
        .push(crate::feature::operations::FeatureOperation {
            feature_id: 40,
            kind: crate::feature::operations::OperationKind::Extrude,
            name: crate::feature::operations::OperationName::Derived,
            recipe: crate::feature::operations::RecipeResolution::Resolved(
                crate::feature::operations::FeatureRecipe::ProtrudeExtrude,
            ),
            display_state_conflict: false,
            depdb: None,
            offset: 0,
            state_offset: 0,
        });
    scan.features.rows.push(crate::feature::rows::FeatureRow {
        feature_id: 40,
        root_schema_class: Some(crate::feature::schema::SchemaClass::Protrusion),
        stream_offset: 0,
        body: vec![0, 0].try_into().expect("two row header bytes"),
        body_offset: 0,
        offset: 0,
    });
    for (id, z) in [(21, -1.0), (22, 1.0)] {
        scan.surfaces.rows.push(crate::surface::SurfaceRow {
            id,
            kind: crate::surface::SurfaceKind::Plane,
            feature_id: 40,
            reversed: false,
            boundary_type: crate::surface::BoundaryType::Code00,
            next_surface: 0,
            offset: 0,
        });
        scan.planes.outlines.push(crate::surface::OutlinePlane {
            surface_id: id,
            origin: [0.0, 0.0, z],
            normal: cadmpeg_ir::units::UnitVector3::Z_AXIS,
            u_axis: cadmpeg_ir::units::UnitVector3::X_AXIS,
            offset: 0,
        });
    }
    scan
}

fn source_circle() -> CadIr {
    let sketch = SketchId::mint("creo:model:sketch#40").expect("sketch identity");
    let entity = SketchEntityId::mint("creo:test:sketch_entity#40").expect("entity identity");
    let mut ir = CadIr::empty();
    ir.model.sketches.push(Sketch {
        id: sketch.clone(),
        name: None,
        configuration: None,
        visible: None,
        placement: SketchPlacement::Unresolved {},
        profiles: SketchProfiles::try_from(vec![vec![SketchEntityUse {
            entity: entity.clone(),
            reversed: false,
        }]])
        .expect("one circle profile"),
        native_ref: None,
    });
    ir.model.sketch_entities.push(SketchEntity::new(
        entity,
        sketch,
        SketchGeometry::try_from(SketchGeometryDefinition::Circle {
            center: Point2::new(1.0, 0.0),
            radius: cadmpeg_ir::scalar::Length::new(2.0).expect("finite radius"),
        })
        .expect("source circle"),
    ));
    ir
}

fn transfer(ctx: &DecodeContext<'_>) -> Result<CadIr, CodecError> {
    let scan = circular_scan();
    let mut ir = source_circle();
    let mut losses = Vec::new();
    let count = super::super::transfer_resolved_circular_extrusion_breps(
        ctx,
        &scan,
        &mut ir,
        &mut AnnotationBuilder::new(),
        &mut losses,
        &mut crate::decode::source_carriers::SourceUnitCarriers::default(),
    )?;
    assert_eq!(count, 1);
    assert!(losses.is_empty());
    Ok(ir)
}

#[test]
fn circular_extrusion_coedge_rows_transfer_both_radial_pairs_in_order() {
    let ir = crate::test_support::assert_retained_boundaries(
        &[
            "creo circular extrusion identity",
            "creo circular extrusion identity copy",
        ],
        transfer,
    );
    assert_eq!(ir.model.bodies.len(), 1);
    assert_eq!(ir.model.shells.len(), 1);
    assert_eq!(ir.model.faces.len(), 3);
    assert_eq!(ir.model.loops.len(), 4);
    assert_eq!(ir.model.coedges.len(), 4);
    for (index, side) in ["bottom", "top"].into_iter().enumerate() {
        let cap = &ir.model.coedges[index];
        let wall = &ir.model.coedges[index + 2];
        assert_eq!(
            cap.id.as_str(),
            format!("creo:feature:extrusion#40:coedge:{side}:cap")
        );
        assert_eq!(
            wall.id.as_str(),
            format!("creo:feature:extrusion#40:coedge:{side}:side")
        );
        assert_eq!(cap.radial_next, wall.id);
        assert_eq!(wall.radial_next, cap.id);
        assert_eq!(cap.edge, wall.edge);
        assert_eq!(
            cap.sense,
            if index == 0 {
                Sense::Reversed
            } else {
                Sense::Forward
            }
        );
        assert_eq!(
            wall.sense,
            if index == 0 {
                Sense::Forward
            } else {
                Sense::Reversed
            }
        );
        assert_eq!(cap.owner_loop, ir.model.loops[index].id);
        assert_eq!(wall.owner_loop, ir.model.loops[index + 2].id);
        assert_eq!(cap.pcurves.len(), 1);
        assert_eq!(wall.pcurves.len(), 1);
    }
}

#[test]
fn circular_extrusion_transfer_preserves_prior_refusal() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let original = ctx
        .charge_work_limit(1, "prior circular refusal")
        .expect_err("refusal");
    assert!(
        matches!(transfer(&ctx), Err(CodecError::ResourceLimit(resource)) if resource == original)
    );
}

#[test]
fn duplicate_circular_body_identity_needs_no_retained_storage() {
    let scan = circular_scan();
    let mut ir = source_circle();
    ir.model.bodies.push(cadmpeg_ir::topology::Body {
        id: cadmpeg_ir::ids::BodyId::mint("creo:feature:extrusion#40:body").expect("body ID"),
        kind: cadmpeg_ir::topology::BodyKind::Solid,
        regions: Vec::new(),
        transform: None,
        name: None,
        color: None,
        visible: None,
    });
    let expected = ir.clone();
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    assert_eq!(
        super::super::transfer_resolved_circular_extrusion_breps(
            &ctx,
            &scan,
            &mut ir,
            &mut AnnotationBuilder::new(),
            &mut Vec::new(),
            &mut crate::decode::source_carriers::SourceUnitCarriers::default()
        )
        .expect("duplicate body identity remains scoped"),
        0
    );
    assert_eq!(ir, expected);
    assert_eq!(ctx.resource_refusal(), None);
}
