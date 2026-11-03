// SPDX-License-Identifier: Apache-2.0

use cadmpeg_container::compound::CompoundSnapshot;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::source_fidelity::parts::PartOutput;
use cadmpeg_ir::CodecFormat;

use crate::native::PropertySetRecord;
use crate::test_support::given_up_records;

use super::stage_source_header;

fn property_stream() -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&0xfffe_u16.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&0x0002_0006_u32.to_le_bytes());
    bytes.extend_from_slice(&[0x31; 16]);
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.extend_from_slice(&[0x42; 16]);
    bytes.extend_from_slice(&48_u32.to_le_bytes());
    bytes.extend_from_slice(&8_u32.to_le_bytes());
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    bytes
}

#[test]
fn actual_cfb_header_native_values_and_reads_share_one_part_stage() {
    let bytes = crate::test_support::test_fixtures::fixture_with_ufrx(&property_stream());
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
        .expect("actual CFB context");
    let snapshot = CompoundSnapshot::new(&ctx, root).expect("actual CFB");
    let descriptors = crate::property_set::inventory(&ctx, &snapshot).expect("actual inventory");
    let descriptor = &descriptors[0];
    let mut output = PartOutput::new(CodecFormat::Inventor, &ctx);
    let (formed, _) =
        stage_source_header(&ctx, &mut output, descriptor).expect("source-owned header");
    assert!(formed);
    let (_, native, fidelity, losses) = output.into_parts();
    assert!(losses.is_empty());
    let records = native
        .namespace("inventor")
        .expect("the inventor namespace exists")
        .arena_as::<PropertySetRecord>("property_sets")
        .expect("the property sets arena holds property set records");
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].version, 1);
    assert_eq!(records[0].system_identifier, 0x0002_0006);
    assert_eq!(records[0].clsid, "31313131313131313131313131313131");
    assert_eq!(records[0].section_count, 1);
    assert_eq!(records[0].directory_id, descriptor.stream.directory_id());
    assert_eq!(records[0].path, descriptor.path);
    assert!(given_up_records(&fidelity).is_empty());
}
