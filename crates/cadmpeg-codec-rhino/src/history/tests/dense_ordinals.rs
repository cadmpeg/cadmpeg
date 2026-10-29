// SPDX-License-Identifier: Apache-2.0
use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureEvaluation, FeatureOperation};
use cadmpeg_ir::CadIr;

use super::{project, record, Diagnostics};

fn with_existing_feature() -> CadIr {
    let mut ir = CadIr::empty();
    ir.model.features.push(Feature {
        id: "test:feature:prefix#0"
            .try_into()
            .expect("authored prefix ID"),
        ordinal: 0,
        name: Some("Existing".into()),
        suppressed: Some(false),
        needs_recompute: false,
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        source_properties: std::collections::BTreeMap::default(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),
        evaluation: FeatureEvaluation::from_definition(FeatureDefinition::Operation(
            FeatureOperation::Native {
                kind: "prefix".into(),
                parameters: std::collections::BTreeMap::default(),
            },
        )),
        native_ref: None,
    });
    ir
}

#[test]
fn nonconsecutive_history_positions_append_after_the_actual_feature_arena() {
    let mut records = [record(3, 11, &[], &[40]), record(1, 12, &[40], &[41])];
    records[0].source_range = 17..23;
    records[1].source_range = 500..505;
    let mut ir = with_existing_feature();
    let prefix = ir.model.features[0].clone();
    assert_eq!(
        project(
            &cadmpeg_test_support::service_decode_context(),
            &records,
            None,
            &mut ir,
            &mut Diagnostics::new()
        )
        .expect("history follows the existing feature"),
        (0, 0, 0, 0)
    );
    ir.finalize();
    let mut features = ir.model.features.iter().collect::<Vec<_>>();
    features.sort_by_key(|feature| feature.ordinal);
    assert_eq!(features[0], &prefix);
    assert_eq!(
        features.iter().map(|f| f.ordinal).collect::<Vec<_>>(),
        [0, 1, 2]
    );
    assert_eq!(
        features[2].dependencies.as_slice(),
        std::slice::from_ref(&features[1].id)
    );
    assert_eq!(
        features[1].native_ref.as_deref(),
        Some("rhino:history:record#00000000-0000-0000-0000-000000000003")
    );
    assert_eq!(
        features[2].native_ref.as_deref(),
        Some("rhino:history:record#00000000-0000-0000-0000-000000000001")
    );
    assert_eq!(records[0].source_range, 17..23);
    assert_eq!(records[1].source_range, 500..505);
}

#[test]
fn dense_offset_preserves_later_cycle_and_ambiguous_producer_rules() {
    for (records, expected_dependencies) in [
        (
            vec![record(3, 11, &[41], &[40]), record(1, 12, &[40], &[41])],
            vec![0, 1],
        ),
        (
            vec![
                record(3, 11, &[], &[40]),
                record(1, 12, &[], &[40]),
                record(2, 13, &[40], &[41]),
            ],
            vec![0, 0, 0],
        ),
    ] {
        let mut ir = with_existing_feature();
        assert_eq!(
            project(
                &cadmpeg_test_support::service_decode_context(),
                &records,
                None,
                &mut ir,
                &mut Diagnostics::new()
            )
            .expect("source producer resolution"),
            (0, 0, 1, 0)
        );
        for (index, count) in expected_dependencies.into_iter().enumerate() {
            let feature = &ir.model.features[index + 1];
            assert_eq!(
                feature.ordinal,
                u64::try_from(index + 1).expect("small ordinal")
            );
            assert_eq!(feature.dependencies.len(), count);
        }
    }
}
