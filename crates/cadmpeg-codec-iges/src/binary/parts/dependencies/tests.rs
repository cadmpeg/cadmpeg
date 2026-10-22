// SPDX-License-Identifier: Apache-2.0
use std::collections::BTreeMap;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

use super::{Address, Dependencies};
use crate::binary::{
    counted_record, parse_parameter_record, parse_sections, read_directory, BinaryValue,
};
use crate::test_support::binary_parts::{broken_parameter_reference, broken_transform_chain};

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
    let plan = Dependencies::inspect(&sections, &ctx).unwrap();
    assert!(plan
        .require(Address::Directory(directory[0].offset))
        .is_err());
    assert!(plan.require(Address::Parameter(offset)).is_err());
    assert!(plan
        .require(Address::Directory(directory[1].offset))
        .is_ok());
}

#[test]
fn speculative_dependency_reads_add_no_record_credit_and_keep_committed_neighbor_reads() {
    let bytes = broken_transform_chain(true);
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    ctx.record("iges:entity:probe#1", root, || {
        let mut reader = root;
        assert_eq!(reader.u8(), Some(b'B'));
        let sections = parse_sections(root)?;
        let _plan = Dependencies::inspect(&sections, &ctx)?;
        assert_eq!(reader.u8(), Some(0));
        Ok(())
    })
    .unwrap();
    let captures = ctx.take_record_captures().unwrap();
    assert_eq!(captures.len(), 1);
    assert_eq!(captures[0].id(), "iges:entity:probe#1");
    assert_eq!(
        captures[0].reads(),
        &BTreeMap::from([("file".to_owned(), vec![0..2])])
    );
}
