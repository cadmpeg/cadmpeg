// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::codec::Codec as _;
use cadmpeg_ir::source_fidelity::parts::PartState;
use cadmpeg_ir::NativeUnknownRecord;

#[test]
fn an_unresolved_line_coordinate_reference_gives_up_only_its_parameter_record() {
    let baseline = crate::test_support::decode(
        crate::test_support::binary_parts::line_with_encoded_coordinate_then_point(false),
    );
    for broken in [false, true] {
        let bytes =
            crate::test_support::binary_parts::line_with_encoded_coordinate_then_point(broken);
        let arena = DecodeArena::new();
        let (ctx, source) =
            DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
                .expect("source-owned session");
        let actual = super::super::normalize(source, &ctx);
        let original_arena = DecodeArena::new();
        let (original_ctx, original_source) =
            DecodeContext::from_root_bytes(&bytes, &original_arena, &DecodePolicy::service())
                .expect("independent unchanged legacy grammar oracle");
        let original = crate::binary::normalize(original_source, &original_ctx);
        if broken {
            // #215 §4.5 and #222: an unresolved reference is not a whole-file
            // refusal. It gives up the nearest part the decoder runs that
            // holds the value: the Line's Parameter Data record.
            let (actual, _) = actual.expect("an unresolved reference gives up one part");
            assert!(actual.admission.parameters.is_some());
            assert!(original
                .expect_err("the pre-handoff normalizer has no part to give up")
                .to_string()
                .contains("Binary Directory reference has no surviving source target"));
            let decoded = crate::IgesCodec
                .decode(
                    &mut std::io::Cursor::new(bytes.clone()),
                    &cadmpeg_ir::codec::DecodeOptions::default(),
                )
                .expect("the decode succeeds");
            let sections = crate::binary::parse_sections(source).expect("original sections");
            let (_, line, _) = crate::binary::counted_record(sections.parameter, 0, "Line")
                .expect("original Line Parameter record");
            let fidelity = decoded.source_fidelity();
            let given_up: Vec<_> = fidelity
                .parts()
                .values()
                .filter(|part| part.state() == &PartState::GivenUp {})
                .collect();
            assert_eq!(given_up.len(), 1);
            assert_eq!(given_up[0].level().as_str(), "iges.binary_parameter_record");
            assert_eq!(
                given_up[0].offset(),
                u64::try_from(line.start()).expect("source offset")
            );
            assert_eq!(
                given_up[0].byte_len(),
                u64::try_from(line.window().len()).expect("source length")
            );
            let unreadable: Vec<_> = decoded
                .report()
                .losses
                .iter()
                .filter(|loss| loss.code.local_code() == "record.unreadable")
                .collect();
            assert_eq!(unreadable.len(), 1);
            assert_eq!(unreadable[0].severity, cadmpeg_ir::report::Severity::Error);
            assert!(unreadable[0]
                .message
                .contains("iges.binary_parameter_record"));
            assert!(unreadable[0]
                .message
                .contains(&format!("offset={}", line.start())));
            // The Point peer is unchanged; the Line keeps its Directory record.
            assert!(decoded.ir().model.curves.is_empty());
            let peer = crate::ids::point(&crate::ids::Stem::directory(3_u32));
            assert_eq!(decoded.ir().model.points.len(), 1);
            assert_eq!(
                serde_json::to_value(&decoded.ir().model.points[0]).expect("peer point"),
                serde_json::to_value(
                    baseline
                        .ir()
                        .model
                        .points
                        .iter()
                        .find(|point| point.id == peer)
                        .expect("baseline peer")
                )
                .expect("baseline peer point")
            );
            let native = decoded.ir().native.namespace("iges").expect("native");
            let old = baseline.ir().native.namespace("iges").expect("baseline");
            assert_eq!(native.arenas()["entities"].len(), 2);
            assert_eq!(
                serde_json::to_value(&native.arenas()["entities"][0]).expect("Line entity"),
                crate::test_support::binary_parts::entity_without_parameter_data(
                    &old.arenas()["entities"][0]
                )
            );
            assert_eq!(native.arenas()["entities"][1], old.arenas()["entities"][1]);
        } else {
            let (actual, _) = actual.expect("valid source reference keeps the legacy route");
            assert!(actual.admission.parameters.is_none());
            assert_eq!(actual.bytes, original.expect("legacy normalization"));
            let decoded = crate::test_support::decode(bytes.clone());
            assert_eq!(decoded.ir().model.curves.len(), 1);
            assert_eq!(decoded.ir().model.points.len(), 3);
            assert_eq!(
                decoded
                    .ir()
                    .native
                    .namespace("iges")
                    .expect("native")
                    .arenas()["entities"]
                    .len(),
                2
            );
        }
    }
}

#[test]
fn a_required_point_symbol_failure_gives_up_only_its_parameter_record_and_keeps_an_exact_peer() {
    let original = crate::test_support::binary_parts::broken_parameter_reference(false);
    let bytes = crate::test_support::binary_parts::broken_parameter_reference(true);
    assert_eq!(original.len(), bytes.len());
    let baseline = crate::test_support::decode(original);
    let decoded = crate::test_support::decode(bytes.clone());
    assert_eq!(decoded.ir().model.points.len(), 1);
    assert_eq!(decoded.ir().model.vertices.len(), 1);
    let peer = crate::ids::point(&crate::ids::Stem::directory(3_u32));
    assert_eq!(decoded.ir().model.points[0].id, peer);
    let expected_point = baseline
        .ir()
        .model
        .points
        .iter()
        .find(|point| point.id == peer)
        .expect("the original independent peer");
    assert_eq!(
        serde_json::to_value(&decoded.ir().model.points[0]).expect("actual point"),
        serde_json::to_value(expected_point).expect("expected point")
    );
    let native = decoded.ir().native.namespace("iges").expect("native");
    let old = baseline
        .ir()
        .native
        .namespace("iges")
        .expect("original native");
    // #215 rule 1: the Point's Directory record keeps its fields; only the
    // Parameter Data record that holds the unresolved symbol is given up.
    assert_eq!(native.arenas()["entities"].len(), 2);
    assert_eq!(
        serde_json::to_value(&native.arenas()["entities"][0]).expect("damaged Point entity"),
        crate::test_support::binary_parts::entity_without_parameter_data(
            &old.arenas()["entities"][0]
        )
    );
    assert_eq!(native.arenas()["entities"][1], old.arenas()["entities"][1]);
    assert!(native.arenas()["quarantined_parameter_records"].is_empty());
    assert!(native.arenas()["quarantined_directory_records"].is_empty());
    let unknowns = native
        .arena_as::<NativeUnknownRecord>("unknowns")
        .expect("unknowns");
    assert_eq!(unknowns.len(), 1);
    let fidelity = decoded.source_fidelity();
    let arena = DecodeArena::new();
    let (_ctx, source) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
        .expect("independent physical source envelope");
    let sections = crate::binary::parse_sections(source).expect("original section boundaries");
    assert!(!fidelity.parts().values().any(|part| {
        part.level().as_str() == "iges.binary_directory_record"
            && part.state() == &PartState::GivenUp {}
    }));
    for (level, section) in [("iges.binary_parameter_record", sections.parameter)] {
        let (_, extent, _) = crate::binary::counted_record(section, 0, "source")
            .expect("original actual counted source record");
        let failed: Vec<_> = fidelity
            .parts()
            .values()
            .filter(|part| part.level().as_str() == level && part.state() == &PartState::GivenUp {})
            .collect();
        assert_eq!(failed.len(), 1);
        let failed = failed[0];
        assert_eq!(
            failed.offset(),
            u64::try_from(extent.start()).expect("source offset")
        );
        assert_eq!(
            failed.byte_len(),
            u64::try_from(extent.window().len()).expect("source length")
        );
        let home = unknowns
            .iter()
            .find(|unknown| fidelity.parts()[&unknown.id].offset() == failed.offset())
            .expect("the single failure home at this actual boundary");
        assert_eq!(
            fidelity
                .retained_record(home.id.as_str())
                .expect("source retention")
                .data(),
            Some(extent.window())
        );
    }
    let errors: Vec<_> = decoded
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code.local_code() == "record.unreadable")
        .collect();
    assert_eq!(errors.len(), 1);
    assert!(errors
        .iter()
        .all(|loss| loss.severity == cadmpeg_ir::report::Severity::Error));
    let stream = fidelity
        .parts()
        .values()
        .find(|part| part.state() == &PartState::GivenUp {})
        .expect("a genuine failed source")
        .stream();
    cadmpeg_test_support::contract::parts::check_one_home(
        cadmpeg_ir::CodecFormat::Iges,
        &decoded.ir().native,
        fidelity,
        &decoded.report().losses,
        stream,
        |requested, range| {
            if requested != stream {
                return None;
            }
            bytes.get(usize::try_from(range.start).ok()?..usize::try_from(range.end).ok()?)
        },
    )
    .expect("required dependency failures have exact one-home source conservation");
}
