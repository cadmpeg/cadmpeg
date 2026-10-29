// SPDX-License-Identifier: Apache-2.0

use cadmpeg_ir::features::{
    BodySelection, BooleanKind, FeatureDefinition, FeatureOperation, FeatureResultMembers,
    FeatureResultTopology,
};
use serde_json::{json, Value};

use super::*;
use crate::test_support::{combine::document, test_fixtures::parse};

#[test]
fn combine_decode_dispatches_slots_and_retains_the_target_result() {
    for (value, expected) in [
        (2, BooleanKind::Cut),
        (3, BooleanKind::Join),
        (4, BooleanKind::Intersect),
    ] {
        for keep in [false, true] {
            let bytes = document(value, keep);
            let (ir, _, _) = InventorCodec
                .decode(&mut std::io::Cursor::new(bytes), &DecodeOptions::default())
                .expect("synthetic Combine decode")
                .into_parts();
            assert_eq!(ir.model.features.len(), 1);
            assert_eq!(ir.model.feature_result_topologies.len(), 1);
            let properties = ir
                .native
                .namespace("inventor")
                .expect("Inventor native namespace")
                .arena_as::<Value>("pm_dc_feature_properties")
                .expect("synthetic Combine graph");
            let target = properties[4]["id"]
                .as_str()
                .expect("native property identity");
            let tool = properties[5]["id"]
                .as_str()
                .expect("native property identity");
            let feature = &ir.model.features[0];
            let FeatureDefinition::Operation(FeatureOperation::Combine {
                operands,
                op,
                keep_tools,
            }) = feature.evaluation.definition()
            else {
                panic!("neutral Combine");
            };
            assert_eq!(*op, expected);
            assert_eq!(*keep_tools, keep);
            assert_eq!(
                operands.clone().into_parts(),
                (
                    BodySelection::Native(target.to_owned()),
                    BodySelection::Native(tool.to_owned()),
                )
            );
            let result = &ir.model.feature_result_topologies[0];
            assert_eq!(result.output_of, feature.id);
            assert_eq!(result.native_ref.as_deref(), properties[0]["id"].as_str());
            assert_eq!(result.bodies().len(), 1);
            assert_eq!(result.bodies()[0].as_str(), target);
            parse(&[], |ctx, _| {
                let findings = InventorCodec
                    .validate_native(ctx, &ir, None)
                    .expect("native validation");
                assert!(findings.is_empty(), "{findings:?}");
            });
        }
    }
}

#[test]
fn combine_validation_checks_operands_state_and_target_result() {
    let (mut ir, _, _) = InventorCodec
        .decode(
            &mut std::io::Cursor::new(document(3, false)),
            &DecodeOptions::default(),
        )
        .expect("synthetic Combine graph")
        .into_parts();
    let properties = ir
        .native
        .namespace("inventor")
        .expect("Inventor native namespace")
        .arena_as::<Value>("pm_dc_feature_properties")
        .expect("synthetic Combine graph");
    parse(&[], |ctx, _| {
        for (index, pointer, value) in [
            (2, "/kind/value", json!(4)),
            (2, "/kind/type_value", json!(11)),
            (3, "/kind/value", json!(true)),
            (0, "/kind/items/references/0/index", json!(7)),
            (1, "/kind/items/references/0/index", json!(6)),
        ] {
            let mut changed = properties.clone();
            *changed[index]
                .pointer_mut(pointer)
                .expect("native property field") = value;
            ir.native
                .namespace_mut("inventor")
                .set_arena(ctx, "pm_dc_feature_properties", &changed)
                .expect("synthetic Combine graph");
            let findings = InventorCodec
                .validate_native(ctx, &ir, None)
                .expect("native validation");
            assert!(findings.iter().any(|finding| finding
                .message
                .contains("operands, operation, or keep-tools")));
        }
        ir.native
            .namespace_mut("inventor")
            .set_arena(ctx, "pm_dc_feature_properties", &properties)
            .expect("synthetic Combine graph");
        let original = &ir.model.feature_result_topologies[0];
        ir.model.feature_result_topologies[0] = FeatureResultTopology::new(
            original.id.clone(),
            original.output_of.clone(),
            FeatureResultMembers::new(
                vec![cadmpeg_core::text::NonBlankString::new(
                    properties[5]["id"]
                        .as_str()
                        .expect("native property identity")
                        .to_owned(),
                )
                .expect("synthetic Combine graph")],
                vec![],
                vec![],
                vec![],
            )
            .expect("synthetic Combine graph"),
            original.native_ref.clone(),
        );
        let findings = InventorCodec
            .validate_native(ctx, &ir, None)
            .expect("native validation");
        assert!(findings
            .iter()
            .any(|finding| finding.message.contains("result bodies")));
    });
}

#[test]
fn body_selector_validation_checks_decoded_member_links() {
    let (mut ir, _, _) = InventorCodec
        .decode(
            &mut std::io::Cursor::new(document(3, false)),
            &DecodeOptions::default(),
        )
        .expect("synthetic Combine graph")
        .into_parts();
    let properties = ir
        .native
        .namespace("inventor")
        .expect("Inventor native namespace")
        .arena_as::<Value>("pm_dc_feature_properties")
        .expect("synthetic Combine graph");
    parse(&[], |ctx, _| {
        for (index, member_finding) in [(0, false), (8, true), (99, true)] {
            for qualified in [false, true] {
                let mut changed = properties.clone();
                changed[0]["kind"]["items"]["references"][0] =
                    json!({"index": index, "qualified": qualified});
                ir.native
                    .namespace_mut("inventor")
                    .set_arena(ctx, "pm_dc_feature_properties", &changed)
                    .expect("synthetic Combine graph");
                let findings = InventorCodec
                    .validate_native(ctx, &ir, None)
                    .expect("native validation");
                assert_eq!(
                    findings.iter().any(|finding| finding
                        .message
                        .contains("feature-property record or reference does not resolve")),
                    member_finding
                );
            }
        }
    });
}
