//! Compact selection projection and resource-limit tests.

use super::super::{
    compact_surface_selection_set_value, cut_with_surface_selection_pair, draft_face_selection,
    full_round_fillet_selection_triple,
    project_compact_body_selections, project_compact_edge_selections,
    project_compact_surface_selections,
    project_unbound_offset_plane_faces,
};
use crate::records::{FeatureInputBodySelection, FeatureInputComponentPathEntry, FeatureInputLane, FeatureInputSurfaceSelection, FeatureInputSurfaceSelectionKind};
use cadmpeg_ir::features::{
    BodyRetentionMode, BodySelection, FeatureDefinition, FeatureId, FeatureOperation,
    UnresolvedFamily,
};
use std::collections::BTreeMap;

fn offset_plane_fixture() -> (
    Vec<cadmpeg_ir::features::Feature>,
    Vec<cadmpeg_ir::topology::Face>,
    Vec<cadmpeg_ir::geometry::Surface>,
) {
    use cadmpeg_ir::features::{DatumPlaneReference, FeatureSupportPlaneFrame};
    use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
    use cadmpeg_ir::ids::{FaceId, ShellId, SurfaceId};
    use cadmpeg_ir::math::{Point3, Vector3};
    use cadmpeg_ir::scalar::Length;
    use cadmpeg_ir::topology::{Face, Sense};

    let surface = cadmpeg_ir::geometry::Surface {
        id: SurfaceId::mint("test:model:entity#plane").expect("identity grammar"),
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
            cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                Point3::new(0.0, 0.0, 5.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
            ).expect("planar test surface"),
        )),
        source_object: None,
    };
    let face = Face {
        id: FaceId::mint("test:model:entity#face").expect("identity grammar"),
        shell: ShellId::mint("test:model:entity#shell").expect("identity grammar"),
        surface: surface.id.clone(),
        sense: Sense::Forward,
        loops: cadmpeg_ir::topology::FaceLoops::unspecified(Vec::new()),
        name: None,
        color: None,
        tolerance: None,
    };
    let mut feature = compact_edge_projection_feature();
    feature.evaluation.set_definition(FeatureDefinition::Operation(
        FeatureOperation::DatumOffsetPlane {
            reference: Some(DatumPlaneReference::ResolvedPlane {
                frame: FeatureSupportPlaneFrame::new(
                    Point3::new(0.0, 0.0, 5.0),
                    Vector3::new(0.0, 0.0, 1.0),
                    Vector3::new(1.0, 0.0, 0.0),
                ).expect("support frame"),
            }),
            distance: Length::new(4.0).expect("offset distance"),
        },
    ));
    (vec![feature], vec![face], vec![surface])
}

#[test]
fn unbound_offset_plane_refuses_work_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let (mut features, faces, surfaces) = offset_plane_fixture();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("test context");
    let error = project_unbound_offset_plane_faces(&ctx, &mut features, &faces, &surfaces)
        .expect_err("planar scan exceeds work limit");
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::WorkUnits
            && limit.operation == "find unique SLDPRT planar face"));
}

#[test]
fn unbound_offset_plane_refuses_retained_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let (mut features, faces, surfaces) = offset_plane_fixture();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("test context");
    let error = project_unbound_offset_plane_faces(&ctx, &mut features, &faces, &surfaces)
        .expect_err("selected face ID exceeds retained limit");
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "find unique SLDPRT planar face"));
}

#[test]
fn unbound_offset_plane_refuses_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let (mut features, faces, surfaces) = offset_plane_fixture();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("test context");
    let error = project_unbound_offset_plane_faces(&ctx, &mut features, &faces, &surfaces)
        .expect_err("selected face vector exceeds collection limit");
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "project SLDPRT unbound offset plane face"));
}

fn compact_edge_projection_feature() -> cadmpeg_ir::features::Feature {
    cadmpeg_ir::features::Feature {
        id: FeatureId::mint("synthetic:test:id#fillet").expect("identity grammar"),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),
        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::Unresolved {
                family: UnresolvedFamily::Fillet,
            }),
        ),
        native_ref: Some("fillet".into()),
    }
}

fn compact_body_projection_fixture() -> (cadmpeg_ir::features::Feature, FeatureInputLane) {
    let mut feature = compact_edge_projection_feature();
    feature.evaluation.set_definition(FeatureDefinition::Operation(
        FeatureOperation::DeleteBody {
            bodies: BodySelection::Unresolved,
            mode: BodyRetentionMode::Unresolved,
        },
    ));
    let lane = FeatureInputLane {
        id: "lane".into(),
        configuration: None,
        native_payload: Vec::new(),
        classes: Vec::new(),
        names: Vec::new(),
        scalars: Vec::new(),
        relation_bindings: Vec::new(),
        relation_instances: Vec::new(),
        body_selections: vec![FeatureInputBodySelection {
            id: "body".into(),
            parent: "lane".into(),
            ordinal: 0,
            offset: 0,
            object_name_ref: "name".into(),
            feature_ref: "fillet".into(),
            local_body_ids: vec![3, 4],
            body_state_ids: Vec::new(),
            mode: Some(BodyRetentionMode::KeepSelected),
        }],
        edge_selections: Vec::new(),
        surface_selections: Vec::new(),
        generated_surface_identities: Vec::new(),
        references: Vec::new(),
        sketch_entities: Vec::new(),
    };
    (feature, lane)
}

#[test]
fn compact_body_projection_preserves_selection_and_retention_mode() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let (mut feature, lane) = compact_body_projection_fixture();
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("test context");
    project_compact_body_selections(&ctx, std::slice::from_mut(&mut feature), &[lane])
        .expect("body projection");
    assert!(matches!(feature.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::DeleteBody {
            bodies: BodySelection::Local { bodies, native },
            mode: BodyRetentionMode::KeepSelected,
        }) if bodies.as_slice() == ["3", "4"] && native.as_str() == "sldprt:feature-input:body-ids:3,4"));
}

#[test]
fn compact_body_projection_refuses_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let (mut feature, lane) = compact_body_projection_fixture();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("test context");
    let error = project_compact_body_selections(&ctx, std::slice::from_mut(&mut feature), &[lane])
        .expect_err("two bodies exceed one collection slot");
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "project SLDPRT compact body selections"));
}

#[test]
fn compact_body_projection_refuses_uniqueness_index_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let (mut feature, lane) = compact_body_projection_fixture();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 2;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("test context");
    let error = project_compact_body_selections(&ctx, std::slice::from_mut(&mut feature), &[lane])
        .expect_err("uniqueness index exceeds remaining collection slots");
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "validate distinct decoded native selections"));
}

#[test]
fn compact_body_projection_keeps_duplicate_selection_unresolved() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let (mut feature, mut lane) = compact_body_projection_fixture();
    lane.body_selections[0].local_body_ids = vec![3, 3];
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("test context");
    project_compact_body_selections(&ctx, std::slice::from_mut(&mut feature), &[lane])
        .expect("duplicate is a semantic no-match");
    assert!(matches!(feature.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::DeleteBody {
            bodies: BodySelection::Unresolved,
            mode: BodyRetentionMode::Unresolved,
        })));
}

#[test]
fn compact_body_projection_refuses_retained_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let (mut feature, lane) = compact_body_projection_fixture();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("test context");
    let error = project_compact_body_selections(&ctx, std::slice::from_mut(&mut feature), &[lane])
        .expect_err("body text exceeds retained limit");
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "project SLDPRT compact body selections"));
}

#[test]
fn compact_body_projection_refuses_work_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let (mut feature, lane) = compact_body_projection_fixture();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("test context");
    let error = project_compact_body_selections(&ctx, std::slice::from_mut(&mut feature), &[lane])
        .expect_err("selection scan exceeds work limit");
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::WorkUnits
            && limit.operation == "project SLDPRT compact body selections"));
}

#[test]
fn compact_edge_projection_refuses_index_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let mut feature = compact_edge_projection_feature();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("test context");
    let error = project_compact_edge_selections(
        &ctx,
        std::slice::from_mut(&mut feature),
        &[],
        &[],
    )
    .expect_err("feature index exceeds collection limit");
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "index SLDPRT compact edge selections"));
}

#[test]
fn compact_edge_projection_refuses_index_work_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let mut feature = compact_edge_projection_feature();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("test context");
    let error = project_compact_edge_selections(
        &ctx,
        std::slice::from_mut(&mut feature),
        &[],
        &[],
    )
    .expect_err("feature index exceeds work limit");
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::WorkUnits
            && limit.operation == "index SLDPRT compact edge selections"));
}

#[test]
fn compact_edge_projection_refuses_index_retained_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let mut feature = compact_edge_projection_feature();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("test context");
    let error = project_compact_edge_selections(
        &ctx,
        std::slice::from_mut(&mut feature),
        &[],
        &[],
    )
    .expect_err("feature identity exceeds retained limit");
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "index SLDPRT compact edge selections"));
}

#[test]
fn compact_surface_projection_refuses_index_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let mut feature = compact_edge_projection_feature();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("test context");
    let error = project_compact_surface_selections(&ctx, std::slice::from_mut(&mut feature), &[], &[])
        .expect_err("surface feature index exceeds collection limit");
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "index SLDPRT compact surface selections"));
}

#[test]
fn compact_surface_projection_refuses_index_retained_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let mut feature = compact_edge_projection_feature();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("test context");
    let error = project_compact_surface_selections(&ctx, std::slice::from_mut(&mut feature), &[], &[])
        .expect_err("surface feature ID exceeds retained limit");
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "index SLDPRT compact surface selections"));
}

#[test]
fn compact_surface_projection_refuses_index_work_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let mut feature = compact_edge_projection_feature();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("test context");
    let error = project_compact_surface_selections(&ctx, std::slice::from_mut(&mut feature), &[], &[])
        .expect_err("surface feature scan exceeds work limit");
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::WorkUnits
            && limit.operation == "index SLDPRT compact surface selections"));
}

fn full_round_selection() -> FeatureInputSurfaceSelection {
    FeatureInputSurfaceSelection {
        id: "surface".into(),
        parent: "lane".into(),
        ordinal: 0,
        offset: 0,
        selector: 0,
        kind: FeatureInputSurfaceSelectionKind::Component,
        object_name_ref: "name".into(),
        feature_ref: "fillet".into(),
        producer_feature_refs: Vec::new(),
        terminal_feature_ref: None,
        components: Vec::new(),
    }
}

#[test]
fn full_round_fillet_grouping_refuses_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let selection = full_round_selection();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("test context");
    let error = full_round_fillet_selection_triple(&ctx, &[&selection])
        .expect_err("lane grouping exceeds collection limit");
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "group SLDPRT full round fillet selections"));
}

#[test]
fn full_round_fillet_grouping_refuses_work_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let selection = full_round_selection();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("test context");
    let error = full_round_fillet_selection_triple(&ctx, &[&selection])
        .expect_err("lane scan exceeds work limit");
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::WorkUnits
            && limit.operation == "group SLDPRT full round fillet selections"));
}

#[test]
fn surface_cut_grouping_refuses_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let selection = full_round_selection();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("test context");
    let error = cut_with_surface_selection_pair(&ctx, &[&selection])
        .expect_err("surface cut lane grouping exceeds collection limit");
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "group SLDPRT surface cut selections"));
}

#[test]
fn surface_cut_grouping_refuses_work_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let selection = full_round_selection();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("test context");
    let error = cut_with_surface_selection_pair(&ctx, &[&selection])
        .expect_err("surface cut lane scan exceeds work limit");
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::WorkUnits
            && limit.operation == "group SLDPRT surface cut selections"));
}

#[test]
fn surface_selection_set_preserves_order_and_deduplicates_native_paths() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let mut first = full_round_selection();
    first.components = vec![FeatureInputComponentPathEntry {
        instance: None,
        type_signature: [0; 12],
        local_id: Some(7),
    }];
    let duplicate = first.clone();
    let mut second = first.clone();
    second.components[0].local_id = Some(9);
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("test context");
    let value = compact_surface_selection_set_value(&ctx, &[&first, &duplicate, &second])
        .expect("charged native selection set");
    assert_eq!(value, "sldprt:feature-input:surface-selection-vectors:sldprt:feature-input:surface-component-ids:7;sldprt:feature-input:surface-component-ids:9");
    assert_eq!(
        compact_surface_selection_set_value(&ctx, &[&first, &duplicate])
            .expect("one unique native path"),
        "sldprt:feature-input:surface-component-ids:7"
    );
    assert_eq!(
        compact_surface_selection_set_value(&ctx, &[]).expect("empty native path set"),
        "sldprt:feature-input:surface-selection-vectors:"
    );
}

#[test]
fn surface_selection_set_refuses_retained_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let selection = full_round_selection();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("test context");
    let error = compact_surface_selection_set_value(&ctx, &[&selection])
        .expect_err("native surface text exceeds retained limit");
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "format SLDPRT surface selection set"));
}

#[test]
fn draft_face_selection_preserves_native_path_order_and_deduplicates() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    use std::collections::HashMap;

    let path = |local_id| vec![FeatureInputComponentPathEntry {
        instance: None,
        type_signature: [0; 12],
        local_id: Some(local_id),
    }];
    let paths = [path(7), path(7), path(9)];
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("test context");
    let selection = draft_face_selection(
        &ctx,
        &paths,
        "consumer",
        &[],
        &HashMap::new(),
        &mut cadmpeg_ir::features::DistinctMembers::default(),
    )
    .expect("draft native path selection");
    assert_eq!(selection, cadmpeg_ir::features::FaceSelection::Native(
        "sldprt:feature-input:draft-surface-vectors:sldprt:feature-input:surface-component-ids:7;sldprt:feature-input:surface-component-ids:9".into()
    ));
}

#[test]
fn draft_face_selection_refuses_retained_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use std::collections::HashMap;

    let paths = [vec![FeatureInputComponentPathEntry {
        instance: None,
        type_signature: [0; 12],
        local_id: Some(7),
    }]];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("test context");
    let error = draft_face_selection(
        &ctx,
        &paths,
        "consumer",
        &[],
        &HashMap::new(),
        &mut cadmpeg_ir::features::DistinctMembers::default(),
    )
    .expect_err("draft native text exceeds retained limit");
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "format SLDPRT draft surface selection set"));
}
