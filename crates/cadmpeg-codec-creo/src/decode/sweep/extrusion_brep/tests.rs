// SPDX-License-Identifier: Apache-2.0

use super::{
    cap_coedge_ids_admitted, cap_record, copy_extrusion_identity, copy_ring_coedges,
    generated_extrusion_identity, missing_cap_message, push_rejected_extrusion,
    refused_lane_message, sketch_profiles_cover_generated_extrusion_sides, JoinedLaneRecords,
};
use crate::decode::tests::surface_row;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, Surface, SurfaceGeometry};
use cadmpeg_ir::ids::SurfaceId;
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::sketches::{Sketch, SketchEntityId, SketchEntityUse, SketchId, SketchPlacement};
use cadmpeg_ir::AnnotationBuilder;

#[test]
fn extrusion_cap_record_refuses_temporary_text_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = 0;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root admitted");
    let error =
        cap_record(&ctx, 7, 2, "bottom", 3).expect_err("cap record exceeds temporary limit");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::MaterializedBytes
            && resource.operation == "creo extrusion cap record text")
    );
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("service root admitted");
    assert_eq!(
        cap_record(&ctx, 7, 2, "bottom", 3)
            .expect("bottom record")
            .0,
        "extrusion feature 7 profile 2 bottom cap at entity 3"
    );
    assert_eq!(
        cap_record(&ctx, 7, 2, "top", 4).expect("top record").0,
        "extrusion feature 7 profile 2 top cap at entity 4"
    );
}

#[test]
fn extrusion_refused_lane_error_refuses_retained_text_limit() {
    let records = ["first".to_string(), "second".to_string()];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root admitted");
    let error = refused_lane_message(&ctx, "cap 7", &records)
        .expect_err("refused lane message exceeds retained limit");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::RetainedBytes
            && resource.operation == "creo extrusion refused lane error")
    );
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("service root admitted");
    assert_eq!(
        refused_lane_message(&ctx, "cap 7", &records).expect("service message"),
        "Refused lanes on cap 7: first; second"
    );
}

#[test]
fn extrusion_missing_cap_error_refuses_retained_text_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root admitted");
    let error =
        missing_cap_message(&ctx, "cap 7").expect_err("missing cap message exceeds retained limit");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::RetainedBytes
            && resource.operation == "creo extrusion missing cap error")
    );
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("service root admitted");
    assert_eq!(
        missing_cap_message(&ctx, "cap 7").expect("service message"),
        "cap 7 states no pcurve geometry"
    );
}

fn admitted_extrusion_fixture() -> (crate::container::ContainerScan<'static>, CadIr) {
    use cadmpeg_ir::sketches::{
        SketchEntity, SketchGeometry, SketchGeometryDefinition, SketchProfiles,
    };
    let mut scan = crate::container::scan_bytes_ok(Vec::new());
    scan.features.rows.push(crate::feature::rows::FeatureRow {
        feature_id: 7,
        root_schema_class: Some(crate::feature::schema::SchemaClass::Protrusion),
        stream_offset: 0,
        body: vec![0; 2].try_into().expect("feature row body"),
        body_offset: 1,
        offset: 0,
    });
    scan.features.definitions.push(definition());
    scan.features.section_transforms.push(
        crate::placement::FeatureSectionTransform::new(
            7,
            Some(7),
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
            feature_id: 7,
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
    scan.features.entity_tables.push(
        crate::feature::entity::FeatureEntityTable::new(
            7,
            29,
            vec![
                crate::feature::entity::FeatureEntityTableEntry {
                    entity_id: 31,
                    payload: crate::feature::entity::entry_payload(204, None, None, None),
                    prefixed: false,
                    offset: 0,
                    end_offset: 0,
                },
                crate::feature::entity::FeatureEntityTableEntry {
                    entity_id: 32,
                    payload: crate::feature::entity::entry_payload(203, None, None, None),
                    prefixed: false,
                    offset: 0,
                    end_offset: 0,
                },
                crate::feature::entity::FeatureEntityTableEntry {
                    entity_id: 33,
                    payload: crate::feature::entity::entry_payload(200, Some(11), None, None),
                    prefixed: false,
                    offset: 0,
                    end_offset: 0,
                },
                crate::feature::entity::FeatureEntityTableEntry {
                    entity_id: 34,
                    payload: crate::feature::entity::entry_payload(200, Some(12), None, None),
                    prefixed: false,
                    offset: 0,
                    end_offset: 0,
                },
                crate::feature::entity::FeatureEntityTableEntry {
                    entity_id: 35,
                    payload: crate::feature::entity::entry_payload(200, Some(13), None, None),
                    prefixed: false,
                    offset: 0,
                    end_offset: 0,
                },
                crate::feature::entity::FeatureEntityTableEntry {
                    entity_id: 36,
                    payload: crate::feature::entity::entry_payload(200, Some(14), None, None),
                    prefixed: false,
                    offset: 0,
                    end_offset: 0,
                },
            ],
            &std::collections::BTreeSet::new(),
            0,
        )
        .with_surface_ids([31, 32, 33, 34, 35, 36]),
    );
    let row = |id| crate::surface::SurfaceRow {
        id,
        kind: crate::surface::SurfaceKind::Plane,
        feature_id: 7,
        reversed: id == 31,
        boundary_type: crate::surface::BoundaryType::Code00,
        next_surface: 0,
        offset: usize::try_from(id).expect("fixture index fits usize"),
    };
    scan.surfaces.rows.extend((31..=36).map(row));
    let plane = |id, z| Surface {
        id: SurfaceId::mint(format!("creo:visibgeom:surface#{id}")).expect("surface identity"),
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
            cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                Point3::new(0.0, 0.0, z),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .expect("plane"),
        )),
        source_object: None,
    };
    let mut ir = CadIr::empty();
    ir.model.surfaces.extend([plane(31, 0.0), plane(32, 2.0)]);
    let sketch_id = SketchId::mint("creo:model:sketch#7").expect("sketch identity");
    let mut uses = Vec::new();
    for (index, (start, end)) in [
        ([0.0, 0.0], [1.0, 0.0]),
        ([1.0, 0.0], [1.0, 1.0]),
        ([1.0, 1.0], [0.0, 1.0]),
        ([0.0, 1.0], [0.0, 0.0]),
    ]
    .into_iter()
    .enumerate()
    {
        let id = SketchEntityId::mint(format!("creo:featdefs:sketch_entity#7:{}", index + 11))
            .expect("sketch entity identity");
        ir.model.sketch_entities.push(SketchEntity::new(
            id.clone(),
            sketch_id.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Line {
                start: Point2::new(start[0], start[1]),
                end: Point2::new(end[0], end[1]),
            })
            .expect("line"),
        ));
        uses.push(SketchEntityUse {
            entity: id,
            reversed: false,
        });
    }
    ir.model.sketches.push(Sketch {
        id: sketch_id,
        name: None,
        configuration: None,
        visible: None,
        placement: SketchPlacement::Unresolved {},
        profiles: SketchProfiles::try_from(vec![uses]).expect("profile"),
        native_ref: None,
    });
    (scan, ir)
}

#[test]
fn closed_extrusion_reaches_brep_admission() {
    let (scan, mut ir) = admitted_extrusion_fixture();
    assert!(super::feature_allows_additive_linear_extrusion(&scan, 7));
    assert!(crate::decode::with_test_decode_ctx(|ctx| super::feature_is_first_material_operation(ctx, &scan, 7)).expect("admitted material lookup"));
    let definition = &scan.features.definitions[0];
    let transform = &scan.features.section_transforms[0];
    let sketch_id =
        crate::decode::with_test_decode_ctx(|ctx| super::model_sketch_id(ctx, &scan, definition))
            .expect("sketch ID admitted")
            .expect("sketch ID");
    crate::decode::with_test_decode_ctx(|ctx| {
        assert!(super::resolved_feature_extrusion_span(
            ctx,
            &scan,
            &ir,
            &crate::decode::source_carriers::SourceUnitCarriers::default(),
            definition,
            transform,
        )
        .expect("span resources")
        .is_some());
        assert!(super::sketch_profiles_cover_generated_extrusion_sides(
            ctx,
            &scan,
            definition,
            7,
            &ir.model.sketches[0],
        )
        .expect("side resources"));
        let profiles = super::resolved_sketch_profiles(
            ctx,
            &ir,
            &crate::decode::source_carriers::SourceUnitCarriers::default(),
            &sketch_id,
            1,
        )
        .expect("profile resources")
        .expect("profile geometry");
        assert!(super::ordered_extrusion_profiles(ctx, profiles)
            .expect("service ordering resources")
            .is_some());
    });
    let mut diagnostics = crate::decode::surfaces::brep::BrepTransferDiagnostics::default();
    let count = crate::decode::with_test_decode_ctx(|ctx| {
        super::transfer_resolved_extrusion_breps(
            ctx,
            &scan,
            &mut ir,
            &mut AnnotationBuilder::new(),
            &mut diagnostics,
            &mut crate::decode::source_carriers::SourceUnitCarriers::default(),
        )
    })
    .expect("admitted transfer");
    assert_eq!(count, 1);
    assert_eq!(ir.model.bodies.len(), 1);
    assert!(diagnostics.rejected_extrusion_bodies.is_empty());
}

fn extrusion_refuses_at_collection_boundary(operation: &'static str) {
    let mut last_refusal = None;
    for limit in 0..4096 {
        let (scan, mut ir) = admitted_extrusion_fixture();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root admitted");
        let mut diagnostics = crate::decode::surfaces::brep::BrepTransferDiagnostics::default();
        let result = super::transfer_resolved_extrusion_breps(
            &ctx,
            &scan,
            &mut ir,
            &mut AnnotationBuilder::new(),
            &mut diagnostics,
            &mut crate::decode::source_carriers::SourceUnitCarriers::default(),
        );
        match result {
            Err(cadmpeg_core::CodecError::ResourceLimit(resource))
                if resource.dimension == ResourceDimension::CollectionItems
                    && resource.operation == operation =>
            {
                return
            }
            Err(cadmpeg_core::CodecError::ResourceLimit(resource)) => {
                last_refusal = Some((limit, resource.dimension, resource.operation));
            }
            Err(error) => panic!("unexpected extrusion error at limit {limit}: {error:?}"),
            Ok(_) => panic!("extrusion succeeded before the named {operation} refusal"),
        }
    }
    panic!("the named {operation} boundary was not reached; last refusal: {last_refusal:?}");
}

macro_rules! extrusion_collection_limit_test {
    ($name:ident, $operation:literal) => {
        #[test]
        fn $name() {
            extrusion_refuses_at_collection_boundary($operation);
        }
    };
}

extrusion_collection_limit_test!(
    extrusion_shell_face_ids_refuse_collection_limit,
    "creo extrusion shell face IDs"
);
extrusion_collection_limit_test!(
    extrusion_profile_vertex_ids_refuse_collection_limit,
    "creo extrusion profile vertex IDs"
);
extrusion_collection_limit_test!(
    extrusion_profile_edge_ids_refuse_collection_limit,
    "creo extrusion profile edge IDs"
);
extrusion_collection_limit_test!(
    extrusion_vertical_edge_ids_refuse_collection_limit,
    "creo extrusion vertical edge IDs"
);
extrusion_collection_limit_test!(
    extrusion_bottom_loop_ids_refuse_collection_limit,
    "creo extrusion bottom loop IDs"
);
extrusion_collection_limit_test!(
    extrusion_top_loop_ids_refuse_collection_limit,
    "creo extrusion top loop IDs"
);
extrusion_collection_limit_test!(
    extrusion_model_loops_refuse_collection_limit,
    "creo extrusion model loops"
);
extrusion_collection_limit_test!(
    extrusion_bottom_coedge_pcurve_uses_refuse_collection_limit,
    "creo extrusion bottom coedge pcurve uses"
);
extrusion_collection_limit_test!(
    extrusion_top_coedge_pcurve_uses_refuse_collection_limit,
    "creo extrusion top coedge pcurve uses"
);
extrusion_collection_limit_test!(
    extrusion_side_coedge_pcurve_uses_refuse_collection_limit,
    "creo extrusion side coedge pcurve uses"
);
extrusion_collection_limit_test!(
    extrusion_side_face_loop_ids_refuse_collection_limit,
    "creo extrusion side face loop IDs"
);
extrusion_collection_limit_test!(
    extrusion_model_shells_refuse_collection_limit,
    "creo extrusion model shells"
);
extrusion_collection_limit_test!(
    extrusion_model_regions_refuse_collection_limit,
    "creo extrusion model regions"
);
extrusion_collection_limit_test!(
    extrusion_region_shell_ids_refuse_collection_limit,
    "creo extrusion region shell IDs"
);
extrusion_collection_limit_test!(
    extrusion_body_region_ids_refuse_collection_limit,
    "creo extrusion body region IDs"
);

fn rejected_extrusion_at_limits(
    collection_limit: u64,
    retained_limit: u64,
) -> Result<crate::decode::surfaces::brep::BrepTransferDiagnostics, cadmpeg_core::CodecError> {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = collection_limit;
    policy.limits.max_retained_bytes = retained_limit;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    let mut diagnostics = crate::decode::surfaces::brep::BrepTransferDiagnostics::default();
    let records = [
        "first refused lane".to_string(),
        "second refused lane".to_string(),
    ];
    push_rejected_extrusion(
        &ctx,
        &mut diagnostics,
        cadmpeg_ir::ids::BodyId::mint("creo:feature:extrusion#7:body")
            .expect("valid body identity"),
        format_args!(
            "refused extrusion side lanes: {}",
            JoinedLaneRecords(&records)
        ),
    )?;
    Ok(diagnostics)
}

#[test]
fn extrusion_rejection_reason_refuses_retained_limit() {
    assert!(matches!(rejected_extrusion_at_limits(1, 0),
        Err(cadmpeg_core::CodecError::ResourceLimit(ref refusal))
        if refusal.operation == "creo extrusion rejection reason"));
}

#[test]
fn extrusion_rejection_diagnostics_refuse_collection_limit() {
    assert!(matches!(rejected_extrusion_at_limits(0, u64::MAX),
        Err(cadmpeg_core::CodecError::ResourceLimit(ref refusal))
        if refusal.operation == "creo extrusion rejection diagnostics"));
}

#[test]
fn extrusion_rejection_preserves_joined_reason_order() {
    let diagnostics = rejected_extrusion_at_limits(1, u64::MAX).expect("admitted rejection");
    assert_eq!(
        diagnostics.rejected_extrusion_bodies[0].1,
        "refused extrusion side lanes: first refused lane; second refused lane"
    );
}

fn cap_ids_at_limits(
    collection_limit: u64,
    retained_limit: u64,
    side: &'static str,
    reversed: bool,
    operation: &'static str,
) -> Result<Vec<cadmpeg_ir::ids::CoedgeId>, cadmpeg_core::CodecError> {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = collection_limit;
    policy.limits.max_retained_bytes = retained_limit;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    cap_coedge_ids_admitted(&ctx, 7, 0, 2, side, reversed, operation)
}

#[test]
fn generated_extrusion_identity_refuses_retained_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    let result =
        generated_extrusion_identity::<cadmpeg_ir::ids::BodyId>(&ctx, format_args!("7:body"));
    assert!(
        matches!(result, Err(cadmpeg_core::CodecError::ResourceLimit(ref refusal))
        if refusal.operation == "creo extrusion generated identities")
    );
    crate::decode::with_test_decode_ctx(|ctx| {
        let id =
            generated_extrusion_identity::<cadmpeg_ir::ids::BodyId>(ctx, format_args!("7:body"))
                .expect("admitted identity");
        assert_eq!(
            id.as_str(),
            cadmpeg_ir::ids::BodyId::compose(
                &crate::identity::FEATURE_EXTRUSION,
                cadmpeg_ir::ids::IdentityKey::from(7).colon(cadmpeg_ir::identity_key!("body")),
            )
            .as_str()
        );
    });
}

#[test]
fn extrusion_entity_id_copy_refuses_retained_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let source = cadmpeg_ir::ids::BodyId::compose(
        &crate::identity::FEATURE_EXTRUSION,
        cadmpeg_ir::ids::IdentityKey::from(7).colon(cadmpeg_ir::identity_key!("body")),
    );
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    let result = copy_extrusion_identity::<cadmpeg_ir::ids::BodyId>(&ctx, source.as_str());
    assert!(
        matches!(result, Err(cadmpeg_core::CodecError::ResourceLimit(ref refusal))
        if refusal.operation == "creo extrusion entity ID copies")
    );
    crate::decode::with_test_decode_ctx(|ctx| {
        let copied = copy_extrusion_identity::<cadmpeg_ir::ids::BodyId>(ctx, source.as_str())
            .expect("admitted identity copy");
        assert_eq!(copied, source);
    });
}

#[test]
fn bottom_cap_coedge_ids_refuse_collection_limit() {
    assert!(matches!(cap_ids_at_limits(0, u64::MAX, "bottom-cap", true,
        "creo extrusion bottom cap coedge IDs"),
        Err(cadmpeg_core::CodecError::ResourceLimit(ref refusal))
        if refusal.operation == "creo extrusion bottom cap coedge IDs"));
}

#[test]
fn top_cap_coedge_ids_refuse_collection_limit() {
    assert!(matches!(cap_ids_at_limits(0, u64::MAX, "top-cap", false,
        "creo extrusion top cap coedge IDs"),
        Err(cadmpeg_core::CodecError::ResourceLimit(ref refusal))
        if refusal.operation == "creo extrusion top cap coedge IDs"));
}

#[test]
fn cap_coedge_identity_refuses_retained_limit() {
    assert!(matches!(cap_ids_at_limits(2, 0, "bottom-cap", true,
        "creo extrusion bottom cap coedge IDs"),
        Err(cadmpeg_core::CodecError::ResourceLimit(ref refusal))
        if refusal.operation == "creo extrusion generated identities"));
}

#[test]
fn cap_coedge_ids_preserve_service_order() {
    let bottom = cap_ids_at_limits(
        2,
        u64::MAX,
        "bottom-cap",
        true,
        "creo extrusion bottom cap coedge IDs",
    )
    .expect("admitted bottom ring");
    let top = cap_ids_at_limits(
        2,
        u64::MAX,
        "top-cap",
        false,
        "creo extrusion top cap coedge IDs",
    )
    .expect("admitted top ring");
    assert!(bottom[0].as_str().ends_with("#7:coedge:0:1:bottom-cap"));
    assert!(bottom[1].as_str().ends_with("#7:coedge:0:0:bottom-cap"));
    assert!(top[0].as_str().ends_with("#7:coedge:0:0:top-cap"));
    assert!(top[1].as_str().ends_with("#7:coedge:0:1:top-cap"));
}

fn ring_copy_at_limits(
    collection_limit: u64,
    retained_limit: u64,
    collection_operation: &'static str,
    identity_operation: &'static str,
) -> Result<Vec<cadmpeg_ir::ids::CoedgeId>, cadmpeg_core::CodecError> {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let ids = [
        cadmpeg_ir::ids::CoedgeId::mint("creo:brep:coedge#10:0").expect("valid coedge identity")
    ];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = collection_limit;
    policy.limits.max_retained_bytes = retained_limit;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    copy_ring_coedges(&ctx, &ids, collection_operation, identity_operation)
}

#[test]
fn bottom_ring_copy_refuses_collection_limit() {
    assert!(matches!(ring_copy_at_limits(0, u64::MAX,
        "creo extrusion bottom ring coedge copies", "creo extrusion bottom ring coedge identities"),
        Err(cadmpeg_core::CodecError::ResourceLimit(ref refusal))
        if refusal.operation == "creo extrusion bottom ring coedge copies"));
}

#[test]
fn bottom_ring_copy_refuses_retained_limit() {
    assert!(matches!(ring_copy_at_limits(1, 0,
        "creo extrusion bottom ring coedge copies", "creo extrusion bottom ring coedge identities"),
        Err(cadmpeg_core::CodecError::ResourceLimit(ref refusal))
        if refusal.operation == "creo extrusion bottom ring coedge identities"));
}

#[test]
fn top_ring_copy_refuses_collection_limit() {
    assert!(matches!(ring_copy_at_limits(0, u64::MAX,
        "creo extrusion top ring coedge copies", "creo extrusion top ring coedge identities"),
        Err(cadmpeg_core::CodecError::ResourceLimit(ref refusal))
        if refusal.operation == "creo extrusion top ring coedge copies"));
}

#[test]
fn top_ring_copy_refuses_retained_limit() {
    assert!(matches!(ring_copy_at_limits(1, 0,
        "creo extrusion top ring coedge copies", "creo extrusion top ring coedge identities"),
        Err(cadmpeg_core::CodecError::ResourceLimit(ref refusal))
        if refusal.operation == "creo extrusion top ring coedge identities"));
}

#[test]
fn side_ring_copy_refuses_collection_limit() {
    assert!(matches!(ring_copy_at_limits(0, u64::MAX,
        "creo extrusion side ring coedge copies", "creo extrusion side ring coedge identities"),
        Err(cadmpeg_core::CodecError::ResourceLimit(ref refusal))
        if refusal.operation == "creo extrusion side ring coedge copies"));
}

#[test]
fn side_ring_copy_refuses_retained_limit() {
    assert!(matches!(ring_copy_at_limits(1, 0,
        "creo extrusion side ring coedge copies", "creo extrusion side ring coedge identities"),
        Err(cadmpeg_core::CodecError::ResourceLimit(ref refusal))
        if refusal.operation == "creo extrusion side ring coedge identities"));
}

#[test]
fn ring_copy_preserves_coedge_identity_order() {
    let ids = ring_copy_at_limits(
        1,
        u64::MAX,
        "creo extrusion side ring coedge copies",
        "creo extrusion side ring coedge identities",
    )
    .expect("admitted ring copy");
    assert_eq!(ids[0].as_str(), "creo:brep:coedge#10:0");
}

fn definition() -> crate::feature::definitions::FeatureDefinition {
    crate::feature::definitions::FeatureDefinition {
        identity: crate::feature::definitions::DefinitionIdentity::Parsed {
            schema_id: std::num::NonZeroU32::new(7),
            owner_feature_id: Some(7),
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
    }
}

fn generated_side_table() -> crate::feature::entity::FeatureEntityTable {
    crate::feature::entity::FeatureEntityTable::new(
        7,
        29,
        vec![crate::feature::entity::FeatureEntityTableEntry {
            entity_id: 31,
            payload: crate::feature::entity::entry_payload(200, Some(11), None, None),
            prefixed: false,
            offset: 0,
            end_offset: 0,
        }],
        &std::collections::BTreeSet::new(),
        0,
    )
    .with_surface_ids([31])
}

fn sketch() -> Sketch {
    let sketch_id = SketchId::mint("creo:model:sketch#7".to_string()).expect("valid test fixture");
    let entity = SketchEntityId::mint("creo:featdefs:sketch_entity#7:11".to_string())
        .expect("valid test fixture");
    Sketch {
        id: sketch_id,
        name: None,
        configuration: None,
        visible: None,
        placement: SketchPlacement::Unresolved {},
        profiles: cadmpeg_ir::sketches::SketchProfiles::try_from(vec![vec![SketchEntityUse {
            entity,
            reversed: false,
        }]])
        .expect("valid test fixture"),
        native_ref: None,
    }
}

fn generated_side_coverage_at_limits(
    collection_limit: u64,
    materialized_limit: u64,
) -> Result<bool, cadmpeg_core::CodecError> {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let mut scan = crate::container::scan_bytes_ok(Vec::new());
    scan.features.entity_tables.push(generated_side_table());
    scan.surfaces
        .rows
        .push(surface_row(31, 7, crate::surface::SurfaceKind::Plane));
    let definition = definition();
    let sketch = sketch();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = collection_limit;
    policy.limits.max_materialized_bytes = materialized_limit;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    sketch_profiles_cover_generated_extrusion_sides(&ctx, &scan, &definition, 7, &sketch)
}

#[test]
fn generated_side_profile_entity_nodes_refuse_limit() {
    assert!(matches!(generated_side_coverage_at_limits(0, u64::MAX),
        Err(cadmpeg_core::CodecError::ResourceLimit(ref refusal))
        if refusal.operation == "creo extrusion profile entity ID nodes"));
}

#[test]
fn generated_side_expected_entity_text_refuses_materialized_limit() {
    assert!(matches!(generated_side_coverage_at_limits(2, 0),
        Err(cadmpeg_core::CodecError::ResourceLimit(ref refusal))
        if refusal.operation == "creo extrusion expected sketch entity ID"));
}

#[test]
fn generated_side_expected_entity_nodes_refuse_limit() {
    assert!(matches!(generated_side_coverage_at_limits(1, u64::MAX),
        Err(cadmpeg_core::CodecError::ResourceLimit(ref refusal))
        if refusal.operation == "creo extrusion expected entity ID nodes"));
    assert!(generated_side_coverage_at_limits(2, u64::MAX).expect("admitted coverage"));
}

#[test]
fn generated_side_coverage_rejects_duplicate_surface_rows() {
    crate::decode::with_test_decode_ctx(|ctx| {
        let definition = definition();
        let mut scan = crate::container::scan_bytes_ok(Vec::new());
        scan.features.entity_tables.push(generated_side_table());
        scan.surfaces
            .rows
            .push(surface_row(31, 7, crate::surface::SurfaceKind::Plane));
        let sketch = sketch();

        assert!(sketch_profiles_cover_generated_extrusion_sides(
            ctx,
            &scan,
            &definition,
            7,
            &sketch,
        )
        .expect("admitted profile coverage"));

        let mut duplicate_profile = sketch.clone();
        let repeated_use = duplicate_profile.profiles[0][0].clone();
        duplicate_profile
            .profiles
            .edit(|profiles| profiles[0].push(repeated_use))
            .expect("valid test fixture");
        assert!(!sketch_profiles_cover_generated_extrusion_sides(
            ctx,
            &scan,
            &definition,
            7,
            &duplicate_profile,
        )
        .expect("admitted profile coverage"));

        let duplicate = scan.surfaces.rows[0].clone();
        scan.surfaces.rows.push(duplicate);
        assert!(!sketch_profiles_cover_generated_extrusion_sides(
            ctx,
            &scan,
            &definition,
            7,
            &sketch,
        )
        .expect("admitted profile coverage"));
    });
}

#[test]
fn generated_side_coverage_accepts_explicit_rowless_results() {
    crate::decode::with_test_decode_ctx(|ctx| {
        let definition = definition();
        let mut scan = crate::container::scan_bytes_ok(Vec::new());
        let mut table = generated_side_table();
        let cap = |entity_id, class_id| crate::feature::entity::FeatureEntityTableEntry {
            payload: crate::feature::entity::entry_payload(class_id, None, None, None),

            entity_id,
            prefixed: false,
            offset: 0,
            end_offset: 0,
        };
        let materialized = crate::feature::entity::FeatureEntityTableEntry {
            entity_id: 32,
            payload: crate::feature::entity::entry_payload(200, Some(13), None, None),
            prefixed: false,
            offset: 0,
            end_offset: 0,
        };
        table.entries = vec![
            cap(29, 204),
            cap(30, 203),
            table.entries[0].clone(),
            materialized,
        ];
        table.mark_surface_ids([29, 30, 32]);
        scan.features.entity_tables.push(table);
        scan.surfaces
            .rows
            .extend([surface_row(29, 7, crate::surface::SurfaceKind::Plane)]);
        scan.surfaces
            .rows
            .extend([surface_row(30, 7, crate::surface::SurfaceKind::Plane)]);
        scan.surfaces
            .rows
            .extend([surface_row(32, 7, crate::surface::SurfaceKind::Plane)]);
        let mut sketch = sketch();
        sketch
            .profiles
            .edit(|profiles| {
                profiles[0].push(SketchEntityUse {
                    entity: SketchEntityId::mint("creo:featdefs:sketch_entity#7:13".to_string())
                        .expect("valid test fixture"),
                    reversed: false,
                });
            })
            .expect("valid test fixture");

        assert!(sketch_profiles_cover_generated_extrusion_sides(
            ctx,
            &scan,
            &definition,
            7,
            &sketch,
        )
        .expect("admitted profile coverage"));
    });
}
