// SPDX-License-Identifier: Apache-2.0

use crate::design::decode::operands::{ConstructionOperandGroupParse, RecordFrame};
use crate::records::feature::scope::DesignParameterScope;
use crate::test_support::indexed_header;

#[test]
fn construction_group_reference_preserves_utf16_work_refusal() {
    let mut bytes = Vec::new();
    indexed_header(&mut bytes, *b"332", 100);
    bytes.extend_from_slice(&[0; 10]);
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.push(1);
    bytes.extend_from_slice(&101u64.to_le_bytes());
    bytes.push(1);
    bytes.extend_from_slice(&7u32.to_le_bytes());
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&(b'x' as u16).to_le_bytes());
    bytes.push(0);
    bytes.extend_from_slice(&36u32.to_le_bytes());
    bytes.extend_from_slice(b"aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee");
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&(b'L' as u16).to_le_bytes());
    bytes.push(0);
    let scope = DesignParameterScope::empty(
        "f3d:test:construction-group-scope#12",
        crate::records::feature::scope::DesignFeatureKind::Extrude,
        12,
    );
    let header = RecordFrame {
        record_index: 100,
        class_tag: crate::records::references::DesignClassTag::try_from("332".to_owned())
            .expect("class tag"),
        byte_offset: 0,
    };
    let error = crate::test_support::resource_refusal_at(
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "validate F3D UTF-16 text",
        0,
        |ctx| match crate::design::decode::operands::parse_construction_operand_group(
            ctx, &bytes, &scope, 0, &header,
        ) {
            ConstructionOperandGroupParse::Refused(error) => Err::<(), _>(error),
            _ => Err::<(), _>(cadmpeg_core::CodecError::Malformed(
                "construction group did not propagate its reference refusal".into(),
            )),
        },
    );
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(failure)
        if failure.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits
            && failure.operation == "validate F3D UTF-16 text"));
}
