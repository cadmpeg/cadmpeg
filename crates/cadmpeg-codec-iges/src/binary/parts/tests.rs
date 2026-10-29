// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

use super::super::{parameter_prefix, PrimitiveLengths};

#[test]
fn repeated_binary_values_keep_the_exact_physical_directory_pointer_extent() {
    let lengths = PrimitiveLengths {
        single_integer: 32,
        double_integer: 32,
        single_exponent: 8,
        single_fraction: 23,
        double_exponent: 11,
        double_fraction: 52,
    };
    for (bytes, pointer, expected) in [
        (vec![0x89, 0, 0, 0, 116, 0, 0, 0, 6], 6, 5..9),
        (vec![0x09, 0, 0, 0, 116], 116, 1..5),
    ] {
        let mut prefixed = b"pre".to_vec();
        prefixed.extend_from_slice(&bytes);
        prefixed.extend_from_slice(b"post");
        let arena = DecodeArena::new();
        let (_ctx, root) =
            DecodeContext::from_root_bytes(&prefixed, &arena, &DecodePolicy::service()).unwrap();
        let body = root.child(3, 3 + bytes.len()).unwrap();
        let prefix = parameter_prefix(body, lengths).unwrap();
        assert_eq!(prefix.entity_type, 116);
        assert_eq!(prefix.directory_pointer, pointer);
        assert_eq!(prefix.stream.value_extent, expected.clone());
        let extent = body
            .child(
                body.start() + prefix.stream.value_extent.start,
                body.start() + prefix.stream.value_extent.end,
            )
            .unwrap();
        assert_eq!(extent.window(), &bytes[expected]);
        assert_eq!(extent.location().space, body.location().space);
    }
}

#[test]
fn binary_reference_values_resolve_actual_source_offsets_before_semantic_creation() {
    use super::super::tests::read_directory;
    use super::super::{parse_sections, resolve_value, BinaryValue};

    let bytes = crate::test_support::binary_parts::point_pair(false);
    let arena = DecodeArena::new();
    let (_ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let sections = parse_sections(root).unwrap();
    let records = read_directory(sections.directory, sections.lengths).unwrap();
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].source_sequence, 1);
    assert_eq!(records[1].source_sequence, 3);
    let index: std::collections::BTreeMap<_, _> = records
        .iter()
        .enumerate()
        .map(|(ordinal, record)| (record.offset, ordinal))
        .collect();
    let target = i64::from(records[1].offset);
    assert_ne!(target, 3);
    assert_eq!(
        resolve_value(&BinaryValue::Pointer(target), &records, &index).unwrap(),
        BinaryValue::Pointer(3)
    );
    assert_eq!(
        resolve_value(&BinaryValue::Pointer(-target), &records, &index).unwrap(),
        BinaryValue::Pointer(-3)
    );
    assert_eq!(
        resolve_value(&BinaryValue::Integer(target), &records, &index).unwrap(),
        BinaryValue::Integer(target)
    );
    assert!(resolve_value(&BinaryValue::Pointer(target + 1), &records, &index).is_err());
}

#[test]
fn normalized_grammar_positions_keep_the_original_counted_record_identity() {
    for corrupt_first in [false, true] {
        let bytes = crate::test_support::binary_parts::point_pair(corrupt_first);
        let arena = DecodeArena::new();
        let (ctx, root) =
            DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
        let (normalized, _) = super::normalize(root, &ctx).unwrap();
        if corrupt_first {
            assert_eq!(normalized.identities.semantic_sequence(1).unwrap(), 3);
            assert!(normalized.identities.semantic_sequence(3).is_err());
        } else {
            assert_eq!(normalized.identities.semantic_sequence(1).unwrap(), 1);
            assert_eq!(normalized.identities.semantic_sequence(3).unwrap(), 3);
        }
        for unbound in [0, 2, 5] {
            assert!(normalized.identities.semantic_sequence(unbound).is_err());
        }
    }
}

#[test]
fn both_declared_binary_integer_formats_preserve_the_same_directory_entity_type() {
    use crate::test_support::binary_parts::{directory_admission_fault, DirectoryAdmissionFault};

    let original = directory_admission_fault(DirectoryAdmissionFault::EntityType, false);
    let entity_type_control = {
        let arena = DecodeArena::new();
        let (_ctx, root) =
            DecodeContext::from_root_bytes(&original, &arena, &DecodePolicy::service())
                .expect("valid Binary point pair");
        let sections = super::super::parse_sections(root).expect("actual Binary sections");
        assert_eq!(sections.lengths.single_integer, 32);
        assert_eq!(sections.lengths.double_integer, 32);
        let (_, _, body) = super::super::counted_record(sections.directory, 0, "Directory")
            .expect("actual first Directory body");
        body.start()
    };
    let mut double_integer = original.clone();
    assert_eq!(double_integer[entity_type_control], 1);
    double_integer[entity_type_control] = 2;
    assert_eq!(
        original
            .iter()
            .zip(&double_integer)
            .filter(|(left, right)| left != right)
            .count(),
        1
    );
    let baseline = crate::test_support::decode(original);
    let decoded = crate::test_support::decode(double_integer);
    assert_eq!(decoded.ir().model.points.len(), 2);
    assert_eq!(decoded.ir().model.vertices.len(), 2);
    assert_eq!(decoded.ir().model, baseline.ir().model);
    let native = decoded.ir().native.namespace("iges").expect("IGES native");
    let expected = baseline.ir().native.namespace("iges").expect("IGES native");
    assert_eq!(native.arenas()["entities"], expected.arenas()["entities"]);
    assert!(native
        .arena_as::<cadmpeg_ir::NativeUnknownRecord>("unknowns")
        .expect("typed failure-stub arena")
        .is_empty());
    assert!(!decoded
        .report()
        .losses
        .iter()
        .any(|loss| loss.code.local_code() == "record.unreadable"));
}

mod semantic_handoff;

mod handoff_boundary;

mod plain_line;

mod source_point;
