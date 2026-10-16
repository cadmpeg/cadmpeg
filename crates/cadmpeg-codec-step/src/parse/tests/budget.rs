// SPDX-License-Identifier: Apache-2.0
//! Part 21 anchor expansion bounds.

use std::fmt::Write as _;

use crate::parse::{Exchange, ParseDiagnostic, ParseDiagnosticKind};

const BOUND: &str = "anchor expansion exceeds 1000000 value nodes";

fn source(anchors: &str, data: &str) -> String {
    format!(
        "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'4;2');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;ANCHOR;{anchors}ENDSEC;DATA;{data}ENDSEC;END-ISO-10303-21;"
    )
}

fn parse(source: &str) -> (Exchange, Vec<ParseDiagnostic>) {
    crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse)
        .expect("a bounded expansion omits items instead of refusing the decode")
}

/// Anchors `a0` to `a{last}`, each holding two copies of the one before.
fn doubling_anchors(last: usize) -> String {
    let mut anchors = String::from("<a0>=(1,1);");
    for index in 1..=last {
        write!(anchors, "<a{index}>=(<#a{}>,<#a{}>);", index - 1, index - 1)
            .expect("write anchor fixture");
    }
    anchors
}

fn bounded(diagnostics: &[ParseDiagnostic], subject: &str) -> bool {
    let expected = format!("{subject} omitted: {BOUND}; exact source retained");
    diagnostics.iter().any(|diagnostic| {
        diagnostic.kind == ParseDiagnosticKind::ExpansionBounded && diagnostic.message == expected
    })
}

fn retains_source(exchange: &Exchange, source: &str, text: &str) -> bool {
    exchange
        .omitted_spans()
        .iter()
        .any(|span| &source[span.clone()] == text)
}

#[test]
fn exponential_anchor_expansion_omits_the_bounded_items() {
    let source = source(
        &doubling_anchors(23),
        "#1=ITEM(<#a23>);#2=ITEM(#1);#3=ITEM(<#a0>);",
    );
    let (exchange, diagnostics) = parse(&source);

    let kept = exchange
        .anchors()
        .iter()
        .map(|anchor| anchor.name.as_str())
        .collect::<Vec<_>>();
    let expected = (0..=16)
        .map(|index| format!("a{index}"))
        .collect::<Vec<_>>();
    assert_eq!(kept, expected);
    for index in 17..=23 {
        assert!(
            bounded(&diagnostics, &format!("anchor <a{index}>")),
            "a{index}"
        );
    }
    assert!(retains_source(&exchange, &source, "<a17>=(<#a16>,<#a16>);"));

    assert!(bounded(&diagnostics, "instance #1"));
    assert!(retains_source(&exchange, &source, "#1=ITEM(<#a23>);"));
    assert!(!exchange.records().contains_key(&2));
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.kind == ParseDiagnosticKind::RecordOmitted && diagnostic.message.contains("#2")
    }));
    assert!(exchange.records().contains_key(&3));
}

#[test]
fn aggregate_anchor_materialization_omits_the_records_past_the_total() {
    let records = (1..=8).fold(String::new(), |mut records, id| {
        write!(records, "#{id}=ITEM(<#a15>);").expect("write anchor record fixture");
        records
    });
    let (exchange, diagnostics) = parse(&source(&doubling_anchors(15), &records));

    assert_eq!(exchange.anchors().len(), 16);
    assert_eq!(
        exchange.records().keys().copied().collect::<Vec<_>>(),
        [1, 2, 3, 4, 5]
    );
    for id in 6..=8 {
        assert!(bounded(&diagnostics, &format!("instance #{id}")), "#{id}");
    }
}
