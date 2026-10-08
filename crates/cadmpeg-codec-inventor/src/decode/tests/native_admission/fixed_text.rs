// SPDX-License-Identifier: Apache-2.0
//! Fixed native text storage and variable value admission.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use crate::decode::{project_protein_state, MetadataProjection};
use crate::native::protein::ProteinRecord;
use crate::protein::ProteinState;

#[test]
fn protein_root_id_admits_exact_storage_with_zero_work() {
    let id = "inventor:protein:state#root";
    let bytes = cadmpeg_core::decode::u64_from_index(id.len());
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = bytes - 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    let error = project_protein_state(&ctx, &ProteinState::Absent).expect_err("ID storage refuses");
    let CodecError::ResourceLimit(original) = error else { panic!("resource refusal"); };
    assert_eq!(original.dimension, ResourceDimension::RetainedBytes);
    assert_eq!(original.operation, "retain Inventor Protein state id");
    assert_eq!(original.used, 0);
    assert_eq!(original.additional, bytes);
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(limit)) if limit == original));

    policy.limits.max_retained_bytes = bytes;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("exact context");
    assert!(matches!(project_protein_state(&ctx, &ProteinState::Absent).expect("fixed ID"),
        ProteinRecord::Absent { id: actual } if actual == id));
    ctx.finish_session().expect("fixed ID uses no input-sized work or scratch");
}

#[test]
fn metadata_attribute_keys_keep_exact_storage_and_no_variable_copy_work() {
    for key in ["title", "author", "description", "part_number"] {
        let mut metadata = MetadataProjection::default();
        let target = match key {
            "title" => &mut metadata.title,
            "author" => &mut metadata.author,
            "description" => &mut metadata.description,
            "part_number" => &mut metadata.part_number,
            _ => unreachable!("fixed key fixture"),
        };
        *target = Some("value".into());
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = cadmpeg_core::decode::u64_from_index(key.len()) - 1;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
        let mut attributes = std::collections::BTreeMap::new();
        let error = metadata.apply_attributes(&ctx, &mut attributes).expect_err("key storage refuses");
        let CodecError::ResourceLimit(original) = error else { panic!("resource refusal"); };
        assert_eq!(original.dimension, ResourceDimension::RetainedBytes);
        assert_eq!(original.operation, "retain Inventor metadata attribute key");
        assert_eq!(original.used, 0);
        assert_eq!(original.additional, cadmpeg_core::decode::u64_from_index(key.len()));
        assert!(attributes.is_empty());
        assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(limit)) if limit == original));

        policy.limits.max_retained_bytes = cadmpeg_core::decode::u64_from_index(key.len());
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("exact key context");
        let error = metadata.apply_attributes(&ctx, &mut attributes).expect_err("value storage remains admitted");
        let CodecError::ResourceLimit(original) = error else { panic!("resource refusal"); };
        assert_eq!(original.operation, "retain Inventor metadata attribute value");
        assert_eq!(original.used, cadmpeg_core::decode::u64_from_index(key.len()));
        assert_eq!(original.additional, cadmpeg_core::decode::u64_from_index("value".len()));
        assert!(attributes.is_empty());
        assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(limit)) if limit == original));

        policy = DecodePolicy::service();
        policy.limits.max_work_units = u64::MAX;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("service context");
        let probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
            ResourceDimension::WorkUnits, "retain Inventor metadata attribute key", None);
        metadata.apply_attributes(&ctx, &mut attributes).expect("fixed key plus variable value");
        drop(probe);
        assert_eq!(attributes.len(), 1);
        assert_eq!(attributes[key], "value");
        ctx.finish_session().expect("no variable copy work at the key operation");
    }
}
