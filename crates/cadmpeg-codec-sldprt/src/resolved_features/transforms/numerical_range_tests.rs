// SPDX-License-Identifier: Apache-2.0

use super::*;
use cadmpeg_ir::scalar::{Angle, Length};
use cadmpeg_ir::sketches::{
    SketchEntity, SketchEntityId, SketchGeometry, SketchGeometryDefinition, SketchId,
};

fn entity(name: &str, definition: SketchGeometryDefinition) -> SketchEntity {
    SketchEntity::new(
        SketchEntityId::mint(format!("f3d:test:entity#{name}")).unwrap(),
        SketchId::mint("f3d:test:sketch#1").unwrap(),
        SketchGeometry::try_from(definition).unwrap(),
    )
}
#[test]
fn numerical_0922_hyperbola_loci_remain_finite() {
    let e = entity(
        "hyperbola",
        SketchGeometryDefinition::Hyperbola {
            center: Point2::new(0., 0.),
            major_angle: Angle::new(0.).unwrap(),
            major_radius: Length::new(1e-100).unwrap(),
            minor_radius: Length::new(1e-100).unwrap(),
            bounds: Some([710., 711.]),
        },
    );
    let r = sketch_entity_loci(&e);
    println!("SW tiny-radius hyperbola endpoints t710,711: {r:?}");
    assert_eq!(r.len(), 3);
    assert!(r.iter().all(|(p, _)| p.is_finite()));
    let reference = cadmpeg_ir::math::scaled_sinh_cosh(1e-100, 711.).unwrap();
    assert_eq!(r[2].0, Point2::new(reference.1, reference.0));
    assert!(super::super::typed_relations::sketch_entity_contains_point(
        &e, r[2].0
    ));
}
