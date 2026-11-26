// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::u64_from_index;

use crate::design::decode::scopes::parameter_scope::parse_parameter_scope;
use crate::records::decal::DesignRecordHeader;
use crate::test_support::lp_utf16;

/// A `CylinderPrimitive` scope whose 78-byte tail has an empty label, lane
/// value 1, preceding history state `previous` and evaluation epoch `epoch`.
fn empty_label_scope(previous: u32, epoch: u32) -> (Vec<u8>, usize) {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"378");
    bytes.extend_from_slice(&12u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 10]);
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.push(1);
    bytes.extend_from_slice(&55u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 6]);
    bytes.extend_from_slice(&7u32.to_le_bytes());
    lp_utf16(&mut bytes, "CylinderPrimitive");
    let kind_end = bytes.len();
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 4]);
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 7]);

    bytes.push(1);
    bytes.push(0x0f);
    bytes.extend_from_slice(&1u64.to_le_bytes());
    bytes.extend_from_slice(&[0; 2]);
    bytes.extend_from_slice(&previous.to_le_bytes());
    bytes.extend_from_slice(&epoch.to_le_bytes());
    bytes.extend_from_slice(&0.25f64.to_le_bytes());
    bytes.extend_from_slice(&epoch.to_le_bytes());
    bytes.push(1);
    bytes.push(0x0e);
    bytes.extend_from_slice(&1u64.to_le_bytes());
    bytes.extend_from_slice(&[0, 1, 0, 0]);
    bytes.push(1);
    bytes.push(0x0d);
    bytes.extend_from_slice(&1u64.to_le_bytes());
    bytes.extend_from_slice(&[0; 3]);

    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"261");
    bytes.extend_from_slice(&12u32.to_le_bytes());
    (bytes, kind_end)
}

fn parse_empty_label_scope(
    bytes: &[u8],
) -> Option<crate::records::feature::scope::DesignParameterScope> {
    let header = DesignRecordHeader {
        id: "generated:scope-header#0".into(),
        record_index: 12,
        class_tag: crate::records::references::DesignClassTag::try_from("378".to_owned()).unwrap(),
        byte_offset: 0,
    };

    parse_parameter_scope(
        bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(bytes),
        header.record_index,
        &header.class_tag,
        header.byte_offset,
    )
    .unwrap()
}

#[test]
fn an_empty_label_tail_reads_its_preceding_state_whatever_its_epoch() {
    for epoch in [252, 250] {
        let (bytes, kind_end) = empty_label_scope(9, epoch);
        let scope = parse_empty_label_scope(&bytes).expect("empty-label scope");
        assert_eq!(
            scope.kind(),
            crate::records::feature::scope::DesignFeatureKind::CylinderPrimitive
        );
        assert_eq!(scope.frame_length(), u64_from_index(kind_end + 78));
        assert_eq!(scope.label, None);
        assert_eq!(scope.history_state_id(), Some(7), "epoch {epoch}");
        assert_eq!(scope.previous_history_state_id(), Some(9), "epoch {epoch}");
        assert_eq!(
            scope.previous_history_state_id_offset(),
            Some(u64_from_index(kind_end + 31)),
            "epoch {epoch}"
        );
    }
}

#[test]
fn parameter_scope_keeps_nonempty_label_and_excludes_it_from_payload_length() {
    let mut bytes = Vec::new();
    crate::test_support::indexed_header(&mut bytes, *b"378", 12);
    bytes.extend_from_slice(&[0; 10]);
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.push(1);
    bytes.extend_from_slice(&55u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 6]);
    bytes.extend_from_slice(&7u32.to_le_bytes());
    lp_utf16(&mut bytes, "CylinderPrimitive");
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    let label_offset = bytes.len() + 4;
    lp_utf16(&mut bytes, "A😀");
    bytes.extend_from_slice(&[0; 7]);
    bytes.extend_from_slice(&[1, 15]);
    bytes.extend_from_slice(&1u64.to_le_bytes());
    bytes.extend_from_slice(&[0; 2]);
    bytes.extend_from_slice(&9u32.to_le_bytes());
    bytes.extend_from_slice(&252u32.to_le_bytes());
    bytes.extend_from_slice(&0.25f64.to_le_bytes());
    bytes.extend_from_slice(&252u32.to_le_bytes());
    bytes.extend_from_slice(&[1, 14]);
    bytes.extend_from_slice(&1u64.to_le_bytes());
    bytes.extend_from_slice(&[0, 1, 0, 0, 1, 13]);
    bytes.extend_from_slice(&1u64.to_le_bytes());
    bytes.extend_from_slice(&[0; 3]);
    crate::test_support::indexed_header(&mut bytes, *b"261", 12);
    let scope = parse_parameter_scope(
        &bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
        12,
        &crate::records::references::DesignClassTag::try_from("378".to_owned()).unwrap(),
        0,
    )
    .unwrap()
    .expect("named scope");
    assert_eq!(
        scope
            .label
            .as_ref()
            .map(|label| (label.value.as_str(), label.offset)),
        Some(("A😀", u64_from_index(label_offset)))
    );
    assert_eq!(
        super::super::parameter_scope::parameter_scope_payload_length(&scope),
        Some(122)
    );
    let wire = cadmpeg_core::serde_erase::json::to_value(&scope).unwrap();
    assert_eq!(wire["label"]["value"], "A😀");
    assert_eq!(wire["label"]["offset"], u64_from_index(label_offset));
    let restored: crate::records::feature::scope::DesignParameterScope =
        cadmpeg_core::serde_erase::json::from_value(wire).unwrap();
    assert_eq!(restored.label, scope.label);
}
