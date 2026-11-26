// SPDX-License-Identifier: Apache-2.0
//! Part 21 anchor expansion bounds.

use std::collections::HashSet;
use std::fmt::Write as _;
use std::ops::RangeInclusive;
use std::sync::LazyLock;

use crate::parse::{Exchange, ParseDiagnosticKind};

const BOUND: &str = "anchor expansion exceeds 1000000 value nodes";

/// Instances after `#3` that each name only the bounded anchor `a23`.
const REPEATED: RangeInclusive<u64> = 4..=2_003;

/// Anchors `a0` to `a{last}`, each holding two copies of the one before.
fn doubling_anchors(last: usize) -> String {
    let mut anchors = String::from("<a0>=(1,1);");
    for index in 1..=last {
        write!(anchors, "<a{index}>=(<#a{}>,<#a{}>);", index - 1, index - 1)
            .expect("write anchor fixture");
    }
    anchors
}

/// One parse of the doubling anchors to `a23` and the instances that name
/// them, shared by the tests in this module.
struct Doubling {
    source: String,
    exchange: Exchange,
    /// Messages of the `ExpansionBounded` diagnostics.
    bounded: HashSet<String>,
}

static DOUBLING: LazyLock<Doubling> = LazyLock::new(|| {
    let mut data = String::from("#1=ITEM(<#a23>);#2=ITEM(#1);#3=ITEM(<#a0>);");
    for id in REPEATED {
        write!(data, "#{id}=ITEM(<#a23>);").expect("write anchor record fixture");
    }
    let source = format!(
        "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'4;2');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;ANCHOR;{}ENDSEC;DATA;{data}ENDSEC;END-ISO-10303-21;",
        doubling_anchors(23)
    );
    let (exchange, diagnostics) =
        crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse)
            .expect("a bounded expansion omits items instead of refusing the decode");
    let bounded = diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.kind == ParseDiagnosticKind::ExpansionBounded)
        .map(|diagnostic| diagnostic.message.clone())
        .collect();
    Doubling {
        source,
        exchange,
        bounded,
    }
});

fn bounded(subject: &str) -> bool {
    DOUBLING.bounded.contains(&format!(
        "{subject} omitted: {BOUND}; exact source retained"
    ))
}

/// The records dropped with the given-up statement `text`.
fn given_up_with(text: &str) -> Vec<String> {
    let Doubling {
        source, exchange, ..
    } = &*DOUBLING;
    exchange
        .given_up()
        .iter()
        .find(|statement| &source[statement.span.clone()] == text)
        .map(|statement| statement.dropped.clone())
        .unwrap_or_default()
}

fn gives_up(kind: &str, text: &str) -> bool {
    let Doubling {
        source, exchange, ..
    } = &*DOUBLING;
    exchange
        .given_up()
        .iter()
        .any(|statement| statement.kind == kind && &source[statement.span.clone()] == text)
}

#[test]
fn exponential_anchor_expansion_omits_the_bounded_items() {
    let Doubling { exchange, .. } = &*DOUBLING;

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
        assert!(bounded(&format!("anchor <a{index}>")), "a{index}");
    }
    assert!(bounded("instance #1"));
    // Each bounded anchor and instance is a part of its own.
    for (kind, statement) in [
        ("anchor_record", "<a17>=(<#a16>,<#a16>);"),
        ("data_record", "#1=ITEM(<#a23>);"),
    ] {
        assert!(gives_up(kind, statement), "{statement}");
    }
    assert!(!exchange.records().contains_key(&2));
    assert!(given_up_with("#1=ITEM(<#a23>);").contains(&String::from("#2")));
    assert!(exchange.records().contains_key(&3));
}

#[test]
fn repeated_instances_naming_a_bounded_anchor_are_omitted() {
    let records = DOUBLING.exchange.records();
    for id in REPEATED {
        assert!(!records.contains_key(&id), "#{id}");
        assert!(bounded(&format!("instance #{id}")), "#{id}");
    }
}
