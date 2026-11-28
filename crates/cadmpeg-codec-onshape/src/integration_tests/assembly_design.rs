// SPDX-License-Identifier: Apache-2.0
//! An Assembly states its design scope: its mates are `Joint` features and
//! its fixed occurrences are grounded (§5.1, §6.7).
use cadmpeg_core::decode::DecodeDepth;
use cadmpeg_core::json;
use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::reader::id;
use crate::test_support::assembly::{bundle, definition, mate, nested};
use crate::test_support::review::{cadir_round_trip, omitted, validate};

fn decode(definition: serde_json::Value) -> cadmpeg_ir::codec::DecodeResult {
    crate::test_support::codec()
        .decode(
            &mut std::io::Cursor::new(bundle(definition)),
            &DecodeOptions::default(),
        )
        .unwrap()
}

#[test]
fn shallow_assembly_omits_design_and_reports_it_at_geometry_depth() {
    let mut root = definition();
    root["rootAssembly"]["features"] = json!([mate("hinge", "REVOLUTE")]);
    for depth in [DecodeDepth::Container, DecodeDepth::Geometry] {
        let result = crate::test_support::codec()
            .decode(
                &mut std::io::Cursor::new(bundle(root.clone())),
                &DecodeOptions {
                    depth,
                    ..DecodeOptions::default()
                },
            )
            .unwrap();
        assert!(result.ir().design.features.is_empty());
        assert!(result.ir().design.scopes.is_empty());
        assert!(result.ir().model.assembly_joints.is_empty());
        if depth == DecodeDepth::Geometry {
            assert!(result
                .report()
                .losses
                .iter()
                .any(|loss| loss.code.local_code() == "depth.not-delivered"));
        }
        validate(result);
    }
}

fn joint(result: &cadmpeg_ir::codec::DecodeResult, key: &str) -> serde_json::Value {
    let wire = cadmpeg_test_support::wire::in_memory_json(result.ir());
    wire["design"]["features"]
        .as_array()
        .unwrap()
        .iter()
        .find(|feature| feature["id"] == id("joint", key))
        .unwrap_or_else(|| panic!("{key}: {:?}", result.report().losses))
        .clone()
}

#[test]
fn a_mate_is_a_joint_feature_of_the_assembly_scope() {
    let mut root = definition();
    root["rootAssembly"]["features"] = json!([mate("hinge", "REVOLUTE")]);
    let result = decode(root);
    let hinge = joint(&result, "[]:hinge");
    let operation = &hinge["definition"]["operation"]["value"];
    assert_eq!(
        operation["kind"],
        json!({"kind":"motion","value":{"motion":{"kind":"revolute","value":{"axis":"z"}},
            "offset":{},"limits":[],"drives":[]}})
    );
    assert_eq!(operation["alignment"], json!({"kind":"aligned"}));
    assert_eq!(hinge["suppressed"], json!({"kind":"literal","value":false}));
    let first = &operation["connectors"][0];
    assert_eq!(first["kind"], "frame");
    let pick = &first["inputs"][0]["reference"]["value"]["picks"]["value"][0];
    assert_eq!(pick["target"], json!({"kind":"component"}));
    assert_eq!(
        pick["context"]["value"],
        json!([{"occurrence":{"kind":"occurrence","value":id("occurrence", "[\"first\"]")}}])
    );
    assert_eq!(
        first["resolved"]["origin"],
        json!({"x":1.0,"y":2.0,"z":3.0})
    );
    let ir = result.ir();
    let scope = ir
        .design
        .scopes
        .iter()
        .find(|scope| {
            ir.model.product_definitions.iter().any(|definition| {
                definition.id == scope.definition && !definition.occurrences.is_empty()
            })
        })
        .unwrap();
    let grounded: Vec<_> = scope
        .occurrences
        .iter()
        .filter(|(_, design)| design.grounded == Some(true))
        .map(|(occurrence, _)| occurrence.as_str())
        .collect();
    assert_eq!(grounded, [id("occurrence", "[\"first\"]")]);
    validate(result);
}

#[test]
fn a_mate_a_sub_assembly_lists_names_it_and_starts_its_paths_there() {
    let result = decode(nested());
    let hinge = joint(&result, "[\"sub\"]:nestedMate");
    assert!(hinge["component"].is_string(), "{hinge}");
    let pick = &hinge["definition"]["operation"]["value"]["connectors"][1]["inputs"][0]
        ["reference"]["value"]["picks"]["value"][0];
    assert_eq!(
        pick["context"]["value"],
        json!([{"occurrence":{"kind":"occurrence","value":id("occurrence", "[\"sub\",\"second\"]")}}])
    );
    validate(result);
}

#[test]
fn a_cadir_write_carries_the_mates() {
    let mut root = definition();
    root["rootAssembly"]["features"] = json!([mate("hinge", "REVOLUTE")]);
    let result = decode(root);
    let (written, losses) = cadir_round_trip(&result);
    assert_eq!(written.design.features, result.ir().design.features);
    assert!(omitted(&losses, &id("joint", "fixed:[\"first\"]")).is_empty());
    assert!(result
        .ir()
        .model
        .assembly_joints
        .iter()
        .any(|joint| joint.id.as_str() == id("joint", "fixed:[\"first\"]")));
    assert!(
        omitted(&losses, &id("joint", "[]:hinge")).is_empty(),
        "{losses:#?}"
    );
}
