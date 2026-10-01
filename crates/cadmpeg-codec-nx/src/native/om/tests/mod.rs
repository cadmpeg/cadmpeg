// SPDX-License-Identifier: Apache-2.0

#[test]
fn parameterized_expression_refuses_scoped_limit() {
    let bytes = b"p1 + 2";

    crate::test_support::with_decode_context_over(
        bytes,
        |policy| {
            policy.limits.max_materialized_bytes = 0;
        },
        |ctx| {
            let error =
                super::evaluate_parameterized_expression(ctx, "p1 + 2", |_| Some(3.0)).unwrap_err();
            assert!(
                matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit) if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes)
            );
        },
    );
}

use crate::test_support::test_om::offset_only_indexed_om_section;
use crate::test_support::test_om::offset_only_indexed_om_section_with_control;
use crate::test_support::test_om::offset_only_indexed_om_section_with_index_values;
use crate::test_support::test_om::size_framed_om_section;
use crate::test_support::test_prt::prt_with_arrangement_attribute;
use crate::test_support::test_prt::prt_with_arrangements;
use crate::test_support::test_prt::prt_with_indexed_om_section;
use crate::test_support::test_prt::prt_with_named_payloads;
use crate::test_support::test_prt::prt_with_size_framed_om_section;
use cadmpeg_test_support::EditableDecodeResult;

#[test]
fn data_block_reference_wire_preserves_feature_token_and_rejects_mismatch() {
    for (value, raw) in [
        (0, vec![0]),
        (0, vec![0x80, 0]),
        (0, vec![0x90, 0, 0]),
        (6466, vec![0x90, 0x19, 0x42]),
    ] {
        let wire = serde_json::json!({"id":"reference", "data_block":"block", "ordinal":0,
            "object_id":value, "raw_object_id":raw, "source_offset":12});
        let record: super::DataBlockReference = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(serde_json::to_value(record).unwrap(), wire);
    }
    for (value, raw) in [
        (1, vec![0]),
        (0, vec![0xff]),
        (0, vec![0xf0, 0]),
        (0, vec![0x90, 0]),
        (0, vec![0, 0]),
    ] {
        let wire = serde_json::json!({"id":"reference", "data_block":"block", "ordinal":0,
            "object_id":value, "raw_object_id":raw, "source_offset":12});
        assert!(serde_json::from_value::<super::DataBlockReference>(wire)
            .unwrap_err()
            .to_string()
            .contains("object_id/raw_object_id"));
    }
}

mod expression_wire;
mod native_units;
mod state_counters;
use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::container;

use crate::NxCodec;

#[test]
fn om_offset_store_values_precede_unique_product_anchor() {
    let mut bytes = vec![0, 0];
    bytes.extend_from_slice(&7u32.to_le_bytes());
    bytes.extend_from_slice(&0x1020u32.to_le_bytes());
    bytes.extend_from_slice(b"\x04\x01\x0eNX 2027.3102\0tail");
    assert_eq!(
        crate::test_support::with_decode_context(|ctx| crate::om::offset_store_control_form(
            ctx, &bytes, None
        ))
        .unwrap(),
        Some(crate::om::OffsetStoreControlForm::ProductAnchored {
            leading_value: Some(
                crate::om::control_leading_value::ControlLeadingValue::from_wire(2, 0).unwrap()
            ),
            values: crate::om::nonempty::NonEmpty::new([7, 0x1020]).unwrap(),
        })
    );

    let mut nonzero_leading = vec![0x34, 0x12, 0x00];
    nonzero_leading.extend_from_slice(&7u32.to_le_bytes());
    nonzero_leading.extend_from_slice(b"\x04\x01\x0eNX 2027.3102\0tail");
    assert_eq!(
        crate::test_support::with_decode_context(|ctx| crate::om::offset_store_control_form(
            ctx,
            &nonzero_leading,
            None
        ))
        .unwrap(),
        Some(crate::om::OffsetStoreControlForm::ProductAnchored {
            leading_value: Some(
                crate::om::control_leading_value::ControlLeadingValue::from_wire(3, 0x1234)
                    .unwrap()
            ),
            values: crate::om::nonempty::NonEmpty::new([7]).unwrap(),
        })
    );

    let mut duplicate = bytes;
    duplicate.extend_from_slice(b"\x04\x01\x0eNX 2027.3102\0");
    assert!(crate::test_support::with_decode_context(|ctx| {
        crate::om::offset_store_control_form(ctx, &duplicate, None)
    })
    .unwrap()
    .is_none());
    assert_eq!(
        crate::test_support::with_decode_context(|ctx| super::control_index_data_block(
            ctx, 2, 700, 496
        ))
        .unwrap()
        .as_deref(),
        Some("nx:om-data-blocks-2:block#496")
    );
    assert!(
        crate::test_support::with_decode_context(|ctx| super::control_index_data_block(
            ctx, 2, 700, 700
        ))
        .unwrap()
        .is_none()
    );
}

fn control_form_route_refusal(
    configure: impl FnOnce(&mut cadmpeg_core::decode::DecodePolicy),
) -> cadmpeg_core::CodecError {
    let file =
        prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", offset_only_indexed_om_section())]);
    let container = crate::test_support::with_decode_context(|ctx| {
        crate::container::scan_bytes(ctx, file.as_slice())
    })
    .expect("control form fixture");
    crate::test_support::with_decode_context(|ctx| container.indexed_om_sections(ctx))
        .expect("cached control form section");
    let forms = crate::test_support::with_decode_context(|ctx| {
        super::data_block_control_forms(ctx, &container)
    })
    .expect("control form projection");
    assert_eq!(forms.len(), 1);
    assert_eq!(forms[0].id, "nx:om-data-block-control-forms:form#0");

    crate::test_support::with_decode_context_over(
        &[],
        |policy| {
            configure(policy);
        },
        |ctx| super::data_block_control_forms(ctx, &container).unwrap_err(),
    )
}

fn control_class_route_refusal(
    configure: impl FnOnce(&mut cadmpeg_core::decode::DecodePolicy),
) -> cadmpeg_core::CodecError {
    let file =
        prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", offset_only_indexed_om_section())]);
    let container =
        crate::test_support::with_decode_context(|ctx| crate::container::scan_bytes(ctx, file))
            .expect("control class fixture");
    let classes = crate::test_support::with_decode_context(|ctx| {
        super::data_block_control_class_references(ctx, &container)
    })
    .expect("control class projection");
    assert_eq!(classes.len(), 1);

    crate::test_support::with_decode_context_over(
        &[],
        |policy| {
            configure(policy);
        },
        |ctx| {
            super::data_block_control_class_references(ctx, &container)
                .expect_err("control class resource refusal")
        },
    )
}

fn data_block_reference_route_refusal(
    configure: impl FnOnce(&mut cadmpeg_core::decode::DecodePolicy),
) -> cadmpeg_core::CodecError {
    let file =
        prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", offset_only_indexed_om_section())]);
    let container =
        crate::test_support::with_decode_context(|ctx| crate::container::scan_bytes(ctx, file))
            .expect("data block reference fixture");
    let record = super::ObjectRecord {
        id: "test-record".to_owned(),
        object_id: (42, 0),
        section_ordinal: 0,
        record_ordinal: 0,
        section_offset: 0,
        byte_len: 0,
        sha256: cadmpeg_ir::hash::digest::Sha256Digest::digest(&[]),
        stable_identity: None,
        dependencies: Vec::new(),
        dependents: Vec::new(),
        source_entry: "/Root/UG_PART/UG_PART".to_owned(),
        source_offset: 0,
    };
    let records = [record];
    let references = crate::test_support::with_decode_context(|ctx| {
        super::data_block_references(ctx, &container, &records, &[])
    })
    .expect("data block reference projection");
    assert_eq!(references.len(), 1);
    assert_eq!(references[0].target_record.as_deref(), Some("test-record"));

    crate::test_support::with_decode_context_over(
        &[],
        |policy| {
            configure(policy);
        },
        |ctx| {
            super::data_block_references(ctx, &container, &records, &[])
                .expect_err("data block reference resource refusal")
        },
    )
}

fn part_color_container() -> crate::container::Container<'static> {
    let mut table = vec![0x02, 0x80, 0xd9, 0x01];
    for ordinal in 0..=216 {
        let name = if ordinal == 0 {
            "Background".to_owned()
        } else {
            format!("Color {ordinal}")
        };
        table.push(u8::try_from(name.len() + 2).expect("test color name length"));
        table.extend_from_slice(name.as_bytes());
        table.push(0);
    }
    table.extend_from_slice(&[
        0x02, 0x14, 0xff, 0x06, 0x00, 0xf0, 0x02, 0x80, 0x9d, 0x80, 0xc7, 0x00, 0xc0, 0x13, 0x0a,
        0xc6, 0x01, 0x80, 0xd9, 0x80, 0xc8, 0x01, 0x01, 0x01,
    ]);
    for color_index in 1u16..=216 {
        table.push(0x05);
        if color_index < 128 {
            table.push(u8::try_from(color_index).expect("test color index"));
        } else {
            table.extend_from_slice(&[
                0x80,
                u8::try_from(color_index - 1).expect("test color index"),
            ]);
        }
        table.extend_from_slice(&[0x01, 0x80, 0xc8]);
        if color_index == 2 {
            table.extend_from_slice(&crate::test_support::test_bytes::shifted_f64_bytes(2.0));
            let mut binary32 = 1.0_f32.to_be_bytes();
            binary32[0] += 0x10;
            table.extend_from_slice(&binary32);
            table.push(0);
        } else {
            table.extend_from_slice(&[0x01, 0x01, 0x01]);
        }
    }
    let mut section = offset_only_indexed_om_section();
    let class = b"UGS::COLOR_table";
    assert_eq!(class.len(), b"UGS::ModlFeature".len());
    section[9..9 + class.len()].copy_from_slice(class);
    section.extend_from_slice(&table);
    let index_start = 8 + 1 + class.len() + 1;
    let end_at = index_start + 3 * 4;
    let end = u32::try_from(section.len()).expect("test color section length");
    section[end_at..end_at + 4].copy_from_slice(&end.to_le_bytes());
    let file = prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", section)]);
    crate::test_support::with_decode_context(|ctx| crate::container::scan_bytes(ctx, file))
        .expect("part color container")
}

fn part_color_route_refusal(
    configure: impl FnOnce(&mut cadmpeg_core::decode::DecodePolicy),
) -> cadmpeg_core::CodecError {
    let container = part_color_container();
    let (tables, definitions) =
        crate::test_support::with_decode_context(|ctx| super::part_color_tables(ctx, &container))
            .expect("part color projection");
    assert_eq!(tables.len(), 1);
    assert_eq!(definitions.len(), 216);

    crate::test_support::with_decode_context_over(
        &[],
        |policy| {
            configure(policy);
        },
        |ctx| super::part_color_tables(ctx, &container).expect_err("part color resource refusal"),
    )
}

#[test]
fn part_color_route_refuses_collection_limit() {
    let error = part_color_route_refusal(|policy| policy.limits.max_collection_items = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems),
        "{error:?}"
    );
}

#[test]
fn part_color_route_refuses_retained_limit() {
    let error = part_color_route_refusal(|policy| policy.limits.max_retained_bytes = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes),
        "{error:?}"
    );
}

#[test]
fn part_color_route_refuses_scoped_limit() {
    let error = part_color_route_refusal(|policy| policy.limits.max_materialized_bytes = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes),
        "{error:?}"
    );
}

#[test]
fn part_color_route_refuses_work_limit() {
    let error = part_color_route_refusal(|policy| policy.limits.max_work_units = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits),
        "{error:?}"
    );
}

#[test]
fn data_block_reference_route_refuses_collection_limit() {
    let error = data_block_reference_route_refusal(|policy| policy.limits.max_collection_items = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems),
        "{error:?}"
    );
}

#[test]
fn data_block_reference_route_refuses_retained_limit() {
    let error = data_block_reference_route_refusal(|policy| policy.limits.max_retained_bytes = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes),
        "{error:?}"
    );
}

#[test]
fn data_block_reference_route_refuses_scoped_limit() {
    let error =
        data_block_reference_route_refusal(|policy| policy.limits.max_materialized_bytes = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes),
        "{error:?}"
    );
}

#[test]
fn data_block_reference_route_refuses_work_limit() {
    let error = data_block_reference_route_refusal(|policy| policy.limits.max_work_units = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits),
        "{error:?}"
    );
}

#[test]
fn control_class_route_refuses_collection_limit() {
    let error = control_class_route_refusal(|policy| policy.limits.max_collection_items = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems),
        "{error:?}"
    );
}

#[test]
fn control_class_route_refuses_retained_limit() {
    let error = control_class_route_refusal(|policy| policy.limits.max_retained_bytes = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes),
        "{error:?}"
    );
}

#[test]
fn control_class_route_refuses_scoped_limit() {
    let error = control_class_route_refusal(|policy| policy.limits.max_materialized_bytes = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes),
        "{error:?}"
    );
}

#[test]
fn control_class_route_refuses_work_limit() {
    let error = control_class_route_refusal(|policy| policy.limits.max_work_units = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits),
        "{error:?}"
    );
}

#[test]
fn data_block_control_form_route_refuses_collection_limit() {
    let error = control_form_route_refusal(|policy| policy.limits.max_collection_items = 4);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems
            && limit.operation == "NX data block control forms"),
        "{error:?}"
    );
}

#[test]
fn data_block_control_form_route_refuses_retained_limit() {
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        "retain NX control form id",
        |limit| {
            Err::<(), _>(control_form_route_refusal(|policy| {
                policy.limits.max_retained_bytes = limit;
            }))
        },
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes
            && limit.operation == "retain NX control form id"),
        "{error:?}"
    );
}

#[test]
fn data_block_control_form_route_refuses_work_limit() {
    let error = control_form_route_refusal(|policy| policy.limits.max_work_units = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits
            && limit.operation == "retain NX control form id"),
        "{error:?}"
    );
}

fn control_reference_route_refusal(
    configure: impl FnOnce(&mut cadmpeg_core::decode::DecodePolicy),
) -> cadmpeg_core::CodecError {
    let control = [0xe0, 0, 0, 0, 1, 0xc0, 0, 0, 1];
    let file = prt_with_named_payloads(&[(
        "/Root/UG_PART/UG_PART",
        offset_only_indexed_om_section_with_control(&control),
    )]);
    let container = crate::test_support::with_decode_context(|ctx| {
        crate::container::scan_bytes(ctx, file.as_slice())
    })
    .expect("control reference fixture");
    crate::test_support::with_decode_context(|ctx| container.indexed_om_sections(ctx))
        .expect("cached control reference section");
    let references = crate::test_support::with_decode_context(|ctx| {
        super::data_block_control_references(ctx, &container)
    })
    .expect("control reference projection");
    assert_eq!(references.len(), 2);
    assert!(references[0]
        .id
        .starts_with("nx:om-data-block-control-references-0:reference#"));

    crate::test_support::with_decode_context_over(
        &[],
        |policy| {
            configure(policy);
        },
        |ctx| super::data_block_control_references(ctx, &container).unwrap_err(),
    )
}

#[test]
fn data_block_control_reference_route_refuses_collection_limit() {
    let error = control_reference_route_refusal(|policy| policy.limits.max_collection_items = 3);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems
            && limit.operation == "NX data block control references"),
        "{error:?}"
    );
}

#[test]
fn data_block_control_reference_route_refuses_retained_limit() {
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        "retain NX control reference id",
        |limit| {
            Err::<(), _>(control_reference_route_refusal(|policy| {
                policy.limits.max_retained_bytes = limit;
            }))
        },
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes
            && limit.operation == "retain NX control reference id"),
        "{error:?}"
    );
}

#[test]
fn data_block_control_reference_route_refuses_scoped_limit() {
    let error = control_reference_route_refusal(|policy| policy.limits.max_materialized_bytes = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes
            && limit.operation == "NX control reference block id"),
        "{error:?}"
    );
}

#[test]
fn data_block_control_reference_route_refuses_work_limit() {
    let error = control_reference_route_refusal(|policy| policy.limits.max_work_units = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits
            && limit.operation == "NX control reference block id"),
        "{error:?}"
    );
}

fn control_value_route_refusal(
    configure: impl FnOnce(&mut cadmpeg_core::decode::DecodePolicy),
) -> cadmpeg_core::CodecError {
    let file =
        prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", offset_only_indexed_om_section())]);
    let container = crate::test_support::with_decode_context(|ctx| {
        crate::container::scan_bytes(ctx, file.as_slice())
    })
    .expect("offset-store control fixture");
    crate::test_support::with_decode_context(|ctx| container.indexed_om_sections(ctx))
        .expect("cached offset-store section");
    let values = crate::test_support::with_decode_context(|ctx| {
        super::data_block_control_values(ctx, &container)
    })
    .expect("control-value projection");
    assert_eq!(values.len(), 2);

    crate::test_support::with_decode_context_over(
        &[],
        |policy| {
            configure(policy);
        },
        |ctx| super::data_block_control_values(ctx, &container).unwrap_err(),
    )
}

#[test]
fn data_block_control_value_route_refuses_collection_limit() {
    let error = control_value_route_refusal(|policy| policy.limits.max_collection_items = 4);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems
            && limit.operation == "NX data block control values"),
        "{error:?}"
    );
}

#[test]
fn data_block_control_value_route_refuses_retained_limit() {
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        "retain NX control value id",
        |limit| {
            Err::<(), _>(control_value_route_refusal(|policy| {
                policy.limits.max_retained_bytes = limit;
            }))
        },
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes
            && limit.operation == "retain NX control value id"),
        "{error:?}"
    );
}

#[test]
fn data_block_control_value_route_refuses_scoped_limit() {
    let error = control_value_route_refusal(|policy| policy.limits.max_materialized_bytes = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes
            && limit.operation == "NX control value block id"),
        "{error:?}"
    );
}

#[test]
fn data_block_control_value_route_refuses_work_limit() {
    let error = control_value_route_refusal(|policy| policy.limits.max_work_units = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits
            && limit.operation == "NX control value block id"),
        "{error:?}"
    );
}

#[test]
fn native_catalog_separates_offset_only_blocks_from_object_records() {
    let file =
        prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", offset_only_indexed_om_section())]);
    let container =
        crate::test_support::with_decode_context(|ctx| container::scan_bytes(ctx, file))
            .expect("required invariant");

    assert!(
        crate::test_support::with_decode_context(|ctx| super::object_records(ctx, &container))
            .unwrap()
            .is_empty()
    );
    let blocks =
        crate::test_support::with_decode_context(|ctx| super::data_blocks(ctx, &container))
            .unwrap();
    assert_eq!(blocks.len(), 3);
    assert_eq!(blocks[0].block_ordinal, 0);
    assert_eq!(blocks[0].role, super::DataBlockRole::Control);
    assert_eq!(blocks[1].role, super::DataBlockRole::Column);
    for (ordinal, block) in blocks.iter().enumerate() {
        assert_eq!(
            usize::try_from(block.block_ordinal).expect("fixture value fits usize"),
            ordinal
        );
        assert_eq!(block.id, format!("nx:om-data-blocks-0:block#{ordinal}"));
    }
    assert!(blocks[0].byte_len > 0);
    assert!(blocks[0].stable_identity.is_some());
    let forms = crate::test_support::with_decode_context(|ctx| {
        super::data_block_control_forms(ctx, &container)
    })
    .unwrap();
    assert_eq!(forms.len(), 1);
    assert_eq!(forms[0].data_block, blocks[0].id);
    assert_eq!(
        forms[0].kind,
        super::DataBlockControlFormKind::ZeroPrefixed {
            value_count: std::num::NonZeroU32::new(2).unwrap()
        }
    );
    assert_eq!(forms[0].kind.value_count(), 2);
    assert_eq!(forms[0].kind.byte_len(), blocks[0].byte_len);
    let control_values = crate::test_support::with_decode_context(|ctx| {
        super::data_block_control_values(ctx, &container)
    })
    .unwrap();
    assert_eq!(control_values.len(), 2);
    assert_eq!(control_values[0].data_block, blocks[0].id);
    assert_eq!(control_values[0].ordinal, 0);
    assert_eq!(control_values[0].value.value(), 0);
    assert_eq!(control_values[1].value.value(), 1);
    let classes = crate::test_support::with_decode_context_over(
        &[0],
        |_| {},
        |ctx| super::data_block_control_class_references(ctx, &container),
    )
    .expect("test OM class ordinals");
    assert_eq!(classes.len(), 1);
    assert_eq!(classes[0].data_block, blocks[0].id);
    assert_eq!(classes[0].ordinal, 0);
    assert_eq!(classes[0].class_ordinal, 0);
    assert_eq!(
        classes[0].class.as_ref().map(|class| class.name.as_str()),
        Some("UGS::ModlFeature")
    );
    assert_eq!(
        classes[0]
            .class
            .as_ref()
            .map(|class| class.definition.as_str()),
        Some("nx:om-entry-0:class#8")
    );
    assert!(
        crate::test_support::with_decode_context(|ctx| super::string_values(ctx, &container))
            .unwrap()
            .is_empty()
    );
    assert!(
        crate::test_support::with_decode_context(|ctx| super::object_references(ctx, &container))
            .unwrap()
            .is_empty()
    );
    let expressions = crate::test_support::with_decode_context_over(
        &[0],
        |_| {},
        |ctx| {
            let declarations = super::expression_declarations(ctx, &container).unwrap();
            super::expressions(ctx, &container, &declarations).unwrap()
        },
    );
    assert_eq!(expressions.len(), 1);
    assert_eq!(
        expressions[0].owner.as_ref().map(|owner| owner.object_id),
        None
    );
    assert_eq!(
        expressions[0].owner.as_ref().map(|owner| &owner.record),
        None
    );
}

#[test]
fn stable_data_block_identity_excludes_position_and_scopes_role() {
    let bytes = [0x01, 0x02, 0x03];
    let digest = |source, role| {
        crate::test_support::with_decode_context_over(
            &[0],
            |_| {},
            |ctx| super::data_block_digest(ctx, source, role, &bytes),
        )
        .unwrap()
    };
    let identity = digest("/Root/UG_PART/UG_PART", super::DataBlockRole::Column);
    assert_eq!(
        identity,
        digest("/Root/UG_PART/UG_PART", super::DataBlockRole::Column)
    );
    assert_ne!(
        identity,
        digest("/Root/UG_PART/UG_PART", super::DataBlockRole::Control)
    );
    assert_ne!(
        identity,
        digest("/Root/other", super::DataBlockRole::Column)
    );
}

#[test]
fn data_blocks_refuses_identity_work_at_caller_limit() {
    let file =
        prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", offset_only_indexed_om_section())]);
    let container = crate::test_support::with_decode_context_over(
        &[0],
        |_| {},
        |ctx| container::scan_bytes(ctx, &file),
    )
    .unwrap();
    crate::test_support::with_decode_context_over(
        &[0],
        |_| {},
        |ctx| container.indexed_om_sections(ctx),
    )
    .unwrap();

    crate::test_support::with_decode_context_over(
        &file,
        |policy| {
            policy.limits.max_work_units = 0;
        },
        |ctx| {
            let error = super::data_blocks(ctx, &container).expect_err("work refusal");
            assert!(matches!(
                error,
                cadmpeg_core::CodecError::ResourceLimit(limit)
                    if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits
                        && limit.operation == "nx data block identity digest"
            ));
        },
    );
}

#[test]
fn control_form_wire_checks_nonempty_counts_and_derived_length() {
    let json = r#"{"id":"c","data_block":"b","kind":"zero_prefixed","value_count":2,"byte_len":8,"source_offset":0}"#;
    let value: super::DataBlockControlForm = serde_json::from_str(json).unwrap();
    assert_eq!(serde_json::to_string(&value).unwrap(), json);
    for (field, invalid) in [("value_count", 0), ("byte_len", 0), ("byte_len", 7)] {
        let mut wire: serde_json::Value = serde_json::from_str(json).unwrap();
        wire[field] = invalid.into();
        assert!(serde_json::from_value::<super::DataBlockControlForm>(wire)
            .unwrap_err()
            .to_string()
            .contains(field));
    }
    let json = r#"{"id":"c","data_block":"b","kind":"product_anchored","value_count":2,"byte_len":1,"source_offset":0}"#;
    let value: super::DataBlockControlForm = serde_json::from_str(json).unwrap();
    assert_eq!(serde_json::to_string(&value).unwrap(), json);
    for field in ["value_count", "byte_len"] {
        let mut wire: serde_json::Value = serde_json::from_str(json).unwrap();
        wire[field] = 0.into();
        assert!(serde_json::from_value::<super::DataBlockControlForm>(wire)
            .unwrap_err()
            .to_string()
            .contains(field));
    }
}

#[test]
fn control_leading_value_preserves_wire_and_rejects_width_mismatch() {
    let json = r#"{"id":"c","data_block":"b","kind":"product_anchored","value_count":2,"leading_value_width":2,"leading_value":0,"byte_len":26,"source_offset":0}"#;
    let value: super::DataBlockControlForm = serde_json::from_str(json).unwrap();
    assert_eq!(serde_json::to_string(&value).unwrap(), json);
    let mut wire: serde_json::Value = serde_json::from_str(json).unwrap();
    wire["leading_value_width"] = 4.into();
    assert!(
        serde_json::from_value::<super::DataBlockControlForm>(wire.clone())
            .unwrap_err()
            .to_string()
            .contains("leading_value_width")
    );
    wire["leading_value_width"] = 2.into();
    wire["leading_value"] = 65536.into();
    assert!(serde_json::from_value::<super::DataBlockControlForm>(wire)
        .unwrap_err()
        .to_string()
        .contains("leading_value"));
}

fn control_index_value_route_refusal(
    configure: impl FnOnce(&mut cadmpeg_core::decode::DecodePolicy),
) -> cadmpeg_core::CodecError {
    let file = prt_with_named_payloads(&[(
        "/Root/UG_PART/UG_PART",
        offset_only_indexed_om_section_with_index_values(),
    )]);
    let container = crate::test_support::with_decode_context(|ctx| {
        crate::container::scan_bytes(ctx, file.as_slice())
    })
    .expect("product-anchored control fixture");
    crate::test_support::with_decode_context(|ctx| container.indexed_om_sections(ctx))
        .expect("cached product-anchored section");
    let values = crate::test_support::with_decode_context(|ctx| {
        super::data_block_control_index_values(ctx, &container)
    })
    .expect("control-index projection");
    assert_eq!(values.len(), 2);

    crate::test_support::with_decode_context_over(
        &[],
        |policy| {
            configure(policy);
        },
        |ctx| super::data_block_control_index_values(ctx, &container).unwrap_err(),
    )
}

#[test]
fn data_block_control_index_value_route_refuses_collection_limit() {
    let error = control_index_value_route_refusal(|policy| policy.limits.max_collection_items = 4);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems
            && limit.operation == "NX data block control index values"),
        "{error:?}"
    );
}

#[test]
fn data_block_control_index_value_route_refuses_retained_limit() {
    let error = control_index_value_route_refusal(|policy| policy.limits.max_retained_bytes = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes),
        "{error:?}"
    );
}

#[test]
fn data_block_control_index_value_route_refuses_scoped_limit() {
    let error =
        control_index_value_route_refusal(|policy| policy.limits.max_materialized_bytes = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes
            && limit.operation == "NX control index value block id"),
        "{error:?}"
    );
}

#[test]
fn data_block_control_index_value_route_refuses_work_limit() {
    let error = control_index_value_route_refusal(|policy| policy.limits.max_work_units = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits
            && limit.operation == "NX control index value block id"),
        "{error:?}"
    );
}

#[test]
fn data_block_control_index_value_target_refuses_retained_limit() {
    let mut section = offset_only_indexed_om_section_with_index_values();
    let control = [0, 0, 7, 0, 0, 0, 0x20, 0x10, 0, 0];
    let at = section
        .windows(control.len())
        .position(|window| window == control)
        .expect("control prefix in synthetic section");
    section[at + 2..at + 6].copy_from_slice(&1u32.to_le_bytes());
    let file = prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", section)]);
    let container = crate::test_support::with_decode_context(|ctx| {
        crate::container::scan_bytes(ctx, file.as_slice())
    })
    .expect("in-range control target fixture");
    crate::test_support::with_decode_context(|ctx| container.indexed_om_sections(ctx))
        .expect("cached in-range section");
    let values = crate::test_support::with_decode_context(|ctx| {
        super::data_block_control_index_values(ctx, &container)
    })
    .expect("in-range control projection");
    assert_eq!(
        values[0].target_data_block.as_deref(),
        Some("nx:om-data-blocks-0:block#1")
    );

    let error = cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        "retain NX control index value target block",
        |limit| {
            crate::test_support::with_decode_context_over(
                &[],
                |policy| policy.limits.max_retained_bytes = limit,
                |ctx| super::data_block_control_index_values(ctx, &container),
            )
        },
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes
            && limit.operation == "retain NX control index value target block"),
        "{error:?}"
    );
}

#[test]
fn native_catalog_classifies_product_anchored_control_atomically() {
    let file = prt_with_named_payloads(&[(
        "/Root/UG_PART/UG_PART",
        offset_only_indexed_om_section_with_index_values(),
    )]);
    let container =
        crate::test_support::with_decode_context(|ctx| container::scan_bytes(ctx, file))
            .expect("required invariant");

    let forms = crate::test_support::with_decode_context(|ctx| {
        super::data_block_control_forms(ctx, &container)
    })
    .unwrap();
    assert_eq!(forms.len(), 1);
    assert_eq!(
        forms[0].kind,
        super::DataBlockControlFormKind::ProductAnchored {
            leading: Some(
                crate::om::control_leading_value::ControlLeadingValue::from_wire(2, 0).unwrap()
            ),
            value_count: std::num::NonZeroU32::new(2).unwrap(),
            byte_len: std::num::NonZeroU64::new(26).unwrap(),
        }
    );
    assert_eq!(forms[0].kind.value_count(), 2);
    assert!(
        crate::test_support::with_decode_context(|ctx| super::data_block_control_values(
            ctx, &container
        ))
        .unwrap()
        .is_empty()
    );
    assert_eq!(
        crate::test_support::with_decode_context(|ctx| super::data_block_control_index_values(
            ctx, &container
        ))
        .unwrap()
        .len(),
        2
    );
}

#[test]
fn offset_store_class_identities_span_ordered_registries() {
    let mut store =
        offset_only_indexed_om_section_with_control(&[0, 1, 0, 0, 0, 10, 0, 0, 0, 5, 0, 0]);
    store.extend_from_slice(&size_framed_om_section());
    let file = prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", store)]);
    let container =
        crate::test_support::with_decode_context(|ctx| container::scan_bytes(ctx, file))
            .expect("required invariant");

    let classes = crate::test_support::with_decode_context_over(
        &[0],
        |_| {},
        |ctx| super::data_block_control_class_references(ctx, &container),
    )
    .expect("test OM class ordinals");
    assert_eq!(classes.len(), 1);
    assert_eq!(classes[0].class_ordinal, 1);
    assert_eq!(
        classes[0].class.as_ref().map(|class| class.name.as_str()),
        Some("UGS::FEATURE_RECORD")
    );
    assert!(classes[0].class.is_some());
}

#[test]
fn om_numeric_expression_retains_formula_without_literal_value() {
    let text = b"(Number [mm]) p9: p2 * 2 + p7_radius; ";
    let mut bytes = b"hostglobalvariables".to_vec();
    bytes.extend_from_slice(&[
        0x99,
        0x04,
        u8::try_from(text.len() + 2).expect("fixture value fits u8"),
    ]);
    bytes.extend_from_slice(text);
    bytes.push(0);

    let expressions = crate::test_support::with_decode_context_over(
        &[0],
        |_| {},
        |ctx| crate::om::numeric_expressions(ctx, &bytes).unwrap(),
    );
    assert_eq!(expressions.len(), 1);
    assert_eq!(expressions[0].name.as_str(), "p9");
    assert_eq!(expressions[0].expression, "p2 * 2 + p7_radius");
    assert_eq!(
        crate::test_support::with_decode_context(|ctx| expressions[0].constant_value(ctx)).unwrap(),
        None
    );
    assert_eq!(
        super::expression_parameter_names(expressions[0].expression).collect::<Vec<_>>(),
        vec!["p2", "p7_radius"]
    );
}

#[test]
fn decode_retains_typed_nx_numeric_expression() {
    let mut cur = Cursor::new(prt_with_indexed_om_section());
    let result = EditableDecodeResult::from(
        NxCodec
            .decode(&mut cur, &DecodeOptions::default())
            .expect("required invariant"),
    );
    let expressions = result
        .ir()
        .native
        .namespace("nx")
        .expect("NX namespace")
        .arena_as::<super::ParameterFormula>("expressions")
        .expect("required invariant");
    assert_eq!(expressions.len(), 1);
    assert_eq!(
        expressions[0].owner.as_ref().map(|owner| owner.object_id),
        Some(0x102)
    );
    assert_eq!(expressions[0].name.index(), Some(8));
    assert_eq!(
        expressions[0].name.qualifier(),
        Some("CircularPattern_pattern_Circular_Dir_offset_angle")
    );
    assert_eq!(
        expressions[0].name.as_str(),
        "p8_CircularPattern_pattern_Circular_Dir_offset_angle"
    );
    assert_eq!(expressions[0].unit, super::ExpressionUnit::Degree);
    assert_eq!(expressions[0].expression, "120");
    assert_eq!(
        expressions[0]
            .value
            .map(cadmpeg_ir::scalar::FiniteReal::get),
        Some(120.0)
    );
    assert_eq!(expressions[0].source_entry, "/Root/UG_PART/UG_PART");
    assert!(expressions[0]
        .source_table
        .as_str()
        .starts_with("nx:om-entry-0:expression-table#"));
    let declarations = result
        .ir()
        .native
        .namespace("nx")
        .expect("NX namespace")
        .arena_as::<super::ExpressionDeclaration>("expression_declarations")
        .expect("required invariant");
    assert_eq!(declarations.len(), 1);
    assert_eq!(declarations[0].object_id, 0x102);
    assert_eq!(declarations[0].name.index(), 8);
    assert_eq!(declarations[0].literal.as_deref(), Some("120"));
    assert_eq!(
        expressions[0].declaration.as_deref(),
        Some(declarations[0].id.as_str())
    );
    let parameter = result
        .ir()
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.name == expressions[0].name.as_str())
        .expect("required invariant");
    assert_eq!(
        parameter.properties.get("declaration"),
        Some(&declarations[0].id)
    );
    assert_eq!(
        parameter.properties.get("declaration_object_id"),
        Some(&"258".to_string())
    );
    let om_records = result
        .source_fidelity()
        .retained_records()
        .iter()
        .filter(|(id, _)| id.as_str().starts_with("nx:om-section-"))
        .map(|(_, record)| record)
        .collect::<Vec<_>>();
    assert_eq!(om_records.len(), 2);
    assert!(om_records.iter().all(|record| {
        record.data().is_some_and(|data| {
            cadmpeg_core::decode::u64_from_index(data.len()) == record.byte_len()
                && cadmpeg_ir::hash::sha256_hex(data) == record.sha256()
        })
    }));
    let object_records = result
        .ir()
        .native
        .namespace("nx")
        .expect("NX namespace")
        .arena_as::<super::ObjectRecord>("object_records")
        .expect("required invariant");
    assert_eq!(object_records.len(), 2);
    let headers = result
        .ir()
        .native
        .namespace("nx")
        .expect("NX namespace")
        .arena_as::<super::StoreHeader>("store_headers")
        .expect("required invariant");
    assert_eq!(headers.len(), 1);
    assert_eq!(headers[0].header().version.as_str(), "NX 2027.3102");
    let super::StoreHeader::Fixed(header) = &headers[0] else {
        panic!("ID-bounded store header");
    };
    assert_eq!(header.object_id, 0x101);
    assert_eq!(object_records[1].object_id.0, 0x102);
    assert_eq!(
        object_records[1].object_id.1,
        object_records[0].object_id.1 + 4
    );
    assert_eq!(
        expressions[0].owner.as_ref().map(|owner| &owner.record),
        Some(&object_records[1].id)
    );
    assert_eq!(object_records[1].record_ordinal, 1);
    assert_eq!(
        object_records[0].section_offset,
        object_records[1].section_offset
    );
    assert_eq!(object_records[1].byte_len, om_records[1].byte_len());
    assert_eq!(object_records[1].sha256.as_str(), om_records[1].sha256());
    assert_eq!(
        object_records[1].dependencies,
        vec![object_records[0].id.clone()]
    );
    assert_eq!(
        object_records[0].dependents,
        vec![object_records[1].id.clone()]
    );
    let strings = result
        .ir()
        .native
        .namespace("nx")
        .expect("NX namespace")
        .arena_as::<super::StringValue>("string_values")
        .expect("required invariant");
    assert_eq!(strings.len(), 1);
    assert_eq!(strings[0].record, object_records[1].id);
    assert_eq!(strings[0].object_id, 0x102);
    assert_eq!(strings[0].value.as_str(), "SKETCH_001");
    let references = result
        .ir()
        .native
        .namespace("nx")
        .expect("NX namespace")
        .arena_as::<super::ObjectReference>("object_references")
        .expect("required invariant");
    assert_eq!(references.len(), 3);
    assert_eq!(references[0].record, object_records[1].id);
    assert_eq!(references[0].object_id, 0x102);
    let wire = serde_json::to_value(&references).unwrap();
    assert_eq!(wire[0]["value"], 0x1234_5678);
    assert_eq!(wire[0]["target_record"], serde_json::Value::Null);
    assert_eq!(wire[1]["kind"], "tagged28");
    assert_eq!(wire[1]["value"], 0x0abc_def0);
    assert_eq!(wire[1]["target_record"], serde_json::Value::Null);
    assert_eq!(wire[2]["kind"], "record_ordinal16");
    assert_eq!(wire[2]["value"], 0);
    assert_eq!(
        wire[2]["target_record"].as_str(),
        Some(object_records[0].id.as_str())
    );
    let handles = result
        .ir()
        .native
        .namespace("nx")
        .expect("NX namespace")
        .arena_as::<super::PersistentHandle>("persistent_handles")
        .expect("required invariant");
    assert_eq!(handles.len(), 1);
    assert_eq!(handles[0].value, 0x1234_5678);
    assert_eq!(handles[0].records, vec![object_records[1].id.clone()]);
    assert_eq!(handles[0].occurrence_count, 1);
    assert!(handles[0].external_records.is_empty());
    assert_eq!(result.ir().model.features.len(), 1);
    assert!(matches!(
        result.ir().model.features[0].evaluation.definition(),
        cadmpeg_ir::features::FeatureDefinition::Operation(
            cadmpeg_ir::features::FeatureOperation::TreeNode {
                role: cadmpeg_ir::features::FeatureTreeNodeRole::Equations,
                ..
            }
        )
    ));
    assert_eq!(result.ir().model.features[0].suppressed, Some(false));
    assert_eq!(result.ir().model.parameters.len(), 1);
    assert_eq!(result.ir().model.parameters[0].expression, "120");
    let parameter = &result.ir().model.parameters[0];
    assert_eq!(parameter.name, expressions[0].name.as_str());
    assert!(matches!(
        parameter.value,
        Some(cadmpeg_ir::features::ParameterValue::Angle(
            value
        )) if value.get() == 120_f64.to_radians()
    ));
    assert_eq!(parameter.native_ref.as_ref(), Some(&expressions[0].id));
    let validation = cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "findings: {:?}", validation.findings);
}

#[test]
fn nx_part_attributes_require_typed_atomic_xml() {
    let xml = br#"<?xml version="1.0" encoding="UTF-8"?>
<UgAttributes version="4" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance">
  <Attribute owner="part" pdmBased="false" title="legacy" utf8title="Material"
    value="legacy-value" utf8value="Steel" version="3" xsi:type="StringAttributeType"/>
</UgAttributes>"#;
    let attributes = crate::test_support::with_decode_context(|ctx| {
        super::parse_part_attributes(ctx, xml, 7, "/Root/part/attrs", 100)
    })
    .expect("typed attribute budget")
    .expect("typed attributes");
    assert_eq!(attributes.len(), 1);
    assert_eq!(attributes[0].id, "nx:part-attributes-7:attribute#0");
    assert_eq!(attributes[0].title, "Material");
    assert_eq!(attributes[0].value, "Steel");
    assert_eq!(attributes[0].value_type, "StringAttributeType");
    assert!(!attributes[0].pdm_based);
    assert!(attributes[0].source_offset > 100);

    let mut terminated = xml.to_vec();
    terminated.push(0);
    assert_eq!(
        crate::test_support::with_decode_context(|ctx| {
            super::parse_part_attributes(ctx, &terminated, 7, "/Root/part/attrs", 100)
        })
        .expect("terminated attribute budget")
        .expect("terminated typed attributes"),
        attributes
    );
    terminated.push(0);
    assert!(crate::test_support::with_decode_context(|ctx| {
        super::parse_part_attributes(ctx, &terminated, 7, "/Root/part/attrs", 100)
    })
    .expect("malformed attribute budget")
    .is_none());

    let malformed = xml
        .windows(b"pdmBased=\"false\"".len())
        .position(|window| window == b"pdmBased=\"false\"")
        .map(|at| {
            let mut malformed = xml.to_vec();
            malformed[at + b"pdmBased=\"".len()..at + b"pdmBased=\"false".len()]
                .copy_from_slice(b"maybe");
            malformed
        })
        .expect("required invariant");
    assert!(crate::test_support::with_decode_context(|ctx| {
        super::parse_part_attributes(ctx, &malformed, 7, "/Root/part/attrs", 100)
    })
    .expect("invalid attribute budget")
    .is_none());
}

#[test]
fn decode_retains_length_framed_nx_class_definition() {
    let mut cur = Cursor::new(prt_with_indexed_om_section());
    let result = NxCodec
        .decode(&mut cur, &DecodeOptions::default())
        .expect("required invariant");
    let classes = result
        .ir()
        .native
        .namespace("nx")
        .expect("NX namespace")
        .arena_as::<super::ClassDefinition>("class_definitions")
        .expect("required invariant");
    assert_eq!(classes.len(), 1);
    assert_eq!(classes[0].name, "UGS::EXP_expression");
    assert_eq!(classes[0].ordinal, 0);
    assert_eq!(classes[0].trailing_code, 0x81);
    assert_eq!(classes[0].source_entry, "/Root/UG_PART/UG_PART");
}

#[test]
fn decode_retains_length_framed_nx_field_definitions() {
    let mut cur = Cursor::new(prt_with_size_framed_om_section());
    let result = NxCodec
        .decode(&mut cur, &DecodeOptions::default())
        .expect("required invariant");
    let fields = result
        .ir()
        .native
        .namespace("nx")
        .expect("NX namespace")
        .arena_as::<super::FieldDefinition>("field_definitions")
        .expect("required invariant");
    let fields: Vec<_> = fields
        .iter()
        .cloned()
        .map(super::FieldDefinitionWire::from)
        .collect();
    assert_eq!(fields.len(), 2);
    assert_eq!(fields[0].name, "m_target");
    assert_eq!(fields[0].ordinal, 0);
    assert_eq!(fields[0].registry_storage_code, Some(2));
    assert_eq!(fields[0].registry_owner_class, Some(2));
    assert_eq!(fields[0].registry_suffix, [0x01, 0x02]);
    assert_eq!(fields[0].layout_prefix, Vec::<u8>::new());
    assert_eq!(fields[0].schema_fingerprint, None);
    assert_eq!(fields[0].layout_terminal, None);
    assert_eq!(fields[1].name, "m_tools");
    assert_eq!(fields[1].trailing_code, 0x81);
    assert!(fields[1].registry_suffix.is_empty());
    assert_eq!(fields[1].source_entry, "/Root/UG_PART/UG_PART");
    let layout = super::registry_layout(&[
        0x81, 0x21, 0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef, 0x06,
    ]);
    let prefix = layout
        .as_ref()
        .map_or_else(Vec::new, |layout| layout.prefix.to_vec());
    let fingerprint = layout.as_ref().map(|layout| layout.fingerprint);
    let terminal = layout.as_ref().map(|layout| layout.terminal);
    assert_eq!(prefix, [0x81, 0x21]);
    assert_eq!(
        fingerprint,
        Some([0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef])
    );
    assert_eq!(terminal, Some(0x06));
    let classes = result
        .ir()
        .native
        .namespace("nx")
        .expect("NX namespace")
        .arena_as::<super::ClassDefinition>("class_definitions")
        .expect("required invariant");
    let classes: Vec<_> = classes
        .iter()
        .cloned()
        .map(super::ClassDefinitionWire::from)
        .collect();
    assert_eq!(classes[0].layout_prefix, &[0x81, 0x21]);
    assert_eq!(
        classes[0].schema_fingerprint,
        Some([0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef])
    );
    assert_eq!(classes[0].layout_terminal, Some(0x06));
}

#[test]
fn class_registry_metadata_requires_a_complete_tail() {
    let legacy_definition = crate::om::TypeDefinition {
        offset: 0,
        name: "UGS::FEATURE_RECORD",
        registry_tail: &[
            0xa0, 0x81, 0x21, 0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef, 0x06,
        ],
    };

    let legacy = super::ClassDefinitionWire::from(super::ClassDefinition {
        id: String::new(),
        name: legacy_definition.name.into(),
        ordinal: 0,
        trailing_code: legacy_definition.registry_tail[0],
        registry_suffix: legacy_definition.registry_tail[1..].to_vec(),
        section_offset: 0,
        source_entry: String::new(),
        source_offset: 0,
    });
    assert_eq!(legacy.registry_storage_code, None);
    assert_eq!(legacy.registry_base_class, None);
    assert_eq!(legacy.registry_reference, None);
    assert_eq!(
        legacy.schema_fingerprint,
        Some([0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef])
    );
    assert_eq!(legacy.layout_terminal, Some(0x06));

    let complete_definition = crate::om::TypeDefinition {
        offset: 0,
        name: "UGS::FEATURE_RECORD",
        registry_tail: &[
            0x38, 0x05, 0x10, 0x20, 0x30, 0x40, 0x50, 0x60, 0x70, 0x80, 0x02,
        ],
    };
    let complete = super::ClassDefinitionWire::from(super::ClassDefinition {
        id: String::new(),
        name: complete_definition.name.into(),
        ordinal: 0,
        trailing_code: complete_definition.registry_tail[0],
        registry_suffix: complete_definition.registry_tail[1..].to_vec(),
        section_offset: 0,
        source_entry: String::new(),
        source_offset: 0,
    });
    assert_eq!(complete.registry_storage_code, Some(0x38));
    assert_eq!(complete.registry_base_class, Some(0x05));
    assert_eq!(complete.registry_reference, Some(0x02));
    assert_eq!(
        complete.schema_fingerprint,
        Some([0x10, 0x20, 0x30, 0x40, 0x50, 0x60, 0x70, 0x80])
    );
    assert_eq!(complete.layout_terminal, None);
}

#[test]
fn decode_retains_nx_arrangement_configurations() {
    let mut cur = Cursor::new(prt_with_arrangements());
    let result = NxCodec
        .decode(&mut cur, &DecodeOptions::default())
        .expect("required invariant");
    let configurations = result
        .ir()
        .native
        .namespace("nx")
        .expect("NX namespace")
        .arena_as::<super::Configuration>("configurations")
        .expect("required invariant");
    assert_eq!(configurations.len(), 2);
    assert_eq!(configurations[0].name, "Model");
    assert!(configurations[0].is_default);
    assert_eq!(configurations[1].name, "Exploded");
    assert!(!configurations[1].is_default);
    assert_eq!(result.ir().model.configurations.len(), 2);
    assert_eq!(result.ir().model.configurations[0].ordinal, 0);
    assert_eq!(result.ir().model.configurations[0].source_index, Some(0));
    assert_eq!(
        result.ir().model.configurations[0].name.as_deref(),
        Some("Model")
    );
    assert!(result.ir().model.configurations[0].active);
    assert_eq!(
        result.ir().model.configurations[0].bodies.as_deref(),
        Some(
            result
                .ir()
                .model
                .bodies
                .iter()
                .map(|body| body.id.clone())
                .collect::<Vec<_>>()
                .as_slice()
        )
    );
    assert_eq!(result.ir().model.configurations[1].ordinal, 1);
    assert_eq!(
        result.ir().model.configurations[1].name.as_deref(),
        Some("Exploded")
    );
    assert!(!result.ir().model.configurations[1].active);
    assert!(result.ir().model.configurations[1].bodies.is_none());
    let uses = result
        .ir()
        .native
        .namespace("nx")
        .expect("required invariant")
        .arena_as::<super::ConfigurationAttributeUse>("configuration_attribute_uses")
        .expect("required invariant");
    assert_eq!(uses.len(), 1);
    assert_eq!(uses[0].configuration, configurations[0].id);
    assert_eq!(uses[0].name, "Model");
    assert_eq!(
        result.ir().model.configurations[0].properties["active_attribute_use"],
        uses[0].id
    );
    let attributes = result
        .ir()
        .native
        .namespace("nx")
        .expect("required invariant")
        .arena_as::<super::PartAttribute>("part_attributes")
        .expect("required invariant");
    let mut mismatch = attributes.clone();
    mismatch[0].value = "Other".to_string();
    assert!(crate::test_support::with_decode_context(|ctx| {
        super::configuration_attribute_uses(ctx, &configurations, &mismatch)
    })
    .expect("mismatched configuration join")
    .is_empty());
    let mut duplicate = attributes.clone();
    duplicate.push(attributes[0].clone());
    assert!(crate::test_support::with_decode_context(|ctx| {
        super::configuration_attribute_uses(ctx, &configurations, &duplicate)
    })
    .expect("duplicate configuration join")
    .is_empty());
    let validation = cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "findings: {:?}", validation.findings);
}

#[test]
fn nx_neutral_active_configuration_requires_the_exact_attribute_join() {
    for active_name in [None, Some("Other")] {
        let mut cur = Cursor::new(prt_with_arrangement_attribute(active_name));
        let result = NxCodec
            .decode(&mut cur, &DecodeOptions::default())
            .expect("required invariant");
        let native = result
            .ir()
            .native
            .namespace("nx")
            .expect("required invariant")
            .arena_as::<super::Configuration>("configurations")
            .expect("required invariant");
        assert!(native[0].is_default);
        assert!(result
            .ir()
            .model
            .configurations
            .iter()
            .all(|configuration| !configuration.active && configuration.bodies.is_none()));
    }
}
mod expression_admission;
mod material_and_external_records;
mod material_catalog_admission;
mod record_area_admission;

mod expression_graph;
