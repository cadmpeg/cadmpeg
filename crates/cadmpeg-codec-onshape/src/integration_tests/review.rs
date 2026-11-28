// SPDX-License-Identifier: Apache-2.0
//! Regression cases for source admission and history fidelity.
use crate::test_support::review::{decode, edit, members, parameter, validate, variable, wire};
use crate::test_support::{codec, fixture, Charge};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodeDepth, DecodePolicy};
use cadmpeg_core::json;
use cadmpeg_core::serde_erase::json;
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::{Codec, CodecBackend, DecodeOptions};
use cadmpeg_ir::unknown::part::Cause;
use serde_json::Value;
use std::io::Cursor;

fn loss(result: &cadmpeg_ir::codec::DecodeResult, code: &str, charge: Charge) -> bool {
    result.report().losses.iter().any(|loss| {
        crate::test_support::states(code, charge, &loss.code.to_string(), &loss.message)
    })
}
fn kind(wire: &Value, ordinal: usize) -> &Value {
    &wire["model"]["features"][ordinal]["definition"]["value"]["kind"]
}

#[test]
fn stored_json_part_refusals_are_losses_and_session_entities_remain_errors() {
    let original = fixture("plate-fillet");
    for resource in ["depth", "entities"] {
        let bytes = edit(&original, |path, value| {
            if path.ends_with("features.json") && resource == "depth" {
                value["extra"] = json!([[[[[[[[0]]]]]]]]);
            }
        });
        let mut policy = DecodePolicy::default();
        if resource == "depth" {
            policy.limits.max_recursion_depth = 7;
        } else {
            policy.limits.max_entities = 3;
        }
        let arena = DecodeArena::new();
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
        let result = admission_only(&ctx, root);
        if resource == "entities" {
            let error = result.unwrap_err();
            let CodecError::ResourceLimit(limit) = error else {
                panic!("expected a session entity limit");
            };
            assert_eq!(limit.scope, cadmpeg_core::decode::LimitScope::Session);
            assert_eq!(
                limit.dimension,
                cadmpeg_core::decode::ResourceDimension::Entities
            );
            assert_eq!(limit.operation, "Onshape history admission");
        } else {
            let decoded = result.unwrap();
            let notes: Vec<_> = decoded
                .body
                .losses
                .iter()
                .filter(|note| {
                    crate::test_support::states(
                        "onshape/member.unreadable",
                        Charge::GivenUp(Cause::Undecoded),
                        &note.code.to_string(),
                        &note.message,
                    )
                })
                .collect();
            assert_eq!(notes.len(), 1, "{resource}");
            // The give-up of the member is the one entry for the lost history.
            assert!(!decoded
                .body
                .losses
                .iter()
                .any(|note| crate::loss::OnshapeLossCode::HistoryNotSupported.matches(note)));
            assert!(decoded.ir.model.features.is_empty());
            assert!(decoded.ir.model.assets.iter().any(|asset| asset
                .name
                .as_ref()
                .is_some_and(|name| name.as_str() == "partstudios/studio/export.step")));
            assert!(decoded.ir.model.assets.iter().all(|asset| asset
                .name
                .as_ref()
                .is_none_or(|name| name.as_str() != "partstudios/studio/features.json")));
            assert!(ctx.resource_refusal().is_none());
        }
    }
}

/// The Onshape admission of `root` with a backend that builds an empty
/// document.
fn admission_only(
    ctx: &DecodeContext<'_>,
    root: cadmpeg_core::decode::View<'_>,
) -> Result<cadmpeg_ir::codec::Decoded, CodecError> {
    let backend = crate::OnshapeCodec::new(|ctx, root| {
        Ok(cadmpeg_ir::codec::Decoded {
            ir: cadmpeg_ir::CadIr::empty(),
            body: cadmpeg_ir::codec::DecodeBody::new(
                cadmpeg_ir::report::decode::DecodeTransfer::at(ctx.depth(), false),
            ),
            source_fidelity: cadmpeg_ir::SourceFidelity::for_root(root),
        })
    });
    backend.decode_impl(ctx, root)
}

/// A stored JSON member's tree is charged to the session's expanded bytes
/// (decode-resource-caps). The `features.json` member with 4096 added
/// numbers needs at least 64 bytes for each of their values. A ceiling one
/// below the session's count at that member refuses the decode in the
/// session, and the default ceiling admits it.
#[test]
fn a_json_member_past_the_expanded_bytes_ceiling_refuses_the_session() {
    use cadmpeg_core::decode::test_support::at_charge;
    use cadmpeg_core::decode::{LimitScope, ResourceDimension};

    let bytes = edit(&fixture("plate-fillet"), |path, value| {
        if path.ends_with("features.json") {
            value["extra"] = json!(vec![0; 4096]);
        }
    });
    let refusal = |total: u64| {
        let mut policy = DecodePolicy::default();
        policy.limits.max_decompressed_bytes_total = total;
        let arena = DecodeArena::new();
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
        match admission_only(&ctx, root) {
            Err(CodecError::ResourceLimit(limit)) => {
                assert_eq!(ctx.resource_refusal(), Some(limit));
                Some(limit)
            }
            Err(error) => panic!("expected a resource limit: {error}"),
            Ok(_) => None,
        }
    };
    let entries = zip::ZipArchive::new(Cursor::new(&bytes)).unwrap().len();
    let found = (0..entries)
        .filter_map(|skip| {
            at_charge(
                ResourceDimension::DecompressedBytes,
                "Onshape JSON member",
                skip,
                || refusal(u64::MAX),
            )
        })
        .find(|limit| limit.additional > 4096 * 64)
        .expect("the wide member is charged");
    assert_eq!(found.scope, LimitScope::Session);
    assert_eq!(refusal(found.used + found.additional - 1), Some(found));
    assert_eq!(
        refusal(DecodePolicy::default().limits.max_decompressed_bytes_total),
        None
    );
}

#[test]
fn latest_refused_variable_binding_blocks_later_consumers() {
    for replacement in ["2 mm", "unsupported(2 mm)", "repeated-name", "missing-name"] {
        let bytes = edit(&fixture("plate-fillet"), |path, value| {
            if !path.ends_with("features.json") {
                return;
            }
            let features = value["features"].as_array_mut().unwrap();
            parameter(&mut features[1], "depth")["expression"] = json!("#span");
            let mut replaced = variable(
                "span",
                "replacement",
                "LENGTH",
                if replacement == "repeated-name" {
                    "2 mm"
                } else {
                    replacement
                },
            );
            if replacement == "missing-name" {
                replaced["parameters"]
                    .as_array_mut()
                    .unwrap()
                    .retain(|p| p["parameterId"] != "name");
            }
            if replacement == "repeated-name" {
                replaced["parameters"]
                    .as_array_mut()
                    .unwrap()
                    .push(json!({"parameterId":"name","value":"span"}));
            }
            features.insert(1, replaced);
            features.insert(1, variable("span", "original", "LENGTH", "1 mm"));
        });
        let result = decode(&bytes);
        let wire = wire(&result);
        assert_eq!(kind(&wire, 2), "native");
        assert_eq!(kind(&wire, 3), "native");
        assert!(result
            .report()
            .losses
            .iter()
            .any(|l| l.message.contains("unresolved latest binding")));
        assert!(loss(&result, "onshape/history.not-supported", Charge::Note));
        validate(result);
    }
}

#[test]
fn boolean_operands_refuse_invalid_and_repeated_values_and_keep_valid_controls() {
    for key in [
        "oppositeDirection",
        "hasDraft",
        "symmetric",
        "hasSecondDirection",
    ] {
        for values in [
            vec![json!(false)],
            vec![json!(true)],
            vec![json!("true")],
            vec![json!(1)],
            vec![json!(false), json!(true)],
        ] {
            let bytes = edit(&fixture("plate-fillet"), |path, response| {
                if !path.ends_with("features.json") {
                    return;
                }
                let parameters = response["features"][1]["parameters"]
                    .as_array_mut()
                    .unwrap();
                for value in &values {
                    parameters.push(json!({"parameterId":key,"value":value}));
                }
            });
            let result = decode(&bytes);
            let wire = wire(&result);
            let valid = values.len() == 1 && values[0].is_boolean();
            let gated = matches!(key, "hasDraft" | "hasSecondDirection") && values[0] == true;
            assert_eq!(
                kind(&wire, 1),
                if valid && !gated { "extrude" } else { "native" },
                "{key} {values:?}"
            );
            if valid && !gated && key == "oppositeDirection" {
                assert_eq!(
                    wire["model"]["features"][1]["definition"]["value"]["value"]["direction"]
                        ["kind"],
                    if values[0] == true {
                        "reversed_profile_normal"
                    } else {
                        "profile_normal"
                    }
                );
            }
            if !valid || gated {
                assert!(loss(&result, "onshape/history.not-supported", Charge::Note));
            }
            validate(result);
        }
    }
    for key in ["suppressed", "parentSuppressed"] {
        for value in [json!("false"), json!(0), json!(null)] {
            let bytes = edit(&fixture("plate-fillet"), |path, response| {
                if path.ends_with("features.json") {
                    response["features"][1][key] = value.clone();
                }
            });
            let result = decode(&bytes);
            let wire = wire(&result);
            assert_eq!(kind(&wire, 1), "native");
            assert!(wire["model"]["features"][1].get("suppressed").is_none());
            assert!(loss(&result, "onshape/history.not-supported", Charge::Note));
        }
    }
}

#[test]
fn quantities_require_dimensioned_expressions_or_explicit_units() {
    for (source, expected) in [
        (json!({"value":1.0}), None),
        (json!({"value":1.0,"units":1}), None),
        (json!({"value":1.0,"units":""}), None),
        (json!({"value":1.0,"units":"mm"}), Some(1.0)),
        (json!({"value":1.0,"units":"m"}), Some(1000.0)),
        (json!({"expression":"1 mm + 2 mm","value":0.0}), Some(3.0)),
        (
            json!({"expression":"(1 cm + 2 mm) / 2","value":0.0}),
            Some(6.0),
        ),
        (
            json!({"expression":"2 * (1 mm + 2 mm)","value":0.0}),
            Some(6.0),
        ),
    ] {
        let bytes = edit(&fixture("plate-fillet"), |path, response| {
            if path.ends_with("features.json") {
                let target = parameter(&mut response["features"][1], "depth");
                *target = source.clone();
                target["parameterId"] = json!("depth");
            }
        });
        let result = decode(&bytes);
        let wire = wire(&result);
        if let Some(expected) = expected {
            assert_eq!(kind(&wire, 1), "extrude");
            assert_eq!(
                wire["model"]["features"][1]["definition"]["value"]["value"]["extent"]["side"]
                    ["termination"]["length"],
                expected
            );
        } else {
            assert_eq!(kind(&wire, 1), "native");
            assert!(loss(&result, "onshape/history.not-supported", Charge::Note));
        }
        validate(result);
    }
    for (units, value, expected) in [
        (json!(null), 1.0, None),
        (json!(17), 1.0, None),
        (json!("deg"), 90.0, Some(std::f64::consts::FRAC_PI_2)),
        (json!("rad"), 1.0, Some(1.0)),
    ] {
        let bytes = edit(&fixture("revolve"), |path, response| {
            if path.ends_with("features.json") {
                let feature = &mut response["features"][1];
                parameter(feature, "revolveType")["value"] = json!("ONE_DIRECTION");
                feature["parameters"]
                    .as_array_mut()
                    .unwrap()
                    .push(json!({"parameterId":"angle","value":value,"units":units}));
            }
        });
        let result = decode(&bytes);
        let wire = wire(&result);
        if let Some(expected) = expected {
            assert_eq!(kind(&wire, 1), "revolve");
            let angle = wire["model"]["features"][1]["definition"]["value"]["value"]
                ["construction"]["value"]["extent"]["termination"]["angle"]
                .as_f64()
                .unwrap();
            assert!((angle - expected).abs() <= f64::EPSILON);
        } else {
            assert_eq!(kind(&wire, 1), "native");
            assert!(loss(&result, "onshape/history.not-supported", Charge::Note));
        }
        validate(result);
    }
}

#[test]
fn thin_open_curves_refuse_unresolved_sides_for_both_directions_and_flips() {
    for reversed in [false, true] {
        for flip in [false, true] {
            let bytes = edit(&fixture("plate-fillet"), |path, response| {
                if !path.ends_with("features.json") {
                    return;
                }
                response["features"][0]["entities"]
                    .as_array_mut()
                    .unwrap()
                    .truncate(1);
                response["features"][0]["constraints"] = json!([]);
                if reversed {
                    response["features"][0]["entities"][0]["geometry"]["pntX"] = json!(0.02);
                    response["features"][0]["entities"][0]["geometry"]["dirX"] = json!(-1.0);
                }
                let feature = &mut response["features"][1];
                parameter(feature, "bodyType")["value"] = json!("THIN");
                parameter(feature, "entities")["parameterId"] = json!("wallShape");
                feature["parameters"].as_array_mut().unwrap().extend([
                    json!({"parameterId":"midplane","value":false}),
                    json!({"parameterId":"flipWall","value":flip}),
                    json!({"parameterId":"thickness1","expression":"1 mm"}),
                    json!({"parameterId":"thickness2","expression":"0 mm"}),
                ]);
            });
            let result = decode(&bytes);
            assert_eq!(kind(&wire(&result), 1), "native");
            assert_eq!(
                crate::test_support::review::design_operation(result.ir(), "Plate"),
                crate::test_support::review::unresolved("extrude")
            );
            assert!(loss(
                &result,
                "onshape/extrude.thin-not-supported",
                Charge::Note
            ));
            validate(result);
        }
    }
}

#[test]
fn blind_holes_require_drill_point_and_depth_reference_and_read_thread_facts() {
    for case in [
        "missing-angle",
        "repeated-angle",
        "missing-depth-reference",
        "invalid-depth-reference",
        "left",
        "tip",
        "missing-thread-hand",
        "missing-thread-mode",
    ] {
        let bytes = edit(&fixture("hole-forms"), |path, response| {
            if !path.ends_with("features.json") {
                return;
            }
            let feature = &mut response["features"][5];
            match case {
                "missing-angle" => feature["parameters"]
                    .as_array_mut()
                    .unwrap()
                    .retain(|p| p["parameterId"] != "tipAngle"),
                "repeated-angle" => feature["parameters"]
                    .as_array_mut()
                    .unwrap()
                    .push(json!({"parameterId":"tipAngle","expression":"90 deg"})),
                "missing-depth-reference" => feature["parameters"]
                    .as_array_mut()
                    .unwrap()
                    .retain(|p| p["parameterId"] != "depthToTip"),
                "invalid-depth-reference" => {
                    parameter(feature, "depthToTip")["value"] = json!("false");
                }
                "left" => {
                    let thread = &mut parameter(feature, "standardTappedOrClearance")["value"];
                    thread["hand"] = json!("left");
                    thread["modeled"] = json!(true);
                    thread["cosmetic"] = json!(false);
                }
                "tip" => parameter(feature, "depthToTip")["value"] = json!(true),
                "missing-thread-hand" => {
                    parameter(feature, "standardTappedOrClearance")["value"]
                        .as_object_mut()
                        .unwrap()
                        .remove("hand");
                }
                "missing-thread-mode" => {
                    parameter(feature, "standardTappedOrClearance")["value"]
                        .as_object_mut()
                        .unwrap()
                        .remove("modeled");
                }
                _ => unreachable!(),
            }
        });
        let result = decode(&bytes);
        let wire = wire(&result);
        if matches!(case, "left" | "tip") {
            assert_eq!(
                kind(&wire, 5),
                "hole",
                "{case}: {:?}",
                result.report().losses
            );
            let hole = &wire["model"]["features"][5]["definition"]["value"]["value"];
            if case == "left" {
                let thread = &hole["shape"]["construction"]["value"]["specification"];
                assert_eq!(thread["hand"], "left");
                assert_eq!(thread["modeled"], true);
                assert_eq!(thread["cosmetic"], false);
            } else {
                assert_eq!(hole["bottom"]["depth_to_tip"], true);
            }
        } else {
            assert_eq!(kind(&wire, 5), "native", "{case}");
            assert!(loss(&result, "onshape/history.not-supported", Charge::Note));
        }
        validate(result);
    }
}

#[test]
fn inch_thread_pitches_convert_to_millimetres() {
    let bytes = edit(&fixture("hole-forms"), |path, response| {
        if path.ends_with("features.json") {
            parameter(&mut response["features"][5], "standardTappedOrClearance")["value"]
                ["pitch"] = json!("0.25 in");
        }
    });
    let result = decode(&bytes);
    let wire = wire(&result);
    let pitch = wire["model"]["features"][5]["definition"]["value"]["value"]["shape"]
        ["construction"]["value"]["specification"]["pitch"]
        .as_f64()
        .unwrap();
    assert!((pitch - 6.35).abs() < 1e-9, "{pitch}");
    assert!(!result
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains("kept the thread without pitch")));
    validate(result);
}

#[test]
fn unresolved_thread_pitches_keep_threads_and_charge_a_loss() {
    for pitch in [json!("0 in"), json!("0 mm"), json!(0.7), json!("0.7")] {
        let bytes = edit(&fixture("hole-forms"), |path, response| {
            if path.ends_with("features.json") {
                parameter(&mut response["features"][5], "standardTappedOrClearance")["value"]
                    ["pitch"] = pitch.clone();
            }
        });
        let result = decode(&bytes);
        let wire = wire(&result);
        assert_eq!(kind(&wire, 5), "hole");
        let thread = &wire["model"]["features"][5]["definition"]["value"]["value"]["shape"]
            ["construction"]["value"]["specification"];
        assert_eq!(thread["kind"], "threaded");
        assert_eq!(thread["designation"], "M4");
        assert!(thread.get("pitch").is_none());
        assert!(result.report().losses.iter().any(|loss| {
            loss.code.to_string() == "onshape/record.malformed"
                && loss.message.contains(&pitch.to_string())
                && loss.message.contains("kept the thread without pitch")
        }));
        validate(result);
    }
}

#[test]
fn negative_variable_angles_keep_their_signed_parameter_value() {
    let bytes = edit(&fixture("revolve"), |path, response| {
        if !path.ends_with("features.json") {
            return;
        }
        let features = response["features"].as_array_mut().unwrap();
        parameter(&mut features[1], "revolveType")["value"] = json!("ONE_DIRECTION");
        features[1]["parameters"]
            .as_array_mut()
            .unwrap()
            .push(json!({"parameterId":"angle","expression":"#sweep"}));
        features.insert(1, variable("sweep", "sweep-var", "ANGLE", "-90 deg"));
    });
    let bytes = members(&bytes, |members| {
        let state = members
            .remove("partstudios/studio/bodydetails/1.json")
            .unwrap();
        members.insert("partstudios/studio/bodydetails/2.json".into(), state);
        members.insert(
            "partstudios/studio/bodydetails/index.json".into(),
            b"[2]".to_vec(),
        );
    });
    let result = decode(&bytes);
    let wire = wire(&result);
    assert_eq!(kind(&wire, 2), "native");
    assert!(result
        .report()
        .losses
        .iter()
        .any(|l| l.code.to_string() == "onshape/history.not-supported"
            && l.message.contains("negative revolve angle")));
    let parameters = wire["model"]["parameters"].as_array().unwrap();
    let variable = parameters
        .iter()
        .find(|p| p["name"] == "sweep" && p["owner"].is_null())
        .unwrap();
    assert!(
        (variable["value"]["value"].as_f64().unwrap() + std::f64::consts::FRAC_PI_2).abs()
            < f64::EPSILON
    );
    assert!(result.ir().model.parameters.iter().all(|p| p
        .owner
        .as_ref()
        .is_none_or(|owner| owner.as_str() != crate::reader::id("feature", "Ring"))));
    validate(result);
}

#[test]
fn stale_unused_and_optional_responses_are_omitted_at_geometry_and_design_depth() {
    for path in [
        "partstudios/studio/bodydetails/77.json",
        "partstudios/studio/configuration.json",
        "partstudios/studio/variables.json",
    ] {
        for depth in [DecodeDepth::Geometry, DecodeDepth::Design] {
            let bytes = members(&fixture("plate-fillet"), |members| {
                members.insert(
                    path.into(),
                    json::to_vec(
                        &json!({"microversionId":{"theId":"another-snapshot"},"bodies":[]}),
                    )
                    .unwrap(),
                );
                if path.contains("bodydetails") {
                    members.insert(
                        "partstudios/studio/bodydetails/index.json".into(),
                        b"[2,77]".to_vec(),
                    );
                }
            });
            let result = codec()
                .decode(
                    &mut Cursor::new(bytes),
                    &DecodeOptions {
                        depth,
                        ..Default::default()
                    },
                )
                .unwrap();
            assert!(result.report().losses.iter().any(|note| {
                crate::test_support::states(
                    "onshape/member.unreadable",
                    Charge::GivenUp(Cause::Unreadable),
                    &note.code.to_string(),
                    &note.message,
                )
            }));
            assert!(result
                .ir()
                .model
                .assets
                .iter()
                .all(|asset| asset.name.as_ref().is_none_or(|name| name.as_str() != path)));
            assert_eq!(result.ir().geometry.bodies.len(), 1);
            validate(result);
        }
    }
}

#[test]
fn dependencies_follow_consumed_sketches_and_preserve_timeline_ordinals() {
    let bytes = edit(&fixture("plate-fillet"), |path, response| {
        if !path.ends_with("features.json") {
            return;
        }
        let features = response["features"].as_array_mut().unwrap();
        let mut independent = features[0].clone();
        independent["featureId"] = json!("independent");
        features.insert(1, independent);
        features.truncate(3);
        features[2]["parameters"].as_array_mut().unwrap().extend([
            json!({"parameterId":"hasDraft","value":false}),
            json!({"parameterId":"draftFaces","queries":[{"featureId":"independent"}]}),
        ]);
    });
    let result = decode(&bytes);
    let wire = wire(&result);
    assert_eq!(wire["model"]["features"][0]["dependencies"], json!([]));
    assert_eq!(wire["model"]["features"][1]["dependencies"], json!([]));
    assert_eq!(
        wire["model"]["features"][2]["dependencies"],
        json!([crate::reader::id("feature", "Base sketch")])
    );
    assert_eq!(wire["model"]["features"][2]["ordinal"], 2);
    validate(result);
}

#[test]
fn multi_body_decode_names_an_unmatched_creator_and_unsupported_thin_bound() {
    let bytes = members(&fixture("plate-fillet"), |members| {
        members.insert(
            "partstudios/studio/export.step".into(),
            crate::test_support::review::two_body_step(),
        );
    });
    let result = decode(&bytes);
    assert_eq!(result.ir().geometry.bodies.len(), 2);
    assert!(loss(
        &result,
        "onshape/history.part-not-matched",
        Charge::Note
    ));
    validate(result);
    let bytes = edit(&fixture("plate-fillet"), |path, response| {
        if !path.ends_with("features.json") {
            return;
        }
        let feature = &mut response["features"][1];
        parameter(feature, "bodyType")["value"] = json!("THIN");
        parameter(feature, "endBound")["value"] = json!("UP_TO_NEXT");
    });
    let result = decode(&bytes);
    assert_eq!(kind(&wire(&result), 1), "native");
    assert!(loss(
        &result,
        "onshape/extrude.thin-not-supported",
        Charge::Note
    ));
    validate(result);
}

#[test]
fn native_sketch_entities_keep_supported_lines_and_gate_whole_sketch_consumers() {
    let bytes = edit(&fixture("plate-fillet"), |path, response| {
        if !path.ends_with("features.json") {
            return;
        }
        response["features"][0]["entities"]
            .as_array_mut()
            .unwrap()
            .push(json!({
            "btType":"BTMSketchCurve-4","entityId":"unresolved","geometry":{
                "btType":"BTCurveGeometryCircle-115","radius":0.0}}));
    });
    let result = decode(&bytes);
    let wire = wire(&result);
    assert_eq!(kind(&wire, 0), "sketch");
    assert_eq!(kind(&wire, 1), "native");
    assert_eq!(
        wire["model"]["sketch_entities"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|e| e["geometry"]["kind"] == "line")
            .count(),
        4
    );
    assert!(loss(
        &result,
        "onshape/sketch.geometry-not-supported",
        Charge::Note
    ));
    validate(result);
}

#[test]
fn manifest_detection_preserves_the_context_tree_refusal() {
    let bytes = edit(&fixture("revolve"), |path, response| {
        if path == "manifest.json" {
            response["extra"] = json!(vec![0; 4096]);
        }
    });
    // The edited manifest holds 4096 added numbers, a tree of more than
    // 4096 * 64 bytes.
    let mut policy = DecodePolicy::default();
    policy.limits.max_decompressed_bytes_total = 32_768;
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    let whole = cadmpeg_core::decode::WholeInput::of_source(root, &mut Cursor::new(&bytes))
        .unwrap()
        .unwrap();
    assert!(matches!(
        codec().detect_whole_impl(&ctx, whole),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::DecompressedBytes
                && limit.operation == "Onshape manifest"
    ));
}

#[test]
fn a_resolved_binding_after_a_refused_initial_assignment_can_drive_a_consumer() {
    let bytes = edit(&fixture("plate-fillet"), |path, response| {
        if !path.ends_with("features.json") {
            return;
        }
        let features = response["features"].as_array_mut().unwrap();
        parameter(&mut features[1], "depth")["expression"] = json!("#span");
        features.insert(1, variable("span", "resolved-binding", "LENGTH", "2 mm"));
        features.insert(
            1,
            variable("span", "refused-binding", "LENGTH", "unsupported(1 mm)"),
        );
    });
    let result = decode(&bytes);
    let wire = wire(&result);
    assert_eq!(kind(&wire, 1), "native");
    assert_eq!(kind(&wire, 2), "tree_node");
    assert_eq!(kind(&wire, 3), "extrude");
    assert_eq!(
        wire["model"]["features"][3]["definition"]["value"]["value"]["extent"]["side"]
            ["termination"]["length"],
        2.0
    );
    validate(result);
}

#[test]
fn rollback_boundary_rows_are_admitted_before_topology_or_step_construction() {
    let bytes = edit(&fixture("plate-fillet"), |path, response| {
        if path.ends_with("bodydetails/2.json") {
            response["bodies"][0]["faces"] = json!([{"id":"face","loops":[{"coedges":vec![json!({"edgeId":"upright","orientation":true});64]}]}]);
        }
    });
    let mut policy = DecodePolicy::default();
    policy.limits.max_entities = 32;
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    let codec = crate::OnshapeCodec::new(|_, _| {
        Err(CodecError::malformed(
            "STEP callback reached before topology admission",
        ))
    });
    let error = codec.decode_impl(&ctx, root).unwrap_err();
    assert!(matches!(error, CodecError::ResourceLimit(_)));
    let CodecError::ResourceLimit(limit) = &error else {
        unreachable!()
    };
    assert_eq!(
        limit.dimension,
        cadmpeg_core::decode::ResourceDimension::Entities
    );
    assert_eq!(limit.operation, "Onshape topology admission");
}

#[test]
fn explicit_unresolved_dependency_relations_retain_native_construction() {
    for reference in ["Plate", "unresolved-source"] {
        let bytes = edit(&fixture("plate-fillet"), |path, response| {
            if path.ends_with("features.json") {
                parameter(&mut response["features"][2], "entities")["queries"][0]["featureId"] =
                    json!(reference);
            }
        });
        let result = decode(&bytes);
        let wire = wire(&result);
        if reference == "Plate" {
            assert_eq!(kind(&wire, 2), "fillet");
            assert_eq!(
                wire["model"]["features"][2]["dependencies"],
                json!([crate::reader::id("feature", "Plate")])
            );
        } else {
            assert_eq!(kind(&wire, 2), "native");
            assert!(result
                .report()
                .losses
                .iter()
                .any(|l| l.code.to_string() == "onshape/history.not-supported"
                    && l.message.contains("feature dependency")));
            assert!(result.ir().model.feature_input_topologies.is_empty());
        }
        validate(result);
    }
}

#[test]
fn opaque_additional_json_members_remain_bytes_and_empty_members_keep_their_loss() {
    let bytes = members(&fixture("plate-fillet"), |members| {
        members.insert(
            "attachments/opaque.json".into(),
            b"opaque native payload".to_vec(),
        );
        members.insert("attachments/empty.json".into(), Vec::new());
        members.insert(
            "partstudios/another/configuration.json".into(),
            b"opaque unrelated payload".to_vec(),
        );
    });
    let result = decode(&bytes);
    assert_eq!(kind(&wire(&result), 1), "extrude");
    assert!(loss(
        &result,
        "onshape/container.empty-member",
        Charge::Note
    ));
    let asset = result
        .ir()
        .model
        .assets
        .iter()
        .find(|a| {
            a.name
                .as_ref()
                .is_some_and(|n| n.as_str() == "attachments/opaque.json")
        })
        .unwrap();
    let cadmpeg_ir::assets::AssetContent::Embedded { data } = &asset.content else {
        unreachable!()
    };
    assert_eq!(data.as_bytes(), b"opaque native payload");
    validate(result);
}

#[test]
fn unresolved_variables_refuse_sketch_dimension_values_and_leave_inactive_values_unused() {
    for constraint_consumer in [false, true] {
        let bytes = edit(&fixture("plate-fillet"), |path, response| {
            if !path.ends_with("features.json") {
                return;
            }
            let features = response["features"].as_array_mut().unwrap();
            if constraint_consumer {
                parameter(&mut features[0]["constraints"][1], "length")["expression"] =
                    json!("#span");
                features.insert(0, variable("span", "replacement", "LENGTH", "2 mm"));
                features.insert(0, variable("span", "original", "LENGTH", "1 mm"));
            } else {
                parameter(&mut features[1], "depth")["expression"] = json!("#span");
                parameter(&mut features[1], "endBound")["value"] = json!("THROUGH_ALL");
                features.insert(1, variable("span", "replacement", "LENGTH", "2 mm"));
                features.insert(1, variable("span", "original", "LENGTH", "1 mm"));
            }
        });
        let result = decode(&bytes);
        let wire = wire(&result);
        assert_eq!(kind(&wire, 3), "extrude");
        let consumer = crate::reader::id(
            "feature",
            if constraint_consumer {
                "Base sketch"
            } else {
                "Plate"
            },
        );
        if constraint_consumer {
            let definition = wire["design"]["parameters"]
                .as_array()
                .unwrap()
                .iter()
                .find(|parameter| parameter["owner"]["value"] == consumer)
                .unwrap();
            assert_eq!(definition["definition"]["value"]["kind"], "source");
            assert_eq!(
                definition["definition"]["value"]["value"]["tokens"][0]["value"],
                "#span"
            );
            assert!(definition.get("value").is_none());
        } else {
            assert!(result
                .ir()
                .model
                .parameters
                .iter()
                .all(|parameter| parameter
                    .owner
                    .as_ref()
                    .is_none_or(|owner| owner.as_str() != consumer)));
            assert_eq!(
                wire["model"]["features"][3]["definition"]["value"]["value"]["extent"]["side"]
                    ["termination"]["kind"],
                "through_all"
            );
        }
        validate(result);
    }
}
