// SPDX-License-Identifier: Apache-2.0
use crate::features::{FeatureDefinition, FeatureOperation, NativeFeatureKind, NonEmptyMembers};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

const OPERATION: &str = "test feature definition copy";

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
        let copied = definition.try_clone_for_decode(&ctx, OPERATION).unwrap();
        assert_eq!(copied, definition);
        assert_eq!(
            serde_json::to_vec(&copied).unwrap(),
            serde_json::to_vec(&definition).unwrap()
        );
    }
}
