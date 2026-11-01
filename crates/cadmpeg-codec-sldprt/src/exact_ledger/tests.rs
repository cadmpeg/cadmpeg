// SPDX-License-Identifier: Apache-2.0
//! Ledger contracts over synthesized Parasolid bodies: no real file bytes.

use super::{ExactReadLedger, ExactReadSpan, EXACT_READ_LEDGER_VERSION};
use crate::test_support::parasolid as syn;

fn collect(body: &[u8]) -> ExactReadLedger {
    ExactReadLedger::collect("", body)
}

fn span_bytes<'a>(body: &'a [u8], span: &ExactReadSpan) -> &'a [u8] {
    body.get(span.offset..span.offset + span.length)
        .expect("ledger span stays in bounds")
}

fn assert_ledger_sound(body: &[u8], ledger: &ExactReadLedger) {
    assert_eq!(ledger.version, EXACT_READ_LEDGER_VERSION);
    let mut previous = None;
    for span in &ledger.spans {
        assert!(span.length > 0);
        if let Some((offset, length)) = previous {
            assert!(
                (span.offset, span.length) >= (offset, length),
                "spans stay ordered"
            );
        }
        previous = Some((span.offset, span.length));
        assert_eq!(
            span.sha256,
            cadmpeg_ir::hash::sha256(span_bytes(body, span)),
            "span digest matches its bytes"
        );
        assert_eq!(span.sha256_hex().len(), 64);
    }
}

fn tags(ledger: &ExactReadLedger) -> std::collections::BTreeSet<u8> {
    ledger.spans.iter().map(|span| span.tag).collect()
}

fn spans_with(ledger: &ExactReadLedger, tag: u8) -> Vec<&ExactReadSpan> {
    ledger.spans.iter().filter(|span| span.tag == tag).collect()
}

#[test]
fn ledger_versions_its_contract_and_collects_nothing_from_empty_input() {
    assert_eq!(EXACT_READ_LEDGER_VERSION, 1);
    let ledger = collect(&[]);
    assert_eq!(ledger.version, 1);
    assert!(ledger.is_empty());
    assert_eq!(ledger.len(), 0);
}

#[test]
fn triangle_topology_spans_name_every_record_family() {
    let body = syn::triangle_body();
    let ledger = collect(&body);
    assert_ledger_sound(&body, &ledger);
    for tag in [0x0e, 0x0f, 0x10, 0x11, 0x12, 0x1d] {
        assert!(
            tags(&ledger).contains(&tag),
            "missing topology tag {tag:#04x}"
        );
    }
    let records: std::collections::BTreeSet<u16> =
        ledger.spans.iter().map(|span| span.record).collect();
    for attr in [10, 20, 30, 31, 32, 40, 41, 42, 50, 51, 52, 60, 61, 62, 100] {
        assert!(records.contains(&attr), "missing record {attr}");
    }
}

#[test]
fn tripled_body_spans_cover_tripled_framings() {
    let body = syn::tripled_triangle_body();
    let ledger = collect(&body);
    assert_ledger_sound(&body, &ledger);
    assert_eq!(spans_with(&ledger, 0x11).len(), 3);
    assert_eq!(spans_with(&ledger, 0x12).len(), 3);
    assert_eq!(spans_with(&ledger, 0x1d).len(), 3);
    assert_eq!(spans_with(&ledger, 0x10).len(), 3);
}

#[test]
fn analytic_carrier_spans_name_tag_record_and_extent() {
    let mut body = syn::line_carrier(70, [0.0, 0.0, 0.0], [1.0, 0.0, 0.0]);
    body.extend(syn::circle_carrier(
        71,
        [0.0, 0.0, 0.0],
        [0.0, 0.0, 1.0],
        0.002,
    ));
    body.extend(syn::plane_carrier(
        72,
        [0.0, 0.0, 0.0],
        [0.0, 0.0, 1.0],
        [1.0, 0.0, 0.0],
    ));
    let ledger = collect(&body);
    assert_ledger_sound(&body, &ledger);
    for (tag, record) in [(0x1e, 70), (0x1f, 71), (0x32, 72)] {
        let found: Vec<&ExactReadSpan> = ledger
            .spans
            .iter()
            .filter(|span| span.tag == tag && span.record == record)
            .collect();
        assert_eq!(found.len(), 1, "carrier {record} with tag {tag:#04x}");
    }
}

#[test]
fn nurbs_curve_spans_cover_wrapper_descriptor_and_semantic_arrays() {
    let body = syn::nurbs_curve_carrier(170, 171);
    let ledger = collect(&body);
    assert_ledger_sound(&body, &ledger);
    assert_eq!(spans_with(&ledger, 0x86).len(), 1);
    assert_eq!(spans_with(&ledger, 0x88).len(), 1);
    let control = spans_with(&ledger, 0x2d);
    assert_eq!(control.len(), 1);
    assert_eq!(control[0].record, 172);
    assert_eq!(control[0].length, 9 + 9 * 8);
    let knots = spans_with(&ledger, 0x80);
    assert_eq!(knots.len(), 1);
    assert_eq!(knots[0].record, 174);
    assert_eq!(knots[0].length, 9 + 2 * 8);
    let mults = spans_with(&ledger, 0x7f);
    assert_eq!(mults.len(), 1);
    assert_eq!(mults[0].record, 173);
    assert_eq!(mults[0].length, 9 + 2 * 2);
}

#[test]
fn surface_knot_spans_exclude_reserved_physical_slots() {
    let body = syn::nurbs_surface_carrier_with_terminal_knot_slot(180, 181, 10);
    let ledger = collect(&body);
    assert_ledger_sound(&body, &ledger);
    assert_eq!(spans_with(&ledger, 0x7c).len(), 1);
    assert_eq!(spans_with(&ledger, 0x7e).len(), 1);
    let knots: Vec<&ExactReadSpan> = spans_with(&ledger, 0x80)
        .into_iter()
        .filter(|span| span.record == 186)
        .collect();
    assert_eq!(knots.len(), 1);
    assert_eq!(knots[0].length, 9 + 2 * 8);
    let values: Vec<f64> = span_bytes(&body, knots[0])[9..]
        .chunks_exact(8)
        .map(|slot| f64::from_be_bytes(slot.try_into().expect("f64 slot")))
        .collect();
    assert!(values.iter().all(|value| value.is_finite()));
    let mults: Vec<&ExactReadSpan> = spans_with(&ledger, 0x7f)
        .into_iter()
        .filter(|span| span.record == 184)
        .collect();
    assert_eq!(mults.len(), 1);
    assert_eq!(mults[0].length, 9 + 2 * 2);
}

#[test]
fn compact_array_spans_use_tag_zero_and_semantic_lengths() {
    let body = syn::compact_counted_nurbs_surface_carrier(180, 181, 10);
    let ledger = collect(&body);
    assert_ledger_sound(&body, &ledger);
    let compact: Vec<&ExactReadSpan> = ledger.spans.iter().filter(|span| span.tag == 0).collect();
    assert!(!compact.is_empty(), "compact arrays name tag zero");
    let control = compact
        .iter()
        .find(|span| span.record == 182)
        .expect("compact control span");
    assert_eq!(control.length, 4 + 12 * 8);
    let mults = compact
        .iter()
        .find(|span| span.record == 184)
        .expect("compact multiplicity span");
    assert_eq!(mults.length, 4 + 2 * 2);
    let knots = compact
        .iter()
        .find(|span| span.record == 186)
        .expect("compact knot span");
    assert_eq!(knots.length, 4 + 2 * 8);
    let values: Vec<f64> = span_bytes(&body, knots)[4..]
        .chunks_exact(8)
        .map(|slot| f64::from_be_bytes(slot.try_into().expect("f64 slot")))
        .collect();
    assert!(values.iter().all(|value| value.is_finite()));
}

#[test]
fn prefixed_edge_body_spans_stay_sound() {
    let body = syn::prefixed_edge_triangle_body();
    let ledger = collect(&body);
    assert_ledger_sound(&body, &ledger);
    assert!(!ledger.is_empty());
}

#[test]
fn combined_body_spans_cover_topology_analytics_and_splines() {
    let body = syn::nurbs_triangle_body();
    let ledger = collect(&body);
    assert_ledger_sound(&body, &ledger);
    for tag in [0x0e, 0x0f, 0x10, 0x11, 0x12, 0x1d] {
        assert!(
            tags(&ledger).contains(&tag),
            "missing topology tag {tag:#04x}"
        );
    }
    assert!(tags(&ledger).contains(&0x32));
    for tag in [0x86, 0x88, 0x7c, 0x7e] {
        assert!(
            tags(&ledger).contains(&tag),
            "missing spline tag {tag:#04x}"
        );
    }
}

#[test]
fn decode_with_exact_read_ledger_matches_plain_decode() {
    use std::io::Cursor;

    use cadmpeg_ir::codec::{Codec, DecodeOptions};

    use crate::test_support::container::sldprt_with_body;
    use crate::SldprtCodec;

    let source = sldprt_with_body(&syn::triangle_body());
    let plain = SldprtCodec
        .decode(&mut Cursor::new(source.clone()), &DecodeOptions::default())
        .expect("synthesized SLDPRT should decode");
    let (result, ledgers) = SldprtCodec
        .decode_with_exact_read_ledger(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("ledger decode should succeed");
    assert_eq!(result.ir().model.faces.len(), plain.ir().model.faces.len());
    assert_eq!(result.report().losses.len(), plain.report().losses.len());
    assert!(!ledgers.is_empty());
    for ledger in &ledgers {
        assert_eq!(ledger.version, EXACT_READ_LEDGER_VERSION);
        assert!(!ledger.stream.is_empty());
        assert!(!ledger.spans.is_empty());
    }
}

#[test]
fn a_plain_stream_face_framed_on_a_stored_tolerance_has_a_span() {
    // The typed FACE 10 supersedes the bridge FACE 10.
    let mut body = syn::triangle_body();
    let sentinel = [0xc2, 0xbc, 0x92, 0x8f, 0x99, 0x6e, 0x00, 0x00];
    let bridge = (0..body.len() - 18)
        .find(|&at| body[at..at + 2] == [0x00, 0x0e] && body[at + 10..at + 18] == sentinel)
        .unwrap();
    let typed = (0..body.len() - 19)
        .find(|&at| body[at..at + 3] == [0x00, 0x0e, 0xff] && body[at + 11..at + 19] == sentinel)
        .unwrap();
    body[bridge + 10..bridge + 18].copy_from_slice(&1.0e-8f64.to_be_bytes());
    body[typed + 11..typed + 19].copy_from_slice(&1.0e-8f64.to_be_bytes());

    let plain = ExactReadLedger::collect_plain("", &body);
    assert_ledger_sound(&body, &plain);
    assert!(plain
        .spans
        .iter()
        .any(|span| span.tag == 0x0e && span.offset == typed));
    assert!(!collect(&body).spans.iter().any(|span| span.tag == 0x0e));
}
