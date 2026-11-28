// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn a_later_timeline_reports_only_built_state_changing_features() {
    use crate::loss::F3dLossCode;
    use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
    use cadmpeg_core::decode::DecodeDepth;

    let mut ir = cadmpeg_ir::CadIr::empty();
    let root = crate::tree::file_root_id();
    ir.model.product_definitions.push(definition(
        root.as_str(),
        ProductDefinition::root(root.clone(), ExternalDocument::default()).storage,
        &[],
    ));
    let extrude = || LegacyOperation::Unresolved {
        family: UnresolvedFamily::Extrude,
    };
    ir.model.features = vec![
        legacy("extrude:10", 0, Some(false), extrude()),
        legacy(
            "sketch:20",
            1,
            Some(false),
            LegacyOperation::Sketch {
                sketch: cadmpeg_ir::features::SketchFeatureBinding::Planar(None),
            },
        ),
        legacy("extrude:30", 2, Some(false), extrude()),
        legacy("extrude:40", 3, Some(false), extrude()),
    ];
    let rolled = ir.model.features[3].id.clone();
    let mut scope = DesignParameterScope::empty(
        &format!("{STREAM}:design-parameter-scope#40"),
        DesignFeatureKind::Extrude,
        40,
    );
    let mut flags = tail_flags([false; 2], false, true);
    flags.state_offset += scope.feature_ordinal_offset();
    flags.no_result_offset += scope.feature_ordinal_offset();
    scope.tail_flags = Some(flags);
    let timelines = [
        timeline(STREAM, 0, &[10]),
        timeline(STREAM, 1, &[20, 30]),
        timeline(STREAM, 2, &[40]),
    ];
    let mut losses = Vec::new();
    crate::test_support::with_decode_context(|ctx| {
        let namespace = ir.native.namespace_mut(cadmpeg_ir::codec_id!("f3d"));
        namespace
            .set_arena(ctx, super::super::TIMELINES, &timelines)
            .unwrap();
        namespace
            .set_arena(ctx, super::super::SCOPES, &[scope])
            .unwrap();
        namespace
            .set_arena(
                ctx,
                super::super::HISTORIES,
                &[cadmpeg_core::json!({"id": "f3d:BREP.test:asm-history#0"})],
            )
            .unwrap();
        super::super::project(ctx, &mut ir, DecodeDepth::Design, None, &mut losses).unwrap();
    });
    assert!(matches!(
        &ir.design.containers[2],
        Container::Ordered { rollback: Some(Rollback::Before(Member::Feature(feature))), .. }
            if feature == &rolled
    ));
    assert!(ir.is_stateless(&ir.design.features[1].id, None).unwrap());
    let [loss] = losses
        .iter()
        .filter(|loss| loss.code == F3dLossCode::HistoryStateUnread.kind())
        .collect::<Vec<_>>()
        .try_into()
        .unwrap();
    assert!(loss.message.contains(ir.design.features[2].id.as_str()));
    for feature in [&ir.design.features[1].id, &rolled] {
        assert!(!loss.message.contains(feature.as_str()), "{loss:?}");
    }
}
