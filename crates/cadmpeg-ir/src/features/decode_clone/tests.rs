// SPDX-License-Identifier: Apache-2.0
use crate::features::{FeatureDefinition, FeatureOperation, NativeFeatureKind, NonEmptyMembers};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

const OPERATION: &str = "test feature definition copy";

#[test]
fn feature_definition_copy_refuses_retained_text_before_allocation() {
    let definition = FeatureDefinition::Operation(FeatureOperation::Native {
        kind: NativeFeatureKind::Other("source雪%".to_owned()),
        parameters: std::collections::BTreeMap::new(),
    });
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_retained_bytes = u64::try_from("source雪%".len() - 1).unwrap();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(matches!(definition.clone_for_decode(&ctx, OPERATION),
        Err(CodecError::ResourceLimit(failure)) if failure.dimension == ResourceDimension::RetainedBytes
            && failure.operation == OPERATION));
}

#[test]
fn feature_definition_copy_refuses_nested_collection_before_allocation() {
    let member = FeatureDefinition::Operation(FeatureOperation::BoundaryFill {
        tools: crate::features::BodySelection::Unresolved,
        cells: NonEmptyMembers::one(crate::features::BodySelection::NativeSet(
            vec!["one".to_owned(), "two".to_owned()].try_into().unwrap(),
        )),
    });
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    // One outer cell and two nested native selections need three items.
    policy.limits.max_collection_items = 2;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(matches!(member.clone_for_decode(&ctx, OPERATION),
        Err(CodecError::ResourceLimit(failure)) if failure.dimension == ResourceDimension::CollectionItems
            && failure.used == 1 && failure.additional == 2 && failure.operation == OPERATION));
}

#[test]
fn feature_definition_copy_refuses_collection_work() {
    let definition = FeatureDefinition::Operation(FeatureOperation::MeshImport {
        tessellations: vec!["one".to_owned()].try_into().unwrap(),
    });
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(matches!(definition.clone_for_decode(&ctx, OPERATION),
        Err(CodecError::ResourceLimit(failure)) if failure.dimension == ResourceDimension::WorkUnits
            && failure.operation == OPERATION));
}

#[test]
fn feature_definition_copy_preserves_nested_fields_and_wire_bytes() {
    let definitions = [
        FeatureDefinition::PostProcess {
            operation: FeatureOperation::Native {
                kind: NativeFeatureKind::Other("source雪%".to_owned()),
                parameters: std::collections::BTreeMap::from([(
                    cadmpeg_core::text::NonBlankString::new("distance".to_owned()).unwrap(),
                    "x + 雪".to_owned(),
                )]),
            },
            refine: true,
            fuzzy_tolerance: crate::features::FuzzyTolerance::Automatic,
        },
        FeatureDefinition::Operation(FeatureOperation::BoundaryFill {
            tools: crate::features::BodySelection::Native("tool".to_owned()),
            cells: NonEmptyMembers::one(crate::features::BodySelection::NativeSet(
                vec!["one".to_owned(), "two".to_owned()].try_into().unwrap(),
            )),
        }),
    ];
    for definition in definitions {
        let arena = DecodeArena::new();
        let policy = DecodePolicy::default();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let copied = definition.clone_for_decode(&ctx, OPERATION).unwrap();
        assert_eq!(copied, definition);
        assert_eq!(
            serde_json::to_vec(&copied).unwrap(),
            serde_json::to_vec(&definition).unwrap()
        );
    }
}

#[test]
fn feature_definition_copy_refuses_retained_identity_before_allocation() {
    let source = crate::features::FeatureId::mint("test:model:feature#source").unwrap();
    let length = source.as_str().len();
    let definition = FeatureDefinition::Operation(FeatureOperation::DerivedGeometry { source });
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_retained_bytes = u64::try_from(length - 1).unwrap();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(matches!(definition.clone_for_decode(&ctx, OPERATION),
        Err(CodecError::ResourceLimit(failure)) if failure.dimension == ResourceDimension::RetainedBytes
            && failure.operation == OPERATION));
}

#[test]
fn feature_definition_copy_refuses_retained_parameter_key_before_allocation() {
    let key = "distance";
    let definition = FeatureDefinition::Operation(FeatureOperation::Native {
        kind: NativeFeatureKind::Fillet,
        parameters: std::collections::BTreeMap::from([(
            cadmpeg_core::text::NonBlankString::new(key.to_owned()).unwrap(),
            "value".to_owned(),
        )]),
    });
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_retained_bytes = u64::try_from(key.len() - 1).unwrap();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(matches!(definition.clone_for_decode(&ctx, OPERATION),
        Err(CodecError::ResourceLimit(failure)) if failure.dimension == ResourceDimension::RetainedBytes
            && failure.operation == OPERATION));
}

#[test]
fn feature_definition_copy_refuses_parameter_map_work() {
    let definition = FeatureDefinition::Operation(FeatureOperation::Native {
        kind: NativeFeatureKind::Fillet,
        parameters: std::collections::BTreeMap::from([(
            cadmpeg_core::text::NonBlankString::new("distance".to_owned()).unwrap(),
            "value".to_owned(),
        )]),
    });
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(matches!(definition.clone_for_decode(&ctx, OPERATION),
        Err(CodecError::ResourceLimit(failure)) if failure.dimension == ResourceDimension::WorkUnits
            && failure.operation == OPERATION));
}
