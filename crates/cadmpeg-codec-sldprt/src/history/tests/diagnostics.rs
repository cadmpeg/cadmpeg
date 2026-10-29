// SPDX-License-Identifier: Apache-2.0
//! Keywords duplicate and inferred-state diagnostics.
#![allow(clippy::unwrap_used)]

use crate::test_support::container::{make_block, outer_header};

#[test]
fn duplicate_ids_retain_spans_in_diagnostics() {
    let payload = br#"<Keywords Name="Part"><Extrusion Name="A" Type="BossExtrude" id="7"/><Extrusion Name="B" Type="CutExtrude" id="7"/></Keywords>"#;
    let mut source = outer_header();
    source.extend(make_block(0x43, "Contents/Keywords", payload));
    let scan = crate::test_support::container::scan(&source);
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &source,
        &arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    )
    .unwrap();
    let mut annotations = cadmpeg_ir::annotations::Annotations::default();
    let mut losses = Vec::new();
    let histories = crate::history::histories(&ctx, &scan, &mut annotations, &mut losses).unwrap();
    assert_eq!(histories.len(), 1);
    let properties = &histories[0].properties;
    let ids = properties
        .iter()
        .find(|(key, _)| key.as_str() == "duplicate_source_ids")
        .map(|(_, value)| value.clone())
        .unwrap_or_default();
    let spans = properties
        .iter()
        .find(|(key, _)| key.as_str() == "duplicate_source_spans")
        .map(|(_, value)| value.clone())
        .unwrap_or_default();
    assert!(ids.split(',').any(|id| id.trim() == "7"), "{ids}");
    assert!(spans.contains("source 7 at"), "{spans}");
    let diagnostics = crate::history::project::duplicate_history_source_diagnostics(&histories);
    assert!(!diagnostics.is_empty());
    assert!(diagnostics[0].contains("source 7 at"));
    let incomplete =
        crate::history::project::incomplete_history_reference_features(&histories).unwrap();
    assert!(incomplete > 0);
}

#[test]
fn unsaved_dependencies_carry_an_inferred_marker() {
    let payload = br#"<Keywords Name="Part"><Extrusion Name="Boss" Type="BossExtrude" id="9" Dependencies="7,8"/></Keywords>"#;
    let mut source = outer_header();
    source.extend(make_block(0x43, "Contents/Keywords", payload));
    let scan = crate::test_support::container::scan(&source);
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &source,
        &arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    )
    .unwrap();
    let mut annotations = cadmpeg_ir::annotations::Annotations::default();
    let mut losses = Vec::new();
    let histories = crate::history::histories(&ctx, &scan, &mut annotations, &mut losses).unwrap();
    let projection = crate::history::project::project_feature_model(&histories).unwrap();
    let feature = projection
        .features
        .iter()
        .find(|feature| {
            feature
                .native_ref
                .as_deref()
                .is_some_and(|reference| reference.ends_with(":0"))
        })
        .expect("one projected feature");
    assert_eq!(
        feature
            .source_properties
            .get("dependencies_decoded")
            .map(String::as_str),
        Some("false")
    );
}
