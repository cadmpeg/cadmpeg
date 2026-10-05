//! Compact selection projection and resource-limit tests.

use super::super::{
    bind_parameter_scalars, compact_surface_selection_set_value, cut_with_surface_selection_pair,
    draft_face_selection, full_round_fillet_selection_triple, project_compact_body_selections,
    project_compact_edge_selections, project_compact_surface_selections,
    project_unbound_cosmetic_thread_faces, project_unbound_offset_plane_faces,
    synthesize_display_relation_parameters,
};
use crate::records::{
    FeatureInputBodySelection, FeatureInputComponentPathEntry, FeatureInputLane,
    FeatureInputSurfaceSelection, FeatureInputSurfaceSelectionKind,
};
use cadmpeg_ir::features::{
    BodyRetentionMode, BodySelection, FeatureDefinition, FeatureId, FeatureOperation,
    UnresolvedFamily,
};
use std::collections::BTreeMap;

fn parameter_scalar_binding_fixture() -> (
    Vec<cadmpeg_ir::features::DesignParameter>,
    Vec<cadmpeg_ir::features::Feature>,
    Vec<crate::records::FeatureHistory>,
    Vec<FeatureInputLane>,
) {
    use crate::records::{FeatureInputName, FeatureInputScalar, FeatureInputScalarRole};
    use cadmpeg_ir::features::{DesignParameter, ParameterId, ParameterValue};

    let mut feature = compact_edge_projection_feature();
    feature.native_ref = Some("native-feature".into());
    let parameter = DesignParameter {
        id: ParameterId::mint("synthetic:test:id#bound-parameter").expect("identity grammar"),
        owner: Some(feature.id.clone()),
        ordinal: 0,
        name: "D1".into(),
        expression: "1".into(),
        display: None,
        value: Some(ParameterValue::Real(
            cadmpeg_ir::scalar::FiniteReal::new(1.0).unwrap(),
        )),
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: None,
    };
    let history = crate::records::FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![crate::records::Feature {
            id: "native-feature".into(),
            parent: "history".into(),
            xml_tag: "Feature".into(),
            tree_parent: None,
            source_id: None,
            ordinal: 0,
            name: "NativeFeature".into(),
            kind: "Feature".into(),
            input_class: None,
            suppressed: false,
            parameters: BTreeMap::new(),
            dimension_properties: BTreeMap::new(),
            properties: BTreeMap::new(),
            text: None,
            content: Vec::new(),
        }],
    };
    let lane = FeatureInputLane {
        id: "lane".into(),
        configuration: None,
        native_payload: Vec::new(),
        classes: Vec::new(),
        names: vec![
            FeatureInputName {
                id: "feature-name".into(),
                parent: "lane".into(),
                ordinal: 0,
                offset: 0,
                object_id: None,
                value: "NativeFeature".into(),
            },
            FeatureInputName {
                id: "parameter-name".into(),
                parent: "lane".into(),
                ordinal: 1,
                offset: 10,
                object_id: None,
                value: "D1".into(),
            },
        ],
        scalars: vec![FeatureInputScalar {
            id: "native-scalar".into(),
            parent: "lane".into(),
            feature_ref: Some("native-feature".into()),
            ordinal: 0,
            offset: 20,
            object_id: 0,
            name: "parameter-name".into(),
            value: cadmpeg_ir::scalar::FiniteReal::new(2.0).unwrap(),
            role: FeatureInputScalarRole::Native,
            operands: Vec::new(),
        }],
        relation_bindings: Vec::new(),
        relation_instances: Vec::new(),
        body_selections: Vec::new(),
        edge_selections: Vec::new(),
        surface_selections: Vec::new(),
        generated_surface_identities: Vec::new(),
        references: Vec::new(),
        sketch_entities: Vec::new(),
    };
    (vec![parameter], vec![feature], vec![history], vec![lane])
}

#[test]
fn parameter_scalar_binding_preserves_native_reference_and_value() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let (mut parameters, features, histories, lanes) = parameter_scalar_binding_fixture();
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("test context");
    bind_parameter_scalars(&ctx, &mut parameters, &features, &histories, &lanes)
        .expect("scalar binding");
    assert_eq!(parameters[0].native_ref.as_deref(), Some("native-scalar"));
    assert!(
        matches!(parameters[0].value, Some(cadmpeg_ir::features::ParameterValue::Real(value))
        if value.get() == 2.0)
    );
}

fn parameter_scalar_limit_result(
    dimension: cadmpeg_core::decode::ResourceDimension,
) -> cadmpeg_core::CodecError {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    let (mut parameters, features, histories, lanes) = parameter_scalar_binding_fixture();
    let mut policy = DecodePolicy::service();
    match dimension {
        ResourceDimension::CollectionItems => policy.limits.max_collection_items = 0,
        ResourceDimension::RetainedBytes => policy.limits.max_retained_bytes = 0,
        ResourceDimension::WorkUnits => policy.limits.max_work_units = 0,
        other => panic!("unsupported parameter scalar limit: {other:?}"),
    }
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("test context");
    bind_parameter_scalars(&ctx, &mut parameters, &features, &histories, &lanes)
        .expect_err("scalar binding exceeds configured limit")
}

#[test]
fn parameter_scalar_binding_refuses_collection_limit() {
    use cadmpeg_core::decode::ResourceDimension;
    assert!(
        matches!(parameter_scalar_limit_result(ResourceDimension::CollectionItems),
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "bind SLDPRT parameter scalars")
    );
}

#[test]
fn parameter_scalar_binding_refuses_retained_limit() {
    use cadmpeg_core::decode::ResourceDimension;
    assert!(
        matches!(parameter_scalar_limit_result(ResourceDimension::RetainedBytes),
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "bind SLDPRT parameter scalars")
    );
}

#[test]
fn parameter_scalar_binding_refuses_work_limit() {
    use cadmpeg_core::decode::ResourceDimension;
    assert!(
        matches!(parameter_scalar_limit_result(ResourceDimension::WorkUnits),
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::WorkUnits
                && limit.operation == "bind SLDPRT parameter scalars")
    );
}

fn display_relation_synthesis_fixture() -> (
    Vec<cadmpeg_ir::features::DesignParameter>,
    Vec<cadmpeg_ir::features::Feature>,
    Vec<FeatureInputLane>,
) {
    use crate::records::{
        FeatureInputRelationFamily, FeatureInputRelationInstance, FeatureInputScalarRole,
    };
    let (_, features, _, mut lanes) = parameter_scalar_binding_fixture();
    lanes[0].scalars[0].role = FeatureInputScalarRole::Display;
    lanes[0].scalars[0].value = cadmpeg_ir::scalar::FiniteReal::new(0.012).unwrap();
    lanes[0]
        .relation_instances
        .push(FeatureInputRelationInstance {
            id: "sldprt:feature-input:relation-instance#lane:10".into(),
            parent: "lane".into(),
            ordinal: 0,
            offset: 10,
            family: FeatureInputRelationFamily::PointPointDistance,
            class_ref: "class".into(),
            feature_ref: "native-feature".into(),
            scalars: crate::records::relation_scalars::RelationScalars::from_refs(
                vec!["native-scalar".into()],
                None,
                Some("native-scalar".into()),
            )
            .expect("display scalar relation"),
            operands: Vec::new(),
        });
    (Vec::new(), features, lanes)
}

#[test]
fn display_relation_synthesis_preserves_reference_parameter() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let (mut parameters, features, lanes) = display_relation_synthesis_fixture();
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("test context");
    synthesize_display_relation_parameters(&ctx, &mut parameters, &features, &lanes)
        .expect("display relation synthesis");
    assert_eq!(parameters.len(), 1);
    assert_eq!(parameters[0].name, "D1@reference");
    assert_eq!(parameters[0].owner.as_ref(), Some(&features[0].id));
    assert_eq!(
        parameters[0]
            .properties
            .get("sldprt_relation_parameter_role"),
        Some(&"reference".into())
    );
}

fn display_relation_synthesis_limit_result(
    dimension: cadmpeg_core::decode::ResourceDimension,
) -> cadmpeg_core::CodecError {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    let (mut parameters, features, lanes) = display_relation_synthesis_fixture();
    let mut policy = DecodePolicy::service();
    match dimension {
        ResourceDimension::CollectionItems => policy.limits.max_collection_items = 0,
        ResourceDimension::RetainedBytes => policy.limits.max_retained_bytes = 0,
        ResourceDimension::MaterializedBytes => policy.limits.max_materialized_bytes = 0,
        ResourceDimension::WorkUnits => policy.limits.max_work_units = 0,
        other => panic!("unsupported display relation limit: {other:?}"),
    }
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("test context");
    synthesize_display_relation_parameters(&ctx, &mut parameters, &features, &lanes)
        .expect_err("display relation synthesis exceeds configured limit")
}

#[test]
fn display_relation_synthesis_refuses_collection_limit() {
    use cadmpeg_core::decode::ResourceDimension;
    assert!(
        matches!(display_relation_synthesis_limit_result(ResourceDimension::CollectionItems),
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems)
    );
}

#[test]
fn display_relation_synthesis_refuses_retained_limit() {
    use cadmpeg_core::decode::ResourceDimension;
    assert!(
        matches!(display_relation_synthesis_limit_result(ResourceDimension::RetainedBytes),
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::RetainedBytes)
    );
}

#[test]
fn display_relation_synthesis_refuses_scoped_limit() {
    use cadmpeg_core::decode::ResourceDimension;
    assert!(
        matches!(display_relation_synthesis_limit_result(ResourceDimension::MaterializedBytes),
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::MaterializedBytes)
    );
}

#[test]
fn display_relation_synthesis_refuses_work_limit() {
    use cadmpeg_core::decode::ResourceDimension;
    assert!(
        matches!(display_relation_synthesis_limit_result(ResourceDimension::WorkUnits),
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::WorkUnits)
    );
}

#[test]
fn compact_surface_projection_binds_generated_thread_face_alias() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    use cadmpeg_ir::features::{DatumPlaneReference, FaceSelection, GeneratedFaceRef};
    use cadmpeg_ir::scalar::Length;

    let mut thread = compact_edge_projection_feature();
    thread.native_ref = Some("native-thread".into());
    let producer = thread.id.clone();
    let generated = GeneratedFaceRef::new(
        producer.clone(),
        "face-1".into(),
        &cadmpeg_test_support::service_decode_context(),
    )
    .expect("selection reference admission")
    .expect("generated face identity");
    thread
        .evaluation
        .set_definition(FeatureDefinition::Operation(
            FeatureOperation::CosmeticThread {
                face: FaceSelection::generated(
                    vec![generated],
                    "native-face".into(),
                    &cadmpeg_test_support::service_decode_context(),
                )
                .expect("selection reference admission")
                .expect("generated face selection"),
                diameter: None,
                extent: None,
            },
        ));
    let mut plane = compact_edge_projection_feature();
    plane.id = FeatureId::mint("synthetic:test:id#face-alias-plane").expect("identity grammar");
    plane.native_ref = Some("native-plane".into());
    plane.source_properties.insert(
        cadmpeg_core::nonblank_literal!("ReferenceFaceFeature"),
        "native-thread".into(),
    );
    plane
        .evaluation
        .set_definition(FeatureDefinition::Operation(
            FeatureOperation::DatumOffsetPlane {
                reference: None,
                distance: Length::new(2.0).expect("offset distance"),
            },
        ));
    let mut features = [thread, plane];
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("test context");
    project_compact_surface_selections(&ctx, &mut features, &[], &[])
        .expect("charged face alias projection");
    assert!(matches!(features[1].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::DatumOffsetPlane {
            reference: Some(DatumPlaneReference::Face {
                face: FaceSelection::Generated { faces, native }
            }), ..
        }) if faces.len() == 1
            && faces[0].feature == producer
            && faces[0].local_id.as_str() == "face-1"
            && native.as_str() == "native-face"));
    assert!(features[1].dependencies.contains(&producer));
}

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
            )
            .expect("planar test surface"),
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
    feature
        .evaluation
        .set_definition(FeatureDefinition::Operation(
            FeatureOperation::DatumOffsetPlane {
                reference: Some(DatumPlaneReference::ResolvedPlane {
                    frame: FeatureSupportPlaneFrame::new(
                        Point3::new(0.0, 0.0, 5.0),
                        Vector3::new(0.0, 0.0, 1.0),
                        Vector3::new(1.0, 0.0, 0.0),
                    )
                    .expect("support frame"),
                }),
                distance: Length::new(4.0).expect("offset distance"),
            },
        ));
    (vec![feature], vec![face], vec![surface])
}

fn cosmetic_fallback_fixture() -> (
    Vec<cadmpeg_ir::features::Feature>,
    Vec<crate::records::FeatureHistory>,
    Vec<cadmpeg_ir::topology::Face>,
    Vec<cadmpeg_ir::geometry::Surface>,
) {
    use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
    use cadmpeg_ir::ids::{FaceId, ShellId, SurfaceId};
    use cadmpeg_ir::math::{Point3, Vector3};
    use cadmpeg_ir::topology::{Face, Sense};

    let surface = cadmpeg_ir::geometry::Surface {
        id: SurfaceId::mint("test:model:entity#cylinder").expect("identity grammar"),
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
            cadmpeg_ir::geometry::analytic::CylinderSurface::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
                4.0,
            )
            .expect("cylindrical test surface"),
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
    let history = crate::records::FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![crate::records::Feature {
            id: "thread-native".into(),
            parent: "history".into(),
            xml_tag: "Feature".into(),
            tree_parent: None,
            source_id: None,
            ordinal: 0,
            name: "thread-native".into(),
            kind: "Feature".into(),
            input_class: None,
            suppressed: false,
            parameters: BTreeMap::new(),
            dimension_properties: BTreeMap::new(),
            properties: BTreeMap::new(),
            text: None,
            content: Vec::new(),
        }],
    };
    let mut feature = compact_edge_projection_feature();
    feature.native_ref = Some("thread-native".into());
    feature
        .evaluation
        .set_definition(FeatureDefinition::Operation(
            FeatureOperation::CosmeticThread {
                face: cadmpeg_ir::features::FaceSelection::Unresolved,
                diameter: Some(cadmpeg_ir::scalar::PositiveLength::new(8.0).expect("diameter")),
                extent: None,
            },
        ));
    (vec![feature], vec![history], vec![face], vec![surface])
}

#[test]
fn unbound_cosmetic_thread_refuses_work_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let (mut features, histories, faces, surfaces) = cosmetic_fallback_fixture();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 2;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("test context");
    let error = project_unbound_cosmetic_thread_faces(
        &ctx,
        &mut features,
        &histories,
        &[],
        &faces,
        &surfaces,
    )
    .expect_err("cylindrical scan exceeds work limit");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::WorkUnits
            && limit.operation == "find unique SLDPRT cylindrical face")
    );
}

#[test]
fn unbound_cosmetic_thread_refuses_retained_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let (features, histories, faces, surfaces) = cosmetic_fallback_fixture();
    let arena = DecodeArena::new();
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        "find unique SLDPRT cylindrical face",
        |cap| {
            let mut policy = DecodePolicy::service();
            policy.limits.max_retained_bytes = cap;
            let (ctx, _) =
                DecodeContext::from_root_bytes(&[], &arena, &policy).expect("test context");
            let mut features = features.clone();
            project_unbound_cosmetic_thread_faces(
                &ctx,
                &mut features,
                &histories,
                &[],
                &faces,
                &surfaces,
            )
        },
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "find unique SLDPRT cylindrical face")
    );
}

#[test]
fn unbound_cosmetic_thread_refuses_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let (mut features, histories, faces, surfaces) = cosmetic_fallback_fixture();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 5;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("test context");
    let error = project_unbound_cosmetic_thread_faces(
        &ctx,
        &mut features,
        &histories,
        &[],
        &faces,
        &surfaces,
    )
    .expect_err("cylindrical face slot exceeds collection limit");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "project SLDPRT unbound cosmetic thread face")
    );
}

#[test]
fn unbound_cosmetic_thread_refuses_scoped_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let (features, histories, faces, surfaces) = cosmetic_fallback_fixture();
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::MaterializedBytes,
        "index SLDPRT cosmetic thread feature IDs",
        |cap| {
            let mut features = features.clone();
            let mut policy = DecodePolicy::service();
            policy.limits.max_materialized_bytes = cap;
            let arena = DecodeArena::new();
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            project_unbound_cosmetic_thread_faces(
                &ctx,
                &mut features,
                &histories,
                &[],
                &faces,
                &surfaces,
            )
        },
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::MaterializedBytes
            && limit.operation == "index SLDPRT cosmetic thread feature IDs")
    );
}

#[test]
fn unbound_cosmetic_thread_generated_face_refuses_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let (mut features, mut histories, faces, surfaces) = cosmetic_fallback_fixture();
    let mut producer = compact_edge_projection_feature();
    producer.id = FeatureId::mint("synthetic:test:id#producer").expect("identity grammar");
    producer.native_ref = Some("producer-native".into());
    features.push(producer);
    let mut producer_record = histories[0].features[0].clone();
    producer_record.id = "producer-native".into();
    producer_record.name = "producer-native".into();
    histories[0].features.push(producer_record);
    let mut selection = full_round_selection();
    selection.feature_ref = "thread-native".into();
    selection.producer_feature_refs = vec!["producer-native".into()];
    selection.components = vec![FeatureInputComponentPathEntry {
        instance: None,
        type_signature: [0; 12],
        local_id: Some(7),
    }];
    let lane = FeatureInputLane {
        id: "lane".into(),
        configuration: None,
        native_payload: Vec::new(),
        classes: Vec::new(),
        names: Vec::new(),
        scalars: Vec::new(),
        relation_bindings: Vec::new(),
        relation_instances: Vec::new(),
        body_selections: Vec::new(),
        edge_selections: Vec::new(),
        surface_selections: vec![selection],
        generated_surface_identities: Vec::new(),
        references: Vec::new(),
        sketch_entities: Vec::new(),
    };
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 12;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("test context");
    let error = project_unbound_cosmetic_thread_faces(
        &ctx,
        &mut features,
        &histories,
        &[lane],
        &faces,
        &surfaces,
    )
    .expect_err("generated face vector exceeds collection limit");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "resolve SLDPRT cosmetic thread generated face")
    );
}

#[test]
fn unbound_cosmetic_thread_token_index_refuses_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let (mut features, histories, faces, surfaces) = cosmetic_fallback_fixture();
    let class_name = "moCylinderRef_w";
    let mut payload = vec![0; 64];
    let token_offset = 6 + class_name.len();
    payload[token_offset..token_offset + 2].copy_from_slice(&0x802f_u16.to_le_bytes());
    let lane = FeatureInputLane {
        id: "lane".into(),
        configuration: None,
        native_payload: payload,
        classes: vec![crate::records::FeatureInputClass {
            id: "class".into(),
            parent: "lane".into(),
            ordinal: 0,
            offset: 0,
            name: class_name.into(),
        }],
        names: vec![crate::records::FeatureInputName {
            id: "name".into(),
            parent: "lane".into(),
            ordinal: 0,
            offset: 0,
            value: "thread-native".into(),
            object_id: crate::records::ObjectId::from_value(7),
        }],
        scalars: Vec::new(),
        relation_bindings: Vec::new(),
        relation_instances: Vec::new(),
        body_selections: Vec::new(),
        edge_selections: Vec::new(),
        surface_selections: Vec::new(),
        generated_surface_identities: Vec::new(),
        references: Vec::new(),
        sketch_entities: Vec::new(),
    };
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 7;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("test context");
    let error = project_unbound_cosmetic_thread_faces(
        &ctx,
        &mut features,
        &histories,
        &[lane],
        &faces,
        &surfaces,
    )
    .expect_err("cylinder token index exceeds collection limit");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "collect SLDPRT cosmetic thread cylinder tokens")
    );
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
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::WorkUnits
            && limit.operation == "find unique SLDPRT planar face")
    );
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
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "find unique SLDPRT planar face")
    );
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
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "project SLDPRT unbound offset plane face")
    );
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
    feature
        .evaluation
        .set_definition(FeatureDefinition::Operation(FeatureOperation::DeleteBody {
            bodies: BodySelection::Unresolved,
            mode: BodyRetentionMode::Unresolved,
        }));
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
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "project SLDPRT compact body selections")
    );
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
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "validate distinct decoded native selections")
    );
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
    assert!(matches!(
        feature.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::DeleteBody {
            bodies: BodySelection::Unresolved,
            mode: BodyRetentionMode::Unresolved,
        })
    ));
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
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "project SLDPRT compact body selections")
    );
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
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::WorkUnits
            && limit.operation == "project SLDPRT compact body selections")
    );
}

#[test]
fn compact_edge_projection_refuses_index_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let mut feature = compact_edge_projection_feature();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("test context");
    let error = project_compact_edge_selections(&ctx, std::slice::from_mut(&mut feature), &[], &[])
        .expect_err("feature index exceeds collection limit");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "index SLDPRT compact edge selections")
    );
}

#[test]
fn compact_edge_projection_refuses_index_work_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let mut feature = compact_edge_projection_feature();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("test context");
    let error = project_compact_edge_selections(&ctx, std::slice::from_mut(&mut feature), &[], &[])
        .expect_err("feature index exceeds work limit");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::WorkUnits
            && limit.operation == "index SLDPRT compact edge selections")
    );
}

#[test]
fn compact_edge_projection_refuses_index_retained_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let mut feature = compact_edge_projection_feature();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("test context");
    let error = project_compact_edge_selections(&ctx, std::slice::from_mut(&mut feature), &[], &[])
        .expect_err("feature identity exceeds retained limit");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "index SLDPRT compact edge selections")
    );
}

#[test]
fn compact_surface_projection_refuses_index_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let mut feature = compact_edge_projection_feature();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("test context");
    let error =
        project_compact_surface_selections(&ctx, std::slice::from_mut(&mut feature), &[], &[])
            .expect_err("surface feature index exceeds collection limit");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "index SLDPRT compact surface selections")
    );
}

#[test]
fn compact_surface_projection_refuses_index_retained_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let mut feature = compact_edge_projection_feature();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("test context");
    let error =
        project_compact_surface_selections(&ctx, std::slice::from_mut(&mut feature), &[], &[])
            .expect_err("surface feature ID exceeds retained limit");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "index SLDPRT compact surface selections")
    );
}

#[test]
fn compact_surface_projection_refuses_index_work_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let mut feature = compact_edge_projection_feature();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("test context");
    let error =
        project_compact_surface_selections(&ctx, std::slice::from_mut(&mut feature), &[], &[])
            .expect_err("surface feature scan exceeds work limit");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::WorkUnits
            && limit.operation == "index SLDPRT compact surface selections")
    );
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
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "group SLDPRT full round fillet selections")
    );
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
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::WorkUnits
            && limit.operation == "group SLDPRT full round fillet selections")
    );
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
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "group SLDPRT surface cut selections")
    );
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
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::WorkUnits
            && limit.operation == "group SLDPRT surface cut selections")
    );
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
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "format SLDPRT surface selection set")
    );
}

#[test]
fn draft_face_selection_preserves_native_path_order_and_deduplicates() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    use std::collections::HashMap;

    let path = |local_id| {
        vec![FeatureInputComponentPathEntry {
            instance: None,
            type_signature: [0; 12],
            local_id: Some(local_id),
        }]
    };
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
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "format SLDPRT draft surface selection set")
    );
}

#[test]
fn relation_diameter_expression_propagates_format_work_refusal() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    assert!(matches!(
        super::super::relation_display_parameter_value(
            &ctx, crate::records::FeatureInputRelationFamily::CircleDiameter, 2.0,
        ),
        Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::WorkUnits
                && limit.operation == "format SLDPRT relation display parameter"
    ));
    let (value, display, expression) = super::super::relation_display_parameter_value(
        &cadmpeg_test_support::service_decode_context(),
        crate::records::FeatureInputRelationFamily::CircleDiameter,
        2.0,
    ).unwrap().expect("finite diameter");
    assert_eq!(expression, "<MOD-DIAM>2000mm");
    assert_eq!(display, Some(cadmpeg_ir::features::DimensionDisplay::Diameter));
    assert!(matches!(value, cadmpeg_ir::features::ParameterValue::Length(length) if length.get() == 2000.0));
}

#[test]
fn surface_lane_group_entry_propagates_slot_refusal() {
    let selection = full_round_selection();
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(matches!(
        super::super::surface_selections_by_lane(&ctx, &[&selection], "group SLDPRT test surface lanes"),
        Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems
                && limit.operation == "group SLDPRT test surface lanes"
    ));
    let grouped = super::super::surface_selections_by_lane(
        &cadmpeg_test_support::service_decode_context(), &[&selection, &selection],
        "group SLDPRT test surface lanes",
    ).unwrap();
    assert_eq!(grouped.len(), 1);
    let entries = grouped.get(selection.parent.as_str()).unwrap();
    assert_eq!(entries.len(), 2);
    assert!(std::ptr::eq(entries[0], &selection));
    assert!(std::ptr::eq(entries[1], &selection));
}

#[test]
fn existing_parameter_ordinal_lookup_propagates_work_refusal() {
    let (parameters, features, _, lanes) = parameter_scalar_binding_fixture();
    crate::test_support::work_refusal_at("lookup SLDPRT existing parameter ordinal", |ctx| {
        synthesize_display_relation_parameters(ctx, &mut parameters.clone(), &features, &lanes)
    });
    let mut actual = parameters.clone();
    synthesize_display_relation_parameters(
        &cadmpeg_test_support::service_decode_context(), &mut actual, &features, &lanes,
    ).unwrap();
    assert_eq!(actual.len(), 1);
    assert_eq!(actual[0].id, parameters[0].id);
    assert_eq!(actual[0].ordinal, 0);
}

#[test]
fn display_parameter_ordinal_lookup_propagates_work_refusal() {
    let (parameters, features, lanes) = display_relation_synthesis_fixture();
    crate::test_support::work_refusal_at("lookup SLDPRT display parameter ordinal", |ctx| {
        synthesize_display_relation_parameters(ctx, &mut parameters.clone(), &features, &lanes)
    });
    let mut actual = parameters;
    synthesize_display_relation_parameters(
        &cadmpeg_test_support::service_decode_context(), &mut actual, &features, &lanes,
    ).unwrap();
    assert_eq!(actual.len(), 1);
    assert_eq!(actual[0].ordinal, 0);
    assert_eq!(actual[0].owner.as_ref(), Some(&features[0].id));
}
