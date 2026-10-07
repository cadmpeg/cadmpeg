// SPDX-License-Identifier: Apache-2.0
//! Projection work boundaries and short-circuit reads.

use super::{
    incomplete_history_reference_features, project_feature_dependencies, zero_offset_roots,
};
use crate::history::tests::feature;
use crate::records::{Feature, FeatureContent, FeatureHistory};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::features::{
    FeatureDefinition, FeatureId, FeatureOperation, PlanarProfileRef, ProfileRef,
};
use std::collections::{BTreeMap, HashMap};

fn limited<T>(run: impl FnOnce(&DecodeContext<'_>) -> T) -> T {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 10_000;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    run(&ctx)
}

fn property(feature: &mut Feature, key: &'static str, value: String) {
    feature.properties.insert(
        cadmpeg_core::text::NonBlankString::try_from(key).unwrap(),
        value,
    );
}

#[test]
fn missing_pattern_seed_does_not_read_the_remaining_text() {
    let mut source = feature("pattern", None, 0);
    property(
        &mut source,
        "Seeds",
        format!("missing,{}", "x".repeat(100_000)),
    );
    let definition = limited(|ctx| {
        super::pattern::project_pattern(ctx, &source, &HashMap::new(), &HashMap::new())
    })
    .unwrap();
    assert!(
        matches!(definition, FeatureDefinition::Operation(FeatureOperation::Pattern { seeds, .. }) if seeds.is_empty())
    );
    crate::test_support::work_refusal_at("scan SLDPRT pattern seed delimiters", |ctx| {
        super::pattern::project_pattern(ctx, &source, &HashMap::new(), &HashMap::new())
    });
}

#[test]
fn ambiguous_extrusion_children_need_only_the_next_nonblank_character() {
    let mut source = feature("extrusion", None, 0);
    property(&mut source, "EndCondition", "ThroughAll".into());
    property(
        &mut source,
        "DissectableChildren",
        format!(" , first ,\u{2003},x{}", "x".repeat(100_000)),
    );
    let definition = limited(|ctx| {
        let index = super::solid::SourceFeatures::new(ctx, &[])?;
        super::solid::project_extrude(ctx, &source, &HashMap::new(), &index)
    })
    .unwrap();
    assert!(matches!(
        definition,
        Some(FeatureDefinition::Operation(FeatureOperation::Extrude {
            profile: ProfileRef::Planar(PlanarProfileRef::Unresolved(_)),
            ..
        }))
    ));
}

#[test]
fn authoritative_cut_class_does_not_read_the_kind_token() {
    let mut source = feature("cut", None, 0);
    source.input_class = Some("moCut_c".into());
    source.kind = "x".repeat(100_000);
    property(&mut source, "EndCondition", "ThroughAll".into());
    let definition = limited(|ctx| {
        let index = super::solid::SourceFeatures::new(ctx, &[])?;
        super::solid::project_extrude(ctx, &source, &HashMap::new(), &index)
    })
    .unwrap();
    assert!(matches!(
        definition,
        Some(FeatureDefinition::Operation(FeatureOperation::Extrude {
            op: cadmpeg_ir::features::BooleanOp::Cut,
            ..
        }))
    ));
}

#[test]
fn incomplete_reference_count_stops_at_the_first_missing_source() {
    let mut source = feature("sldprt:history:feature#0:1", Some("1"), 0);
    property(
        &mut source,
        "Dependencies",
        format!("999,{}", "x".repeat(100_000)),
    );
    let history = FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![source],
    };
    assert_eq!(
        limited(|ctx| incomplete_history_reference_features(ctx, std::slice::from_ref(&history)))
            .unwrap(),
        1
    );
}

#[test]
fn dependency_delimiters_preserve_source_order_and_uniqueness() {
    let mut source = feature("owner", None, 0);
    property(
        &mut source,
        "Dependencies",
        " ,1;\u{2003}2,,1;owner\t".into(),
    );
    let owner = FeatureId::mint("synthetic:test:feature#owner").unwrap();
    let first = FeatureId::mint("synthetic:test:feature#first").unwrap();
    let second = FeatureId::mint("synthetic:test:feature#second").unwrap();
    let index = HashMap::from([("1", &first), ("2", &second), ("owner", &owner)]);
    let result = project_feature_dependencies(
        &cadmpeg_test_support::service_decode_context(),
        &source,
        &owner,
        &index,
    )
    .unwrap();
    assert_eq!(result.as_slice(), [first, second]);
}

#[test]
fn ordered_hole_dimensions_do_not_traverse_unused_parameters() {
    let mut source = feature("profile", None, 0);
    source.parameters.insert(
        cadmpeg_core::nonblank_literal!("diameter"),
        "<MOD-DIAM>4".into(),
    );
    source
        .parameters
        .insert(cadmpeg_core::nonblank_literal!("depth"), "6".into());
    source.content = vec![
        FeatureContent::Dimension("diameter".into()),
        FeatureContent::Dimension("depth".into()),
    ];
    for index in 0..12_000 {
        source.parameters.insert(
            cadmpeg_core::text::NonBlankString::try_from(format!("unused-{index}")).unwrap(),
            "ignored".into(),
        );
    }
    let construction = limited(|ctx| super::solid::hole_sketch_construction(ctx, &source))
        .unwrap()
        .unwrap();
    assert_eq!(construction.diameter.get(), 4.0);
    assert_eq!(construction.depth.unwrap().get(), 6.0);
}

#[test]
fn invalid_fillet_position_does_not_read_its_radius() {
    let mut source = feature("fillet", None, 0);
    source.kind = "VarFillet".into();
    source.parameters.insert(
        cadmpeg_core::nonblank_literal!("Position0"),
        "invalid".into(),
    );
    source.parameters.insert(
        cadmpeg_core::nonblank_literal!("Radius0"),
        "x".repeat(100_000),
    );
    let definition = limited(|ctx| super::modify::project_fillet(ctx, &source)).unwrap();
    assert!(
        matches!(definition, FeatureDefinition::Operation(FeatureOperation::Fillet { groups }) if matches!(groups[0].radius, cadmpeg_ir::features::edge_treatments::RadiusSpec::Unresolved { form: Some(cadmpeg_ir::features::edge_treatments::RadiusForm::Variable) }))
    );
}

#[test]
fn explicit_retention_mode_does_not_trim_the_kind() {
    let mut source = feature("delete", None, 0);
    source.kind = " ".repeat(100_000);
    property(&mut source, "Mode", "delete".into());
    let definition = limited(|ctx| super::modify::project_delete_body(ctx, &source)).unwrap();
    assert!(matches!(
        definition,
        Some(FeatureDefinition::Operation(FeatureOperation::DeleteBody {
            mode: cadmpeg_ir::features::BodyRetentionMode::DeleteSelected,
            ..
        }))
    ));
}

#[test]
fn pattern_count_refuses_at_its_numeric_parse() {
    let mut source = feature("pattern", None, 0);
    source.kind = "LinearPattern".into();
    source
        .parameters
        .insert(cadmpeg_core::nonblank_literal!("Spacing"), "4mm".into());
    source
        .parameters
        .insert(cadmpeg_core::nonblank_literal!("Count"), "2".into());
    crate::test_support::work_refusal_at("parse SLDPRT pattern count", |ctx| {
        super::pattern::project_pattern(ctx, &source, &HashMap::new(), &HashMap::new())
    });
}

#[test]
fn zero_offset_roots_keep_cycle_entry_and_each_cycle_member() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut storage = ctx.reserve_scoped(0, "test zero offsets").unwrap();
    let roots = zero_offset_roots(
        &ctx,
        &mut storage,
        &[Some(1), Some(2), Some(1), None, Some(3)],
    )
    .unwrap();
    assert_eq!(roots, [1, 1, 2, 3, 3]);
}
