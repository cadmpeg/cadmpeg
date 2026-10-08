// SPDX-License-Identifier: Apache-2.0
//! Control record projection and resource admission.

use crate::native::om as native_om;
use crate::container;
use crate::test_support::test_om::{offset_only_indexed_om_section, offset_only_indexed_om_section_with_control, offset_only_indexed_om_section_with_index_values};
use crate::test_support::test_prt::prt_with_named_payloads;

fn control_form_matches(
    control: &[u8],
    first_record: Option<&[u8]>,
    expected: Option<crate::om::OffsetStoreControlForm>,
) -> bool {
    crate::test_support::with_decode_context(|ctx| -> Result<bool, cadmpeg_core::CodecError> {
        match crate::om::offset_store_control_form(ctx, control, first_record)? {
            Some((form, storage)) => {
                let matches = expected.as_ref() == Some(&form);
                drop(form);
                drop(storage);
                Ok(matches)
            }
            None => Ok(expected.is_none()),
        }
    })
    .unwrap()
}

fn warm_indexed_sections(container: &crate::container::Container) {
    crate::test_support::with_decode_context(|ctx| {
        let (sections, storage) = container.indexed_om_sections(ctx)?;
        drop(sections);
        drop(storage);
        Ok::<(), cadmpeg_core::CodecError>(())
    })
    .expect("cached indexed sections");
}

#[test]
fn om_offset_store_values_precede_unique_product_anchor() {
    let mut bytes = vec![0, 0];
    bytes.extend_from_slice(&7u32.to_le_bytes());
    bytes.extend_from_slice(&0x1020u32.to_le_bytes());
    bytes.extend_from_slice(b"\x04\x01\x0eNX 2027.3102\0tail");
    assert!(control_form_matches(
        &bytes,
        None,
        Some(crate::om::OffsetStoreControlForm::ProductAnchored {
            leading_value: Some(
                crate::om::control_leading_value::ControlLeadingValue::from_wire(2, 0).unwrap()
            ),
            values: crate::om::nonempty::NonEmpty::new([7, 0x1020]).unwrap(),
        }),
    ));

    let mut nonzero_leading = vec![0x34, 0x12, 0x00];
    nonzero_leading.extend_from_slice(&7u32.to_le_bytes());
    nonzero_leading.extend_from_slice(b"\x04\x01\x0eNX 2027.3102\0tail");
    assert!(control_form_matches(
        &nonzero_leading,
        None,
        Some(crate::om::OffsetStoreControlForm::ProductAnchored {
            leading_value: Some(
                crate::om::control_leading_value::ControlLeadingValue::from_wire(3, 0x1234)
                    .unwrap()
            ),
            values: crate::om::nonempty::NonEmpty::new([7]).unwrap(),
        }),
    ));

    let mut duplicate = bytes;
    duplicate.extend_from_slice(b"\x04\x01\x0eNX 2027.3102\0");
    assert!(control_form_matches(&duplicate, None, None));
    assert_eq!(
        crate::test_support::with_decode_context(|ctx| native_om::control_index_data_block(
            ctx, 2, 700, 496
        ))
        .unwrap()
        .as_deref(),
        Some("nx:om-data-blocks-2:block#496")
    );
    assert!(
        crate::test_support::with_decode_context(|ctx| native_om::control_index_data_block(
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
    warm_indexed_sections(&container);
    let forms = crate::test_support::with_decode_context(|ctx| {
        native_om::data_block_control_forms(ctx, &container)
    })
    .expect("control form projection");
    assert_eq!(forms.len(), 1);
    assert_eq!(forms[0].id, "nx:om-data-block-control-forms:form#0");

    crate::test_support::with_decode_context_over(
        &[],
        |policy| {
            configure(policy);
        },
        |ctx| native_om::data_block_control_forms(ctx, &container).unwrap_err(),
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
        native_om::data_block_control_class_references(ctx, &container)
    })
    .expect("control class projection");
    assert_eq!(classes.len(), 1);

    crate::test_support::with_decode_context_over(
        &[],
        |policy| {
            configure(policy);
        },
        |ctx| {
            native_om::data_block_control_class_references(ctx, &container)
                .expect_err("control class resource refusal")
        },
    )
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
    let file =
        prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", offset_only_indexed_om_section())]);
    let container = crate::test_support::with_decode_context(|ctx| {
        crate::container::scan_bytes(ctx, file.as_slice())
    })
    .expect("control route fixture");
    crate::test_support::with_decode_context(|ctx| {
        let (sections, storage) = container.indexed_om_sections(ctx)?;
        drop(sections);
        drop(storage);
        Ok::<(), cadmpeg_core::CodecError>(())
    })
        .expect("cached control section");
    let values = crate::test_support::with_decode_context(|ctx| {
        native_om::data_block_control_forms(ctx, &container)
    })
    .expect("route succeeds before refusal");
    assert_eq!(values.len(), 1);
    assert_eq!(values[0].id, "nx:om-data-block-control-forms:form#0");
    let error = crate::test_support::resource_refusal_at(
        &[],
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        "NX data block control forms",
        |ctx| native_om::data_block_control_forms(ctx, &container),
    );
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
    let file =
        prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", offset_only_indexed_om_section())]);
    let container = crate::test_support::with_decode_context(|ctx| {
        crate::container::scan_bytes(ctx, file.as_slice())
    })
    .expect("control route fixture");
    crate::test_support::with_decode_context(|ctx| {
        let (sections, storage) = container.indexed_om_sections(ctx)?;
        drop(sections);
        drop(storage);
        Ok::<(), cadmpeg_core::CodecError>(())
    })
        .expect("cached control section");
    let values = crate::test_support::with_decode_context(|ctx| {
        native_om::data_block_control_forms(ctx, &container)
    })
    .expect("route succeeds before refusal");
    assert_eq!(values.len(), 1);
    assert_eq!(values[0].id, "nx:om-data-block-control-forms:form#0");
    let error = crate::test_support::resource_refusal_at(
        &[],
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "retain NX control form id",
        |ctx| native_om::data_block_control_forms(ctx, &container),
    );
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
    warm_indexed_sections(&container);
    let references = crate::test_support::with_decode_context(|ctx| {
        native_om::data_block_control_references(ctx, &container)
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
        |ctx| native_om::data_block_control_references(ctx, &container).unwrap_err(),
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
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::MaterializedBytes,
        "NX control reference block id",
        |limit| {
            Err::<(), _>(control_reference_route_refusal(|policy| {
                policy.limits.max_materialized_bytes = limit;
            }))
        },
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes
            && limit.operation == "NX control reference block id"),
        "{error:?}"
    );
}

#[test]
fn data_block_control_reference_route_refuses_work_limit() {
    let file = prt_with_named_payloads(&[(
        "/Root/UG_PART/UG_PART",
        offset_only_indexed_om_section_with_control(&[0xe0, 0, 0, 0, 1, 0xc0, 0, 0, 1]),
    )]);
    let container = crate::test_support::with_decode_context(|ctx| {
        crate::container::scan_bytes(ctx, file.as_slice())
    })
    .expect("control route fixture");
    crate::test_support::with_decode_context(|ctx| {
        let (sections, storage) = container.indexed_om_sections(ctx)?;
        drop(sections);
        drop(storage);
        Ok::<(), cadmpeg_core::CodecError>(())
    })
        .expect("cached control section");
    let values = crate::test_support::with_decode_context(|ctx| {
        native_om::data_block_control_references(ctx, &container)
    })
    .expect("route succeeds before refusal");
    assert_eq!(values.len(), 2);
    assert!(values[0]
        .id
        .starts_with("nx:om-data-block-control-references-0:reference#"));
    let error = crate::test_support::resource_refusal_at(
        &[],
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "NX control reference block id",
        |ctx| native_om::data_block_control_references(ctx, &container),
    );
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
    warm_indexed_sections(&container);
    let values = crate::test_support::with_decode_context(|ctx| {
        native_om::data_block_control_values(ctx, &container)
    })
    .expect("control-value projection");
    assert_eq!(values.len(), 2);

    crate::test_support::with_decode_context_over(
        &[],
        |policy| {
            configure(policy);
        },
        |ctx| native_om::data_block_control_values(ctx, &container).unwrap_err(),
    )
}

#[test]
fn data_block_control_value_route_refuses_collection_limit() {
    let file =
        prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", offset_only_indexed_om_section())]);
    let container = crate::test_support::with_decode_context(|ctx| {
        crate::container::scan_bytes(ctx, file.as_slice())
    })
    .expect("control route fixture");
    crate::test_support::with_decode_context(|ctx| {
        let (sections, storage) = container.indexed_om_sections(ctx)?;
        drop(sections);
        drop(storage);
        Ok::<(), cadmpeg_core::CodecError>(())
    })
        .expect("cached control section");
    let values = crate::test_support::with_decode_context(|ctx| {
        native_om::data_block_control_values(ctx, &container)
    })
    .expect("route succeeds before refusal");
    assert_eq!(values.len(), 2);
    let error = crate::test_support::resource_refusal_at(
        &[],
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        "NX data block control values",
        |ctx| native_om::data_block_control_values(ctx, &container),
    );
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
    let file =
        prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", offset_only_indexed_om_section())]);
    let container = crate::test_support::with_decode_context(|ctx| {
        crate::container::scan_bytes(ctx, file.as_slice())
    })
    .expect("control route fixture");
    crate::test_support::with_decode_context(|ctx| {
        let (sections, storage) = container.indexed_om_sections(ctx)?;
        drop(sections);
        drop(storage);
        Ok::<(), cadmpeg_core::CodecError>(())
    })
        .expect("cached control section");
    let values = crate::test_support::with_decode_context(|ctx| {
        native_om::data_block_control_values(ctx, &container)
    })
    .expect("route succeeds before refusal");
    assert_eq!(values.len(), 2);
    let error = crate::test_support::resource_refusal_at(
        &[],
        cadmpeg_core::decode::ResourceDimension::MaterializedBytes,
        "NX control value block id",
        |ctx| native_om::data_block_control_values(ctx, &container),
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes
            && limit.operation == "NX control value block id"),
        "{error:?}"
    );
}

#[test]
fn data_block_control_value_route_refuses_work_limit() {
    let file =
        prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", offset_only_indexed_om_section())]);
    let container = crate::test_support::with_decode_context(|ctx| {
        crate::container::scan_bytes(ctx, file.as_slice())
    })
    .expect("control route fixture");
    crate::test_support::with_decode_context(|ctx| {
        let (sections, storage) = container.indexed_om_sections(ctx)?;
        drop(sections);
        drop(storage);
        Ok::<(), cadmpeg_core::CodecError>(())
    })
        .expect("cached control section");
    let values = crate::test_support::with_decode_context(|ctx| {
        native_om::data_block_control_values(ctx, &container)
    })
    .expect("route succeeds before refusal");
    assert_eq!(values.len(), 2);
    let error = crate::test_support::resource_refusal_at(
        &[],
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "NX control value block id",
        |ctx| native_om::data_block_control_values(ctx, &container),
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits
            && limit.operation == "NX control value block id"),
        "{error:?}"
    );
}

#[test]
fn control_value_projection_charges_each_value_once() {
    let file =
        prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", offset_only_indexed_om_section())]);
    let container = crate::test_support::with_decode_context(|ctx| {
        crate::container::scan_bytes(ctx, file.as_slice())
    })
    .expect("control value fixture");
    warm_indexed_sections(&container);

    let error = crate::test_support::resource_refusal_at(
        &[],
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "NX control value projection",
        |ctx| native_om::data_block_control_values(ctx, &container),
    );
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits
            && limit.operation == "NX control value projection"
            && limit.additional == 1));
}

#[test]
fn control_form_wire_checks_nonempty_counts_and_derived_length() {
    let json = r#"{"id":"c","data_block":"b","kind":"zero_prefixed","value_count":2,"byte_len":8,"source_offset":0}"#;
    let value: native_om::DataBlockControlForm = serde_json::from_str(json).unwrap();
    assert_eq!(serde_json::to_string(&value).unwrap(), json);
    for (field, invalid) in [("value_count", 0), ("byte_len", 0), ("byte_len", 7)] {
        let mut wire: serde_json::Value = serde_json::from_str(json).unwrap();
        wire[field] = invalid.into();
        assert!(serde_json::from_value::<native_om::DataBlockControlForm>(wire)
            .unwrap_err()
            .to_string()
            .contains(field));
    }
    let json = r#"{"id":"c","data_block":"b","kind":"product_anchored","value_count":2,"byte_len":1,"source_offset":0}"#;
    let value: native_om::DataBlockControlForm = serde_json::from_str(json).unwrap();
    assert_eq!(serde_json::to_string(&value).unwrap(), json);
    for field in ["value_count", "byte_len"] {
        let mut wire: serde_json::Value = serde_json::from_str(json).unwrap();
        wire[field] = 0.into();
        assert!(serde_json::from_value::<native_om::DataBlockControlForm>(wire)
            .unwrap_err()
            .to_string()
            .contains(field));
    }
}

#[test]
fn control_leading_value_preserves_wire_and_rejects_width_mismatch() {
    let json = r#"{"id":"c","data_block":"b","kind":"product_anchored","value_count":2,"leading_value_width":2,"leading_value":0,"byte_len":26,"source_offset":0}"#;
    let value: native_om::DataBlockControlForm = serde_json::from_str(json).unwrap();
    assert_eq!(serde_json::to_string(&value).unwrap(), json);
    let mut wire: serde_json::Value = serde_json::from_str(json).unwrap();
    wire["leading_value_width"] = 4.into();
    assert!(
        serde_json::from_value::<native_om::DataBlockControlForm>(wire.clone())
            .unwrap_err()
            .to_string()
            .contains("leading_value_width")
    );
    wire["leading_value_width"] = 2.into();
    wire["leading_value"] = 65536.into();
    assert!(serde_json::from_value::<native_om::DataBlockControlForm>(wire)
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
    warm_indexed_sections(&container);
    let values = crate::test_support::with_decode_context(|ctx| {
        native_om::data_block_control_index_values(ctx, &container)
    })
    .expect("control-index projection");
    assert_eq!(values.len(), 2);

    crate::test_support::with_decode_context_over(
        &[],
        |policy| {
            configure(policy);
        },
        |ctx| native_om::data_block_control_index_values(ctx, &container).unwrap_err(),
    )
}

#[test]
fn data_block_control_index_value_route_refuses_collection_limit() {
    let file = prt_with_named_payloads(&[(
        "/Root/UG_PART/UG_PART",
        offset_only_indexed_om_section_with_index_values(),
    )]);
    let container = crate::test_support::with_decode_context(|ctx| {
        crate::container::scan_bytes(ctx, file.as_slice())
    })
    .expect("control route fixture");
    crate::test_support::with_decode_context(|ctx| {
        let (sections, storage) = container.indexed_om_sections(ctx)?;
        drop(sections);
        drop(storage);
        Ok::<(), cadmpeg_core::CodecError>(())
    })
        .expect("cached control section");
    let values = crate::test_support::with_decode_context(|ctx| {
        native_om::data_block_control_index_values(ctx, &container)
    })
    .expect("route succeeds before refusal");
    assert_eq!(values.len(), 2);
    let error = crate::test_support::resource_refusal_at(
        &[],
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        "NX data block control index values",
        |ctx| native_om::data_block_control_index_values(ctx, &container),
    );
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
    let file = prt_with_named_payloads(&[(
        "/Root/UG_PART/UG_PART",
        offset_only_indexed_om_section_with_index_values(),
    )]);
    let container = crate::test_support::with_decode_context(|ctx| {
        crate::container::scan_bytes(ctx, file.as_slice())
    })
    .expect("control route fixture");
    crate::test_support::with_decode_context(|ctx| {
        let (sections, storage) = container.indexed_om_sections(ctx)?;
        drop(sections);
        drop(storage);
        Ok::<(), cadmpeg_core::CodecError>(())
    })
        .expect("cached control section");
    let values = crate::test_support::with_decode_context(|ctx| {
        native_om::data_block_control_index_values(ctx, &container)
    })
    .expect("route succeeds before refusal");
    assert_eq!(values.len(), 2);
    let error = crate::test_support::resource_refusal_at(
        &[],
        cadmpeg_core::decode::ResourceDimension::MaterializedBytes,
        "NX control index value block id",
        |ctx| native_om::data_block_control_index_values(ctx, &container),
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes
            && limit.operation == "NX control index value block id"),
        "{error:?}"
    );
}

#[test]
fn data_block_control_index_value_route_refuses_work_limit() {
    let file = prt_with_named_payloads(&[(
        "/Root/UG_PART/UG_PART",
        offset_only_indexed_om_section_with_index_values(),
    )]);
    let container = crate::test_support::with_decode_context(|ctx| {
        crate::container::scan_bytes(ctx, file.as_slice())
    })
    .expect("control route fixture");
    crate::test_support::with_decode_context(|ctx| {
        let (sections, storage) = container.indexed_om_sections(ctx)?;
        drop(sections);
        drop(storage);
        Ok::<(), cadmpeg_core::CodecError>(())
    })
        .expect("cached control section");
    let values = crate::test_support::with_decode_context(|ctx| {
        native_om::data_block_control_index_values(ctx, &container)
    })
    .expect("route succeeds before refusal");
    assert_eq!(values.len(), 2);
    let error = crate::test_support::resource_refusal_at(
        &[],
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "NX control index value block id",
        |ctx| native_om::data_block_control_index_values(ctx, &container),
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits
            && limit.operation == "NX control index value block id"),
        "{error:?}"
    );
}

#[test]
fn control_index_projection_charges_each_value_once() {
    let file = prt_with_named_payloads(&[(
        "/Root/UG_PART/UG_PART",
        offset_only_indexed_om_section_with_index_values(),
    )]);
    let container = crate::test_support::with_decode_context(|ctx| {
        crate::container::scan_bytes(ctx, file.as_slice())
    })
    .expect("control index fixture");
    warm_indexed_sections(&container);

    let error = crate::test_support::resource_refusal_at(
        &[],
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "NX control index value projection",
        |ctx| native_om::data_block_control_index_values(ctx, &container),
    );
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits
            && limit.operation == "NX control index value projection"
            && limit.additional == 1));
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
    warm_indexed_sections(&container);
    let values = crate::test_support::with_decode_context(|ctx| {
        native_om::data_block_control_index_values(ctx, &container)
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
                |ctx| native_om::data_block_control_index_values(ctx, &container),
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
        native_om::data_block_control_forms(ctx, &container)
    })
    .unwrap();
    assert_eq!(forms.len(), 1);
    assert_eq!(
        forms[0].kind,
        native_om::DataBlockControlFormKind::ProductAnchored {
            leading: Some(
                crate::om::control_leading_value::ControlLeadingValue::from_wire(2, 0).unwrap()
            ),
            value_count: std::num::NonZeroU32::new(2).unwrap(),
            byte_len: std::num::NonZeroU64::new(26).unwrap(),
        }
    );
    assert_eq!(forms[0].kind.value_count(), 2);
    assert!(
        crate::test_support::with_decode_context(|ctx| native_om::data_block_control_values(
            ctx, &container
        ))
        .unwrap()
        .is_empty()
    );
    assert_eq!(
        crate::test_support::with_decode_context(|ctx| native_om::data_block_control_index_values(
            ctx, &container
        ))
        .unwrap()
        .len(),
        2
    );
}
