// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn view_child_typecode_refuses_materialized_limit() {
    cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::MaterializedBytes,
        "Rhino view child typecode",
        |cap| Err::<(), _>(view_record_materialized_refusal(cap)),
    );
}

#[test]
fn view_child_sha256_refuses_materialized_limit() {
    cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::MaterializedBytes,
        "Rhino view child SHA-256",
        |cap| Err::<(), _>(view_record_materialized_refusal(cap)),
    );
}

#[test]
fn view_id_refuses_materialized_limit_and_losses_remain_retained() {
    assert!(
        matches!(view_record_retained_refusal(crate::test_support::retained_limit_at("Rhino view loss message", 0, |cap| { match view_record_retained_refusal(cap) { cadmpeg_core::CodecError::ResourceLimit(limit) => limit, error => panic!("unexpected fixture refusal: {error:?}") } })), cadmpeg_core::CodecError::ResourceLimit(limit) if limit.operation == "Rhino view loss message")
    );
    cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::MaterializedBytes,
        "Rhino view ID",
        |cap| Err::<(), _>(view_record_materialized_refusal(cap)),
    );
    let archive = ArchiveVersion::V5;
    let (bytes, record) = one_end_marker_view(archive);
    let service_context = cadmpeg_test_support::service_decode_context();
    let admitted = {
        let context = &service_context;
        let mut staging = context
            .reserve_scoped(0, "Rhino test view staging")
            .unwrap();
        parse_list(
            context,
            &bytes,
            &record,
            archive,
            crate::settings::MillimeterScale::IDENTITY,
            ViewListKind::Named,
            &mut staging,
        )
    }
    .expect("view record fits service profile");
    assert_eq!(admitted.0.len(), 1);
}

#[test]
fn view_name_refuses_materialized_limit() {
    let archive = ArchiveVersion::V5;
    let mut view_body = crc_chunk(archive, super::super::VIEW_NAME, &utf16_bytes("saved view"));
    view_body.extend(short_chunk(archive, super::super::TCODE_ENDOFTABLE, 0));
    let mut bytes = 1_i32.to_le_bytes().to_vec();
    bytes.extend(crc_chunk(archive, super::super::VIEW_RECORD, &view_body));
    let record = Record::long(super::super::NAMED_VIEWS, 0..bytes.len(), 0..bytes.len());
    let error = with_materialized_limit(
        &bytes,
        materialized_limit_at("Rhino view name", |cap| {
            match with_materialized_limit(&bytes, cap, |ctx| {
                {
                    let context = ctx;
                    let mut staging = context
                        .reserve_scoped(0, "Rhino test view staging")
                        .unwrap();
                    parse_list(
                        context,
                        &bytes,
                        &record,
                        archive,
                        crate::settings::MillimeterScale::IDENTITY,
                        ViewListKind::Named,
                        &mut staging,
                    )
                }
                .expect_err("view name exceeds materialized limit")
            }) {
                cadmpeg_core::CodecError::ResourceLimit(limit) => limit,
                error => panic!("unexpected resource refusal: {error:?}"),
            }
        }),
        |ctx| {
            {
                let context = ctx;
                let mut staging = context
                    .reserve_scoped(0, "Rhino test view staging")
                    .unwrap();
                parse_list(
                    context,
                    &bytes,
                    &record,
                    archive,
                    crate::settings::MillimeterScale::IDENTITY,
                    ViewListKind::Named,
                    &mut staging,
                )
            }
            .expect_err("view name exceeds materialized limit")
        },
    );
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "Rhino view name"
    ));
}

#[test]
fn view_printer_name_refuses_retained_limit() {
    let bytes = view_attributes_with_page(2, "printer", false, [0; 16], [0; 16], [0; 16]);
    let error = with_retained_limit(&bytes, 0, |ctx| {
        parse_attributes(
            ctx,
            &bytes,
            0..bytes.len(),
            ArchiveVersion::V5,
            crate::settings::MillimeterScale::IDENTITY,
        )
        .expect_err("printer name exceeds retained limit")
    });
    assert_resource(&error, "Rhino view printer name");
}

#[test]
fn view_display_uuid_refuses_retained_limit() {
    let bytes = view_attributes_with_page(2, "", false, [0x11; 16], [0; 16], [0; 16]);
    let error = with_retained_limit(&bytes, 0, |ctx| {
        parse_attributes(
            ctx,
            &bytes,
            0..bytes.len(),
            ArchiveVersion::V5,
            crate::settings::MillimeterScale::IDENTITY,
        )
        .expect_err("display UUID exceeds retained limit")
    });
    assert_resource(&error, "Rhino view display UUID");
    let (admitted, _) = parse_attributes(
        &cadmpeg_test_support::service_decode_context(),
        &bytes,
        0..bytes.len(),
        ArchiveVersion::V5,
        crate::settings::MillimeterScale::IDENTITY,
    )
    .expect("display UUID fits service profile");
    assert!(admitted.display.is_some());
}

#[test]
fn view_clipping_plane_uuid_refuses_retained_limit() {
    let bytes = view_attributes_with_page(4, "", true, [0; 16], [0x11; 16], [0; 16]);
    let error = with_retained_limit(
        &bytes,
        crate::test_support::retained_limit_at("Rhino view clipping plane UUID", 0, |cap| {
            match with_retained_limit(&bytes, cap, |ctx| {
                parse_attributes(
                    ctx,
                    &bytes,
                    0..bytes.len(),
                    ArchiveVersion::V5,
                    crate::settings::MillimeterScale::IDENTITY,
                )
                .expect_err("clipping plane UUID exceeds retained limit")
            }) {
                FramingError::Resource(limit) => limit,
                error => panic!("unexpected resource refusal: {error:?}"),
            }
        }),
        |ctx| {
            parse_attributes(
                ctx,
                &bytes,
                0..bytes.len(),
                ArchiveVersion::V5,
                crate::settings::MillimeterScale::IDENTITY,
            )
            .expect_err("clipping plane UUID exceeds retained limit")
        },
    );
    assert_resource(&error, "Rhino view clipping plane UUID");
}

#[test]
fn named_view_uuid_refuses_retained_limit() {
    let bytes = view_attributes_with_page(5, "", true, [0; 16], [0; 16], [0x11; 16]);
    let error = with_retained_limit(
        &bytes,
        crate::test_support::retained_limit_at("Rhino named view UUID", 0, |cap| {
            match with_retained_limit(&bytes, cap, |ctx| {
                parse_attributes(
                    ctx,
                    &bytes,
                    0..bytes.len(),
                    ArchiveVersion::V5,
                    crate::settings::MillimeterScale::IDENTITY,
                )
                .expect_err("named-view UUID exceeds retained limit")
            }) {
                FramingError::Resource(limit) => limit,
                error => panic!("unexpected resource refusal: {error:?}"),
            }
        }),
        |ctx| {
            parse_attributes(
                ctx,
                &bytes,
                0..bytes.len(),
                ArchiveVersion::V5,
                crate::settings::MillimeterScale::IDENTITY,
            )
            .expect_err("named-view UUID exceeds retained limit")
        },
    );
    assert_resource(&error, "Rhino named view UUID");
}

#[test]
fn view_attribute_checksum_children_refuse_collection_limit() {
    let bytes = view_attributes_with_page(2, "", false, [0; 16], [0; 16], [0; 16]);
    let error = with_collection_limit(&bytes, 0, |ctx| {
        parse_attributes(
            ctx,
            &bytes,
            0..bytes.len(),
            ArchiveVersion::V5,
            crate::settings::MillimeterScale::IDENTITY,
        )
        .expect_err("page checksum child exceeds collection limit")
    });
    assert_resource(&error, "Rhino view attribute checksum children");
}

#[test]
fn view_clipping_planes_refuse_collection_limit() {
    let bytes = view_attributes_with_page(4, "", true, [0; 16], [0; 16], [0; 16]);
    let error = with_collection_limit(&bytes, 1, |ctx| {
        parse_attributes(
            ctx,
            &bytes,
            0..bytes.len(),
            ArchiveVersion::V5,
            crate::settings::MillimeterScale::IDENTITY,
        )
        .expect_err("clipping plane exceeds collection limit")
    });
    assert_resource(&error, "Rhino view clipping planes");
    let (attributes, _) = parse_attributes(
        &cadmpeg_test_support::service_decode_context(),
        &bytes,
        0..bytes.len(),
        ArchiveVersion::V5,
        crate::settings::MillimeterScale::IDENTITY,
    )
    .expect("service profile admits the clipping plane");
    assert_eq!(attributes.clipping_planes.len(), 1);
}

#[test]
fn view_checksum_children_refuse_collection_limit() {
    assert!(matches!(
        view_collection_refusal(0),
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "Rhino view checksum children"
    ));
}

#[test]
fn view_children_refuse_collection_limit() {
    assert!(matches!(
        view_collection_refusal(1),
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "Rhino view children"
    ));
}

#[test]
fn view_list_records_refuse_collection_limit() {
    assert!(matches!(
        view_collection_refusal(2),
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "Rhino view losses"
    ));
    reaches_resource_operation(2, "Rhino view list records", view_collection_refusal);
}

#[test]
fn viewport_userdata_children_refuse_collection_limit() {
    let archive = ArchiveVersion::V5;
    let bytes = short_chunk(archive, super::super::TCODE_CLASS_END, 0);
    let error = with_collection_limit(&bytes, 0, |ctx| {
        super::super::scan_viewport_userdata(ctx, &bytes, 0..bytes.len(), archive, &mut Vec::new())
            .err()
            .expect("class end exceeds the collection limit")
    });
    assert_resource(&error, "Rhino viewport userdata children");
}

#[test]
fn viewport_userdata_children_ceiling_is_a_resource_refusal() {
    let archive = ArchiveVersion::V8;
    let cap = super::super::VIEWPORT_USERDATA_CHILD_CAP;
    let child = short_chunk(archive, crate::chunks::TCODE_SHORT | 7, 0);
    let mut bytes = child.repeat(cap);
    bytes.extend(short_chunk(archive, super::super::TCODE_CLASS_END, 0));
    with_collection_limit(
        &bytes,
        u64::try_from(cap + 1).expect("fixture count fits"),
        |ctx| {
            let mut losses = Vec::new();
            let error = super::super::scan_viewport_userdata(
                ctx,
                &bytes,
                0..bytes.len(),
                archive,
                &mut losses,
            )
            .err()
            .expect("child ceiling refuses the class-end slot");
            assert!(
                matches!(super::super::codec_error(ctx, error).expect("resource conversion"), cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::Codec("Rhino viewport userdata children")
                && limit.limit == u64::try_from(cap).expect("fixture count fits")
                && limit.used == limit.limit && limit.additional == 1
                && Some(limit) == ctx.resource_refusal())
            );
            assert!(losses.is_empty());
        },
    );
}
