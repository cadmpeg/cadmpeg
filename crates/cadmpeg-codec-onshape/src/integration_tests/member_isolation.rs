// SPDX-License-Identifier: Apache-2.0
//! Member isolation and admitted-feature annotation regression tests.
use crate::loss::OnshapeLossCode;
use crate::test_support::fixture;
use crate::test_support::review::{decode, edit, members, validate, wire};
use cadmpeg_core::decode::{index_from_u32, View};
use cadmpeg_core::json;
use std::io::{Cursor, Read, Write};

const TABLE: &str = "partstudios/studio/bodydetails/2.json";

fn has(result: &cadmpeg_ir::codec::DecodeResult, code: OnshapeLossCode) -> usize {
    result
        .report()
        .losses
        .iter()
        .filter(|note| code.matches(note))
        .count()
}

/// The `record.unreadable` losses of the archive entry `name` given up as a part.
fn given_up_entries(result: &cadmpeg_ir::codec::DecodeResult, name: &str) -> usize {
    let head = format!("part archive-entry {name:?} stream=\"\" offset=");
    result
        .report()
        .losses
        .iter()
        .filter(|loss| {
            loss.code == cadmpeg_ir::unknown::part::record_unreadable_kind()
                && loss.message.starts_with(&head)
        })
        .count()
}

/// A pinned decode of `bytes` refuses through the part loss of entry `name`.
fn assert_pinned_refusal(bytes: Vec<u8>, name: &str) {
    use cadmpeg_ir::codec::{Codec, DecodeFailure, DecodeOptions};
    use cadmpeg_ir::unknown::part::record_unreadable_kind;
    let mut options = DecodeOptions::default();
    options.strictness.pinned = true;
    let error = crate::test_support::codec()
        .decode(&mut Cursor::new(bytes), &options)
        .expect_err("a pinned decode refuses the given-up entry");
    let head = format!("part archive-entry {name:?} stream=\"\" offset=");
    assert!(matches!(
        error,
        DecodeFailure::StrictRejected { rejection }
            if rejection.loss().code == record_unreadable_kind()
                && rejection.loss().message.starts_with(&head)
    ));
}

fn retained(bytes: &[u8]) {
    let result = decode(bytes);
    assert_eq!(has(&result, OnshapeLossCode::MemberUnreadable), 1);
    assert_eq!(result.ir().model.features.len(), 3);
    assert_eq!(result.ir().geometry.bodies.len(), 1);
    validate(result);
}

fn central(bytes: &[u8], name: &str) -> usize {
    bytes
        .windows(46 + name.len())
        .position(|row| row.starts_with(b"PK\x01\x02") && &row[46..] == name.as_bytes())
        .unwrap()
}

fn patch(bytes: &mut [u8], name: &str, central_field: usize, local_field: usize, value: &[u8]) {
    let offset = central(bytes, name);
    let local = index_from_u32(View::u32_le_at(bytes, offset + 42).unwrap());
    bytes[offset + central_field..offset + central_field + value.len()].copy_from_slice(value);
    bytes[local + local_field..local + local_field + value.len()].copy_from_slice(value);
}

fn add_stored_padding(output: &mut zip::ZipWriter<Cursor<Vec<u8>>>) {
    output
        .start_file(
            "notes/allowance.bin",
            zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored),
        )
        .unwrap();
    let chunk = [0; 8192];
    for _ in 0..128 {
        output.write_all(&chunk).unwrap();
    }
}

fn deflated_table() -> Vec<u8> {
    let mut source = zip::ZipArchive::new(Cursor::new(fixture("plate-fillet"))).unwrap();
    let mut output = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for index in 0..source.len() {
        let mut member = source.by_index(index).unwrap();
        let name = member.name().unwrap().into_owned();
        let mut bytes = Vec::new();
        member.read_to_end(&mut bytes).unwrap();
        let method = if name == TABLE {
            zip::CompressionMethod::Deflated
        } else {
            zip::CompressionMethod::Stored
        };
        output
            .start_file(
                name,
                zip::write::SimpleFileOptions::default().compression_method(method),
            )
            .unwrap();
        output.write_all(&bytes).unwrap();
    }
    output.finish().unwrap().into_inner()
}

#[test]
fn crc_failure_omits_one_member() {
    let mut bytes = fixture("plate-fillet");
    patch(&mut bytes, TABLE, 16, 14, &0u32.to_le_bytes());
    retained(&bytes);
}

#[test]
fn strict_decode_refuses_a_member_with_a_crc_failure() {
    let mut bytes = fixture("plate-fillet");
    patch(&mut bytes, TABLE, 16, 14, &0u32.to_le_bytes());
    let admitted = decode(&bytes);
    assert_eq!(has(&admitted, OnshapeLossCode::MemberUnreadable), 1);
    assert_eq!(given_up_entries(&admitted, TABLE), 1);
    assert_pinned_refusal(bytes, TABLE);
}

#[test]
fn inflate_failure_omits_one_member() {
    let mut bytes = deflated_table();
    let offset = central(&bytes, TABLE);
    let local = index_from_u32(View::u32_le_at(&bytes, offset + 42).unwrap());
    let data = local
        + 30
        + usize::from(View::u16_le_at(&bytes, local + 26).unwrap())
        + usize::from(View::u16_le_at(&bytes, local + 28).unwrap());
    bytes[data] = 7;
    retained(&bytes);
}

#[test]
fn lying_expanded_header_omits_one_member() {
    let mut bytes = deflated_table();
    patch(&mut bytes, TABLE, 24, 22, &1u32.to_le_bytes());
    retained(&bytes);
}

#[test]
fn unreadable_local_header_omits_one_member() {
    let mut bytes = fixture("plate-fillet");
    let offset = central(&bytes, TABLE);
    let local = index_from_u32(View::u32_le_at(&bytes, offset + 42).unwrap());
    bytes[local..local + 4].fill(0);
    retained(&bytes);
}

#[test]
fn non_utf8_json_omits_one_member() {
    retained(&members(&fixture("plate-fillet"), |rows| {
        rows.insert(TABLE.into(), vec![255]);
    }));
}

#[test]
fn malformed_json_omits_one_member() {
    retained(&members(&fixture("plate-fillet"), |rows| {
        rows.insert(TABLE.into(), b"{\"bodies\":[".to_vec());
    }));
}

#[test]
fn bad_member_names_omit_one_member() {
    for name in ["notes/a:b.txt", "notes/../a.txt", "notes\\a.txt"] {
        retained(&members(&fixture("plate-fillet"), |rows| {
            rows.insert(name.into(), vec![1]);
        }));
    }
}

#[test]
fn unsupported_method_omits_one_member() {
    let mut bytes = fixture("plate-fillet");
    patch(&mut bytes, TABLE, 10, 8, &99u16.to_le_bytes());
    retained(&bytes);
}

#[test]
fn encrypted_member_is_given_up_and_a_pinned_decode_refuses() {
    let mut bytes = fixture("plate-fillet");
    patch(&mut bytes, TABLE, 8, 6, &1u16.to_le_bytes());
    retained(&bytes);
    assert_eq!(given_up_entries(&decode(&bytes), TABLE), 1);
    assert_pinned_refusal(bytes, TABLE);
}

#[test]
fn repeated_name_keeps_first_member() {
    let mut bytes = members(&fixture("plate-fillet"), |rows| {
        rows.insert("notes/first.txt".into(), b"first".to_vec());
        rows.insert("notes/other.txt".into(), b"second".to_vec());
    });
    for offset in 0..bytes.len() - 15 {
        if bytes[offset..].starts_with(b"notes/other.txt") {
            bytes[offset..offset + 15].copy_from_slice(b"notes/first.txt");
        }
    }
    let result = decode(&bytes);
    assert_eq!(has(&result, OnshapeLossCode::MemberUnreadable), 1);
    assert_eq!(given_up_entries(&result, "notes/first.txt"), 1);
    let asset = result
        .ir()
        .model
        .assets
        .iter()
        .find(|asset| {
            asset
                .name
                .as_ref()
                .is_some_and(|name| name.as_str() == "notes/first.txt")
        })
        .unwrap();
    let cadmpeg_ir::assets::AssetContent::Embedded { data } = &asset.content else {
        panic!("embedded member")
    };
    assert_eq!(data.as_bytes(), b"first");
    assert_eq!(result.ir().model.features.len(), 3);
    assert_eq!(result.ir().geometry.bodies.len(), 1);
    validate(result);
}

#[test]
fn two_directory_records_of_one_stored_span_are_one_part() {
    let mut bytes = members(&fixture("plate-fillet"), |rows| {
        rows.insert("notes/a.bin".into(), b"alias".to_vec());
        rows.insert("notes/b.bin".into(), b"alias".to_vec());
    });
    let first = central(&bytes, "notes/a.bin");
    let local = bytes[first + 42..first + 46].to_vec();
    let second = central(&bytes, "notes/b.bin");
    bytes[second + 42..second + 46].copy_from_slice(&local);
    patch(&mut bytes, "notes/a.bin", 16, 14, &0u32.to_le_bytes());
    patch(&mut bytes, "notes/b.bin", 16, 14, &0u32.to_le_bytes());
    let result = decode(&bytes);
    assert_eq!(has(&result, OnshapeLossCode::MemberUnreadable), 2);
    assert_eq!(given_up_entries(&result, "notes/a.bin"), 1);
    assert_eq!(given_up_entries(&result, "notes/b.bin"), 0);
    let stubs = result
        .ir()
        .native
        .0
        .get("onshape")
        .and_then(|namespace| namespace.arenas().get("unknowns"))
        .map_or(0, Vec::len);
    assert_eq!(stubs, 1);
    assert_eq!(result.ir().model.features.len(), 3);
    validate(result);
}

#[test]
fn oversized_step_is_omitted_before_or_after_history() {
    for step_first in [true, false] {
        let mut source = zip::ZipArchive::new(Cursor::new(fixture("plate-fillet"))).unwrap();
        let mut output = zip::ZipWriter::new(Cursor::new(Vec::new()));
        add_stored_padding(&mut output);
        let write_step = |output: &mut zip::ZipWriter<Cursor<Vec<u8>>>| {
            output
                .start_file(
                    "partstudios/studio/export.step",
                    zip::write::SimpleFileOptions::default()
                        .compression_method(zip::CompressionMethod::Deflated),
                )
                .unwrap();
            let chunk = [b' '; 8192];
            for _ in 0..38400 {
                output.write_all(&chunk).unwrap();
            }
        };
        if step_first {
            write_step(&mut output);
        }
        for index in 0..source.len() {
            let mut member = source.by_index(index).unwrap();
            let name = member.name().unwrap().into_owned();
            if name.ends_with("export.step") {
                continue;
            }
            let mut bytes = Vec::new();
            member.read_to_end(&mut bytes).unwrap();
            output
                .start_file(
                    name,
                    zip::write::SimpleFileOptions::default()
                        .compression_method(zip::CompressionMethod::Stored),
                )
                .unwrap();
            output.write_all(&bytes).unwrap();
        }
        if !step_first {
            write_step(&mut output);
        }
        let result = decode(&output.finish().unwrap().into_inner());
        assert_eq!(has(&result, OnshapeLossCode::MemberTooLarge), 1);
        assert_eq!(has(&result, OnshapeLossCode::StepUnreadable), 1);
        assert_eq!(result.ir().model.features.len(), 3);
        assert!(result.ir().geometry.bodies.is_empty());
        validate(result);
    }
}

#[test]
fn stale_or_malformed_microversion_omits_one_table() {
    for value in [json!({"theId":"stale"}), json!(42)] {
        let bytes = edit(&fixture("plate-fillet"), |path, response| {
            if path == TABLE {
                response["microversionId"] = value.clone();
            }
        });
        let result = decode(&bytes);
        assert_eq!(has(&result, OnshapeLossCode::MemberUnreadable), 1);
        assert_eq!(
            wire(&result)["model"]["features"][2]["definition"]["value"]["kind"],
            "native"
        );
        assert_eq!(result.ir().geometry.bodies.len(), 1);
        validate(result);
    }
}

#[test]
fn malformed_features_retain_step() {
    for value in [
        json!({"sourceMicroversion":"stale","features":[]}),
        json!({"features":42}),
        json!({"sourceMicroversion":42,"features":[]}),
    ] {
        let bytes = edit(&fixture("plate-fillet"), |path, response| {
            if path.ends_with("features.json") {
                *response = value.clone();
            }
        });
        let result = decode(&bytes);
        assert_eq!(has(&result, OnshapeLossCode::HistoryNotSupported), 1);
        assert_eq!(
            given_up_entries(&result, "partstudios/studio/features.json"),
            1
        );
        assert!(result.ir().model.features.is_empty());
        assert_eq!(result.ir().geometry.bodies.len(), 1);
        validate(result);
    }
}

#[test]
fn inconsistent_bundle_retains_readable_records() {
    let bytes = edit(&fixture("plate-fillet"), |path, response| {
        if path == "manifest.json" {
            response["consistent"] = json!(false);
        }
    });
    let result = decode(&bytes);
    assert_eq!(result.ir().model.features.len(), 3);
    assert_eq!(result.ir().geometry.bodies.len(), 1);
    validate(result);
}

#[test]
fn repeated_feature_keeps_first_placement_and_suppression_annotations() {
    let bytes = edit(&fixture("default-plane-regions"), |path, response| {
        if path.ends_with("features.json") {
            response["features"][0]["suppressed"] = json!(false);
            response["features"][1]["featureId"] = json!("drawn-profile");
            response["features"][1]
                .as_object_mut()
                .unwrap()
                .remove("suppressed");
            response["features"][1]
                .as_object_mut()
                .unwrap()
                .remove("parentSuppressed");
        }
    });
    let result = decode(&bytes);
    let (ir, report, fidelity) = result.into_parts();
    let sketch = crate::reader::id("sketch", "drawn-profile");
    assert_eq!(
        fidelity
            .annotations
            .exactness()
            .get(&sketch)
            .unwrap()
            .fields()
            .get("placement"),
        Some(&cadmpeg_ir::Exactness::Derived)
    );
    let feature = crate::reader::id("feature", "drawn-profile");
    assert!(!fidelity
        .annotations
        .exactness()
        .get(&feature)
        .unwrap()
        .fields()
        .contains_key("suppressed"));
    assert!(
        cadmpeg_ir::validate_neutral_with_source_fidelity(&ir, &fidelity, report.losses)
            .unwrap()
            .is_ok()
    );
}

#[test]
fn optional_member_session_expansion_failure_remains_an_error() {
    use cadmpeg_ir::codec::{Codec, DecodeOptions};
    let bytes = deflated_table();
    let mut options = DecodeOptions::default();
    options.policy.limits.max_decompressed_bytes_total = 1;
    let error = crate::test_support::codec()
        .decode(&mut Cursor::new(bytes), &options)
        .unwrap_err();
    assert!(
        matches!(error, cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::ResourceLimit(limit)) if limit.scope == cadmpeg_core::decode::LimitScope::Session)
    );
}

#[test]
fn remaining_capacity_omits_the_later_member_in_both_orders() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    for step_first in [true, false] {
        let mut output = zip::ZipWriter::new(Cursor::new(Vec::new()));
        add_stored_padding(&mut output);
        let mut order = [
            ("notes/padding.bin", 200),
            ("partstudios/studio/export.step", 60),
        ];
        if step_first {
            order.reverse();
        }
        for (name, megabytes) in order {
            output
                .start_file(
                    name,
                    zip::write::SimpleFileOptions::default()
                        .compression_method(zip::CompressionMethod::Deflated),
                )
                .unwrap();
            let chunk = [b' '; 8192];
            for _ in 0..megabytes * 128 {
                output.write_all(&chunk).unwrap();
            }
        }
        let mut source = zip::ZipArchive::new(Cursor::new(fixture("plate-fillet"))).unwrap();
        for index in 0..source.len() {
            let mut member = source.by_index(index).unwrap();
            let name = member.name().unwrap().into_owned();
            if name.ends_with("export.step") {
                continue;
            }
            let mut data = Vec::new();
            member.read_to_end(&mut data).unwrap();
            output
                .start_file(
                    name,
                    zip::write::SimpleFileOptions::default()
                        .compression_method(zip::CompressionMethod::Stored),
                )
                .unwrap();
            output.write_all(&data).unwrap();
        }
        let bytes = output.finish().unwrap().into_inner();
        let arena = DecodeArena::new();
        let (ctx, root) =
            DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default()).unwrap();
        assert!(ctx.per_expand_allowance() >= (200 << 20));
        let bundle = crate::archive::Bundle::open(&ctx, root).unwrap();
        assert_eq!(
            bundle
                .losses
                .iter()
                .filter(|note| OnshapeLossCode::MemberTooLarge.matches(note))
                .count(),
            1
        );
        assert!(bundle.contains(order[0].0));
        assert!(!bundle.contains(order[1].0));
        assert_eq!(
            bundle.json("partstudios/studio/features.json").unwrap()["features"]
                .as_array()
                .unwrap()
                .len(),
            3
        );
    }
}

#[test]
fn unreadable_step_member_has_one_typed_loss() {
    let mut bytes = fixture("plate-fillet");
    patch(
        &mut bytes,
        "partstudios/studio/export.step",
        16,
        14,
        &0u32.to_le_bytes(),
    );
    let result = decode(&bytes);
    assert_eq!(has(&result, OnshapeLossCode::StepUnreadable), 1);
    assert_eq!(result.ir().model.features.len(), 3);
    assert!(result.ir().geometry.bodies.is_empty());
    validate(result);
}

#[test]
fn unreadable_step_named_asset_keeps_the_canonical_step_geometry() {
    let mut bytes = members(&fixture("plate-fillet"), |rows| {
        rows.insert("notes/export.step".into(), b"opaque asset".to_vec());
    });
    patch(&mut bytes, "notes/export.step", 16, 14, &0u32.to_le_bytes());
    retained(&bytes);
}

#[test]
fn invalid_optional_timestamp_omits_one_member_and_keeps_history_and_step() {
    let mut bytes = members(&fixture("plate-fillet"), |rows| {
        rows.insert("notes/time.txt".into(), b"x".to_vec());
    });
    let offset = central(&bytes, "notes/time.txt");
    let name_end = offset + 46 + "notes/time.txt".len();
    assert_eq!(View::u16_le_at(&bytes, offset + 30), Some(0));
    bytes.splice(name_end..name_end, [0x55, 0x54, 0, 0]);
    bytes[offset + 30..offset + 32].copy_from_slice(&4_u16.to_le_bytes());
    let footer = bytes.len() - 22;
    let size = View::u32_le_at(&bytes, footer + 12).unwrap();
    bytes[footer + 12..footer + 16].copy_from_slice(&(size + 4).to_le_bytes());
    let result = decode(&bytes);
    assert_eq!(has(&result, OnshapeLossCode::MemberUnreadable), 1);
    assert!(result.report().losses.iter().any(|note| {
        OnshapeLossCode::MemberUnreadable.matches(note) && note.message.contains("notes/time.txt")
    }));
    assert!(result.ir().model.assets.iter().all(|asset| {
        asset
            .name
            .as_ref()
            .is_none_or(|name| name.as_str() != "notes/time.txt")
    }));
    assert_eq!(result.ir().model.features.len(), 3);
    assert_eq!(result.ir().geometry.bodies.len(), 1);
    validate(result);
}

#[test]
fn broken_central_directory_refuses_the_bundle() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let mut bytes = fixture("plate-fillet");
    let offset = central(&bytes, "manifest.json");
    bytes[offset..offset + 4].fill(0);
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let Err(error) = crate::archive::Bundle::open(&ctx, root) else {
        panic!("broken directory must refuse the bundle")
    };
    assert!(error.to_string().contains("the ZIP directory is invalid"));
}
