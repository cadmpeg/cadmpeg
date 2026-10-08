// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::ResourceDimension;
use cadmpeg_core::CodecError;

fn registry_search_refusal(operation: &str, creation_display: bool) {
    let container = if creation_display {
        crate::test_support::with_decode_context(|ctx| {
            crate::container::scan_bytes(
                ctx,
                crate::test_support::test_prt::prt_with_named_payloads(&[(
                    "/Root/FastLoad/RMFastLoad",
                    crate::test_support::test_om::size_framed_om_section_with_record_area(),
                )]),
            )
        })
        .expect("fastload registry container")
    } else {
        super::part_color_container()
    };
    crate::test_support::with_decode_context(|ctx| {
        container.om_sections(ctx).map(|(sections, _storage)| sections)?;
        container.indexed_om_sections(ctx).map(|_| ())
    })
    .expect("built section caches");
    if creation_display {
        let sections = crate::test_support::with_decode_context(|ctx| container.om_sections(ctx).map(|(sections, _storage)| sections))
            .expect("creation display source sections");
        assert!(sections.iter().any(|(entry, section)| {
            entry.name == "/Root/FastLoad/RMFastLoad"
                && section.record_area.is_some()
                && !section.types.is_empty()
        }));
    }
    let error = crate::test_support::resource_refusal_at(
        container.data.as_ref(),
        ResourceDimension::WorkUnits,
        operation,
        |ctx| {
            if creation_display {
                super::super::creation_display::rm_creation_display_data_relations(
                    ctx,
                    &container,
                    &[],
                )
                .map(|_| ())
            } else {
                super::super::part_color_tables(ctx, &container).map(|_| ())
            }
        },
    );
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::WorkUnits && limit.operation == operation));
}

#[test]
fn part_color_registry_search_preserves_work_refusal() {
    registry_search_refusal("NX part color registry types", false);
}

#[test]
fn creation_display_class_search_preserves_work_refusal() {
    registry_search_refusal("find NX creation display class", true);
}

fn store_search_refusal(offset_only: bool) {
    let file = if offset_only {
        crate::test_support::test_prt::prt_with_named_payloads(&[(
            "/Root/UG_PART/UG_PART",
            crate::test_support::test_om::offset_only_indexed_om_section(),
        )])
    } else {
        crate::test_support::test_prt::prt_with_indexed_om_section()
    };
    let container = crate::test_support::with_decode_context(|ctx| {
        let container = crate::container::scan_bytes(ctx, file)?;
        // Build the section cache once so every walk step charges the same route.
        container.indexed_om_sections(ctx).map(|(sections, _storage)| sections)?;
        Ok::<_, CodecError>(container)
    })
    .expect("indexed store container");
    let operation = if offset_only {
        "NX offset store version records"
    } else {
        "NX fixed store version records"
    };
    let error = crate::test_support::resource_refusal_at(
        container.data.as_ref(),
        ResourceDimension::WorkUnits,
        operation,
        |ctx| super::super::store_headers(ctx, &container).map(|_| ()),
    );
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::WorkUnits && limit.operation == operation));
}

#[test]
fn fixed_store_version_search_preserves_work_refusal() {
    store_search_refusal(false);
}

#[test]
fn offset_store_version_search_preserves_work_refusal() {
    store_search_refusal(true);
}

#[test]
fn column_storage_block_search_preserves_work_refusal() {
    let records = [crate::om::EntityRecord {
        offset: 4,
        bytes: &[1, 2, 3],
    }];
    let operation = "locate NX column row opening";
    let error = crate::test_support::resource_refusal_at(
        &[],
        ResourceDimension::WorkUnits,
        operation,
        |ctx| super::super::column_storage_block_at(ctx, &records, 5).map(|_| ()),
    );
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::WorkUnits && limit.operation == operation));
}
