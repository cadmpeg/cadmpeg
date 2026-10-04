// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::annotations::StreamHandle;
use cadmpeg_ir::{AnnotationBuilder, CadIr};

fn attach_one_attribute(configure: impl FnOnce(&mut DecodePolicy)) -> Result<CadIr, CodecError> {
    crate::test_support::with_decode_context_over(
        &[],
        |policy| {
            configure(policy);
        },
        |ctx| {
            let mut ir = CadIr::empty();
            let mut annotations = AnnotationBuilder::new();
            let stream = StreamHandle::new(
                &cadmpeg_test_support::service_decode_context(),
                cadmpeg_ir::stream_name!("nx:container"),
                "fixture stream handle",
            )
            .unwrap();
            super::super::attach_part_attributes(
                ctx,
                &mut ir,
                &[("nx:part:attribute#0", "Title", "Value", 0)],
                |fields| *fields,
                &mut annotations,
                &stream,
            )?;
            Ok(ir)
        },
    )
}

#[test]
fn part_attribute_attachment_preserves_value() {
    let ir = attach_one_attribute(|_| {}).unwrap();
    let [attribute] = ir.model.attributes.as_slice() else {
        panic!("one attached part attribute");
    };
    assert_eq!(attribute.name.as_str(), "Title");
    assert_eq!(
        attribute.values.as_slice(),
        [cadmpeg_ir::attributes::AttributeValue::String(
            "Value".into()
        )]
    );
}

#[test]
fn part_attribute_attachment_refuses_collection_limit() {
    let error = attach_one_attribute(|policy| policy.limits.max_collection_items = 0).unwrap_err();
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::CollectionItems)
    );
}

#[test]
fn part_attribute_attachment_refuses_retained_limit() {
    let error = attach_one_attribute(|policy| policy.limits.max_retained_bytes = 0).unwrap_err();
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::RetainedBytes)
    );
}
