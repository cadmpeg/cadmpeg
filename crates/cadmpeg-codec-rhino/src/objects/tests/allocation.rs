// SPDX-License-Identifier: Apache-2.0

use super::*;

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

fn fixed_display_materials(legacy_count: usize, explicit: bool) -> Vec<u8> {
    let mut bytes = fixed_attributes(6, 0, None);
    if explicit {
        let offset = fixed_attributes(5, 0, None).len() + 1;
        let mut values = 1_i32.to_le_bytes().to_vec();
        values.extend([3_u8; 16]);
        values.extend([4_u8; 16]);
        bytes.splice(offset..offset + 4, values);
    }
    let offset = fixed_attributes(2, 0, None).len();
    let mut values = i32::try_from(legacy_count).unwrap().to_le_bytes().to_vec();
    for _ in 0..legacy_count {
        values.extend([1_u8; 16]);
        values.extend([2_u8; 16]);
    }
    bytes.splice(offset..offset + 4, values);
    bytes
}

#[test]
fn explicit_display_materials_release_the_replaced_legacy_list() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    for count in [1, 32, 257] {
        let bytes = fixed_display_materials(count, true);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes =
            u64::try_from("name".len() + "https://example.test".len() + 32).unwrap();
        let legacy_bytes = u64::try_from(count * 32).unwrap();
        policy.limits.max_materialized_bytes = legacy_bytes;
        let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
        let parsed = crate::objects::parse_attributes(
            &ctx, &bytes, 0..bytes.len(), 0..bytes.len(), ArchiveVersion::V4, None,
            &mut Diagnostics::new(),
        ).unwrap();
        assert_eq!(parsed.name, "name");
        assert_eq!(parsed.url, "https://example.test");
        assert_eq!(parsed.display_materials,
            [(Uuid::from_wire([3; 16]), Uuid::from_wire([4; 16]))]);
        let reusable = ctx.reserve_scoped(legacy_bytes, "next legacy display list")
            .expect("replaced legacy slots released while explicit values stay live");
        drop(reusable);
        ctx.finish_session().unwrap();
    }
}

#[test]
fn empty_explicit_display_materials_retain_the_selected_legacy_list() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    for count in [1, 32, 257] {
        let bytes = fixed_display_materials(count, false);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        let legacy_bytes = u64::try_from(count * 32).unwrap();
        policy.limits.max_materialized_bytes = legacy_bytes;
        policy.limits.max_retained_bytes =
            u64::try_from("name".len() + "https://example.test".len()).unwrap() + legacy_bytes;
        let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
        let parsed = crate::objects::parse_attributes(
            &ctx, &bytes, 0..bytes.len(), 0..bytes.len(), ArchiveVersion::V4, None,
            &mut Diagnostics::new(),
        ).unwrap();
        assert_eq!(parsed.display_materials.len(), count);
        assert!(parsed.display_materials.iter().all(|pair|
            *pair == (Uuid::from_wire([1; 16]), Uuid::from_wire([2; 16]))));
        let reusable = ctx.reserve_scoped(legacy_bytes, "next legacy display list")
            .expect("selected legacy slots transferred out of scratch");
        drop(reusable);
        ctx.finish_session().unwrap();
    }
}
