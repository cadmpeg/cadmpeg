// SPDX-License-Identifier: Apache-2.0
//! Keywords XML that does not parse: what is read and what is named.
#![allow(clippy::unwrap_used)]

use crate::loss::SldprtLossCode;
use crate::records::FeatureHistory;
use crate::test_support::container::{make_block, outer_header};
use cadmpeg_ir::annotations::Annotations;
use cadmpeg_ir::report::loss::LossNote;

struct Decoded {
    histories: Vec<FeatureHistory>,
    annotations: Annotations,
    losses: Vec<LossNote>,
}

fn decode(section: &str, payload: &[u8]) -> Decoded {
    let mut source = outer_header();
    source.extend(make_block(0x43, section, payload));
    let scan = crate::test_support::container::scan(&source);
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &source,
        &arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    )
    .unwrap();
    let mut annotations = Annotations::default();
    let mut losses = Vec::new();
    let histories = crate::history::histories(&ctx, &scan, &mut annotations, &mut losses).unwrap();
    Decoded {
        histories,
        annotations,
        losses,
    }
}

fn codes(losses: &[LossNote]) -> Vec<SldprtLossCode> {
    [
        SldprtLossCode::HistoryUnread,
        SldprtLossCode::HistoryXmlPrefixUnbound,
        SldprtLossCode::HistoryXmlTruncated,
    ]
    .into_iter()
    .flat_map(|code| {
        losses
            .iter()
            .filter(move |loss| loss.code == code.kind())
            .map(move |_| code)
    })
    .collect()
}

fn property<'a>(history: &'a FeatureHistory, feature: &str, key: &str) -> Option<&'a str> {
    history
        .features
        .iter()
        .find(|candidate| candidate.name == feature)?
        .properties
        .iter()
        .find(|(name, _)| name.as_str() == key)
        .map(|(_, value)| value.as_str())
}

#[test]
fn prefixed_attribute_name_is_read_as_written() {
    let decoded = decode(
        "swXmlContents/KeyWords",
        br#"<Keywords Name="Part"><Feature id="1" Name="Fillet1" Type="Fillet" fillet:parameters="r=2" fillet:id="9"/></Keywords>"#,
    );
    assert_eq!(decoded.histories.len(), 1);
    let history = &decoded.histories[0];
    assert_eq!(history.features.len(), 1);
    assert_eq!(history.features[0].kind, "Fillet");
    assert_eq!(history.features[0].source_value(), Some(1));
    assert_eq!(
        property(history, "Fillet1", "fillet:parameters"),
        Some("r=2")
    );
    assert_eq!(property(history, "Fillet1", "fillet:id"), Some("9"));
    assert_eq!(
        codes(&decoded.losses),
        [SldprtLossCode::HistoryXmlPrefixUnbound]
    );
    assert!(decoded.losses[0].message.contains("fillet"));
}

#[test]
fn unbound_element_prefix_is_kept_in_the_tag_and_offsets_stay_in_the_source() {
    let payload = br#"<Keywords Name="Part"><sw:Sketch id="2" Name="Sketch1"/><Feature id="3" Name="Boss" Type="Extrusion"/></Keywords>"#;
    let decoded = decode("swXmlContents/KeyWords", payload);
    assert_eq!(decoded.histories.len(), 1);
    let features = &decoded.histories[0].features;
    let tags = features
        .iter()
        .map(|feature| feature.xml_tag.as_str())
        .collect::<Vec<_>>();
    assert_eq!(tags, ["sw:Sketch", "Feature"]);
    assert_eq!(features[0].kind, "sw:Sketch");
    let boss = payload
        .windows(b"<Feature".len())
        .position(|window| window == b"<Feature")
        .unwrap();
    let offset = decoded.annotations.provenance[&features[1].id].offset;
    assert_eq!(offset, u64::try_from(boss).unwrap());
    assert_eq!(
        codes(&decoded.losses),
        [SldprtLossCode::HistoryXmlPrefixUnbound]
    );
}

#[test]
fn truncated_history_keeps_the_elements_before_the_cut() {
    let decoded = decode(
        "swXmlContents/KeyWords",
        br#"<Keywords Name="Part"><Configuration Name="Default"/><Feature id="1" Name="Sketch1" Type="ProfileFeature"/><Feature id="2" Name="Boss" Type="Extrusion"><Dimension Name="D1">10mm</Dimension><Dimension Name="D2">5"#,
    );
    assert_eq!(decoded.histories.len(), 1);
    let history = &decoded.histories[0];
    assert_eq!(history.part_name.as_deref(), Some("Part"));
    assert_eq!(history.configurations.len(), 1);
    assert_eq!(history.configurations[0].name, "Default");
    let names = history
        .features
        .iter()
        .map(|feature| feature.name.as_str())
        .collect::<Vec<_>>();
    assert_eq!(names, ["Sketch1", "Boss"]);
    let boss = &history.features[1];
    let parameters = boss
        .parameters
        .iter()
        .map(|(name, value)| (name.as_str(), value.as_str()))
        .collect::<Vec<_>>();
    assert_eq!(parameters, [("D1", "10mm")]);
    assert_eq!(
        codes(&decoded.losses),
        [SldprtLossCode::HistoryXmlTruncated]
    );
    assert!(
        decoded.losses[0].message.contains("<Dimension"),
        "{}",
        decoded.losses[0].message
    );
}

#[test]
fn history_cut_at_a_mismatched_end_tag_keeps_the_complete_elements() {
    let decoded = decode(
        "swXmlContents/KeyWords",
        br#"<Keywords Name="Part"><Feature id="1" Name="Sketch1"/><Feature id="2" Name="Boss"></Sketch><Feature id="3" Name="Cut"/></Keywords>"#,
    );
    let names = decoded.histories[0]
        .features
        .iter()
        .map(|feature| feature.name.as_str())
        .collect::<Vec<_>>();
    assert_eq!(names, ["Sketch1"]);
    assert_eq!(
        codes(&decoded.losses),
        [SldprtLossCode::HistoryXmlTruncated]
    );
}

#[test]
fn prefix_and_truncation_repairs_combine() {
    let decoded = decode(
        "swXmlContents/KeyWords",
        br#"<Keywords Name="Part"><Feature id="1" Name="Fillet1" fillet:parameters="r=2"/><Feature id="2" Name="Boss""#,
    );
    let history = &decoded.histories[0];
    assert_eq!(history.features.len(), 1);
    assert_eq!(
        property(history, "Fillet1", "fillet:parameters"),
        Some("r=2")
    );
    assert_eq!(
        codes(&decoded.losses),
        [
            SldprtLossCode::HistoryXmlPrefixUnbound,
            SldprtLossCode::HistoryXmlTruncated
        ]
    );
}

#[test]
fn history_cut_inside_its_root_tag_is_unread() {
    let decoded = decode("swXmlContents/KeyWords", br#"<Keywords Name="Pa"#);
    assert!(decoded.histories.is_empty());
    assert_eq!(codes(&decoded.losses), [SldprtLossCode::HistoryUnread]);
}

#[test]
fn history_member_that_is_not_text_is_unread() {
    let decoded = decode("swXmlContents/KeyWords", &[0x84, b'<', 0xff, b'>']);
    assert!(decoded.histories.is_empty());
    assert_eq!(codes(&decoded.losses), [SldprtLossCode::HistoryUnread]);
}

#[test]
fn history_after_either_lead_marker_is_read() {
    for marker in [0x85, 0x86] {
        let mut payload = vec![marker];
        payload.extend_from_slice(
            br#"<?xml version="1.0" encoding="UTF-8"?><Keywords Name="Part"><Feature id="1" Name="Boss"/></Keywords>"#,
        );
        let decoded = decode("swXmlContents/KeyWords", &payload);
        assert_eq!(decoded.histories.len(), 1, "lead {marker:#x}");
        assert_eq!(decoded.histories[0].features.len(), 1);
        assert!(codes(&decoded.losses).is_empty());
    }
}

#[test]
fn other_xml_that_does_not_parse_is_not_a_history() {
    let decoded = decode(
        "swXmlContents/MATERIALTREE",
        br#"<MaterialTree><Material Name="Steel"></MaterialTree>"#,
    );
    assert!(decoded.histories.is_empty());
    assert!(codes(&decoded.losses).is_empty());
}
