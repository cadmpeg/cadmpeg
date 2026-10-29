// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

fn point_pair(first_x: i32, invalid_coordinate: bool) -> Vec<u8> {
    let mut bytes = crate::test_support::binary_parts::point_pair(false);
    let (control, value) = {
        let arena = DecodeArena::new();
        let (_ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
            .expect("actual point source");
        let sections = crate::binary::parse_sections(root).expect("actual sections");
        let (_, _, body) = crate::binary::counted_record(sections.parameter, 0, "Point")
            .expect("actual counted point");
        let mut prefix = crate::binary::parameter_prefix(body, sections.lengths)
            .expect("actual point navigation");
        assert_eq!(prefix.entity_type, 116);
        assert_eq!(
            prefix.stream.next().expect("first coordinate"),
            Some(crate::binary::BinaryValue::Integer(1))
        );
        let value = prefix.stream.value_extent.clone();
        assert_eq!(value.len(), first_x.to_be_bytes().len());
        (
            body.start() + value.start - 1,
            body.start() + value.start..body.start() + value.end,
        )
    };
    assert_eq!(bytes[control], 1);
    bytes[value].copy_from_slice(&first_x.to_be_bytes());
    if invalid_coordinate {
        bytes[control] = 7;
    }
    bytes
}

#[test]
fn original_point_coordinates_keep_exact_nonzero_windows_native_values_and_source_flip() {
    let original = point_pair(1, false);
    let changed = point_pair(9, false);
    assert_eq!(original.len(), changed.len());
    assert_eq!(
        original
            .iter()
            .zip(&changed)
            .filter(|(a, b)| a != b)
            .count(),
        1
    );
    for (x, bytes) in [(1, original), (9, changed)] {
        let mut enclosing = b"prefix!".to_vec();
        enclosing.extend_from_slice(&bytes);
        enclosing.extend_from_slice(b"suffix");
        let arena = DecodeArena::new();
        let (ctx, root) =
            DecodeContext::from_root_bytes(&enclosing, &arena, &DecodePolicy::service())
                .expect("actual caller source");
        let source = root
            .child(7, 7 + bytes.len())
            .expect("actual nonzero window");
        let sections = crate::binary::parse_sections(source).expect("original sections");
        crate::binary::counted_record(sections.parameter, 0, "Point")
            .expect("actual counted point");
        let (normalized, _) = super::super::normalize(source, &ctx).expect("source-owned points");
        let records = &normalized.admission.parameters;
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].directory_sequence, 1);
        for (index, value) in [116, x, 2, 3].into_iter().enumerate() {
            assert_eq!(records[0].integer(index), Some(i64::from(value)));
        }
        let scan = crate::card::scan(&normalized.bytes).expect("rendered grammar");
        let (directory, quarantined) = crate::directory::parse_with_keys(
            &scan,
            normalized.admission.global.global_table(),
            &ctx,
            |key| normalized.identities.semantic_sequence(key),
        )
        .expect("unchanged Directory grammar");
        assert!(quarantined.is_empty());
        let legacy =
            crate::parameter::assemble(&scan, &directory, [], &normalized.admission.global)
                .expect("unchanged Parameter grammar");
        assert_eq!(records, &legacy.records);
        let decoded = crate::test_support::decode(bytes);
        assert_eq!(decoded.ir().model.points.len(), 2);
        assert_eq!(
            decoded.ir().model.points[0].position().get(),
            cadmpeg_ir::math::Point3::new(f64::from(x), 2.0, 3.0)
        );
        let native = decoded
            .ir()
            .native
            .namespace("iges")
            .expect("native source points");
        assert_eq!(native.arenas()["entities"].len(), 2);
        let values = native.arenas()["entities"][0]
            .field("parameters")
            .expect("native parameters");
        let values: Vec<_> = values
            .as_array()
            .expect("tokens")
            .iter()
            .take(4)
            .map(|token| token["value"]["value"].as_i64().expect("actual integer"))
            .collect();
        assert_eq!(values, vec![116, i64::from(x), 2, 3]);
        assert!(!decoded
            .report()
            .losses
            .iter()
            .any(|loss| loss.code.local_code() == "record.unreadable"));
    }
}

#[test]
fn bad_point_coordinate_gives_up_the_rest_of_its_parameter_values_and_keeps_an_unchanged_peer() {
    let original = point_pair(1, false);
    let bytes = point_pair(1, true);
    assert_eq!(original.len(), bytes.len());
    assert_eq!(
        original.iter().zip(&bytes).filter(|(a, b)| a != b).count(),
        1
    );
    // The one changed byte is the X control byte. Its format 7 has no width,
    // so the group and the rest of the Point record (X, Y, Z and the display
    // symbol, four 5-byte groups) are one given-up
    // `iges.binary_parameter_value` (iges.md, Binary Parameter Data; #215
    // rule 1).
    let control = original
        .iter()
        .zip(&bytes)
        .position(|(a, b)| a != b)
        .expect("changed control byte");
    let given_up = control..control + 4 * 5;
    let baseline = crate::test_support::decode(original);
    let decoded = crate::test_support::decode(bytes.clone());
    assert_eq!(decoded.ir().model.points.len(), 1);
    let peer = crate::ids::point(&crate::ids::Stem::directory(3_u32));
    assert_eq!(decoded.ir().model.points[0].id, peer);
    assert_eq!(
        serde_json::to_value(&decoded.ir().model.points[0]).expect("peer"),
        serde_json::to_value(&baseline.ir().model.points[1]).expect("original peer")
    );
    let native = decoded.ir().native.namespace("iges").expect("native");
    let old = baseline.ir().native.namespace("iges").expect("baseline");
    // #215 rule 1: the Point keeps its Directory fields, its entity type and
    // one `null` slot for the given-up X; no later value is read.
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
    let fidelity = decoded.source_fidelity();
    let arena = DecodeArena::new();
    let (_ctx, source) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
        .expect("original source");
    let sections = crate::binary::parse_sections(source).expect("actual sections");
    let (_, record, _) =
        crate::binary::counted_record(sections.parameter, 0, "source").expect("actual record");
    assert_eq!(record.end(), given_up.end);
    let parts = crate::test_support::given_up_parts(fidelity);
    assert_eq!(parts.len(), 1);
    let part = parts[0];
    assert!(!part.untyped().is_empty());
    assert_eq!(
        part.offset(),
        u64::try_from(given_up.start).expect("source offset")
    );
    assert_eq!(
        part.byte_len(),
        u64::try_from(given_up.len()).expect("source size")
    );
    assert_eq!(part.data(), Some(&bytes[given_up.clone()]));
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
    assert!(!errors
        .iter()
        .any(|loss| loss.message.contains("iges.binary_directory_record")));
}

#[test]
fn point_source_repeat_rolls_back_at_its_real_boundary_and_keeps_the_peer_and_foreign_guard() {
    let lengths = crate::binary::PrimitiveLengths {
        single_integer: 8,
        double_integer: 8,
        single_exponent: 8,
        single_fraction: 23,
        double_exponent: 11,
        double_fraction: 52,
    };
    let encode = |values: &[i64]| {
        let mut bytes = vec![0x80 | ((6_u8 - 1) << 3) | 1];
        for value in values {
            let magnitude = u8::try_from(value.unsigned_abs()).expect("eight-bit test magnitude");
            assert!(magnitude < 0x80);
            bytes.push((u8::from(*value < 0) << 7) | magnitude);
        }
        bytes
    };
    let bad = encode(&[116, 1, 1]);
    let good = encode(&[116, 1, 1, 2, 3, 0]);
    let mut bytes = b"pre".to_vec();
    bytes.extend_from_slice(&bad);
    bytes.extend_from_slice(&good);
    bytes.extend_from_slice(b"suffix");
    let arena = DecodeArena::new();
    let (_ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
        .expect("actual source");
    let consume = |source: cadmpeg_core::decode::View<'_>| {
        let mut stream = crate::binary::ValueStream::new(source.window(), lengths);
        let mut values = Vec::new();
        while let Some(value) = stream.next()? {
            values.push(value);
        }
        stream.finish()?;
        Ok::<_, cadmpeg_core::CodecError>(values)
    };
    let failed = root.child(3, 3 + bad.len()).expect("actual failed body");
    let error = consume(failed).expect_err("cannot read into actual peer");
    assert!(error.to_string().contains("primitive is truncated"));
    let peer = root
        .child(3 + bad.len(), 3 + bad.len() + good.len())
        .expect("actual peer body");
    assert_eq!(
        consume(peer).expect("unchanged peer"),
        [116, 1, 1, 2, 3, 0].map(crate::binary::BinaryValue::Integer)
    );
}
