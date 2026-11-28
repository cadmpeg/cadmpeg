// SPDX-License-Identifier: Apache-2.0
//! A CC0 Fusion save whose Cylinder primitive uses the prefixed 489-byte form.

use super::decode;
use crate::loss::F3dLossCode;

#[test]
fn a_prefixed_489_byte_cylinder_takes_its_scope_local_frame() {
    let result = decode(
        include_bytes!("../../../../corpus/f3d/cylinder_primitive_prefixed_489.f3d").to_vec(),
    );
    assert!(
        result
            .report()
            .losses
            .iter()
            .all(|loss| loss.code != F3dLossCode::CylinderPlacementUnbound.kind()),
        "{:#?}",
        result.report().losses
    );
    let wire = cadmpeg_test_support::wire::in_memory_json(result.ir());
    let cylinder = wire["model"]["features"]
        .as_array()
        .expect("features")
        .iter()
        .find(|feature| feature["name"] == "CylinderPrimitive1")
        .expect("the cylinder feature");
    let definition = &cylinder["definition"]["value"]["value"];
    assert_eq!(definition["op"], "new_body", "{definition:#}");
    // Base centre (-40, -10, 0) mm, axis along z.
    let placement = definition["placement"].to_string();
    for row in [
        "[1.0,0.0,0.0,-40.0]",
        "[0.0,1.0,0.0,-10.0]",
        "[0.0,0.0,1.0,0.0]",
    ] {
        assert!(placement.contains(row), "{placement}");
    }
}
