// SPDX-License-Identifier: Apache-2.0
//! Exact report and class conservation for source bytes without typed owners.

use std::collections::BTreeSet;

pub(crate) fn assert_report(decoded: &cadmpeg_ir::codec::DecodeResult) {
    let expected = decoded
        .source_fidelity()
        .b1_spans()
        .iter()
        .flat_map(|(stream, coverage)| {
            coverage.spans().iter().map(move |span| {
                let range = span.range();
                (stream.clone(), range.start, range.end - range.start)
            })
        })
        .collect::<BTreeSet<_>>();
    let actual = decoded
        .report()
        .losses
        .iter()
        .map(|loss| {
            assert_eq!(loss.code.namespace(), "decode", "{loss:?}");
            assert_eq!(loss.code.local_code(), "source.bytes-undecoded", "{loss:?}");
            assert_eq!(loss.severity, cadmpeg_ir::report::Severity::Warning);
            let source = loss
                .provenance
                .as_ref()
                .expect("B1 has exact source provenance");
            assert_eq!(source.format(), "fcstd");
            (
                source
                    .stream_name()
                    .expect("B1 names its stream")
                    .to_owned(),
                source.offset,
                source.byte_len().expect("B1 names its length"),
            )
        })
        .collect::<BTreeSet<_>>();
    assert_eq!(
        decoded.report().losses.len(),
        actual.len(),
        "one loss per B1 span"
    );
    assert_eq!(
        actual, expected,
        "complete exact B1 report with no other losses"
    );
    crate::fidelity::b1::assert_classified_spans(decoded);
}
