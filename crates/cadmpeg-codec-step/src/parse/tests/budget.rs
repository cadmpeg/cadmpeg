// SPDX-License-Identifier: Apache-2.0
//! Part 21 anchor expansion bounds.

#![allow(clippy::unwrap_used)]

use std::fmt::Write as _;

fn expansion_refusal(source: &str) -> String {
    let error =
        crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner)
            .unwrap_err();
    let crate::parse::ParseError::Syntax { message, .. } = error else {
        panic!("anchor expansion must fail as malformed input: {error:?}");
    };
    message
}

#[test]
fn parser_bounds_exponential_anchor_expansion() {
    let mut anchors = String::from("<a0>=(1,1);\n");
    for index in 1..40 {
        writeln!(anchors, "<a{index}>=(<#a{}>,<#a{}>);", index - 1, index - 1)
            .expect("write anchor fixture");
    }
    let source = format!(
        "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'4;2');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;ANCHOR;{anchors}ENDSEC;DATA;#1=ITEM(<#a39>);ENDSEC;END-ISO-10303-21;"
    );
    assert_eq!(
        expansion_refusal(&source),
        "anchor expansion exceeds 1000000 value nodes"
    );
}

#[test]
fn parser_bounds_aggregate_anchor_materialization() {
    let mut anchors = String::from("<a0>=(1,1);\n");
    for index in 1..18 {
        writeln!(anchors, "<a{index}>=(<#a{}>,<#a{}>);", index - 1, index - 1)
            .expect("write anchor fixture");
    }
    let records = (1..=8).fold(String::new(), |mut records, id| {
        write!(records, "#{id}=ITEM(<#a17>);").expect("write anchor record fixture");
        records
    });
    let source = format!(
        "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'4;2');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;ANCHOR;{anchors}ENDSEC;DATA;{records}ENDSEC;END-ISO-10303-21;"
    );
    assert_eq!(
        expansion_refusal(&source),
        "anchor expansion exceeds 1000000 value nodes"
    );
}
