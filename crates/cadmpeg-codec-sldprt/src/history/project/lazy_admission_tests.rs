// SPDX-License-Identifier: Apache-2.0
//! Projection lookup construction follows its first consumer.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use crate::history::tests::feature;

#[test]
fn sketch_history_does_not_build_solid_or_construction_indexes() {
    let mut first = feature("first", Some("1"), 0);
    first.kind = "Sketch".into();
    first.xml_tag = "Sketch".into();
    let mut second = first.clone();
    second.id = "second".into();
    second.source_id = Some(crate::records::FeatureSource::from_value(2).unwrap());
    let history = crate::records::FeatureHistory { id: "history".into(), part_name: None, properties: Default::default(), content: Vec::new(), configurations: Vec::new(), features: vec![first, second] };
    for operation in ["index SLDPRT source profiles", "index SLDPRT history records by identity", "index SLDPRT native construction sources", "index SLDPRT native construction features"] {
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
        let _probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(ResourceDimension::MaterializedBytes, operation, None);
        let features = super::project_features(&ctx, std::slice::from_ref(&history)).unwrap();
        assert_eq!(features.len(), 2);
        assert!(features.iter().all(|feature| matches!(feature.evaluation.definition(), cadmpeg_ir::features::FeatureDefinition::Operation(cadmpeg_ir::features::FeatureOperation::Sketch { .. }))));
        ctx.finish_session().unwrap();
    }
}

#[test]
fn rejected_equation_domain_does_not_read_or_copy_expressions() {
    let mut source = feature("equation", None, 0);
    source.properties.insert(cadmpeg_core::nonblank_literal!("Start"), "NaN".into());
    source.properties.insert(cadmpeg_core::nonblank_literal!("End"), "1".into());
    for key in ["Parameter", "XEquation", "YEquation", "ZEquation"] {
        source.properties.insert(key.try_into().unwrap(), "x".repeat(100_000));
    }
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 1_000;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(super::datum::project_equation_curve(&ctx, &source).unwrap().is_none());
    ctx.finish_session().unwrap();
}

#[test]
fn shared_count_parser_preserves_whitespace_sign_range_and_positive_grammar() {
    let ctx = cadmpeg_test_support::service_decode_context();
    for (text, expected) in [("1", Some(1)), (" +2 ", Some(2)), ("\u{2003}3\u{2003}", Some(3)), ("4294967295", Some(u32::MAX)), ("0", None), ("-1", None), ("1.0", None), ("4294967296", None), ("", None)] {
        assert_eq!(super::pattern::parse_count(&ctx, text).unwrap(), expected, "{text:?}");
    }
}

#[test]
fn indexed_radius_name_uses_the_writer_grammar_for_decode() {
    let ctx = cadmpeg_test_support::service_decode_context();
    for text in ["Radius", "Radius0", "Radius001", "Radius9", "Radius-1", "Radius1x", "Radius١", "radius1", "Radius "] {
        assert_eq!(crate::history::classify::indexed_name_for_decode(&ctx, text, "Radius").unwrap(), crate::history::classify::indexed_name(text, "Radius"), "{text:?}");
    }
}
