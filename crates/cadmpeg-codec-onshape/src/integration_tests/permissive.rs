// SPDX-License-Identifier: Apache-2.0
//! One malformed record is a named loss and the rest of the file decodes.
use crate::test_support::fixture;
use crate::test_support::review::{decode, edit, wire};
use serde_json::Value;

fn feature_count(wire: &Value) -> usize {
    wire["model"]["features"].as_array().unwrap().len()
}

fn malformed_losses(result: &cadmpeg_ir::codec::DecodeResult) -> usize {
    result
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code.to_string() == "onshape/record.malformed")
        .count()
}

#[test]
fn a_feature_without_an_id_is_omitted_with_one_named_loss() {
    let whole = wire(&decode(&fixture("plate-fillet")));
    let total = feature_count(&whole);
    assert!(total >= 3);
    let bytes = edit(&fixture("plate-fillet"), |path, response| {
        if path.ends_with("features.json") {
            let last = response["features"].as_array().unwrap().len() - 1;
            response["features"][last]
                .as_object_mut()
                .unwrap()
                .remove("featureId");
        }
    });
    let result = decode(&bytes);
    assert_eq!(malformed_losses(&result), 1, "{:?}", result.report().losses);
    assert_eq!(feature_count(&wire(&result)), total - 1);
}

#[test]
fn a_repeated_feature_id_with_other_content_gives_up_the_repeat_and_keeps_the_rest() {
    let whole = wire(&decode(&fixture("plate-fillet")));
    let total = feature_count(&whole);
    let mut repeat = String::new();
    let bytes = edit(&fixture("plate-fillet"), |path, response| {
        if path.ends_with("features.json") {
            let first = response["features"][0]["featureId"].clone();
            let last = response["features"].as_array().unwrap().len() - 1;
            response["features"][last]["featureId"] = first;
            repeat = format!("{path}#/features/{last}");
        }
    });
    let result = decode(&bytes);
    assert_eq!(malformed_losses(&result), 1, "{:?}", result.report().losses);
    assert_eq!(feature_count(&wire(&result)), total - 1);
    let repeat = crate::archive::source_record(&repeat);
    let given_up = result
        .report()
        .losses
        .iter()
        .filter(|loss| {
            loss.code == cadmpeg_ir::unknown::part::record_unreadable_kind()
                && loss
                    .message
                    .split_whitespace()
                    .any(|word| word == repeat.as_str())
        })
        .count();
    assert_eq!(given_up, 1, "{:?}", result.report().losses);
}

#[test]
fn a_missing_rollback_index_keeps_the_features_with_one_named_loss() {
    let whole = wire(&decode(&fixture("plate-fillet")));
    let total = feature_count(&whole);
    let bytes = crate::test_support::review::members(&fixture("plate-fillet"), |members| {
        members.retain(|name, _| !name.ends_with("bodydetails/index.json"));
    });
    let result = decode(&bytes);
    assert_eq!(malformed_losses(&result), 1, "{:?}", result.report().losses);
    assert_eq!(feature_count(&wire(&result)), total);
}

fn named_losses(result: &cadmpeg_ir::codec::DecodeResult, code: &str) -> usize {
    result
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code.to_string() == code)
        .count()
}

#[test]
fn an_oversized_member_is_dropped_while_history_and_geometry_decode() {
    let bytes = crate::test_support::review::members(&fixture("plate-fillet"), |members| {
        members.insert(
            "extra.json".into(),
            std::iter::repeat_n(b' ', (32 << 20) + 1).collect(),
        );
    });
    let result = decode(&bytes);
    assert_eq!(named_losses(&result, "onshape/member.too-large"), 1);
    assert_eq!(feature_count(&wire(&result)), 3);
    assert_eq!(result.ir().geometry.bodies.len(), 1);
    assert!(result.ir().model.assets.iter().all(|asset| asset
        .name
        .as_ref()
        .is_none_or(|name| name.as_str() != "extra.json")));
    crate::test_support::review::validate(result);
}

#[test]
fn a_member_expansion_limit_drops_the_member_and_keeps_the_bundle() {
    use cadmpeg_ir::codec::{Codec, DecodeOptions};
    use std::io::{Cursor, Write};
    let bytes = fixture("plate-fillet");
    let mut output = zip::ZipWriter::new_append(Cursor::new(bytes)).unwrap();
    output
        .start_file(
            "extra.bin",
            zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated),
        )
        .unwrap();
    let chunk = [b' '; 8192];
    for _ in 0..256 {
        output.write_all(&chunk).unwrap();
    }
    let bytes = output.finish().unwrap().into_inner();
    let mut options = DecodeOptions::default();
    options.policy.limits.max_decompressed_bytes_per_expand = 1 << 20;
    let result = crate::test_support::codec()
        .decode(&mut Cursor::new(bytes), &options)
        .unwrap();
    assert_eq!(named_losses(&result, "onshape/member.too-large"), 1);
    assert_eq!(feature_count(&wire(&result)), 3);
    assert_eq!(result.ir().geometry.bodies.len(), 1);
    crate::test_support::review::validate(result);
}

#[test]
fn an_oversized_topology_table_keeps_other_tables_and_degrades_its_consumer() {
    for table in ["bodies", "faces", "edges", "vertices", "loops", "coedges"] {
        let bytes = edit(&fixture("plate-fillet"), |path, response| {
            if path.ends_with("bodydetails/2.json") {
                let rows = cadmpeg_core::json!([null]);
                let oversized = Value::Array(
                    std::iter::repeat_n(rows[0].clone(), crate::reader::ROWS + 1).collect(),
                );
                match table {
                    "bodies" => response["bodies"] = oversized,
                    "loops" => {
                        response["bodies"][0]["faces"] =
                            cadmpeg_core::json!([{"loops": oversized}]);
                    }
                    "coedges" => {
                        response["bodies"][0]["faces"] =
                            cadmpeg_core::json!([{"loops": [{"coedges": oversized}]}]);
                    }
                    key => response["bodies"][0][key] = oversized,
                }
            }
        });
        let result = decode(&bytes);
        assert_eq!(
            named_losses(&result, "onshape/topology.too-large"),
            1,
            "{table}"
        );
        assert!(
            result.report().losses.iter().any(|loss| {
                loss.code == cadmpeg_ir::unknown::part::record_unreadable_kind()
                    && loss.message.starts_with(
                        "part archive-entry \"partstudios/studio/bodydetails/2.json\" ",
                    )
            }),
            "{table}"
        );
        assert_eq!(
            named_losses(&result, "onshape/history.not-supported"),
            1,
            "{table}"
        );
        let wire = wire(&result);
        assert_eq!(feature_count(&wire), 3);
        assert_eq!(
            wire["model"]["features"][0]["definition"]["value"]["kind"],
            "sketch"
        );
        assert_eq!(
            wire["model"]["features"][1]["definition"]["value"]["kind"],
            "extrude"
        );
        assert_eq!(
            wire["model"]["features"][2]["definition"]["value"]["kind"],
            "native"
        );
        assert_eq!(result.ir().geometry.bodies.len(), 1);
        crate::test_support::review::validate(result);
    }
}

#[test]
fn unreadable_step_keeps_readable_history_and_reports_missing_geometry() {
    for step in [vec![0xff, 0xfe], b"unreadable STEP".to_vec()] {
        let bytes = crate::test_support::review::members(&fixture("plate-fillet"), |members| {
            members.insert("partstudios/studio/export.step".into(), step);
        });
        let result = decode(&bytes);
        assert_eq!(named_losses(&result, "onshape/step.unreadable"), 1);
        assert_eq!(
            result
                .report()
                .losses
                .iter()
                .filter(
                    |loss| loss.code == cadmpeg_ir::unknown::part::record_unreadable_kind()
                        && loss
                            .message
                            .starts_with("part archive-entry \"partstudios/studio/export.step\" ")
                )
                .count(),
            1
        );
        assert_eq!(feature_count(&wire(&result)), 3);
        assert!(result.ir().geometry.bodies.is_empty());
        let [root] = result.ir().model.product_definitions.as_slice() else {
            panic!("history-only decode must state one root definition");
        };
        assert!(root.is_root());
        assert!(root.bodies.is_empty());
        crate::test_support::review::validate(result);
    }
}

#[test]
fn unreadable_step_without_history_remains_an_error() {
    use cadmpeg_ir::codec::{Codec, DecodeOptions};
    let bytes = edit(&fixture("plate-fillet"), |path, value| {
        if path.ends_with("features.json") {
            value["features"] = cadmpeg_core::json!([]);
        }
    });
    let bytes = crate::test_support::review::members(&bytes, |members| {
        members.insert(
            "partstudios/studio/export.step".into(),
            b"unreadable STEP".to_vec(),
        );
    });
    assert!(crate::test_support::codec()
        .decode(&mut std::io::Cursor::new(bytes), &DecodeOptions::default())
        .is_err());
}

#[test]
fn step_session_budget_failures_remain_errors_with_readable_history() {
    use cadmpeg_core::decode::{LimitScope, ResourceDimension, ResourceFailure, ResourceLimit};
    use cadmpeg_ir::codec::{Codec, DecodeOptions};
    let codec = crate::OnshapeCodec::new(|_, _| {
        Err(cadmpeg_core::CodecError::ResourceLimit(ResourceLimit {
            scope: LimitScope::Session,
            dimension: ResourceDimension::Entities,
            reason: ResourceFailure::BudgetExceeded,
            limit: 1,
            used: 1,
            additional: 1,
            operation: "synthetic STEP entity admission",
        }))
    });
    let error = codec
        .decode(
            &mut std::io::Cursor::new(fixture("plate-fillet")),
            &DecodeOptions::default(),
        )
        .unwrap_err();
    assert!(error.to_string().contains("resource limit"));
}

#[test]
fn an_oversized_history_member_retains_readable_geometry() {
    let bytes = crate::test_support::review::members(&fixture("plate-fillet"), |members| {
        members.insert(
            "partstudios/studio/features.json".into(),
            std::iter::repeat_n(b' ', (32 << 20) + 1).collect(),
        );
    });
    let result = decode(&bytes);
    assert_eq!(named_losses(&result, "onshape/member.too-large"), 1);
    assert_eq!(named_losses(&result, "onshape/history.not-supported"), 1);
    assert_eq!(feature_count(&wire(&result)), 0);
    assert_eq!(result.ir().geometry.bodies.len(), 1);
    crate::test_support::review::validate(result);
}

#[test]
fn a_step_part_budget_refusal_keeps_history_and_clears_the_part_refusal() {
    use cadmpeg_ir::codec::{Codec, DecodeOptions};
    let codec = crate::OnshapeCodec::new(|ctx, _| {
        let _writer = ctx.begin_expand(cadmpeg_core::decode::ExpandSpec::Exact(u64::MAX))?;
        Err(cadmpeg_core::CodecError::malformed(
            "synthetic STEP expansion",
        ))
    });
    let result = codec
        .decode(
            &mut std::io::Cursor::new(fixture("plate-fillet")),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert_eq!(named_losses(&result, "onshape/step.unreadable"), 1);
    assert_eq!(feature_count(&wire(&result)), 3);
    assert!(result.ir().geometry.bodies.is_empty());
    crate::test_support::review::validate(result);
}
