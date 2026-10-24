// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::geometry::SolvedCurveGeometry;
use cadmpeg_ir::source_fidelity::parts::PartState;
use cadmpeg_ir::NativeUnknownRecord;

#[test]
fn plain_line_values_are_returned_by_the_actual_binary_callback_and_source_flip() {
    let original = crate::test_support::binary_parts::plain_line_then_point(1, false);
    let changed = crate::test_support::binary_parts::plain_line_then_point(9, false);
    assert_eq!(original.len(), changed.len());
    assert_eq!(
        original
            .iter()
            .zip(&changed)
            .filter(|(a, b)| a != b)
            .count(),
        1
    );
    for (start_x, bytes) in [(1, original), (9, changed)] {
        let mut prefixed = b"prefix".to_vec();
        prefixed.extend_from_slice(&bytes);
        prefixed.extend_from_slice(b"suffix");
        let arena = DecodeArena::new();
        let (ctx, root) =
            DecodeContext::from_root_bytes(&prefixed, &arena, &DecodePolicy::service())
                .expect("actual caller source session");
        let source = root
            .child(6, 6 + bytes.len())
            .expect("actual nonzero window");
        let sections = crate::binary::parse_sections(source).expect("original physical sections");
        let (_, extent, body) = crate::binary::counted_record(sections.parameter, 0, "Line")
            .expect("original counted Line Parameter");
        let (normalized, _) =
            super::super::normalize(source, &ctx).expect("actual source callbacks");
        let records = &normalized.admission.parameters;
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].directory_sequence, 1);
        for (index, expected) in [110, start_x, 2, 3, 4, 5, 6].into_iter().enumerate() {
            assert_eq!(records[0].integer(index), Some(i64::from(expected)));
        }
        let scan = crate::card::scan(&normalized.bytes).expect("renderer cards");
        let (directory, quarantined) = crate::directory::parse_with_keys(
            &scan,
            normalized.admission.global.global_table(),
            &ctx,
            |key| normalized.identities.semantic_sequence(key),
        )
        .expect("same actual Directory grammar");
        assert!(quarantined.is_empty());
        let legacy =
            crate::parameter::assemble(&scan, &directory, [], &normalized.admission.global)
                .expect("unchanged grammar oracle");
        assert_eq!(records, &legacy.records);
        let captures = ctx
            .take_record_captures()
            .expect("one completed source capture drain");
        assert_eq!(captures.len(), 2);
        let line = captures
            .iter()
            .find(|capture| capture.id() == "iges:entity:directory#1")
            .expect("same actual native Line owner");
        assert_eq!(
            line.offset(),
            u64::try_from(extent.start()).expect("source offset")
        );
        assert_eq!(
            line.byte_len(),
            u64::try_from(extent.window().len()).expect("source length")
        );
        let integer_bytes = usize::from(sections.lengths.single_integer).div_ceil(8);
        let value_bytes = integer_bytes + 1;
        let expected_reads: Vec<_> = (0..6)
            .map(|index| {
                let start = body.start() + (2 + index) * value_bytes + 1;
                u64::try_from(start).expect("actual coordinate offset")
                    ..u64::try_from(start + integer_bytes).expect("actual coordinate end")
            })
            .collect();
        assert_eq!(
            line.reads(),
            &std::collections::BTreeMap::from([("file".to_owned(), expected_reads)])
        );
        assert!(
            line.offset() + line.byte_len() <= u64::try_from(source.end()).expect("source end")
        );
        let decoded = crate::test_support::decode(bytes);
        assert_eq!(decoded.ir().model.curves.len(), 1);
        assert_eq!(decoded.ir().model.edges.len(), 1);
        assert_eq!(decoded.ir().model.points.len(), 3);
        let Some(SolvedCurveGeometry::Line(line)) = decoded.ir().model.curves[0].geometry.solved()
        else {
            panic!("actual finite Line geometry");
        };
        assert_eq!(
            line.origin().get(),
            cadmpeg_ir::math::Point3::new(f64::from(start_x), 2.0, 3.0)
        );
        let native = decoded
            .ir()
            .native
            .namespace("iges")
            .expect("native source values");
        assert_eq!(native.arenas()["entities"].len(), 2);
        let line = &native.arenas()["entities"][0];
        assert_eq!(line.id(), "iges:entity:directory#1");
        let parameters = line
            .field("parameters")
            .expect("actual retained native values");
        let values: Vec<_> = parameters
            .as_array()
            .expect("native parameter list")
            .iter()
            .map(|token| token["value"]["value"].as_i64().expect("source integer"))
            .collect();
        assert_eq!(values, vec![110, i64::from(start_x), 2, 3, 4, 5, 6]);
        assert!(!decoded
            .report()
            .losses
            .iter()
            .any(|loss| loss.code.local_code() == "record.unreadable"));
    }
}

#[test]
fn an_invalid_line_coordinate_control_gives_up_the_rest_of_its_parameter_values_and_keeps_the_point_peer(
) {
    let original = crate::test_support::binary_parts::plain_line_then_point(1, false);
    let bytes = crate::test_support::binary_parts::plain_line_then_point(1, true);
    assert_eq!(original.len(), bytes.len());
    assert_eq!(
        original.iter().zip(&bytes).filter(|(a, b)| a != b).count(),
        1
    );
    // The one changed byte is the start X control byte. Its format 7 has no
    // width, so the group and the rest of the Line record, six 5-byte
    // integer groups, are one given-up `iges.binary_parameter_value`
    // (iges.md, Binary Parameter Data; #215 rule 1).
    let control = original
        .iter()
        .zip(&bytes)
        .position(|(a, b)| a != b)
        .expect("changed control byte");
    let given_up = control..control + 6 * 5;
    let baseline = crate::test_support::decode(original);
    let decoded = crate::test_support::decode(bytes.clone());
    assert!(decoded.ir().model.curves.is_empty());
    assert_eq!(decoded.ir().model.points.len(), 1);
    let peer = crate::ids::point(&crate::ids::Stem::directory(3_u32));
    assert_eq!(decoded.ir().model.points[0].id, peer);
    let expected = baseline
        .ir()
        .model
        .points
        .iter()
        .find(|point| point.id == peer)
        .expect("source peer");
    assert_eq!(
        serde_json::to_value(&decoded.ir().model.points[0]).expect("current peer"),
        serde_json::to_value(expected).expect("baseline peer")
    );
    let native = decoded.ir().native.namespace("iges").expect("native");
    let old = baseline
        .ir()
        .native
        .namespace("iges")
        .expect("baseline native");
    // #215 rule 1: the Line keeps its Directory fields, its entity type
    // and one `null` slot for the given-up start X; no later value is read.
    assert_eq!(native.arenas()["entities"].len(), 2);
    assert_eq!(
        crate::test_support::binary_parts::entity_without_padding(&native.arenas()["entities"][0]),
        crate::test_support::binary_parts::entity_with_given_up_rest(
            &old.arenas()["entities"][0],
            0,
            1
        )
    );
    assert_eq!(native.arenas()["entities"][1], old.arenas()["entities"][1]);
    assert!(native.arenas()["quarantined_directory_records"].is_empty());
    assert!(native.arenas()["quarantined_parameter_records"].is_empty());
    let unknowns = native
        .arena_as::<NativeUnknownRecord>("unknowns")
        .expect("unknowns");
    assert_eq!(unknowns.len(), 1);
    let fidelity = decoded.source_fidelity();
    assert!(!fidelity.parts().values().any(|part| {
        part.level().as_str() == "iges.binary_directory_record"
            && part.state() == &PartState::GivenUp {}
    }));
    let arena = DecodeArena::new();
    let (_ctx, source) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
        .expect("original source boundaries");
    let sections = crate::binary::parse_sections(source).expect("actual source sections");
    let (_, record, _) = crate::binary::counted_record(sections.parameter, 0, "source")
        .expect("actual counted record");
    assert_eq!(record.end(), given_up.end);
    let part = fidelity
        .parts()
        .values()
        .find(|part| part.state() == &PartState::GivenUp {})
        .expect("the failed source value");
    assert_eq!(part.level().as_str(), "iges.binary_parameter_value");
    assert_eq!(
        part.offset(),
        u64::try_from(given_up.start).expect("source offset")
    );
    assert_eq!(
        part.byte_len(),
        u64::try_from(given_up.len()).expect("source length")
    );
    assert_eq!(
        fidelity.parts()[part.parent().expect("record part")].state(),
        &PartState::Decoded {}
    );
    let home = unknowns
        .iter()
        .find(|unknown| fidelity.parts()[&unknown.id].offset() == part.offset())
        .expect("one exact home");
    assert_eq!(
        fidelity
            .retained_record(home.id.as_str())
            .expect("retained source")
            .data(),
        Some(&bytes[given_up.clone()])
    );
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
    assert!(errors[0].message.contains("iges.binary_parameter_value"));
    let stream = fidelity
        .parts()
        .values()
        .find(|part| part.state() == &PartState::GivenUp {})
        .expect("source")
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
    .expect("one home per actual failed counted source");
}
