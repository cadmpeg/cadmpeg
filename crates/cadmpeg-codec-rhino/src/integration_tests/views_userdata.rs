// SPDX-License-Identifier: Apache-2.0
//! Viewport class-userdata retention contracts.

use super::{assert_valid, decode};
use crate::chunks::ArchiveVersion;
use crate::test_support as support;

fn point(bytes: &mut Vec<u8>, value: [f64; 3]) {
    for coordinate in value {
        bytes.extend(coordinate.to_le_bytes());
    }
}

fn viewport_body() -> Vec<u8> {
    let mut bytes = vec![0x15];
    for value in [1_i32, 1, 1, 2] {
        bytes.extend(value.to_le_bytes());
    }
    point(&mut bytes, [1.0, 2.0, 3.0]);
    for vector in [
        [0.0, 0.0, -1.0],
        [0.0, 1.0, 0.0],
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
    ] {
        point(&mut bytes, vector);
    }
    for value in [-2.0_f64, 2.0, -1.0, 1.0, 0.1, 100.0] {
        bytes.extend(value.to_le_bytes());
    }
    for value in [0_i32, 1920, 0, 1080, 1, 100] {
        bytes.extend(value.to_le_bytes());
    }
    bytes.extend([0x11; 16]);
    bytes.extend([1, 0, 1, 0, 1]);
    point(&mut bytes, [4.0, 5.0, 6.0]);
    bytes.push(1);
    for value in [1.0_f64, 2.0, 3.0] {
        bytes.extend(value.to_le_bytes());
    }
    bytes
}

fn viewport_userdata_with_options(
    archive: ArchiveVersion,
    class_end_value: i64,
    corrupt_userdata_crc: bool,
) -> Vec<u8> {
    let application = crate::wire::Uuid::from_canonical([
        0x17, 0xb3, 0xec, 0xda, 0x17, 0xba, 0x4e, 0x45, 0x9e, 0x67, 0xa2, 0xb8, 0xd9, 0xbe, 0x52,
        0x0d,
    ])
    .to_wire();
    let payload = [
        2_i32.to_le_bytes().as_slice(),
        0_i32.to_le_bytes().as_slice(),
        [0xde, 0xad].as_slice(),
    ]
    .concat();
    let mut userdata = support::test_dump::class_userdata_v2_with_direct_payload(
        archive,
        crate::objects::USER_STRING_LIST.to_wire(),
        application,
        60,
        202_608_010,
        &payload,
    );
    if corrupt_userdata_crc {
        let outer_header_len = if archive.uses_eight_byte_values() {
            12
        } else {
            8
        };
        let header = crate::chunks::chunk_at(
            &userdata,
            outer_header_len + 1,
            userdata.len(),
            archive,
            false,
        )
        .expect("viewport userdata fixture has a class header");
        let crc = userdata
            .get_mut(header.body().end)
            .expect("viewport userdata class header has a checksum");
        *crc ^= 1;
    }
    let class_end = support::test_dump::short_chunk(archive, 0x8002_7fff, class_end_value);
    let class_end_start = userdata.len();
    let mut body = userdata;
    body.extend(&class_end);
    body.extend([0xca, 0xfe]);
    let class_end_range = class_end_start..class_end_start + class_end.len();
    support::test_dump::crc_chunk_excluding(
        archive,
        0x2000_8d3b,
        &body,
        &[0..class_end_start, class_end_range],
    )
}

fn viewport_userdata(archive: ArchiveVersion) -> Vec<u8> {
    viewport_userdata_with_options(archive, 0, false)
}

fn named_views_record(archive: ArchiveVersion) -> Vec<u8> {
    named_views_record_with_userdata(archive, viewport_userdata(archive))
}

fn named_views_record_with_userdata(archive: ArchiveVersion, userdata: Vec<u8>) -> Vec<u8> {
    let viewport = support::test_dump::crc_chunk(archive, 0x2000_823b, &viewport_body());
    let end_marker = support::test_dump::short_chunk(archive, crate::chunks::TCODE_ENDOFTABLE, 0);
    let mut view_body = viewport;
    let viewport_range = 0..view_body.len();
    let userdata_start = view_body.len();
    view_body.extend(userdata);
    let userdata_range = userdata_start..view_body.len();
    let end_start = view_body.len();
    view_body.extend(end_marker);
    let end_range = end_start..view_body.len();
    let view = support::test_dump::crc_chunk_excluding(
        archive,
        0x2000_803b,
        &view_body,
        &[viewport_range, userdata_range, end_range],
    );
    let mut list_body = 1_i32.to_le_bytes().to_vec();
    let view_range = list_body.len()..list_body.len() + view.len();
    list_body.extend(view);
    support::test_dump::crc_chunk_excluding(
        archive,
        0x2000_8036,
        &list_body,
        std::slice::from_ref(&view_range),
    )
}

fn view_with_children(
    archive: ArchiveVersion,
    children: &[Vec<u8>],
    end_marker: Option<Vec<u8>>,
) -> Vec<u8> {
    let mut view_body = Vec::new();
    let mut excluded_ranges =
        Vec::with_capacity(children.len() + usize::from(end_marker.is_some()));
    for child in children {
        let start = view_body.len();
        view_body.extend(child);
        excluded_ranges.push(start..view_body.len());
    }
    if let Some(end_marker) = end_marker {
        let start = view_body.len();
        view_body.extend(end_marker);
        excluded_ranges.push(start..view_body.len());
    }
    support::test_dump::crc_chunk_excluding(archive, 0x2000_803b, &view_body, &excluded_ranges)
}

fn view_with_child(archive: ArchiveVersion, child: Vec<u8>) -> Vec<u8> {
    view_with_children(
        archive,
        &[child],
        Some(support::test_dump::short_chunk(archive, 0xffff_ffff, 0)),
    )
}

fn trace_child(archive: ArchiveVersion, reference: &[u8]) -> Vec<u8> {
    let mut trace_body = vec![0x14];
    trace_body.extend(support::test_dump::utf16_bytes("trace-witness.png"));
    trace_body.extend(42.0_f64.to_le_bytes());
    trace_body.extend(24.0_f64.to_le_bytes());
    for point in [
        [0.0_f64, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
    ] {
        for coordinate in point {
            trace_body.extend(coordinate.to_le_bytes());
        }
    }
    trace_body.extend(
        [0.0_f64, 0.0, 1.0, 0.0]
            .into_iter()
            .flat_map(f64::to_le_bytes),
    );
    trace_body.extend([0, 1, 1]);
    let reference_start = trace_body.len();
    trace_body.extend(reference);
    let trace_reference_range = reference_start..trace_body.len();
    let trace = support::test_dump::crc_chunk_excluding(
        archive,
        0x2000_863b,
        &trace_body,
        std::slice::from_ref(&trace_reference_range),
    );
    trace
}

fn trace_view(archive: ArchiveVersion, reference: &[u8]) -> Vec<u8> {
    view_with_child(archive, trace_child(archive, reference))
}

fn wallpaper_view(archive: ArchiveVersion, reference: Vec<u8>) -> Vec<u8> {
    let mut wallpaper_body = vec![0x12];
    wallpaper_body.extend(support::test_dump::utf16_bytes("wallpaper-witness.png"));
    wallpaper_body.extend([1, 0]);
    let reference_start = wallpaper_body.len();
    wallpaper_body.extend(reference);
    let reference_range = reference_start..wallpaper_body.len();
    let wallpaper = support::test_dump::crc_chunk_excluding(
        archive,
        0x2000_874b,
        &wallpaper_body,
        std::slice::from_ref(&reference_range),
    );
    view_with_child(archive, wallpaper)
}

fn view_list_record(archive: ArchiveVersion, typecode: u32, views: &[Vec<u8>]) -> Vec<u8> {
    let mut list_body = (views.len() as i32).to_le_bytes().to_vec();
    let mut view_ranges = Vec::with_capacity(views.len());
    for view in views {
        let start = list_body.len();
        list_body.extend(view);
        view_ranges.push(start..list_body.len());
    }
    support::test_dump::crc_chunk_excluding(archive, typecode, &list_body, &view_ranges)
}

fn named_views_record_with_trace(archive: ArchiveVersion, corrupt_reference: bool) -> Vec<u8> {
    let mut reference =
        support::test_dump::file_reference(archive, "/trace/source.png", "source.png");
    if corrupt_reference {
        let last = reference.len() - 1;
        reference[last] ^= 1;
    }
    view_list_record(archive, 0x2000_8036, &[trace_view(archive, &reference)])
}

fn document_with_views(archive: ArchiveVersion, views: Vec<u8>) -> Vec<u8> {
    support::test_dump::minimal_document(
        &archive.value().to_string(),
        &[
            support::test_dump::table(archive, 0x1000_0014, &[]),
            support::test_dump::table(
                archive,
                0x1000_0015,
                &[support::test_dump::units_record(archive, 2), views],
            ),
            support::test_dump::table(archive, 0x1000_0013, &[]),
        ],
    )
}

fn source_offset(document: &[u8], fragment: &[u8]) -> usize {
    let mut locations = document
        .windows(fragment.len())
        .enumerate()
        .filter_map(|(offset, window)| (window == fragment).then_some(offset));
    let offset = locations
        .next()
        .expect("fixture fragment has a source location");
    assert!(
        locations.next().is_none(),
        "fixture fragment must have one source location"
    );
    offset
}

#[test]
#[should_panic(expected = "fixture fragment must have one source location")]
fn ambiguous_fixture_source_offsets_are_rejected() {
    source_offset(&[1, 2, 1, 2], &[1, 2]);
}

#[test]
fn viewport_userdata_future_payload_retains_typed_view_list_record() {
    let archive = ArchiveVersion::V8;
    let named_views = named_views_record(archive);
    let bytes = support::test_dump::minimal_document(
        "80",
        &[
            support::test_dump::table(archive, 0x1000_0014, &[]),
            support::test_dump::table(
                archive,
                0x1000_0015,
                &[
                    support::test_dump::units_record(archive, 2),
                    named_views.clone(),
                ],
            ),
            support::test_dump::table(archive, 0x1000_0013, &[]),
        ],
    );
    let result = decode(bytes);

    let views = &result.ir().native.namespace("rhino").unwrap().arenas()["views"];
    assert_eq!(views.len(), 1);
    assert!(result.report().losses.iter().any(|loss| {
        loss.code == crate::loss::RhinoLossCode::ViewportUserdataDropped.kind()
            && loss.message.contains("no typed CADIR owner")
    }));
    let retained = result
        .source_fidelity()
        .retained_records()
        .iter()
        .find(|(id, _)| {
            id.as_str()
                .starts_with("rhino:opaque:record#10000015-20008036-")
        })
        .map(|(_, record)| record)
        .expect("view list record is retained");
    assert_eq!(retained.data(), Some(named_views.as_slice()));
    assert_valid(&result);
}

#[test]
fn malformed_viewport_userdata_retains_typed_view_list_record() {
    let archive = ArchiveVersion::V8;
    let malformed_userdata = support::test_dump::crc_chunk(archive, 0x2000_8d3b, &[0xde, 0xad]);
    let named_views = named_views_record_with_userdata(archive, malformed_userdata);
    let bytes = support::test_dump::minimal_document(
        "80",
        &[
            support::test_dump::table(archive, 0x1000_0014, &[]),
            support::test_dump::table(
                archive,
                0x1000_0015,
                &[
                    support::test_dump::units_record(archive, 2),
                    named_views.clone(),
                ],
            ),
            support::test_dump::table(archive, 0x1000_0013, &[]),
        ],
    );
    let result = decode(bytes);

    let views = &result.ir().native.namespace("rhino").unwrap().arenas()["views"];
    assert_eq!(views.len(), 1);
    assert!(result.report().losses.iter().any(|loss| {
        loss.code == crate::loss::RhinoLossCode::ViewportUserdataDropped.kind()
            && loss.message.contains("could not be framed")
    }));
    let retained = result
        .source_fidelity()
        .retained_records()
        .iter()
        .find(|(id, _)| {
            id.as_str()
                .starts_with("rhino:opaque:record#10000015-20008036-")
        })
        .map(|(_, record)| record)
        .expect("malformed view list record is retained");
    assert_eq!(retained.data(), Some(named_views.as_slice()));
    assert_valid(&result);
}

#[test]
fn view_list_with_native_or_unavailable_units_is_retained_without_scale_one() {
    let archive = ArchiveVersion::V8;
    for unit in [Some(0), Some(255), None] {
        let named_views = named_views_record(archive);
        let settings = unit
            .map(|value| vec![support::test_dump::units_record(archive, value)])
            .unwrap_or_default();
        let bytes = support::test_dump::minimal_document(
            "80",
            &[
                support::test_dump::table(archive, 0x1000_0014, &[]),
                support::test_dump::table(
                    archive,
                    0x1000_0015,
                    &[settings, vec![named_views.clone()]].concat(),
                ),
                support::test_dump::table(archive, 0x1000_0013, &[]),
            ],
        );
        let result = decode(bytes);
        let views = &result.ir().native.namespace("rhino").unwrap().arenas()["views"];
        assert!(views.is_empty(), "unit {unit:?} unexpectedly entered CADIR");
        let retained = result
            .source_fidelity()
            .retained_records()
            .iter()
            .find(|(id, record)| {
                id.as_str()
                    .starts_with("rhino:opaque:record#10000015-20008036-")
                    && record.data() == Some(named_views.as_slice())
            });
        assert!(
            retained.is_some(),
            "unit {unit:?} list source was not retained"
        );
        let label = if unit == Some(0) {
            "native"
        } else {
            "unavailable"
        };
        assert!(result.report().losses.iter().any(|loss| {
            loss.code == crate::loss::RhinoLossCode::PresentationRecordDropped.kind()
                && loss
                    .message
                    .contains(&format!("no physical millimetre binding ({label})"))
        }));
        assert_valid(&result);
    }
}

#[test]
fn complete_decode_propagates_nested_view_file_reference_crc_loss() {
    let archive = ArchiveVersion::V8;
    let valid = named_views_record_with_trace(archive, false);
    let invalid = named_views_record_with_trace(archive, true);
    let document = |named_views: Vec<u8>| {
        support::test_dump::minimal_document(
            "80",
            &[
                support::test_dump::table(archive, 0x1000_0014, &[]),
                support::test_dump::table(
                    archive,
                    0x1000_0015,
                    &[support::test_dump::units_record(archive, 2), named_views],
                ),
                support::test_dump::table(archive, 0x1000_0013, &[]),
            ],
        )
    };

    let valid_result = decode(document(valid));
    let valid_views = &valid_result
        .ir()
        .native
        .namespace("rhino")
        .unwrap()
        .arenas()["views"];
    assert_eq!(valid_views.len(), 1);
    assert!(!valid_result.report().losses.iter().any(|loss| {
        loss.provenance
            .as_ref()
            .and_then(|provenance| provenance.tag.as_deref())
            == Some("VIEW/TRACE_IMAGE/FILE_REFERENCE")
    }));
    assert_valid(&valid_result);

    let invalid_result = decode(document(invalid.clone()));
    let invalid_views = &invalid_result
        .ir()
        .native
        .namespace("rhino")
        .unwrap()
        .arenas()["views"];
    assert_eq!(invalid_views.len(), 1);
    let loss = invalid_result
        .report()
        .losses
        .iter()
        .find(|loss| {
            loss.code == crate::loss::RhinoLossCode::IntegrityFailure.kind()
                && loss
                    .provenance
                    .as_ref()
                    .and_then(|provenance| provenance.tag.as_deref())
                    == Some("VIEW/TRACE_IMAGE/FILE_REFERENCE")
        })
        .expect("nested view checksum loss reaches the complete decode report");
    assert!(loss.message.contains("file reference"));
    assert!(loss
        .provenance
        .as_ref()
        .is_some_and(|provenance| provenance.offset > 0));
    let retained = invalid_result
        .source_fidelity()
        .retained_records()
        .iter()
        .any(|(_, record)| record.data() == Some(invalid.as_slice()));
    assert!(retained, "invalid view list source was not retained");
    assert_valid(&invalid_result);
}

#[test]
fn malformed_trace_reference_preserves_prior_diagnostic_and_recovers_later_view() {
    let archive = ArchiveVersion::V8;
    let malformed = trace_view(
        archive,
        &support::test_dump::file_reference_with_digest_warning_and_missing_second_digest(
            archive,
            "/trace/malformed.png",
            "malformed.png",
        ),
    );
    let valid = trace_view(
        archive,
        &support::test_dump::file_reference(archive, "/trace/valid.png", "valid.png"),
    );
    let named_views = view_list_record(archive, 0x2000_8036, &[malformed, valid]);
    let bytes = support::test_dump::minimal_document(
        "80",
        &[
            support::test_dump::table(archive, 0x1000_0014, &[]),
            support::test_dump::table(
                archive,
                0x1000_0015,
                &[
                    support::test_dump::units_record(archive, 2),
                    named_views.clone(),
                ],
            ),
            support::test_dump::table(archive, 0x1000_0013, &[]),
        ],
    );

    let result = decode(bytes);
    let views = &result.ir().native.namespace("rhino").unwrap().arenas()["views"];
    assert_eq!(views.len(), 1);
    assert_eq!(
        views[0]
            .field("list_index")
            .and_then(|value| value.as_u64()),
        Some(1)
    );
    let reference_loss = result
        .report()
        .losses
        .iter()
        .find(|loss| {
            loss.code == crate::loss::RhinoLossCode::IntegrityFailure.kind()
                && loss
                    .provenance
                    .as_ref()
                    .and_then(|provenance| provenance.tag.as_deref())
                    == Some("VIEW/TRACE_IMAGE/FILE_REFERENCE")
        })
        .expect("digest diagnostic survives the later file-reference parse failure");
    assert!(reference_loss.message.contains("file reference"));
    assert!(reference_loss.message.contains("SHA-1 hash CRC mismatch"));
    assert!(reference_loss
        .provenance
        .as_ref()
        .is_some_and(|provenance| provenance.offset > 0));
    assert!(result.report().losses.iter().any(|loss| {
        loss.code == crate::loss::RhinoLossCode::PresentationRecordDropped.kind()
            && loss.message.contains("named view record")
    }));
    assert!(result
        .source_fidelity()
        .retained_records()
        .iter()
        .any(|(_, record)| record.data() == Some(named_views.as_slice())));
    assert_valid(&result);
}

#[test]
fn malformed_wallpaper_reference_preserves_prior_diagnostic_and_recovers_later_view() {
    let archive = ArchiveVersion::V8;
    let malformed = wallpaper_view(
        archive,
        support::test_dump::file_reference_with_digest_warning_and_missing_second_digest(
            archive,
            "/wallpaper/malformed.png",
            "malformed.png",
        ),
    );
    let valid = trace_view(
        archive,
        &support::test_dump::file_reference(archive, "/trace/valid.png", "valid.png"),
    );
    let named_views = view_list_record(archive, 0x2000_8036, &[malformed, valid]);
    let bytes = support::test_dump::minimal_document(
        "80",
        &[
            support::test_dump::table(archive, 0x1000_0014, &[]),
            support::test_dump::table(
                archive,
                0x1000_0015,
                &[
                    support::test_dump::units_record(archive, 2),
                    named_views.clone(),
                ],
            ),
            support::test_dump::table(archive, 0x1000_0013, &[]),
        ],
    );

    let result = decode(bytes);
    let views = &result.ir().native.namespace("rhino").unwrap().arenas()["views"];
    assert_eq!(views.len(), 1);
    assert_eq!(
        views[0]
            .field("list_index")
            .and_then(|value| value.as_u64()),
        Some(1)
    );
    let reference_loss = result
        .report()
        .losses
        .iter()
        .find(|loss| {
            loss.code == crate::loss::RhinoLossCode::IntegrityFailure.kind()
                && loss
                    .provenance
                    .as_ref()
                    .and_then(|provenance| provenance.tag.as_deref())
                    == Some("VIEW/WALLPAPER/FILE_REFERENCE")
        })
        .expect("digest diagnostic survives the later wallpaper reference parse failure");
    assert!(reference_loss.message.contains("file reference"));
    assert!(reference_loss.message.contains("SHA-1 hash CRC mismatch"));
    assert!(reference_loss
        .provenance
        .as_ref()
        .is_some_and(|provenance| provenance.offset > 0));
    assert!(result.report().losses.iter().any(|loss| {
        loss.code == crate::loss::RhinoLossCode::PresentationRecordDropped.kind()
            && loss.message.contains("named view record")
    }));
    assert!(result
        .source_fidelity()
        .retained_records()
        .iter()
        .any(|(_, record)| record.data() == Some(named_views.as_slice())));
    assert_valid(&result);
}

#[test]
fn later_view_recovery_keeps_prior_child_checksum_loss_when_following_child_fails() {
    let archive = ArchiveVersion::V8;
    let mut corrupt_reference =
        support::test_dump::file_reference(archive, "/trace/prior-crc.png", "prior-crc.png");
    let reference_crc = corrupt_reference
        .last_mut()
        .expect("file-reference fixture has an outer checksum");
    *reference_crc ^= 1;
    let trace = trace_child(archive, &corrupt_reference);
    let mut malformed_target = support::test_dump::crc_chunk(archive, 0x2000_883b, &[0; 8]);
    let target_crc = malformed_target
        .last_mut()
        .expect("malformed target has an outer checksum");
    *target_crc ^= 1;
    let malformed_view = view_with_children(
        archive,
        &[trace, malformed_target.clone()],
        Some(support::test_dump::short_chunk(
            archive,
            crate::chunks::TCODE_ENDOFTABLE,
            0,
        )),
    );
    let valid_view = trace_view(
        archive,
        &support::test_dump::file_reference(archive, "/trace/later.png", "later.png"),
    );
    let named_views = view_list_record(archive, 0x2000_8036, &[malformed_view.clone(), valid_view]);
    let document = document_with_views(archive, named_views.clone());
    let result = decode(document.clone());

    let views = &result.ir().native.namespace("rhino").unwrap().arenas()["views"];
    assert_eq!(views.len(), 1);
    assert_eq!(
        views[0]
            .field("list_index")
            .and_then(|value| value.as_u64()),
        Some(1)
    );

    let reference_losses: Vec<_> = result
        .report()
        .losses
        .iter()
        .filter(|loss| {
            loss.code == crate::loss::RhinoLossCode::IntegrityFailure.kind()
                && loss
                    .provenance
                    .as_ref()
                    .and_then(|provenance| provenance.tag.as_deref())
                    == Some("VIEW/TRACE_IMAGE/FILE_REFERENCE")
        })
        .collect();
    assert_eq!(reference_losses.len(), 1);
    assert!(reference_losses[0].message.contains("file reference"));
    assert!(reference_losses[0].message.contains("CRC mismatch"));
    assert_eq!(
        reference_losses[0]
            .provenance
            .as_ref()
            .expect("nested checksum loss is located")
            .offset as usize,
        source_offset(&document, &corrupt_reference)
    );

    let target_loss = result
        .report()
        .losses
        .iter()
        .find(|loss| {
            loss.code == crate::loss::RhinoLossCode::IntegrityFailure.kind()
                && loss
                    .provenance
                    .as_ref()
                    .and_then(|provenance| provenance.tag.as_deref())
                    == Some("VIEW/TARGET")
        })
        .expect("direct child checksum loss is retained before the later child failure");
    assert_eq!(
        target_loss
            .provenance
            .as_ref()
            .expect("direct child checksum loss is located")
            .offset as usize,
        source_offset(&document, &malformed_target)
    );

    let dropped: Vec<_> = result
        .report()
        .losses
        .iter()
        .filter(|loss| {
            loss.code == crate::loss::RhinoLossCode::PresentationRecordDropped.kind()
                && loss
                    .message
                    .contains("was omitted after child parsing failed")
        })
        .collect();
    assert_eq!(dropped.len(), 1);
    assert!(
        dropped[0].message.contains("exceeds bound"),
        "unexpected target failure: {}",
        dropped[0].message
    );
    assert_eq!(
        dropped[0]
            .provenance
            .as_ref()
            .expect("dropped view loss is located")
            .offset as usize,
        source_offset(&document, &malformed_view)
    );
    assert_eq!(
        dropped[0]
            .provenance
            .as_ref()
            .and_then(|provenance| provenance.tag.as_deref()),
        Some("VIEW/RECORD")
    );
    assert!(result
        .source_fidelity()
        .retained_records()
        .iter()
        .any(|(_, record)| record.data() == Some(named_views.as_slice())));
    assert_valid(&result);
}

#[test]
fn later_view_recovery_keeps_viewport_warning_before_bad_end_marker() {
    let archive = ArchiveVersion::V8;
    for has_invalid_end_marker in [true, false] {
        let malformed_viewport = support::test_dump::crc_chunk(archive, 0x2000_823b, &[0]);
        let malformed_view = view_with_children(
            archive,
            std::slice::from_ref(&malformed_viewport),
            has_invalid_end_marker.then(|| {
                support::test_dump::short_chunk(archive, crate::chunks::TCODE_ENDOFTABLE, 1)
            }),
        );
        let valid_view = trace_view(
            archive,
            &support::test_dump::file_reference(archive, "/trace/recovered.png", "recovered.png"),
        );
        let named_views =
            view_list_record(archive, 0x2000_8036, &[malformed_view.clone(), valid_view]);
        let document = document_with_views(archive, named_views.clone());
        let result = decode(document.clone());

        let views = &result.ir().native.namespace("rhino").unwrap().arenas()["views"];
        assert_eq!(views.len(), 1, "invalid_end={has_invalid_end_marker}");
        assert_eq!(
            views[0]
                .field("list_index")
                .and_then(|value| value.as_u64()),
            Some(1),
            "invalid_end={has_invalid_end_marker}"
        );

        let viewport_losses: Vec<_> = result
            .report()
            .losses
            .iter()
            .filter(|loss| {
                loss.code == crate::loss::RhinoLossCode::PresentationRecordDropped.kind()
                    && loss.message.contains("viewport retained")
            })
            .collect();
        assert_eq!(
            viewport_losses.len(),
            1,
            "invalid_end={has_invalid_end_marker}"
        );
        assert_eq!(
            viewport_losses[0]
                .provenance
                .as_ref()
                .expect("viewport warning is located")
                .offset as usize,
            source_offset(&document, &malformed_viewport),
            "invalid_end={has_invalid_end_marker}"
        );
        assert_eq!(
            viewport_losses[0]
                .provenance
                .as_ref()
                .and_then(|provenance| provenance.tag.as_deref()),
            Some("VIEW/VIEWPORT"),
            "invalid_end={has_invalid_end_marker}"
        );

        let end_error = if has_invalid_end_marker {
            "view end marker is invalid"
        } else {
            "view is missing its end marker"
        };
        let dropped: Vec<_> = result
            .report()
            .losses
            .iter()
            .filter(|loss| {
                loss.code == crate::loss::RhinoLossCode::PresentationRecordDropped.kind()
                    && loss
                        .message
                        .contains("was omitted after child parsing failed")
            })
            .collect();
        assert_eq!(dropped.len(), 1, "invalid_end={has_invalid_end_marker}");
        assert!(
            dropped[0].message.contains(end_error),
            "invalid_end={has_invalid_end_marker}: {}",
            dropped[0].message
        );
        assert_eq!(
            dropped[0]
                .provenance
                .as_ref()
                .expect("end-marker loss is located")
                .offset as usize,
            source_offset(&document, &malformed_view),
            "invalid_end={has_invalid_end_marker}"
        );
        assert!(result
            .source_fidelity()
            .retained_records()
            .iter()
            .any(|(_, record)| record.data() == Some(named_views.as_slice())));
        assert_valid(&result);
    }
}

#[test]
fn malformed_viewport_userdata_keeps_prior_checksum_loss_and_recovers_later_view() {
    let archive = ArchiveVersion::V8;
    let malformed_userdata = viewport_userdata_with_options(archive, 1, true);
    let malformed_view = view_with_child(archive, malformed_userdata.clone());
    let valid_view = trace_view(
        archive,
        &support::test_dump::file_reference(
            archive,
            "/trace/userdata-later.png",
            "userdata-later.png",
        ),
    );
    let named_views = view_list_record(archive, 0x2000_8036, &[malformed_view, valid_view]);
    let document = document_with_views(archive, named_views);
    let result = decode(document.clone());

    let views = &result.ir().native.namespace("rhino").unwrap().arenas()["views"];
    assert_eq!(views.len(), 2);
    assert_eq!(
        views[1]
            .field("list_index")
            .and_then(|value| value.as_u64()),
        Some(1)
    );

    let checksum_losses: Vec<_> = result
        .report()
        .losses
        .iter()
        .filter(|loss| {
            loss.code == crate::loss::RhinoLossCode::IntegrityFailure.kind()
                && loss
                    .provenance
                    .as_ref()
                    .and_then(|provenance| provenance.tag.as_deref())
                    == Some("VIEW/VIEWPORT_USERDATA")
        })
        .collect();
    assert_eq!(
        checksum_losses.len(),
        1,
        "all losses: {:#?}",
        result.report().losses
    );
    let userdata_source = source_offset(&document, &malformed_userdata);
    let archive_header_len = if archive.uses_eight_byte_values() {
        12
    } else {
        8
    };
    assert_eq!(
        checksum_losses[0]
            .provenance
            .as_ref()
            .expect("userdata checksum loss is located")
            .offset as usize,
        userdata_source + archive_header_len
    );
    assert!(checksum_losses[0].message.contains("viewport userdata"));

    let malformed_losses: Vec<_> = result
        .report()
        .losses
        .iter()
        .filter(|loss| {
            loss.code == crate::loss::RhinoLossCode::ViewportUserdataDropped.kind()
                && loss.message.contains("could not be framed")
        })
        .collect();
    assert_eq!(malformed_losses.len(), 1);
    assert!(malformed_losses[0]
        .message
        .contains("class end must be a short zero chunk"));
    assert_eq!(
        malformed_losses[0]
            .provenance
            .as_ref()
            .expect("malformed userdata loss is located")
            .offset as usize,
        userdata_source
    );
    assert_valid(&result);
}

#[test]
fn active_view_recovery_preserves_earlier_losses_and_exact_source() {
    for archive in [ArchiveVersion::V5, ArchiveVersion::V8] {
        let mut reference = support::test_dump::file_reference(
            archive,
            "/trace/active-corrupt.png",
            "active-corrupt.png",
        );
        *reference.last_mut().expect("reference checksum") ^= 1;
        let bad_target = support::test_dump::crc_chunk(archive, 0x2000_883b, &[0; 8]);
        let rejected = view_with_children(
            archive,
            &[trace_child(archive, &reference), bad_target],
            Some(support::test_dump::short_chunk(archive, 0xffff_ffff, 0)),
        );
        let later = trace_view(
            archive,
            &support::test_dump::file_reference(
                archive,
                "/trace/active-later.png",
                "active-later.png",
            ),
        );
        let list = view_list_record(archive, 0x2000_8037, &[rejected.clone(), later]);
        let document = document_with_views(archive, list.clone());
        assert_eq!(
            crate::chunks::parse_header(&document)
                .unwrap()
                .archive_version,
            archive
        );
        let result = decode(document.clone());
        let views = &result.ir().native.namespace("rhino").unwrap().arenas()["views"];
        assert_eq!(views.len(), 1);
        assert_eq!(
            views[0].field("list_kind"),
            Some(serde_json::json!("active"))
        );
        assert_eq!(views[0].field("list_index"), Some(serde_json::json!(1)));
        for (code, tag, offset) in [
            (
                crate::loss::RhinoLossCode::IntegrityFailure,
                "VIEW/TRACE_IMAGE/FILE_REFERENCE",
                source_offset(&document, &reference),
            ),
            (
                crate::loss::RhinoLossCode::PresentationRecordDropped,
                "VIEW/RECORD",
                source_offset(&document, &rejected),
            ),
        ] {
            let losses = result
                .report()
                .losses
                .iter()
                .filter(|loss| {
                    loss.code == code.kind()
                        && loss
                            .provenance
                            .as_ref()
                            .and_then(|provenance| provenance.tag.as_deref())
                            == Some(tag)
                })
                .collect::<Vec<_>>();
            assert_eq!(losses.len(), 1, "{archive:?}/{tag}");
            assert_eq!(
                losses[0].provenance.as_ref().unwrap().offset as usize,
                offset
            );
        }
        assert!(result
            .source_fidelity()
            .retained_records()
            .iter()
            .any(|(_, record)| record.data() == Some(list.as_slice())));
        assert_valid(&result);
    }
}
