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

/// The Fusion readback states one input body per produced mesh body, and the
/// surface deviation `0.005 mm`, normal deviation `15`, and aspect ratio
/// `21.5` of each feature.
#[test]
fn a_tessellate_scope_counts_one_operand_per_source_body() {
    for (bytes, bodies) in [
        (
            &include_bytes!("../../../../corpus/f3d/tessellate_one_body.f3d")[..],
            1usize,
        ),
        (
            &include_bytes!("../../../../corpus/f3d/tessellate_two_bodies.f3d")[..],
            2,
        ),
    ] {
        let feature = tessellate_feature(bytes);
        let properties = &feature["source_properties"];
        assert_eq!(
            properties["mesh-tessellate-operand-count"],
            bodies.to_string()
        );
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
        assert_eq!(role_expression(&feature, "surface-deviation"), "0.005 mm");
        assert_eq!(role_expression(&feature, "normal-deviation"), "15");
        assert_eq!(role_expression(&feature, "aspect-ratio"), "21.5");
        assert!(properties
            .get("mesh-tessellate-parameter:maximum-edge-length")
            .is_some());
    }
}
