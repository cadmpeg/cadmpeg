// SPDX-License-Identifier: Apache-2.0

use cadmpeg_ir::compare::floats_agree;

use super::{mesh_options, CHORD_RATIO, MIN_CHORD_TOLERANCES};
use crate::test_support::decode;

/// A complete sphere has no vertices. The chord is a share of the diameter
/// box, not the tessellator's 0.01 default.
#[test]
fn an_untrimmed_sphere_takes_its_chord_from_the_radius() {
    let decoded = decode(
        "#usda 1.0\n(\n    metersPerUnit = 0.01\n)\ndef Sphere \"Ball\"\n{\n    double radius = 2\n}\n",
    );
    let ir = decoded.ir();
    let [body] = ir.model.bodies.as_slice() else {
        panic!("one body");
    };
    let options = mesh_options(&cadmpeg_ir::index::ModelIndex::new_model_only(ir), body);
    let radius = 20.0_f64;
    let diagonal = (3.0 * (2.0 * radius).powi(2)).sqrt();
    let expected = (diagonal * CHORD_RATIO).max(ir.tolerances.linear.get() * MIN_CHORD_TOLERANCES);
    assert!(
        floats_agree(options.chord, expected),
        "chord {} vs {expected} from diagonal {diagonal}",
        options.chord
    );
}

#[test]
fn a_box_keeps_the_chord_of_its_vertices() {
    let decoded = decode(
        "#usda 1.0\n(\n    metersPerUnit = 0.01\n)\ndef Cube \"Box\"\n{\n    double size = 3\n}\n",
    );
    let ir = decoded.ir();
    let [body] = ir.model.bodies.as_slice() else {
        panic!("one body");
    };
    let options = mesh_options(&cadmpeg_ir::index::ModelIndex::new_model_only(ir), body);
    assert!(options.chord > cadmpeg_tessellate::Options::default().chord);
    assert!(floats_agree(
        options.chord,
        30.0 * 3.0_f64.sqrt() * CHORD_RATIO
    ));
}
