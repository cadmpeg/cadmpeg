use cadmpeg_test_support::edit;

use crate::features::FeatureOperation;
use crate::features::{
    BinderTarget, BodyMembers, BodySelection, CombineOperands, Feature, FeatureDefinition,
    FeatureId, FuzzyTolerance, GeometryImportPath, LoftPointSection, NonEmptyMembers, PathRef,
    PlanarProfileRef, ProfileRef, SectionOperands, SelectionMembers, SewBodySelection,
    SplitFacePlanes, ThreePointSelection, TreeChildren, TrimBodyOperands, VertexSelection,
};
use crate::ids::{BodyId, FeatureInputTopologyId, HistoricalVertexId};

#[test]
fn charged_native_selections_refuse_uniqueness_index_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = 1;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let result = crate::features::NativeSelections::try_from_for_decode(
        vec!["first".into(), "second".into()],
        &ctx,
        "test native selection uniqueness",
    );
    assert!(matches!(result, Err(failure)
        if failure.dimension == ResourceDimension::CollectionItems
            && failure.operation == "test native selection uniqueness"));
}

fn feature_id(suffix: &str) -> FeatureId {
    FeatureId::mint(format!("test:model:feature#{suffix}")).unwrap()
}

fn body_id(suffix: &str) -> BodyId {
    BodyId::mint(format!("test:model:body#{suffix}")).unwrap()
}

#[test]
fn local_collection_admission_preserves_order_and_rejects_invalid_membership() {
    let first = feature_id("first");
    let second = feature_id("second");
    assert!(NonEmptyMembers::<PathRef>::try_from(Vec::new()).is_err());
    assert!(SelectionMembers::<String>::try_from(vec!["mesh".into(), "mesh".into()]).is_err());
    assert!(SplitFacePlanes::try_from(vec![first.clone()]).is_err());
    assert!(SplitFacePlanes::try_from(vec![first.clone(), first.clone()]).is_err());
    let planes = SplitFacePlanes::try_from(vec![second.clone(), first.clone()]).unwrap();
    assert_eq!(
        serde_json::to_value(&planes).unwrap(),
        serde_json::json!([second, first])
    );
    assert!(TreeChildren::new(vec![first.clone(), first.clone()], None, &cadmpeg_test_support::service_decode_context()).is_err());
    assert!(TreeChildren::new(vec![first.clone()], Some(second.clone()), &cadmpeg_test_support::service_decode_context()).is_err());
    let mut children = TreeChildren::new(vec![first.clone()], Some(first), &cadmpeg_test_support::service_decode_context()).unwrap();
    let before = children.clone();
    assert!({
        let active = Some(second.clone());
        edit::replace(&mut children, |previous| {
            crate::features::TreeChildren::new(previous.to_vec(), active, &cadmpeg_test_support::service_decode_context())
        })
    }
    .is_err());
    assert_eq!(children, before);
    children.insert(second.clone());
    children.insert(second.clone());
    assert_eq!(children.len(), 2);
    {
        let active = Some(second);
        edit::replace(&mut children, |previous| {
            crate::features::TreeChildren::new(previous.to_vec(), active, &cadmpeg_test_support::service_decode_context())
        })
    }
    .unwrap();
}

#[test]
fn tree_children_charged_insert_refuses_collection_limit() {
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(b"children", &arena, &policy).unwrap();
    let mut children = TreeChildren::default();
    let error = children
        .insert_for_decode(&ctx, feature_id("child"), "collect tree children")
        .unwrap_err();
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(_)));
    assert!(children.is_empty());
}

#[test]
fn charged_selection_members_refuse_uniqueness_index_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(matches!(
        SelectionMembers::try_from_for_decode(
            vec!["first", "second"], &ctx, "selection uniqueness",
        ),
        Err(failure)
            if failure.operation == "selection uniqueness"
                && failure.dimension == ResourceDimension::CollectionItems
    ));
}

#[test]
fn selection_owners_enforce_local_arity_and_atomic_nonoverlap() {
    let first = BodySelection::Bodies(crate::features::DistinctMembers::try_from(vec![body_id("first")], &cadmpeg_test_support::service_decode_context()).expect("distinct bodies"));
    let second =
        BodySelection::Bodies(crate::features::DistinctMembers::try_from(vec![body_id("second")], &cadmpeg_test_support::service_decode_context()).expect("distinct bodies"));
    let pair = BodySelection::Bodies(
        crate::features::DistinctMembers::try_from(vec![body_id("first"), body_id("second")], &cadmpeg_test_support::service_decode_context())
            .expect("distinct bodies"),
    );
    assert!(SewBodySelection::try_from(first.clone()).is_err());
    assert!(SewBodySelection::try_from(pair.clone()).is_ok());
    assert!(SewBodySelection::try_from(BodySelection::Unresolved).is_ok());
    assert!(SewBodySelection::try_from(BodySelection::Native("native".into())).is_ok());
    assert!(CombineOperands::new(pair, BodySelection::Unresolved, &cadmpeg_test_support::service_decode_context(),).expect("operand admission").is_err());
    assert!(CombineOperands::new(first.clone(), first.clone(), &cadmpeg_test_support::service_decode_context(),).expect("operand admission").is_err());
    assert!(SectionOperands::new(first.clone(), first.clone(), &cadmpeg_test_support::service_decode_context(),).expect("operand admission").is_err());
    assert!(TrimBodyOperands::new(first.clone(), first.clone(), &cadmpeg_test_support::service_decode_context(),).expect("operand admission").is_err());
    let mut operands = CombineOperands::new(first, second, &cadmpeg_test_support::service_decode_context(),).expect("operand admission").unwrap();
    let before = operands.clone();
    assert!(operands
        .try_edit(|first, second| *second = first.clone())
        .is_err());
    assert_eq!(operands, before);
}

#[test]
fn resolved_body_selection_wire_rejects_repeated_bodies_before_sew_arity() {
    let body = body_id("repeated");
    for wire in [
        serde_json::json!({"kind": "bodies", "value": [body.as_str(), body.as_str()]}),
        serde_json::json!({"kind": "resolved", "value": {
            "bodies": [body.as_str(), body.as_str()], "native": "native-selection"
        }}),
    ] {
        let error = serde_json::from_value::<BodySelection>(wire.clone())
            .expect_err("repeated resolved bodies")
            .to_string();
        assert!(error.contains("members must be distinct"), "{error}");
        let error = serde_json::from_value::<SewBodySelection>(wire)
            .expect_err("repeated bodies do not meet sew arity")
            .to_string();
        assert!(error.contains("members must be distinct"), "{error}");
    }
}

#[test]
fn feature_evaluation_rejects_duplicate_outputs_at_admission() {
    let body = body_id("output");
    let duplicate = vec![body.clone(), body.clone()];
    assert!(crate::features::DistinctMembers::try_from(duplicate.clone(), &cadmpeg_test_support::service_decode_context()).is_err());

    let definition = FeatureDefinition::Operation(FeatureOperation::BaseFeature {
        bodies: BodySelection::Unresolved,
    });
    let mut evaluation = crate::features::FeatureEvaluation::new(
        definition.clone(),
        crate::features::DistinctMembers::try_from(vec![body.clone()], &cadmpeg_test_support::service_decode_context()).unwrap(),
    );
    evaluation.edit(|_, outputs| {
        assert!(!outputs.insert(body.clone()));
    });
    assert_eq!(evaluation.outputs(), &vec![body.clone()]);
    evaluation.set_outputs(crate::features::DistinctMembers::try_from(vec![body.clone()], &cadmpeg_test_support::service_decode_context()).unwrap());
    let prior = evaluation.clone();
    assert!(crate::features::DistinctMembers::try_from(duplicate, &cadmpeg_test_support::service_decode_context()).is_err());
    assert_eq!(evaluation, prior);
}

#[test]
fn feature_wire_rejects_duplicate_outputs_in_standalone_and_model_routes() {
    let body = body_id("output");
    let definition = FeatureDefinition::Operation(FeatureOperation::BaseFeature {
        bodies: BodySelection::Unresolved,
    });
    let wire = serde_json::json!({
        "id": feature_id("wire").as_str(),
        "ordinal": 0,
        "suppressed": null,
        "definition": serde_json::to_value(definition).unwrap(),
        "outputs": [body.as_str(), body.as_str()]
    });
    let error = serde_json::from_value::<Feature>(wire)
        .unwrap_err()
        .to_string();
    assert!(error.contains("distinct"), "{error}");

    let mut model = serde_json::to_value(crate::document::Model::default()).unwrap();
    model["features"] = serde_json::json!([{
        "id": feature_id("wire").as_str(),
        "ordinal": 0,
        "suppressed": null,
        "definition": serde_json::to_value(FeatureDefinition::Operation(
            FeatureOperation::BaseFeature { bodies: BodySelection::Unresolved }
        )).unwrap(),
        "outputs": [body.as_str(), body.as_str()]
    }]);
    let error = serde_json::from_value::<crate::document::Model>(model)
        .unwrap_err()
        .to_string();
    assert!(error.contains("distinct"), "{error}");
}

#[test]
fn historical_body_overlap_spans_direct_and_paired_member_selections() {
    use crate::ids::{FeatureInputTopologyId, HistoricalBodyId};

    let state =
        FeatureInputTopologyId::mint("test:model:entity#test:input").expect("valid identity");
    let target = BodySelection::historical(
        state.clone(),
        vec![HistoricalBodyId::mint("test:body:4").expect("valid identity")],
        "target".into(), &cadmpeg_test_support::service_decode_context()).expect("selection storage is admitted")
    .unwrap();
    let overlapping = BodySelection::HistoricalSet {
        state: state.clone(),
        members: BodyMembers::try_from_rows(vec![
            crate::features::BodyMember::new(
                HistoricalBodyId::mint("test:body:2").expect("valid identity"),
                cadmpeg_core::text::NonBlankString::new("tool-a")
                    .expect("valid historical body selection row"),
            ),
            crate::features::BodyMember::new(
                HistoricalBodyId::mint("test:body:4").expect("valid identity"),
                cadmpeg_core::text::NonBlankString::new("tool-b")
                    .expect("valid historical body selection row"),
            ),
        ], &cadmpeg_test_support::service_decode_context()).expect("selection storage is admitted")
        .expect("valid historical body selection rows"),
    };
    let disjoint = BodySelection::HistoricalSet {
        state,
        members: BodyMembers::try_from_rows(vec![crate::features::BodyMember::new(
            HistoricalBodyId::mint("test:body:5").expect("valid identity"),
            cadmpeg_core::text::NonBlankString::new("tool")
                .expect("valid historical body selection row"),
        )], &cadmpeg_test_support::service_decode_context()).expect("selection storage is admitted")
        .expect("valid historical body selection rows"),
    };

    assert!(SectionOperands::new(target.clone(), overlapping, &cadmpeg_test_support::service_decode_context(),).expect("operand admission").is_err());
    assert!(SectionOperands::new(target, disjoint, &cadmpeg_test_support::service_decode_context(),).expect("operand admission").is_ok());
}

#[test]
fn three_point_admission_compares_targets_and_historical_states() {
    let state = FeatureInputTopologyId::mint("test:model:feature-input#first").unwrap();
    let other = FeatureInputTopologyId::mint("test:model:feature-input#second").unwrap();
    let vertex = |state: &FeatureInputTopologyId, suffix: &str, native: &str| {
        VertexSelection::historical(
            state.clone(),
            HistoricalVertexId::mint(format!("test:model:historical-vertex#{suffix}")).unwrap(),
            native.into(),
        )
        .unwrap()
    };
    assert!(ThreePointSelection::try_from(Box::new([
        vertex(&state, "a", "one"),
        vertex(&state, "a", "two"),
        vertex(&state, "b", "three"),
    ]))
    .is_err());
    assert!(ThreePointSelection::try_from(Box::new([
        vertex(&state, "a", "one"),
        vertex(&state, "b", "two"),
        vertex(&other, "c", "three"),
    ]))
    .is_err());
    let mixed = ThreePointSelection::try_from(Box::new([
        vertex(&state, "a", "one"),
        VertexSelection::native("two".into()).unwrap(),
        VertexSelection::Unresolved,
    ]))
    .unwrap();
    assert_eq!(
        serde_json::from_value::<ThreePointSelection>(serde_json::to_value(&mixed).unwrap())
            .unwrap(),
        mixed
    );
}

#[test]
fn an_inserted_body_selection_does_not_restate_the_feature_outputs() {
    let body = body_id("inserted");
    let definition = FeatureDefinition::Operation(FeatureOperation::InsertBodies {
        bodies: crate::features::InsertedBodies::Resolved {
            native: "copied".into(),
        },
    });
    let mut feature = Feature {
        id: feature_id("insert"),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: crate::features::DistinctMembers::default(),
        source_properties: std::collections::BTreeMap::default(),
        source_tag: None,
        source_text: None,
        source_content: crate::features::FeatureContent::default(),
        evaluation: crate::features::FeatureEvaluation::from_definition(definition),
        native_ref: None,
    };
    assert!(feature.evaluation.outputs().is_empty());
    feature
        .evaluation
        .set_outputs(crate::features::DistinctMembers::try_from(vec![body.clone()], &cadmpeg_test_support::service_decode_context()).unwrap());
    let wire = serde_json::to_value(&feature).unwrap();
    assert!(wire.get("evaluation").is_none());
    assert_eq!(wire["outputs"], serde_json::json!([body.as_str()]));
    assert!(wire["definition"]["bodies"]["value"]
        .get("bodies")
        .is_none());
    assert_eq!(
        serde_json::from_value::<Feature>(wire.clone()).unwrap(),
        feature
    );

    let mut restated = wire;
    restated["definition"]["bodies"]["value"]["bodies"] = serde_json::json!([body.as_str()]);
    assert!(serde_json::from_value::<Feature>(restated.clone()).is_err());
    let error =
        serde_json::from_value::<crate::features::FeatureOperation>(restated["definition"].clone())
            .unwrap_err()
            .to_string();
    assert!(error.contains("bodies"), "{error}");
}

#[test]
fn local_feature_wire_rejects_empty_collections_and_invalid_strings() {
    for wire in [
        serde_json::json!({"definition":"mesh_import","tessellations":[]}),
        serde_json::json!({"definition":"mesh_import","tessellations":["same","same"]}),
        serde_json::json!({"definition":"fillet","groups":[]}),
        serde_json::json!({"definition":"chamfer","groups":[]}),
        serde_json::json!({"definition":"full_round_fillet","groups":[]}),
        serde_json::json!({"definition":"composite_curve","segments":[],"closed":false}),
        serde_json::json!({"definition":"boundary_fill","tools":{"kind":"unresolved"},"cells":[]}),
        serde_json::json!({"definition":"imported_geometry","path":"","format":"step"}),
        serde_json::json!({"definition":"imported_geometry","path":"a\u{0}b","format":"step"}),
    ] {
        assert!(
            serde_json::from_value::<FeatureDefinition>(wire.clone()).is_err(),
            "{wire}"
        );
    }
    assert!(serde_json::from_value::<LoftPointSection>(
        serde_json::json!({"kind":"native_point","value":""})
    )
    .is_err());
    assert!(serde_json::from_value::<LoftPointSection>(
        serde_json::json!({"kind":"native_point","value":" p "})
    )
    .is_ok());
    assert!(GeometryImportPath::try_from(" ".to_owned()).is_ok());
    for wire in [
        serde_json::json!({"kind":"external","document":"","object":"object"}),
        serde_json::json!({"kind":"external","document":"document","object":""}),
        serde_json::json!({"kind":"native","reference":""}),
    ] {
        assert!(serde_json::from_value::<BinderTarget>(wire).is_err());
    }
}

#[test]
fn a_post_process_inside_a_post_process_is_refused_as_an_unknown_variant() {
    let spatial = ProfileRef::SpatialSketchProfiles {
        sketch: crate::sketches::SpatialSketchId::mint("test:test:spatial-sketch#one").unwrap(),
        profiles: vec![0].try_into().unwrap(),
    };
    assert!(spatial.planar().is_none());
    assert!(
        serde_json::from_value::<PlanarProfileRef>(serde_json::to_value(&spatial).unwrap())
            .is_err()
    );

    let inner = FeatureDefinition::PostProcess {
        operation: FeatureOperation::BaseFeature {
            bodies: BodySelection::Unresolved,
        },
        refine: true,
        fuzzy_tolerance: FuzzyTolerance::KernelDefault,
    };
    let wire = serde_json::to_value(&inner).unwrap();
    assert_eq!(wire["definition"], "post_process");
    assert_eq!(
        serde_json::from_value::<FeatureDefinition>(wire.clone()).unwrap(),
        inner
    );

    let mut nested = wire.clone();
    nested["operation"] = wire.clone();
    assert!(serde_json::from_value::<FeatureDefinition>(nested).is_err());
    let error = serde_json::from_value::<FeatureOperation>(wire)
        .expect_err("a post-processed operation carries no post-processing layer of its own");
    assert!(error.to_string().contains("unknown variant"), "{error}");
}

#[test]
fn local_wire_errors_name_the_rejected_field() {
    for (field, wire) in [
        (
            "tessellations",
            serde_json::json!({"definition":"mesh_import","tessellations":[]}),
        ),
        (
            "groups",
            serde_json::json!({"definition":"fillet","groups":[]}),
        ),
        (
            "groups",
            serde_json::json!({"definition":"chamfer","groups":[]}),
        ),
        (
            "groups",
            serde_json::json!({"definition":"full_round_fillet","groups":[]}),
        ),
        (
            "segments",
            serde_json::json!({"definition":"composite_curve","segments":[],"closed":false}),
        ),
        (
            "cells",
            serde_json::json!({"definition":"boundary_fill","tools":{"kind":"unresolved"},"cells":[]}),
        ),
        (
            "path",
            serde_json::json!({"definition":"imported_geometry","path":"","format":"step"}),
        ),
    ] {
        let error = serde_json::from_value::<FeatureOperation>(wire).unwrap_err();
        assert!(error.to_string().contains(field), "{field}: {error}");
    }
    for (field, wire) in [
        (
            "document",
            serde_json::json!({"kind":"external","document":"","object":"object"}),
        ),
        (
            "object",
            serde_json::json!({"kind":"external","document":"document","object":""}),
        ),
        (
            "reference",
            serde_json::json!({"kind":"native","reference":""}),
        ),
    ] {
        let error = serde_json::from_value::<BinderTarget>(wire).unwrap_err();
        assert!(error.to_string().contains(field), "{field}: {error}");
    }
}

#[test]
fn selection_operand_parts_move_retained_storage() {
    let target_text = String::from("target-native");
    let tools_text = String::from("tools-native");
    let target_pointer = target_text.as_ptr();
    let tools_pointer = tools_text.as_ptr();
    let operands = CombineOperands::new(
        BodySelection::Native(target_text),
        BodySelection::Native(tools_text), &cadmpeg_test_support::service_decode_context(),
    ).expect("operand admission")
    .unwrap();
    let (target, tools) = operands.into_parts();
    let BodySelection::Native(target) = target else {
        panic!("native target");
    };
    let BodySelection::Native(tools) = tools else {
        panic!("native tools");
    };
    assert_eq!(target.as_ptr(), target_pointer);
    assert_eq!(tools.as_ptr(), tools_pointer);

    let targets_text = String::from("targets-native");
    let replacements_text = String::from("replacements-native");
    let targets_pointer = targets_text.as_ptr();
    let replacements_pointer = replacements_text.as_ptr();
    let operands = crate::features::ReplaceFaceOperands::new(
        crate::features::FaceSelection::Native(targets_text),
        crate::features::FaceSelection::Native(replacements_text), &cadmpeg_test_support::service_decode_context(),
    ).expect("operand admission")
    .unwrap();
    let (targets, replacements) = operands.into_parts();
    let crate::features::FaceSelection::Native(targets) = targets else {
        panic!("native targets");
    };
    let crate::features::FaceSelection::Native(replacements) = replacements else {
        panic!("native replacements");
    };
    assert_eq!(targets.as_ptr(), targets_pointer);
    assert_eq!(replacements.as_ptr(), replacements_pointer);
}

#[test]
fn tree_child_admission_preserves_first_and_later_active_comparison_refusals() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension, u64_from_index};
    use crate::features::FeatureCollectionError;
    let first = feature_id("left");
    let second = feature_id("next");
    for cap in [0, 1, u64_from_index(first.as_str().len()) + 1, u64_from_index(first.as_str().len()) + 2] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let Err(FeatureCollectionError::Resource(limit)) = TreeChildren::new(
            vec![first.clone(), second.clone()], Some(second.clone()), &ctx,
        ) else { panic!("active child lookup must refuse"); };
        assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
        assert_eq!(limit.operation, "validate active tree child");
        assert!(matches!(ctx.finish_session(), Err(cadmpeg_core::CodecError::ResourceLimit(original)) if original == limit));
    }
    let ctx = cadmpeg_test_support::service_decode_context();
    let children = TreeChildren::new(vec![second.clone(), first.clone()], Some(first.clone()), &ctx).unwrap();
    assert_eq!(&children[..], &[second.clone(), first.clone()]);
    assert_eq!(children.active_child(), &Some(first.clone()));
    assert_eq!(TreeChildren::new(vec![first], Some(second), &ctx).unwrap_err(), FeatureCollectionError::Invalid("active_child must belong to children"));
    ctx.finish_session().unwrap();
}

#[test]
fn operand_constructors_preserve_each_overlap_refusal_in_the_caller_session() {
    use cadmpeg_core::decode::{u64_from_index, DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use crate::features::{FaceBlendOperands, FaceSelection, ReplaceFaceOperands};
    use crate::features::edge_treatments::{FullRoundFilletGroup, FullRoundSideSelection};

    let face = |name: &str| FaceSelection::Faces(vec![crate::ids::FaceId::mint(format!("test:model:face#{name}")).unwrap()]);
    let body = |name: &str| BodySelection::Bodies(crate::features::DistinctMembers::try_from(
        vec![BodyId::mint(format!("test:model:body#{name}")).unwrap()],
        &cadmpeg_test_support::service_decode_context(),
    ).unwrap());
    let first_face = face("first");
    let other_face = face("other");
    let third_face = face("third");
    let first_body = body("first");
    let other_body = body("other");
    let comparison_work = 3 + u64_from_index("test:model:face#first".len());
    for kind in 0..6 {
        let required = if kind == 5 { 3 * comparison_work } else { comparison_work };
        for cap in 0..=required {
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            policy.limits.max_materialized_bytes = 0;
            policy.limits.max_retained_bytes = 0;
            policy.limits.max_collection_items = 0;
            policy.limits.max_recursion_depth = 0;
            let arena = DecodeArena::new();
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let result = match kind {
                0 => FaceBlendOperands::new(first_face.clone(), other_face.clone(), &ctx).map(|result| result.map(|_| ())),
                1 => ReplaceFaceOperands::new(first_face.clone(), other_face.clone(), &ctx).map(|result| result.map(|_| ())),
                2 => SectionOperands::new(first_body.clone(), other_body.clone(), &ctx).map(|result| result.map(|_| ())),
                3 => CombineOperands::new(first_body.clone(), other_body.clone(), &ctx).map(|result| result.map(|_| ())),
                4 => TrimBodyOperands::new(first_body.clone(), other_body.clone(), &ctx).map(|result| result.map(|_| ())),
                _ => FullRoundFilletGroup::new(first_face.clone(),
                    FullRoundSideSelection::Explicit(other_face.clone()),
                    FullRoundSideSelection::Explicit(third_face.clone()), &ctx,
                ).map(|result| result.map(|_| ())),
            };
            if cap == required {
                result.expect("exact comparison admission").expect("disjoint operands");
                ctx.finish_session().expect("no storage or depth needed");
            } else {
                let limit = result.expect_err("every visit and comparison must be admitted");
                assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
                assert_eq!(limit.operation, "IR selection membership overlap");
                assert!(matches!(ctx.finish_session(), Err(cadmpeg_core::CodecError::ResourceLimit(original)) if original == limit));
            }
        }
    }
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    FullRoundFilletGroup::new(first_face, FullRoundSideSelection::Automatic,
        FullRoundSideSelection::Automatic, &ctx).unwrap().unwrap();
    ctx.finish_session().expect("automatic sides have no membership comparisons");
}
