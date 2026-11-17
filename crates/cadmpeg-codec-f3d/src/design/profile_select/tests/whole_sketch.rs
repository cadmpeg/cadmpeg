// SPDX-License-Identifier: Apache-2.0

use cadmpeg_ir::features::SketchProfileRegion;
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::sketches::{Sketch, SketchEntityId, SketchEntityUse, SketchId, SketchPlacement};

fn whole_sketch_test_sketch(profile_count: u32) -> Sketch {
    let mut sketch = Sketch {
        id: SketchId::mint("synthetic:test:sketch#whole").unwrap(),
        name: None,
        configuration: None,
        visible: None,
        native_ref: None,
        placement: SketchPlacement::try_resolved(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .unwrap(),
        profiles: cadmpeg_ir::sketches::SketchProfiles::default(),
    };
    sketch.profiles = cadmpeg_ir::sketches::SketchProfiles::try_from(
        (0..profile_count)
            .map(|index| {
                vec![SketchEntityUse {
                    entity: SketchEntityId::mint(format!("synthetic:test:curve#{index}")).unwrap(),
                    reversed: false,
                }]
            })
            .collect::<Vec<_>>(),
    )
    .unwrap();
    sketch
}

#[test]
fn whole_sketch_selection_names_every_profile_as_its_own_region() {
    let sketch = whole_sketch_test_sketch(3);
    let every = cadmpeg_ir::features::PlanarProfileRef::sketch_regions(
        sketch.id.clone(),
        (0..3)
            .map(|outer| SketchProfileRegion::loops(outer, Vec::new()).unwrap())
            .collect(),
    )
    .unwrap();
    assert!(crate::design::profile_select::selects_whole_sketch(
        &every, &sketch
    ));
}

#[test]
fn partial_or_holed_selection_is_not_the_whole_sketch() {
    let sketch = whole_sketch_test_sketch(3);
    let some = cadmpeg_ir::features::PlanarProfileRef::sketch_regions(
        sketch.id.clone(),
        vec![SketchProfileRegion::loops(0, Vec::new()).unwrap()],
    )
    .unwrap();
    assert!(!crate::design::profile_select::selects_whole_sketch(
        &some, &sketch
    ));
    let holed = cadmpeg_ir::features::PlanarProfileRef::sketch_regions(
        sketch.id.clone(),
        vec![SketchProfileRegion::loops(0, vec![1, 2]).unwrap()],
    )
    .unwrap();
    assert!(!crate::design::profile_select::selects_whole_sketch(
        &holed, &sketch
    ));
}
