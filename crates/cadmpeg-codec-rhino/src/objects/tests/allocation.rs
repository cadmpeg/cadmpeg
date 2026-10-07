// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn object_boxes_refuse_before_allocating_their_inline_storage() {
    use crate::chunks::FramingError;
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    let archive = ArchiveVersion::V5;
    let attributes = fixed_attributes(1, 0, None);
    let bytes = object_record_with_attribute_userdata(archive, 1, POINT_CLASS, &attributes, &[]);
    let chunk = crate::chunks::chunk_at(&bytes, 0, bytes.len(), archive, false).expect("framed object");
    let record = crate::container::Record::long(chunk.typecode, chunk.range(), chunk.body());
    for (dimension, operation) in [(ResourceDimension::RetainedBytes, "Rhino object attribute box"), (ResourceDimension::MaterializedBytes, "Rhino framed object box"), (ResourceDimension::RetainedBytes, "Rhino resolved object box")] {
        cadmpeg_test_support::refusal::resource_limit_at(dimension, operation, |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            if dimension == ResourceDimension::MaterializedBytes { policy.limits.max_materialized_bytes = cap; } else { policy.limits.max_retained_bytes = cap; }
            let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
            let mut workspace = ctx.reserve_scoped(0, "Rhino test object workspace").unwrap();
        let result = match crate::objects::parse_object_record(&ctx, &mut workspace, &bytes, &record, archive, None, &mut Diagnostics::new()) {
                Ok(object) => crate::objects::resolve_identities(&ctx, vec![object], &settings::DocumentMetadata::default(), &mut Diagnostics::new()),
                Err(FramingError::Resource(limit)) => Err(cadmpeg_core::CodecError::ResourceLimit(limit)),
                Err(error) => panic!("valid object failed framing: {error:?}"),
            };
            if let Err(cadmpeg_core::CodecError::ResourceLimit(limit)) = &result { assert_eq!(ctx.resource_refusal().as_ref(), Some(limit)); }
            result
        });
    }
}

#[test]
fn skipped_uuid_lists_validate_the_bounded_prefix_without_allocating_values() {
    let archive = ArchiveVersion::V5;
    let mut body = 1_i32.to_le_bytes().to_vec();
    body.extend(0_i32.to_le_bytes());
    body.extend(2_i32.to_le_bytes());
    body.extend([7_u8; 32]);
    let bytes = crc_chunk(archive, 0x4000_8000, &body);
    let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).unwrap();
    crate::objects::skip_uuid_list(&mut reader, archive).expect("bounded UUID prefix");
    assert_eq!(reader.position(), bytes.len());
    body[8..12].copy_from_slice(&3_i32.to_le_bytes());
    let bytes = crc_chunk(archive, 0x4000_8000, &body);
    let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).unwrap();
    assert!(crate::objects::skip_uuid_list(&mut reader, archive).is_err());
}
