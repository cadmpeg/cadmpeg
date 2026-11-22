// SPDX-License-Identifier: Apache-2.0
//! Keywords member text: what is read, and what is named when it is not.
#![allow(clippy::unwrap_used)]

use crate::loss::SldprtLossCode;
use crate::records::{FeatureHistory, HistoryContent, HistoryElement, HistoryElementContent};
use crate::test_support::container::{make_block, outer_header};
use cadmpeg_core::text::NonBlankString;
use cadmpeg_ir::report::loss::LossNote;
use std::collections::BTreeMap;
use std::fmt::Write as _;

struct Decoded {
    histories: Vec<FeatureHistory>,
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
    let mut annotations = cadmpeg_ir::annotations::Annotations::default();
    let mut losses = Vec::new();
    let histories = crate::history::histories(&ctx, &scan, &mut annotations, &mut losses).unwrap();
    Decoded { histories, losses }
}

fn codes(losses: &[LossNote]) -> Vec<SldprtLossCode> {
    [
        SldprtLossCode::HistoryUnread,
        SldprtLossCode::HistoryApplicationNotTransferred,
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

#[test]
fn history_that_does_not_parse_is_unread_with_the_parser_message() {
    for text in [
        &br#"<Keywords Name="Part"><Feature id="1" Name="Fillet1" fillet:parameters="r=2"/></Keywords>"#[..],
        br#"<Keywords Name="Part"><Feature id="1" Name="Sketch1"/><Feature id="2" Name="Boss"><Dimension Name="D1">5"#,
    ] {
        let decoded = decode("swXmlContents/KeyWords", text);
        assert!(decoded.histories.is_empty());
        assert_eq!(codes(&decoded.losses), [SldprtLossCode::HistoryUnread]);
        let message = &decoded.losses[0].message;
        assert!(message.contains("its XML does not parse: "), "{message}");
        assert!(message.contains("swXmlContents/KeyWords"), "{message}");
    }
}

#[test]
fn only_history_member_names_charge_an_unread_history() {
    for name in [
        "swXmlContents/KeyWords",
        "swXmlContents/KeyWords__ZLB",
        "Contents/Keywords-0",
        "Contents/Keywords-12",
    ] {
        let decoded = decode(name, &[0x84, b'<', 0xff, b'>']);
        assert_eq!(
            codes(&decoded.losses),
            [SldprtLossCode::HistoryUnread],
            "{name}"
        );
    }
    for name in [
        "swXmlContents/KeyWordsIndex",
        "Contents/Keywords-",
        "Contents/Keywords-Equations",
    ] {
        let decoded = decode(name, &[0x84, b'<', 0xff, b'>']);
        assert!(codes(&decoded.losses).is_empty(), "{name}");
    }
}

#[test]
fn application_element_is_not_a_feature_and_is_named() {
    let decoded = decode(
        "swXmlContents/KeyWords",
        br#"<Keywords Name="Part"><Application Name="Simulation"><Keyword Name="Analysis type">Static</Keyword><Keyword Name="Mesh type">Solid Mesh</Keyword></Application><Feature id="1" Name="Sketch1" Type="ProfileFeature"/><Feature id="2" Name="Boss" Type="Extrusion"><Dimension Name="D1">10mm</Dimension></Feature></Keywords>"#,
    );
    assert_eq!(decoded.histories.len(), 1);
    let history = &decoded.histories[0];
    let names = history
        .features
        .iter()
        .map(|feature| (feature.name.as_str(), feature.ordinal))
        .collect::<Vec<_>>();
    assert_eq!(names, [("Sketch1", 0), ("Boss", 1)]);
    let keyword = |name: &str, value: &str| {
        HistoryElementContent::Element(HistoryElement {
            tag: "Keyword".into(),
            attributes: BTreeMap::from([(NonBlankString::new("Name").unwrap(), name.to_owned())]),
            content: vec![HistoryElementContent::Text(value.into())],
        })
    };
    assert_eq!(history.content.len(), 3);
    assert_eq!(
        history.content[0],
        HistoryContent::Element(HistoryElement {
            tag: "Application".into(),
            attributes: BTreeMap::from([(
                NonBlankString::new("Name").unwrap(),
                "Simulation".to_owned(),
            )]),
            content: vec![
                keyword("Analysis type", "Static"),
                keyword("Mesh type", "Solid Mesh"),
            ],
        })
    );
    assert!(matches!(
        history.content[1..],
        [HistoryContent::Feature(_), HistoryContent::Feature(_)]
    ));
    assert_eq!(
        codes(&decoded.losses),
        [SldprtLossCode::HistoryApplicationNotTransferred]
    );
    let message = &decoded.losses[0].message;
    assert!(
        message.contains(r#"Application element "Simulation""#)
            && message.contains(r#"Keyword "Analysis type", Keyword "Mesh type""#)
            && message.contains("kept in the native history"),
        "{message}"
    );
}

#[test]
fn history_member_with_another_root_is_unread() {
    let decoded = decode(
        "swXmlContents/KeyWords",
        br#"<Other Name="Part"><Feature id="1" Name="Boss"/></Other>"#,
    );
    assert!(decoded.histories.is_empty());
    assert_eq!(codes(&decoded.losses), [SldprtLossCode::HistoryUnread]);
    let message = &decoded.losses[0].message;
    assert!(message.contains(r#"its root element "Other""#), "{message}");
}

#[test]
fn history_whose_xml_is_refused_is_unread() {
    let attributes = (0..257).fold(String::new(), |mut text, index| {
        write!(text, " a{index}=\"{index}\"").unwrap();
        text
    });
    let text =
        format!(r#"<Keywords Name="Part"><Feature id="1" Name="Boss"{attributes}/></Keywords>"#);
    let decoded = decode("swXmlContents/KeyWords", text.as_bytes());
    assert!(decoded.histories.is_empty());
    assert_eq!(codes(&decoded.losses), [SldprtLossCode::HistoryUnread]);
    let message = &decoded.losses[0].message;
    assert!(message.contains("its XML is refused: "), "{message}");
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
