// SPDX-License-Identifier: Apache-2.0

use super::*;
use cadmpeg_ir::features::SketchProfileBoundaryUse;
use cadmpeg_ir::sketches::SketchId;

fn id() -> SketchId {
    SketchId::mint("test:model:sketch#section").expect("sketch")
}

#[test]
fn whole_region_curve_membership_checks_the_selected_outer_and_hole_loops() {
    let profile = PlanarProfileRef::sketch_regions(
        id(),
        vec![SketchProfileRegion::loops(2, vec![4]).expect("region")],
    )
    .expect("profile");
    assert!(contains_entity(&profile, &id(), "curve", |index| index == 2));
    assert!(contains_entity(&profile, &id(), "curve", |index| index == 4));
    assert!(!contains_entity(&profile, &id(), "curve", |index| index == 3));
    let other = SketchId::mint("test:model:sketch#other").expect("sketch");
    assert!(!contains_entity(&profile, &other, "curve", |index| index == 2));
    assert!(!contains_entity(
        &PlanarProfileRef::Native("opaque".into()),
        &id(),
        "curve",
        |index| index == 2
    ));
}

#[test]
fn trimmed_region_curve_membership_uses_each_boundary_entity_identity() {
    let boundary = |name| {
        serde_json::from_value::<SketchProfileBoundaryUse>(serde_json::json!({
            "entity":format!("test:model:sketch-entity#{name}"), "parameter_range":[0.0,1.0]
        }))
        .expect("boundary")
    };
    let profile = PlanarProfileRef::sketch_regions(
        id(),
        vec![
            SketchProfileRegion::trimmed(vec![boundary("outer")], vec![vec![boundary("hole")]])
                .expect("region"),
        ],
    )
    .expect("profile");
    for name in ["outer", "hole"] {
        assert!(contains_entity(
            &profile,
            &id(),
            &format!("test:model:sketch-entity#{name}"),
            |index| index == u32::MAX
        ));
    }
    assert!(!contains_entity(
        &profile,
        &id(),
        "test:model:sketch-entity#other",
        |index| index == u32::MAX
    ));
}
