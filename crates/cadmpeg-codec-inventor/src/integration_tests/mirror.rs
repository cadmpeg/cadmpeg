// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};

use crate::loss::InventorLossCode;
use crate::test_support::mirror::{pattern, refusal_cases, Fixture};

#[test]
fn refused_mirror_operands_reach_the_feature_projection_loss_list() {
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).expect("context");
    for (name, fixture, source) in refusal_cases() {
        let native = source.id();
        let projected = fixture
            .project_history(&ctx, source)
            .expect("history projection");
        let feature = projected
            .features
            .iter()
            .find(|f| f.native_ref.as_deref() == Some(native.as_str()))
            .expect("retained native mirror");
        assert!(
            matches!(
                feature.evaluation.definition(),
                FeatureDefinition::Operation(FeatureOperation::Native { .. })
            ),
            "{name}"
        );
        assert_eq!(projected.mirror_losses.len(), 1, "{name}");
        assert_eq!(
            projected.mirror_losses[0].code,
            InventorLossCode::FeatureMirrorOperandsNative
                .note("test")
                .code,
            "{name}"
        );
        assert_eq!(
            projected.mirror_losses[0].code.local_code(),
            "feature.mirror-operands-native",
            "{name}"
        );
    }
}

#[test]
fn admitted_mirror_modes_reach_the_history_with_ordered_seeds_and_results() {
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).expect("context");
    for body in [false, true] {
        let fixture = Fixture::new(body);
        let source = pattern(19, body, if body { &[] } else { &[5, 3] }, &[]);
        let native = source.id();
        let projected = fixture
            .project_history(&ctx, source)
            .expect("history projection");
        let feature = projected
            .features
            .iter()
            .find(|f| f.native_ref.as_deref() == Some(native.as_str()))
            .expect("neutral mirror");
        assert!(matches!(
            feature.evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::Pattern { .. })
        ));
        assert_eq!(feature.dependencies.len(), if body { 0 } else { 2 });
        assert_eq!(projected.result_topologies.len(), 1);
        assert!(projected.mirror_losses.is_empty());
    }
}

#[test]
fn mirrors_without_a_history_or_with_multiple_segments_keep_the_pinned_loss() {
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).expect("context");
    let design = crate::design::DesignInventory {
        parameters: Vec::new(),
        expressions: Vec::new(),
        units: Vec::new(),
        parameter_tables: Vec::new(),
        model_roots: Vec::new(),
        brep_segments: Vec::new(),
        issues: Vec::new(),
    };
    let sketch = crate::sketch::SketchInventory {
        sketches: Vec::new(),
        entities: Vec::new(),
        transforms: Vec::new(),
        directions: Vec::new(),
        constraints: Vec::new(),
        issues: Vec::new(),
    };
    for count in [1, 2] {
        let mut inventory = crate::feature::FeatureInventory::default();
        for index in 0..count {
            let mut source = pattern(19, false, &[3], &[]);
            source.identity.segment_token =
                cadmpeg_ir::ids::IdentityKey::try_new(format!("segment-{index}"))
                    .expect("segment key");
            inventory.pattern_features.push(source);
        }
        let projected = crate::feature::project(
            &ctx,
            &inventory,
            &design,
            &sketch,
            &[],
            &crate::feature::ProjectedSketches {
                sketches: &[],
                entities: &[],
                profile_bodies: &[],
                result_model: None,
            },
        )
        .expect("native history");
        assert!(projected.features.is_empty());
        assert_eq!(projected.unresolved_features, count);
        assert_eq!(projected.mirror_losses.len(), count);
        assert!(projected
            .mirror_losses
            .iter()
            .all(|loss| { loss.code.local_code() == "feature.mirror-operands-native" }));
    }
}
