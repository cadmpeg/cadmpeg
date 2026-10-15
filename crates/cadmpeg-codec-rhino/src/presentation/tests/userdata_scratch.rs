// SPDX-License-Identifier: Apache-2.0
//! Class userdata workspace ends before typed presentation output starts.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

use crate::chunks::{ArchiveVersion, FramingError};
use crate::container::Record;
use crate::objects::UserdataDescriptor;
use crate::presentation::{class_data_with_userdata, MATERIAL};
use crate::test_support::test_dump::{
    class_userdata_v1_with_direct_payload, class_wrapper_with_userdata,
};
use crate::wire::Uuid;

fn wrapper() -> (Vec<u8>, Record) {
    let userdata = class_userdata_v1_with_direct_payload(
        ArchiveVersion::V5, [9; 16], &[17, 23],
    );
    let bytes = class_wrapper_with_userdata(
        ArchiveVersion::V5, MATERIAL.to_wire(), &[5, 7], &userdata,
    );
    let record = Record::long(0x2000_8040, 0..bytes.len(), 0..bytes.len());
    (bytes, record)
}

fn backing_bytes() -> u64 {
    // One descriptor uses the core four-slot initial Vec growth bound.
    let item_bytes = std::mem::size_of::<UserdataDescriptor>();
    assert!((2..=1024).contains(&item_bytes));
    u64::try_from(4 * item_bytes).unwrap()
}

#[test]
fn class_userdata_scratch_releases_after_fixed_selection() {
    let (bytes, record) = wrapper();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = backing_bytes();
    policy.limits.max_collection_items = 2;
    let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    let first = class_data_with_userdata(&ctx, &bytes, &record, ArchiveVersion::V5, MATERIAL).unwrap();
    let range = first.range.clone();
    let selected = first.userdata[0].known().unwrap().clone();
    assert_eq!(&bytes[range.clone()], &[5, 7]);
    assert_eq!(selected.class_uuid, Uuid::from_wire([9; 16]));
    assert_eq!(&bytes[selected.payload_range.clone()], &[17, 23]);
    drop(first);
    let second = class_data_with_userdata(&ctx, &bytes, &record, ArchiveVersion::V5, MATERIAL).unwrap();
    assert_eq!(second.range, range);
    assert_eq!(second.userdata[0].known(), Some(&selected));
    drop(second);
    let released = ctx.reserve_scoped(backing_bytes(), "class userdata scratch reuse")
        .expect("only fixed selected metadata remains live");
    drop(released);
    assert_eq!(ctx.resource_refusal(), None);
    ctx.finish_session().unwrap();
}

fn assert_original_refusal(materialized: u64, collection: u64, dimension: ResourceDimension, additional: u64) {
    let (bytes, record) = wrapper();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = materialized;
    policy.limits.max_collection_items = collection;
    let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    let FramingError::Resource(original) = class_data_with_userdata(
        &ctx, &bytes, &record, ArchiveVersion::V5, MATERIAL,
    ).unwrap_err() else { panic!("userdata refusal"); };
    assert_eq!(original.operation, "Rhino class userdata");
    assert_eq!(original.dimension, dimension);
    assert_eq!(original.used, 0);
    assert_eq!(original.additional, additional);
    assert_eq!(ctx.resource_refusal(), Some(original));
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == original));
}

#[test]
fn class_userdata_scratch_keeps_original_collection_priority() {
    assert_original_refusal(0, 0, ResourceDimension::CollectionItems, 1);
}

#[test]
fn class_userdata_scratch_refuses_exact_vector_backing() {
    assert_original_refusal(0, 1, ResourceDimension::MaterializedBytes, backing_bytes());
}

#[test]
fn class_userdata_scratch_stays_admitted_while_descriptors_live() {
    let (bytes, record) = wrapper();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = backing_bytes();
    let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    let first = class_data_with_userdata(&ctx, &bytes, &record, ArchiveVersion::V5, MATERIAL).unwrap();
    let FramingError::Resource(original) = class_data_with_userdata(
        &ctx, &bytes, &record, ArchiveVersion::V5, MATERIAL,
    ).unwrap_err() else { panic!("live first descriptor workspace prevents second backing"); };
    assert_eq!(first.userdata.len(), 1);
    assert_eq!(original.operation, "Rhino class userdata");
    assert_eq!(original.dimension, ResourceDimension::MaterializedBytes);
    assert_eq!(original.used, backing_bytes());
    assert_eq!(original.additional, backing_bytes());
    assert_eq!(ctx.resource_refusal(), Some(original));
    drop(first);
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == original));
}
