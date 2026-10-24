// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::source_fidelity::parts::PartState;

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
        let (_, extent, body) = crate::binary::counted_record(sections.parameter, 0, "Point")
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
        let captures = ctx.take_record_captures().expect("one actual drain");
        assert_eq!(captures.len(), 2);
        let capture = captures
            .iter()
            .find(|capture| capture.id() == "iges:entity:directory#1")
            .expect("actual first point owner");
        assert_eq!(
            capture.offset(),
            u64::try_from(extent.start()).expect("source offset")
        );
        assert_eq!(
            capture.byte_len(),
            u64::try_from(extent.window().len()).expect("source extent")
        );
        let primitive = usize::from(sections.lengths.single_integer).div_ceil(8);
        let expected: Vec<_> = (2..5)
            .map(|field| {
                let start = body.start() + field * (primitive + 1) + 1;
                u64::try_from(start).expect("source field")
                    ..u64::try_from(start + primitive).expect("source end")
            })
            .collect();
        assert_eq!(
            capture.reads(),
            &std::collections::BTreeMap::from([("file".to_owned(), expected)])
        );
        assert!(
            capture.offset() + capture.byte_len()
                <= u64::try_from(source.end()).expect("source end")
        );
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
fn bad_point_coordinate_gives_up_only_its_parameter_record_and_keeps_an_unchanged_peer() {
    let original = point_pair(1, false);
    let bytes = point_pair(1, true);
    assert_eq!(original.len(), bytes.len());
    assert_eq!(
        original.iter().zip(&bytes).filter(|(a, b)| a != b).count(),
        1
    );
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
    // #215 rule 1: the Directory record keeps its fields; only its Parameter
    // Data record is given up.
    assert_eq!(native.arenas()["entities"].len(), 2);
    assert_eq!(
        serde_json::to_value(&native.arenas()["entities"][0]).expect("damaged entity"),
        crate::test_support::binary_parts::entity_without_parameter_data(
            &old.arenas()["entities"][0]
        )
    );
    assert_eq!(native.arenas()["entities"][1], old.arenas()["entities"][1]);
    let unknowns = decoded.ir().native_unknowns("iges").expect("unknowns");
    assert_eq!(unknowns.len(), 1);
    let fidelity = decoded.source_fidelity();
    assert!(!fidelity.parts().values().any(|part| {
        part.level().as_str() == "iges.binary_directory_record"
            && part.state() == &PartState::GivenUp {}
    }));
    let arena = DecodeArena::new();
    let (_ctx, source) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
        .expect("original source");
    let sections = crate::binary::parse_sections(source).expect("actual sections");
    for (level, section) in [("iges.binary_parameter_record", sections.parameter)] {
        let (_, extent, _) =
            crate::binary::counted_record(section, 0, "source").expect("actual failed extent");
        let part = fidelity
            .parts()
            .values()
            .find(|part| part.level().as_str() == level && part.state() == &PartState::GivenUp {})
            .expect("owning failed part");
        assert_eq!(
            part.offset(),
            u64::try_from(extent.start()).expect("source offset")
        );
        assert_eq!(
            part.byte_len(),
            u64::try_from(extent.window().len()).expect("source size")
        );
        let home = unknowns
            .iter()
            .find(|unknown| fidelity.parts()[&unknown.id].offset() == part.offset())
            .expect("exact failed home");
        assert_eq!(
            fidelity
                .retained_record(home.id.as_str())
                .expect("source bytes")
                .data(),
            Some(extent.window())
        );
        assert!(fidelity.record_layout()[home.id.as_str()]
            .reads()
            .is_empty());
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
    .expect("one actual home per failed part");
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
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
        .expect("actual source");
    let consume = |source| {
        let mut stream = crate::binary::ValueStream::source_point(source, lengths);
        let mut values = Vec::new();
        while let Some(value) = stream.next()? {
            values.push(value);
        }
        stream.finish()?;
        Ok::<_, cadmpeg_core::CodecError>(values)
    };
    let failed = root.child(3, 3 + bad.len()).expect("actual failed body");
    let error = ctx
        .record("iges:entity:directory#1", failed, || consume(failed))
        .expect_err("cannot read into actual peer");
    assert!(error.to_string().contains("primitive is truncated"));
    let peer = root
        .child(3 + bad.len(), 3 + bad.len() + good.len())
        .expect("actual peer body");
    assert_eq!(
        ctx.record("iges:entity:directory#3", peer, || consume(peer))
            .expect("unchanged peer"),
        [116, 1, 1, 2, 3, 0].map(crate::binary::BinaryValue::Integer)
    );
    let foreign_arena = DecodeArena::new();
    let (other, _) =
        DecodeContext::from_root_bytes(&bytes, &foreign_arena, &DecodePolicy::service())
            .expect("independent session");
    assert!(other
        .record("iges:entity:directory#5", peer, || consume(peer))
        .is_err());
    let captures = ctx
        .take_record_captures()
        .expect("one drain after both actual windows");
    assert_eq!(captures.len(), 1);
    assert_eq!(captures[0].id(), "iges:entity:directory#3");
    let start = u64::try_from(peer.start() + 3).expect("actual first coordinate");
    assert_eq!(
        captures[0].reads(),
        &std::collections::BTreeMap::from([("file".to_owned(), vec![start..start + 3])])
    );
    assert!(other
        .take_record_captures()
        .expect("foreign source cannot publish captures")
        .is_empty());
}
