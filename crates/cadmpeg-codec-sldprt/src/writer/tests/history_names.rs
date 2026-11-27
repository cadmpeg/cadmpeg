// SPDX-License-Identifier: Apache-2.0
//! A regenerated Keywords text: what XML cannot hold is left out or replaced
//! and named, and the written history reads back.
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_core::text::NonBlankString;
use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::report::loss::LossNote;

use crate::loss::SldprtLossCode;
use crate::native::SldprtNative;
use crate::records::{FeatureHistory, HistoryContent, HistoryElement, HistoryElementContent};
use crate::test_support::container::{make_block, sldprt_with_body};
use crate::test_support::native::{sldprt_native, update_sldprt_native};
use crate::test_support::parasolid::triangle_body;
use crate::SldprtCodec;

const KEYWORDS: &str = r#"<Keywords Name="Part"><Application Name="Simulation"><Keyword Name="Analysis type">Static</Keyword><Keyword Name="Mesh type">Solid Mesh</Keyword></Application><Configuration Name="Default" SourceIndex="0"/><Feature Name="Sketch1" Type="ProfileFeature" id="1"/><Extrusion Name="Boss" Type="BossExtrude" id="2"><Dimension Name="Depth">12mm</Dimension></Extrusion></Keywords>"#;

struct Written {
    /// What the writer charged.
    losses: Vec<LossNote>,
    /// The written Keywords member.
    keywords: Vec<u8>,
    /// The history a decode of the written file reads.
    history: FeatureHistory,
    /// What that decode charged.
    reread: Vec<LossNote>,
}

fn decoded(keywords: &[u8]) -> cadmpeg_test_support::EditableDecodeResult {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(0x42, "Contents/Keywords", keywords));
    SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap()
        .into()
}

/// Writes the part of `keywords` after `edit` of its native history and
/// decodes the written file.
fn write_edited(keywords: &[u8], edit: impl FnOnce(&mut SldprtNative)) -> Written {
    let mut decoded = decoded(keywords);
    update_sldprt_native(&mut decoded.ir_mut(), edit);
    let mut bytes = Vec::new();
    let output = crate::test_support::write_in_native_envelope(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut bytes,
    )
    .unwrap();
    let scan = crate::test_support::container::scan(&bytes);
    let written_keywords = scan
        .blocks
        .iter()
        .find(|block| block.section.name() == Some("Contents/Keywords"))
        .unwrap()
        .payload
        .clone();
    let reread = SldprtCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let histories = sldprt_native(reread.ir()).feature_histories;
    assert_eq!(histories.len(), 1);
    Written {
        losses: output.losses,
        keywords: written_keywords,
        history: histories.into_iter().next().unwrap(),
        reread: reread.report().losses.clone(),
    }
}

fn charged(losses: &[LossNote], code: SldprtLossCode) -> Vec<&str> {
    losses
        .iter()
        .filter(|loss| loss.code == code.kind())
        .map(|loss| loss.message.as_str())
        .collect()
}

/// The writer's charges against the Keywords text.
fn history_charges(losses: &[LossNote]) -> Vec<&LossNote> {
    losses
        .iter()
        .filter(|loss| {
            let code = loss.code.local_code();
            code.starts_with("writer.history-")
                || code == "writer.keywords-characters-replaced"
                || code == "writer.keywords-text-trimmed"
                || code == "writer.feature-fields-omitted"
        })
        .collect()
}

/// The regenerated history was written and the reader reads it.
fn assert_regenerated(written: &Written) {
    assert!(
        charged(
            &written.losses,
            SldprtLossCode::WriterHistoryRegenerationRefused
        )
        .is_empty(),
        "{:?}",
        written.losses
    );
    assert!(charged(&written.reread, SldprtLossCode::HistoryUnread).is_empty());
}

fn key(name: &str) -> NonBlankString {
    NonBlankString::new(name).unwrap()
}

fn application(history: &mut FeatureHistory) -> &mut HistoryElement {
    match &mut history.content[0] {
        HistoryContent::Element(element) => element,
        other => panic!("{other:?}"),
    }
}

fn child(element: &mut HistoryElement, index: usize) -> &mut HistoryElement {
    match &mut element.content[index] {
        HistoryElementContent::Element(child) => child,
        other @ HistoryElementContent::Text(_) => panic!("{other:?}"),
    }
}

#[test]
fn names_xml_cannot_hold_are_left_out_and_the_history_reads_back() {
    let mut written = write_edited(KEYWORDS.as_bytes(), |native| {
        let history = &mut native.feature_histories[0];
        history.properties.insert(key("xmlns"), "urn:x".into());
        history.configurations[0]
            .properties
            .insert(key("1st"), "x".into());
        let boss = &mut history.features[1];
        boss.properties.insert(key("bad key"), "1".into());
        boss.properties.insert(key("Suppressed"), "1".into());
        let application = application(history);
        application.attributes.insert(key("Na me"), "x".into());
        child(application, 1).tag = "Mesh type".into();
    });
    assert_regenerated(&written);
    let names = charged(&written.losses, SldprtLossCode::WriterHistoryNameNotWritten);
    assert_eq!(names.len(), 1, "{names:?}");
    assert!(
        names[0].contains("leaves out 1 Keywords element(s)")
            && names[0].contains(r#"> Application "Simulation" > Mesh type"#),
        "{}",
        names[0]
    );
    let fields = charged(&written.losses, SldprtLossCode::WriterFeatureFieldsOmitted);
    assert_eq!(fields.len(), 1, "{fields:?}");
    for name in [
        "4 record(s)",
        "(xmlns)",
        "(1st)",
        "(Suppressed, bad key)",
        r#"> Application "Simulation" (Na me)"#,
    ] {
        assert!(fields[0].contains(name), "{name}: {}", fields[0]);
    }
    let history = &mut written.history;
    assert!(!history.properties.contains_key("xmlns"));
    assert!(!history.configurations[0].properties.contains_key("1st"));
    let boss = &history.features[1];
    assert!(!boss.suppressed);
    assert!(!boss.properties.contains_key("bad key"));
    let application = application(history);
    assert_eq!(application.attributes.len(), 1);
    assert_eq!(application.content.len(), 1);
}

#[test]
fn a_feature_tag_xml_cannot_hold_or_another_record_owns_is_written_as_feature() {
    for (tag, expected) in [
        ("Bad Tag", "Feature"),
        ("a:b", "Feature"),
        ("1st", "Feature"),
        ("Dimension", "Feature"),
        ("Configuration", "Feature"),
        ("Keywords", "Feature"),
        ("Application", "Feature"),
        ("Schlüssel", "Schlüssel"),
        ("Boss-Extrude.1", "Boss-Extrude.1"),
    ] {
        let written = write_edited(KEYWORDS.as_bytes(), |native| {
            native.feature_histories[0].features[1].xml_tag = tag.into();
        });
        assert_regenerated(&written);
        let boss = written
            .history
            .features
            .iter()
            .find(|feature| feature.name == "Boss")
            .unwrap();
        assert_eq!(boss.xml_tag, expected, "{tag}");
        assert_eq!(boss.kind, "BossExtrude", "{tag}");
        assert_eq!(boss.parameters["Depth"], "12mm", "{tag}");
        let names = charged(&written.losses, SldprtLossCode::WriterHistoryNameNotWritten);
        assert_eq!(
            names.len(),
            usize::from(tag != expected),
            "{tag}: {names:?}"
        );
        assert!(
            names.iter().all(|note| note.contains(&format!("<{tag}>"))),
            "{names:?}"
        );
    }
}

#[test]
fn characters_xml_cannot_hold_are_replaced_and_tabs_and_returns_are_kept() {
    let mut written = write_edited(KEYWORDS.as_bytes(), |native| {
        let history = &mut native.feature_histories[0];
        let boss = &mut history.features[1];
        boss.name = "Bo\u{1}ss\u{ffff}".into();
        boss.properties.insert(key("Note"), "a\tb\r\nc".into());
        child(application(history), 0).content =
            vec![HistoryElementContent::Text("Sta\r\ntic\tx".into())];
    });
    assert_regenerated(&written);
    let characters = charged(
        &written.losses,
        SldprtLossCode::WriterKeywordsCharactersReplaced,
    );
    assert_eq!(characters.len(), 1);
    assert!(
        characters[0].contains("in the texts of 1 record(s)"),
        "{}",
        characters[0]
    );
    let history = &mut written.history;
    let boss = &history.features[1];
    assert_eq!(boss.name, "Bo\u{fffd}ss\u{fffd}");
    assert_eq!(boss.properties["Note"], "a\tb\r\nc");
    assert_eq!(
        child(application(history), 0).content,
        [HistoryElementContent::Text("Sta\r\ntic\tx".into())]
    );
}

#[test]
fn adjacent_text_items_read_back_as_separate_items() {
    let texts = |values: &[&str]| {
        values
            .iter()
            .map(|value| HistoryElementContent::Text((*value).into()))
            .collect::<Vec<_>>()
    };
    let mut written = write_edited(KEYWORDS.as_bytes(), |native| {
        let history = &mut native.feature_histories[0];
        history.content.insert(1, HistoryContent::Text("a".into()));
        history.content.insert(2, HistoryContent::Text("b".into()));
        child(application(history), 0).content = texts(&["lead", "mid", "tail"]);
    });
    assert_regenerated(&written);
    assert!(history_charges(&written.losses).is_empty());
    assert!(
        charged(&written.reread, SldprtLossCode::HistoryXmlNormalised).is_empty(),
        "{:?}",
        written.reread
    );
    let history = &mut written.history;
    assert!(matches!(
        &history.content[1..3],
        [HistoryContent::Text(a), HistoryContent::Text(b)] if a == "a" && b == "b"
    ));
    assert_eq!(
        child(application(history), 0).content,
        texts(&["lead", "mid", "tail"])
    );
}

#[test]
fn a_history_that_cannot_be_regenerated_writes_its_source_member() {
    let written = write_edited(KEYWORDS.as_bytes(), |native| {
        let features = &mut native.feature_histories[0].features;
        features[1].name = "Edited".into();
        features[1].source_id = features[0].source_id;
    });
    let refused = charged(
        &written.losses,
        SldprtLossCode::WriterHistoryRegenerationRefused,
    );
    assert_eq!(refused.len(), 1);
    assert!(
        refused[0].contains("cannot be built: duplicate feature source id")
            && refused[0]
                .contains("the retained source block of the member is written in its place"),
        "{}",
        refused[0]
    );
    assert_eq!(written.keywords, KEYWORDS.as_bytes());
    assert_eq!(written.history.features[1].name, "Boss");
}

/// A Keywords text whose `Application` element holds 15 nested `Group`
/// elements; the level-16 one holds text runs around two element chains of 3
/// and 1 levels.
fn branching_cut() -> String {
    format!(
        r#"<Keywords Name="Part"><Application Name="Simulation">{}lead<A><B><C/></B></A>mid<D/>tail{}</Application><Configuration Name="Default" SourceIndex="0"/><Extrusion Name="Boss" Type="BossExtrude" id="2"><Dimension Name="Depth">12mm</Dimension></Extrusion></Keywords>"#,
        "<Group>".repeat(15),
        "</Group>".repeat(15)
    )
}

#[test]
fn an_edited_write_of_a_cut_element_writes_the_kept_levels_and_text_runs() {
    let keywords = branching_cut();
    let mut stored = sldprt_native(decoded(keywords.as_bytes()).ir()).feature_histories[0].clone();
    let stored = application(&mut stored).clone();
    let written = write_edited(keywords.as_bytes(), |native| {
        native.feature_histories[0].features[0].name = "Edited".into();
    });
    assert_regenerated(&written);
    assert!(history_charges(&written.losses).is_empty());
    let mut history = written.history;
    assert_eq!(history.features[0].name, "Edited");
    assert_eq!(*application(&mut history), stored);
    let mut deepest = &stored;
    for _ in 1..16 {
        deepest = match &deepest.content[0] {
            HistoryElementContent::Element(child) => child,
            other @ HistoryElementContent::Text(_) => panic!("{other:?}"),
        };
    }
    assert_eq!(
        deepest.content,
        ["lead", "mid", "tail"]
            .map(|value| HistoryElementContent::Text(value.into()))
            .to_vec()
    );
    assert!(charged(
        &written.reread,
        SldprtLossCode::HistoryApplicationNestingCut
    )
    .is_empty());
}

#[test]
fn a_regenerated_member_keeps_the_lead_byte_and_declaration_of_its_source() {
    let prefix = b"\x85<?xml version=\"1.0\" encoding=\"UTF-8\"?>\r\n";
    let source = [&prefix[..], KEYWORDS.as_bytes()].concat();
    let written = write_edited(&source, |native| {
        native.feature_histories[0].features[1].name = "Edited".into();
    });
    assert_regenerated(&written);
    assert!(written.keywords.starts_with(prefix));
    assert_eq!(
        &written.keywords[prefix.len()..prefix.len() + 9],
        b"<Keywords"
    );
    assert_eq!(written.history.features[1].name, "Edited");

    let written = write_edited(KEYWORDS.as_bytes(), |native| {
        native.feature_histories[0].features[1].name = "Edited".into();
    });
    assert!(written.keywords.starts_with(b"<Keywords"));
}

/// `write_edited` of [`KEYWORDS`] with `elements` more empty elements in its
/// `Application` element.
fn wide(elements: usize) -> Written {
    write_edited(KEYWORDS.as_bytes(), |native| {
        let application = application(&mut native.feature_histories[0]);
        application.content.extend(
            std::iter::repeat_with(|| {
                HistoryElementContent::Element(HistoryElement {
                    tag: "a".into(),
                    attributes: std::collections::BTreeMap::new(),
                    content: Vec::new(),
                })
            })
            .take(elements),
        );
    })
}

#[test]
fn a_history_the_decoder_reads_from_the_written_file_is_written_and_one_it_does_not_is_not() {
    let mut written = wide(60_000);
    assert_regenerated(&written);
    assert_eq!(application(&mut written.history).content.len(), 60_002);

    let written = wide(300_000);
    let refused = charged(
        &written.losses,
        SldprtLossCode::WriterHistoryRegenerationRefused,
    );
    assert_eq!(refused.len(), 1, "{:?}", written.losses);
    assert!(
        refused[0].contains("per-expansion allowance")
            && refused[0]
                .contains("the retained source block of the member is written in its place"),
        "{}",
        refused[0]
    );
    assert_eq!(written.keywords, KEYWORDS.as_bytes());
    assert!(charged(&written.reread, SldprtLossCode::HistoryUnread).is_empty());
}

#[test]
fn blank_and_edge_padded_text_items_are_trimmed_or_left_out_and_named() {
    let texts = |values: &[&str]| {
        values
            .iter()
            .map(|value| HistoryElementContent::Text((*value).into()))
            .collect::<Vec<_>>()
    };
    let mut written = write_edited(KEYWORDS.as_bytes(), |native| {
        let history = &mut native.feature_histories[0];
        history
            .content
            .insert(1, HistoryContent::Text(" edge ".into()));
        history.content.insert(2, HistoryContent::Text("  ".into()));
        history
            .content
            .insert(3, HistoryContent::Text("tail".into()));
        child(application(history), 0).content = texts(&["lead", "", " x\t"]);
    });
    assert_regenerated(&written);
    let trimmed = charged(&written.losses, SldprtLossCode::WriterKeywordsTextTrimmed);
    assert_eq!(trimmed.len(), 1, "{:?}", written.losses);
    assert!(
        trimmed[0].contains("of 2 record(s)")
            && trimmed[0].contains("Application \"Simulation\" > Keyword \"Analysis type\""),
        "{}",
        trimmed[0]
    );
    assert!(
        charged(&written.reread, SldprtLossCode::HistoryXmlNormalised).is_empty(),
        "{:?}",
        written.reread
    );
    let history = &mut written.history;
    assert!(matches!(
        &history.content[1..3],
        [HistoryContent::Text(a), HistoryContent::Text(b)] if a == "edge" && b == "tail"
    ));
    assert_eq!(
        child(application(history), 0).content,
        texts(&["lead", "x"])
    );
}

/// A Keywords text whose one feature holds 256 attributes and no `Type`:
/// the reader reads it, and refuses the regenerated text, which adds `Type`.
fn full_feature() -> String {
    let attributes = (0..254)
        .map(|index| format!(" a{index}=\"{index}\""))
        .collect::<Vec<_>>()
        .concat();
    format!(
        "<Keywords Name=\"Second\"><ReferencePlane id=\"21\" Name=\"Q1\"{attributes}><Dimension Name=\"D1\">3mm</Dimension></ReferencePlane></Keywords>"
    )
}

#[test]
fn a_refused_history_keeps_the_stored_blocks_of_its_member_for_every_history_of_it() {
    let second = full_feature();
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(0x42, "Contents/Keywords", KEYWORDS.as_bytes()));
    source.extend(make_block(0x42, "Contents/Keywords", second.as_bytes()));
    let mut decoded: cadmpeg_test_support::EditableDecodeResult = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap()
        .into();
    assert_eq!(sldprt_native(decoded.ir()).feature_histories.len(), 2);
    crate::test_support::make_source_image_unavailable(decoded.source_fidelity_mut());
    let mut bytes = Vec::new();
    let output = SldprtCodec
        .plan(
            EncodeInput::new(decoded.ir(), Some(decoded.source_fidelity())),
            TargetRequest::Unnamed,
        )
        .unwrap()
        .write_to(&mut bytes)
        .unwrap();
    let refused = charged(
        &output.losses,
        SldprtLossCode::WriterHistoryRegenerationRefused,
    );
    assert_eq!(refused.len(), 2, "{:?}", output.losses);
    assert!(
        refused
            .iter()
            .any(|note| note.contains("does not read back")
                && note.contains(
                    "the 2 retained source block(s) of the member are written as stored"
                )),
        "{refused:?}"
    );
    assert!(
        refused.iter().any(|note| note
            .contains("another history of the member is not regenerated")
            && note.contains("the 2 retained source block(s) of the member are written as stored")),
        "{refused:?}"
    );
    assert!(
        output
            .losses
            .iter()
            .all(|loss| !loss.message.contains("sldprt:file:block#")),
        "{:?}",
        output.losses
    );
    let mut stored = crate::test_support::container::scan(&bytes)
        .blocks
        .iter()
        .filter(|block| block.section.name() == Some("Contents/Keywords"))
        .map(|block| block.payload.clone())
        .collect::<Vec<_>>();
    stored.sort();
    let mut sources = vec![KEYWORDS.as_bytes().to_vec(), second.into_bytes()];
    sources.sort();
    assert_eq!(stored, sources);
    let reread = SldprtCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let histories = sldprt_native(reread.ir()).feature_histories;
    assert_eq!(histories.len(), 2);
    assert!(histories
        .iter()
        .flat_map(|history| &history.features)
        .any(|feature| feature.name == "Q1" && feature.properties.len() == 254));
}
