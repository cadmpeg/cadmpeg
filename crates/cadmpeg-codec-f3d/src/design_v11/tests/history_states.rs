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

fn ordered(name: &str, features: &[&str]) -> Container {
    Container::Ordered {
        id: cadmpeg_ir::design::ids::ContainerId::mint(format!("f3d:design:timeline#{name}"))
            .unwrap(),
        kind: OrderedKind::Timeline,
        members: features
            .iter()
            .map(|feature| {
                Member::Feature(FeatureId::mint(format!("test:design:feature#{feature}")).unwrap())
            })
            .collect(),
        folders: Vec::new(),
        rollback: Some(Rollback::End),
        placement: None,
    }
}

#[test]
fn a_scope_whose_states_the_walk_does_not_reach_charges_a_loss() {
    let definition = ProductDefinitionId::mint("test:design:definition#scope").unwrap();
    let ir = cadmpeg_ir::CadIr::empty();
    let codes = |prefix: &str, bodiless: bool, containers: &[Container]| {
        let mut losses = Vec::new();
        super::super::unread_scope(&ir, &definition, prefix, bodiless, containers, &mut losses);
        losses
    };
    let [hosted] = codes("Hosted/", false, &[ordered("0", &["a"])])
        .try_into()
        .unwrap();
    assert_eq!(
        hosted.code,
        crate::loss::F3dLossCode::HistoryStatesUnread.kind()
    );
    assert!(
        hosted.message.contains("a document other than the root"),
        "{}",
        hosted.message
    );
    let [bodiless] = codes("", true, &[ordered("0", &["a"])]).try_into().unwrap();
    assert!(
        bodiless.message.contains("no current body"),
        "{}",
        bodiless.message
    );
    let [later] = codes("", false, &[ordered("0", &["a"]), ordered("1", &["b"])])
        .try_into()
        .unwrap();
    assert_eq!(
        later.code,
        crate::loss::F3dLossCode::HistoryStateUnread.kind()
    );
    assert!(
        later.message.contains("test:design:feature#b")
            && !later.message.contains("test:design:feature#a"),
        "{}",
        later.message
    );
    assert!(codes("", false, &[ordered("0", &["a"])]).is_empty());
    assert!(codes("", true, &[ordered("0", &[])]).is_empty());
}
