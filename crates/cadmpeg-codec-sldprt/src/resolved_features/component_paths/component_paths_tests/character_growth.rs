//! Compact selection text character growth and refusal.

use crate::records::{FeatureInputComponentPathEntry, FeatureInputEdgeSelection};
use crate::resolved_features::component_paths::{compact_body_selection_value_charged,
    compact_edge_path_value_charged, compact_edge_selection_set_value_charged};

fn selection(components: Vec<FeatureInputComponentPathEntry>) -> FeatureInputEdgeSelection {
    FeatureInputEdgeSelection {
        id: "selection".into(), parent: "lane".into(), ordinal: 0, offset: 0,
        object_name_ref: "name".into(), feature_ref: "feature".into(),
        local_edge_ids: vec![4, 0], components, references: Vec::new(),
        producer_feature_refs: Vec::new(), terminal_feature_ref: None,
    }
}

#[test]
fn compact_edge_local_id_character_growth_preserves_text_and_refusal() {
    let selection = selection(Vec::new());
    let format = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| {
        compact_edge_path_value_charged(ctx, &selection)
    };
    assert_eq!(format(&cadmpeg_test_support::service_decode_context()).unwrap(), "4,0");
    crate::test_support::work_refusal_at("format SLDPRT compact edge local id separator", format);
}

#[test]
fn compact_edge_component_character_growth_preserves_text_and_refusals() {
    let components = [Some(4), None].map(|local_id| FeatureInputComponentPathEntry {
        instance: None, type_signature: [0; 12], local_id,
    });
    let selection = selection(components.into());
    let format = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| {
        compact_edge_path_value_charged(ctx, &selection)
    };
    assert_eq!(format(&cadmpeg_test_support::service_decode_context()).unwrap(), "4,_");
    for operation in ["format SLDPRT compact edge component separator",
        "format SLDPRT compact edge absent component"] {
        crate::test_support::work_refusal_at(operation, format);
    }
}

#[test]
fn compact_edge_set_local_id_character_growth_preserves_text_and_refusal() {
    let selection = selection(Vec::new());
    let format = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| {
        compact_edge_selection_set_value_charged(ctx, &[&selection])
    };
    assert_eq!(format(&cadmpeg_test_support::service_decode_context()).unwrap(),
        "sldprt:feature-input:edge-ids:4,0");
    crate::test_support::work_refusal_at("format SLDPRT compact edge set local id separator", format);
}

#[test]
fn compact_edge_selection_character_growth_preserves_text_and_refusal() {
    let first = selection(Vec::new());
    let second = selection([Some(4), None].map(|local_id| FeatureInputComponentPathEntry {
        instance: None, type_signature: [0; 12], local_id,
    }).into());
    let format = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| {
        compact_edge_selection_set_value_charged(ctx, &[&first, &second])
    };
    assert_eq!(format(&cadmpeg_test_support::service_decode_context()).unwrap(),
        "sldprt:feature-input:edge-selection-vectors:4,0;4,_");
    crate::test_support::work_refusal_at("format SLDPRT compact edge selection separator", format);
}

#[test]
fn compact_body_local_id_character_growth_preserves_text_and_refusal() {
    let format = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| {
        compact_body_selection_value_charged(ctx, &[4, 0])
    };
    assert_eq!(format(&cadmpeg_test_support::service_decode_context()).unwrap(),
        "sldprt:feature-input:body-ids:4,0");
    crate::test_support::work_refusal_at("format SLDPRT compact body local id separator", format);
}
