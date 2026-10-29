// SPDX-License-Identifier: Apache-2.0
use std::collections::BTreeMap;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

use super::{Address, Dependencies};
use crate::binary::tests::{parse_parameter_record, read_directory};
use crate::binary::{counted_record, parse_sections, BinaryValue};
use crate::test_support::binary_parts::broken_parameter_reference;

#[test]
fn format_five_point_reference_uses_an_actual_nonrecord_source_offset() {
    let bytes = broken_parameter_reference(true);
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let sections = parse_sections(root).unwrap();
    let directory = read_directory(sections.directory, sections.lengths).unwrap();
    let index: BTreeMap<_, _> = directory
        .iter()
        .enumerate()
        .map(|(index, record)| (record.offset, index))
        .collect();
    let (offset, _, body) = counted_record(sections.parameter, 0, "Parameter").unwrap();
    let record = parse_parameter_record(offset, body, sections.lengths, &index).unwrap();
    assert_eq!(record.entity_type, 116);
    assert_eq!(
        record.values[3],
        BinaryValue::Pointer(i64::from(directory[1].offset + 1))
    );
    let global = source_global(&sections);
    let plan = Dependencies::inspect(&sections, &global, &ctx).unwrap();
    // #215 rule 1: the Parameter record needs its owner Directory record;
    // the Directory record does not need its Parameter record.
    assert!(plan
        .require(Address::Directory(directory[0].offset))
        .is_ok());
    assert!(plan.require(Address::Parameter(offset)).is_err());
    assert!(plan
        .require(Address::Directory(directory[1].offset))
        .is_ok());
}

fn source_global(sections: &crate::binary::BinarySections<'_>) -> crate::global::ResolvedGlobal {
    let values = crate::binary::tests::global_values(sections);
    let text = crate::binary::normalize_global(&values).expect("same Global grammar");
    crate::global::parse_stream(&text)
        .expect("actual Global interpretation")
        .0
}
