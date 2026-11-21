// SPDX-License-Identifier: Apache-2.0
//! Mesh Tessellate of one and of two B-rep bodies.

use std::io::Cursor;

use cadmpeg_ir::{Codec, DecodeOptions};
use serde_json::Value;

use crate::F3dCodec;

fn tessellate_feature(bytes: &[u8]) -> Value {
    let result = F3dCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode the corpus file");
    let ir = serde_json::to_value(result.ir()).expect("serialize the IR");
    let mut features = ir["model"]["features"]
        .as_array()
        .expect("feature list")
        .iter()
        .filter(|feature| feature["source_tag"] == "MeshTessellate");
    let feature = features.next().expect("one MeshTessellate feature").clone();
    assert!(features.next().is_none());
    feature
}

/// The expression of the parameter that the owner slot of `role` names.
fn role_expression<'a>(feature: &'a Value, role: &str) -> &'a str {
    let properties = &feature["source_properties"];
    let name = properties[format!("mesh-tessellate-parameter:{role}")]
        .as_str()
        .expect("role parameter name");
    feature["definition"]["value"]["value"]["parameters"][name]
        .as_str()
        .expect("native parameter expression")
}

const ROLES: [&str; 4] = [
    "surface-deviation",
    "normal-deviation",
    "maximum-edge-length",
    "aspect-ratio",
];

/// The Fusion readback states one input body per produced mesh body, the
/// refinement selector, and the model parameter of each owner role, in
/// `ROLES` order.
#[test]
fn a_tessellate_scope_counts_one_operand_per_source_body() {
    let default = ["0.005 mm", "15", "34.641 mm", "21.5"];
    for (bytes, bodies, refinement, expressions) in [
        (
            &include_bytes!("../../../../corpus/f3d/tessellate_one_body.f3d")[..],
            1usize,
            "1",
            default,
        ),
        (
            &include_bytes!("../../../../corpus/f3d/tessellate_two_bodies.f3d")[..],
            2,
            "1",
            default,
        ),
        (
            &include_bytes!("../../../../corpus/f3d/tessellate_convert.f3d")[..],
            1,
            "1",
            default,
        ),
        (
            &include_bytes!("../../../../corpus/f3d/tessellate_reduce.f3d")[..],
            1,
            "3",
            ["0.05 mm", "15", "2 mm", "21.5"],
        ),
    ] {
        let feature = tessellate_feature(bytes);
        let properties = &feature["source_properties"];
        assert_eq!(
            properties["mesh-tessellate-operand-count"],
            bodies.to_string()
        );
        assert_eq!(properties["mesh-tessellate-refinement"], refinement);
        let produced = (0..bodies)
            .map(|ordinal| {
                properties[format!("mesh-tessellate-body:{ordinal}")]
                    .as_str()
                    .expect("produced body")
                    .to_owned()
            })
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(produced.len(), bodies);
        assert!(properties
            .get(format!("mesh-tessellate-body:{bodies}"))
            .is_none());
        for (role, expression) in ROLES.into_iter().zip(expressions) {
            assert_eq!(role_expression(&feature, role), expression, "{role}");
        }
    }
}

fn features_by_tag(bytes: &[u8], tag: &str) -> Vec<Value> {
    let result = F3dCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode the corpus file");
    let ir = serde_json::to_value(result.ir()).expect("serialize the IR");
    ir["model"]["features"]
        .as_array()
        .expect("feature list")
        .iter()
        .filter(|feature| feature["source_tag"] == tag)
        .cloned()
        .collect()
}

/// The later Mesh Reduce and Convert Mesh take the mesh body that the
/// Tessellate produces as their source.
#[test]
fn a_later_mesh_operation_takes_the_produced_body() {
    for (bytes, tag, key) in [
        (
            &include_bytes!("../../../../corpus/f3d/tessellate_reduce.f3d")[..],
            "MeshReduce",
            "mesh-body-reference:0",
        ),
        (
            &include_bytes!("../../../../corpus/f3d/tessellate_convert.f3d")[..],
            "Convert Mesh",
            "mesh-conversion-body:0:0",
        ),
    ] {
        let produced = tessellate_feature(bytes)["source_properties"]["mesh-tessellate-body:0"].clone();
        let later = features_by_tag(bytes, tag);
        assert_eq!(later.len(), 1, "{tag}");
        assert_eq!(later[0]["source_properties"][key], produced, "{tag}");
    }
}

/// A Convert Mesh of the one produced body leaves the collection listing no
/// body, an empty ParaMesh registry.
#[test]
fn a_converted_produced_body_leaves_an_empty_collection() {
    let result = F3dCodec
        .decode(
            &mut Cursor::new(&include_bytes!("../../../../corpus/f3d/tessellate_convert.f3d")[..]),
            &DecodeOptions::default(),
        )
        .expect("decode the corpus file");
    let codes = result
        .report()
        .losses
        .iter()
        .map(|loss| loss.code.local_code().to_owned())
        .collect::<Vec<_>>();
    assert!(!codes.contains(&"mesh.container-undecoded".to_owned()), "{codes:?}");
    assert!(codes.contains(&"mesh.container-unjoined".to_owned()), "{codes:?}");
}
