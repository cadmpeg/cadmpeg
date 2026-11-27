// SPDX-License-Identifier: Apache-2.0
//! Keywords member text: what is read, and what is named when it is not.
#![allow(clippy::unwrap_used)]

use crate::history::MAX_APPLICATION_LEVELS;
use crate::loss::SldprtLossCode;
use crate::records::{FeatureHistory, HistoryContent, HistoryElement, HistoryElementContent};
use crate::test_support::container::{make_block, outer_header, sldprt_with_body};
use crate::test_support::native::sldprt_native;
use crate::test_support::parasolid::triangle_body;
use crate::SldprtCodec;
use cadmpeg_core::text::NonBlankString;
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::report::loss::LossNote;
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::io::Cursor;

struct Decoded {
    histories: Vec<FeatureHistory>,
    losses: Vec<LossNote>,
    given_up: Vec<cadmpeg_ir::unknown::part::GivenUpPart>,
}

fn decode(section: &str, payload: &[u8]) -> Decoded {
    decode_under(
        section,
        payload,
        cadmpeg_core::decode::DecodePolicy::service(),
    )
    .unwrap()
}

fn decode_under(
    section: &str,
    payload: &[u8],
    policy: cadmpeg_core::decode::DecodePolicy,
) -> Result<Decoded, cadmpeg_core::CodecError> {
    let mut source = outer_header();
    source.extend(make_block(0x43, section, payload));
    let scan = crate::test_support::container::scan(&source);
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&source, &arena, &policy).unwrap();
    let mut annotations = cadmpeg_ir::annotations::Annotations::default();
    let mut losses = Vec::new();
    let mut given_up = Vec::new();
    let histories =
        crate::history::histories(&ctx, &scan, &mut annotations, &mut losses, &mut given_up)?;
    Ok(Decoded {
        histories,
        losses,
        given_up,
    })
}

/// The one give-up of an unread member: no note stands beside it.
fn unread(decoded: &Decoded) -> LossNote {
    assert!(codes(&decoded.losses).is_empty());
    let [part] = decoded.given_up.as_slice() else {
        panic!("one give-up: {:?}", decoded.given_up);
    };
    part.loss()
}

fn codes(losses: &[LossNote]) -> Vec<SldprtLossCode> {
    [
        SldprtLossCode::HistoryApplicationNotTransferred,
        SldprtLossCode::HistoryApplicationNestingCut,
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
        let loss = unread(&decoded);
        assert_eq!(
            loss.code,
            cadmpeg_ir::unknown::part::record_unreadable_kind()
        );
        let message = &loss.message;
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
            unread(&decoded).code,
            cadmpeg_ir::unknown::part::record_unreadable_kind(),
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
        assert!(decoded.given_up.is_empty(), "{name}");
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

/// A Keywords text whose `Application` element holds a chain of `Group`
/// elements `levels` deep, the `Application` element being level 1 and the
/// deepest element holding text.
fn nested_application(levels: usize) -> String {
    let mut text = String::from(r#"<Keywords Name="Part"><Application Name="Simulation">"#);
    text.push_str(&"<Group>".repeat(levels - 1));
    text.push_str("leaf");
    text.push_str(&"</Group>".repeat(levels - 1));
    text.push_str(
        r#"</Application><Feature id="1" Name="Sketch1" Type="ProfileFeature"/></Keywords>"#,
    );
    text
}

/// Element levels of a stored element, the element itself being level 1.
fn levels(element: &HistoryElement) -> usize {
    1 + element
        .content
        .iter()
        .filter_map(|item| match item {
            HistoryElementContent::Element(child) => Some(levels(child)),
            HistoryElementContent::Text(_) => None,
        })
        .max()
        .unwrap_or(0)
}

fn stored_application(history: &FeatureHistory) -> &HistoryElement {
    match &history.content[0] {
        HistoryContent::Element(application) => application,
        other => panic!("{other:?}"),
    }
}

#[test]
fn application_element_is_kept_to_the_level_bound_and_the_cut_is_named() {
    let at_bound = decode(
        "swXmlContents/KeyWords",
        nested_application(MAX_APPLICATION_LEVELS).as_bytes(),
    );
    let application = stored_application(&at_bound.histories[0]);
    assert_eq!(levels(application), MAX_APPLICATION_LEVELS);
    let mut deepest = application;
    while let [HistoryElementContent::Element(child)] = deepest.content.as_slice() {
        deepest = child;
    }
    assert_eq!(
        deepest.content,
        [HistoryElementContent::Text("leaf".into())]
    );
    assert_eq!(
        codes(&at_bound.losses),
        [SldprtLossCode::HistoryApplicationNotTransferred]
    );

    let past = decode(
        "swXmlContents/KeyWords",
        nested_application(MAX_APPLICATION_LEVELS + 1).as_bytes(),
    );
    let history = &past.histories[0];
    assert_eq!(levels(stored_application(history)), MAX_APPLICATION_LEVELS);
    assert_eq!(history.features.len(), 1);
    assert_eq!(
        codes(&past.losses),
        [
            SldprtLossCode::HistoryApplicationNotTransferred,
            SldprtLossCode::HistoryApplicationNestingCut,
        ]
    );
    let cut = past
        .losses
        .iter()
        .find(|loss| loss.code == SldprtLossCode::HistoryApplicationNestingCut.kind())
        .unwrap();
    assert!(
        cut.message.contains(r#"Application element "Simulation" nests 17 element levels"#)
            && cut
                .message
                .contains("1 element below level 16 is not kept (the first: element Group inside the level-16 element Group)"),
        "{}",
        cut.message
    );
}

#[test]
fn a_nested_application_element_decodes_and_its_document_reads_back() {
    for depth in [MAX_APPLICATION_LEVELS, MAX_APPLICATION_LEVELS + 1, 200] {
        let mut source = sldprt_with_body(&triangle_body());
        source.extend(make_block(
            0x42,
            "Contents/Keywords",
            nested_application(depth).as_bytes(),
        ));
        let decoded = SldprtCodec
            .decode(&mut Cursor::new(source), &DecodeOptions::default())
            .unwrap();
        let text = decoded.ir().to_canonical_json().unwrap();
        let reread = cadmpeg_ir::CadIr::from_json(&text).unwrap();
        let history = &sldprt_native(&reread).feature_histories[0];
        assert_eq!(
            levels(stored_application(history)),
            MAX_APPLICATION_LEVELS,
            "{depth}"
        );
        let cut = decoded
            .report()
            .losses
            .iter()
            .filter(|loss| loss.code == SldprtLossCode::HistoryApplicationNestingCut.kind())
            .count();
        assert_eq!(cut, usize::from(depth > MAX_APPLICATION_LEVELS), "{depth}");
    }
}

#[test]
fn history_member_with_another_root_is_unread() {
    let decoded = decode(
        "swXmlContents/KeyWords",
        br#"<Other Name="Part"><Feature id="1" Name="Boss"/></Other>"#,
    );
    assert!(decoded.histories.is_empty());
    let loss = unread(&decoded);
    assert_eq!(
        loss.code,
        cadmpeg_ir::unknown::part::record_undecoded_kind()
    );
    let message = &loss.message;
    assert!(message.contains(r#"its root element "Other""#), "{message}");
}

/// The parse of one tag with 2^13 added attributes compares more than
/// 2^25 - 2^12 attribute pairs, one expanded byte each: a 16 MiB ceiling on
/// the bytes expanded in all refuses the decode, and the service ceiling
/// reads the history.
#[test]
fn history_whose_xml_passes_the_expanded_bytes_ceiling_refuses_the_decode() {
    let attributes = (0..1 << 13).fold(String::new(), |mut text, index| {
        write!(text, " a{index}=\"{index}\"").unwrap();
        text
    });
    let text =
        format!(r#"<Keywords Name="Part"><Feature id="1" Name="Boss"{attributes}/></Keywords>"#);
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_decompressed_bytes_total = 1 << 24;
    let refused = decode_under("swXmlContents/KeyWords", text.as_bytes(), policy);
    let Err(cadmpeg_core::CodecError::ResourceLimit(limit)) = refused else {
        panic!("expected a resource refusal");
    };
    assert_eq!(
        (limit.dimension, limit.scope, limit.limit),
        (
            cadmpeg_core::decode::ResourceDimension::DecompressedBytes,
            cadmpeg_core::decode::LimitScope::Session,
            1 << 24
        )
    );
    assert!(limit.additional > (1 << 25) - (1 << 12));
    let decoded = decode("swXmlContents/KeyWords", text.as_bytes());
    assert_eq!(decoded.histories.len(), 1);
}

#[test]
fn history_member_that_is_not_text_is_unread() {
    let decoded = decode("swXmlContents/KeyWords", &[0x84, b'<', 0xff, b'>']);
    assert!(decoded.histories.is_empty());
    let charge = unread(&decoded);
    assert_eq!(
        charge.code,
        cadmpeg_ir::unknown::part::record_unreadable_kind()
    );
    assert!(
        charge
            .message
            .starts_with("part history-member \"swXmlContents/KeyWords\" stream="),
        "{}",
        charge.message
    );
    assert!(
        charge
            .message
            .contains(": Keywords member swXmlContents/KeyWords: its bytes are not"),
        "{}",
        charge.message
    );
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

#[test]
fn a_branching_cut_counts_every_element_below_the_bound_and_keeps_the_text_runs() {
    let text = format!(
        r#"<Keywords Name="Part"><Application Name="Simulation">{}lead<A><B><C/></B></A>mid<D/>tail{}</Application></Keywords>"#,
        "<Group>".repeat(MAX_APPLICATION_LEVELS - 1),
        "</Group>".repeat(MAX_APPLICATION_LEVELS - 1)
    );
    let decoded = decode("swXmlContents/KeyWords", text.as_bytes());
    let application = stored_application(&decoded.histories[0]);
    assert_eq!(levels(application), MAX_APPLICATION_LEVELS);
    let mut deepest = application;
    while let [HistoryElementContent::Element(child), ..] = deepest.content.as_slice() {
        deepest = child;
    }
    assert_eq!(
        deepest.content,
        ["lead", "mid", "tail"].map(|value| HistoryElementContent::Text(value.into()))
    );
    let cut = decoded
        .losses
        .iter()
        .find(|loss| loss.code == SldprtLossCode::HistoryApplicationNestingCut.kind())
        .unwrap();
    assert!(
        cut.message.contains("nests 19 element levels")
            && cut
                .message
                .contains("4 elements below level 16 are not kept")
            && cut
                .message
                .contains("(the first: element A inside the level-16 element Group)"),
        "{}",
        cut.message
    );
}

#[test]
fn forms_the_history_drops_are_counted_once_per_member() {
    let decoded = decode(
        "swXmlContents/KeyWords",
        br#"<Keywords xmlns:s="urn:s" Name="Part">
  <!-- note --><?pi data?>
  <s:Feature id="1" Name="Boss" s:Kind="x"><Dimension Name="D1"> 12mm </Dimension><Dimension Name="D2"> </Dimension></s:Feature>
</Keywords>"#,
    );
    let notes = decoded
        .losses
        .iter()
        .filter(|loss| loss.code == SldprtLossCode::HistoryXmlNormalised.kind())
        .collect::<Vec<_>>();
    assert_eq!(notes.len(), 1);
    assert_eq!(
        notes[0].message,
        "Keywords member swXmlContents/KeyWords: the native history drops 1 comment(s), 1 processing instruction(s), the edge whitespace of 2 text value(s), the namespace of 2 name(s); a SolidWorks write that regenerates the history writes the text without them"
    );

    let separated = decode(
        "swXmlContents/KeyWords",
        br#"<Keywords Name="Part"><Feature id="1" Name="Boss">lead<!---->text<!-- x -->more<!----><a/><!----></Feature></Keywords>"#,
    );
    let notes = separated
        .losses
        .iter()
        .filter(|loss| loss.code == SldprtLossCode::HistoryXmlNormalised.kind())
        .collect::<Vec<_>>();
    assert_eq!(notes.len(), 1);
    assert!(
        notes[0].message.contains("drops 3 comment(s);"),
        "{}",
        notes[0].message
    );

    let plain = decode(
        "swXmlContents/KeyWords",
        b"<Keywords Name=\"Part\">\n  <Feature id=\"1\" Name=\"Boss\"><Dimension Name=\"D1\">12mm</Dimension></Feature>\n</Keywords>",
    );
    assert!(plain
        .losses
        .iter()
        .all(|loss| loss.code != SldprtLossCode::HistoryXmlNormalised.kind()));
}
