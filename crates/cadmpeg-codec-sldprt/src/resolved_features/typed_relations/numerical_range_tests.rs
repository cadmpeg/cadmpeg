// SPDX-License-Identifier: Apache-2.0

use super::*;
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
fn numerical_0922_finite_line_midpoint() {
    let e = entity(
        "line",
        SketchGeometryDefinition::Line {
            start: Point2::new(1e308, 0.),
            end: Point2::new(1e308, 2.),
        },
    );
    let r = sketch_entity_midpoint(&e).unwrap();
    println!("SW finite midpoint (1e308,1) returned as {r:?}");
    assert_eq!(r, Point2::new(1e308, 1.));
}
