// SPDX-License-Identifier: Apache-2.0

use std::io::Cursor;

use cadmpeg_core::decode::InspectOptions;
use cadmpeg_ir::codec::Codec;

use crate::UsdCodec;

#[test]
fn inspection_counts_prims_and_lists_named_assets() {
    let layer = b"#usda 1.0\n(\n    defaultPrim = \"A\"\n    subLayers = [@base.usda@]\n)\ndef Xform \"A\" (\n    prepend references = @part.usd@</P>\n)\n{\n    float a = 1\n    def Mesh \"M\" (\n        payload = @mesh.usdc@\n    )\n    {\n    }\n}\n";
    let summary = UsdCodec
        .inspect(&mut Cursor::new(&layer[..]), &InspectOptions::default())
        .unwrap();
    assert_eq!(summary.format(), "usd");
    let layer_entry = &summary.entries[0];
    assert_eq!(layer_entry.name, "layer");
    assert_eq!(layer_entry.attributes["prim_specs"], "2");
    assert_eq!(layer_entry.attributes["property_specs"], "1");
    assert_eq!(layer_entry.attributes["defaultPrim"], "A");
    assert_eq!(layer_entry.attributes["header"], "#usda 1.0");
    let assets: Vec<(&str, &str)> = summary.entries[1..]
        .iter()
        .map(|entry| (entry.name.as_str(), entry.attributes["arc"].as_str()))
        .collect();
    assert_eq!(
        assets,
        [
            ("base.usda", "subLayers"),
            ("mesh.usdc", "payload"),
            ("part.usd", "references")
        ]
    );
}
