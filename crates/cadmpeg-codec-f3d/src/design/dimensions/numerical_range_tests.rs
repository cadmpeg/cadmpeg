// SPDX-License-Identifier: Apache-2.0

use super::*;

const SHORT_CHORD_LENGTH: f64 = 1e-6;
use cadmpeg_ir::features::ParameterId;
use cadmpeg_ir::sketches::{SketchConstraintDefinitionInput, SketchLocus};
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
fn numerical_0922_extension_requires_incidence() {
    let line = entity(
        "line",
        SketchGeometryDefinition::Line {
            start: Point2::new(0., 0.),
            end: Point2::new(SHORT_CHORD_LENGTH, 0.),
        },
    );
    let endpoint = entity(
        "endpoint",
        SketchGeometryDefinition::Point {
            position: Point2::new(SHORT_CHORD_LENGTH, 0.),
        },
    );
    let detached = entity(
        "detached",
        SketchGeometryDefinition::Point {
            position: Point2::new(2e-6, 1e-4),
        },
    );
    let candidate = SketchConstraintDefinitionInput::HorizontalDistance {
        first: SketchLocus::Entity(endpoint.id().clone()),
        second: SketchLocus::Entity(detached.id().clone()),
        parameter: ParameterId::mint("f3d:test:parameter#1").unwrap(),
    };
    let result = recipe_extension_point_dimension(
        &[candidate],
        &[line, endpoint, detached],
        &SketchId::mint("f3d:test:sketch#1").unwrap(),
    );
    println!(
        "Fusion short-line detached point 100 chord lengths off carrier selected={}",
        result.is_some()
    );
    assert!(result.is_none());
}
#[test]
fn numerical_0922_long_lines_keep_perpendicular_relation() {
    for length in [1., 1e200] {
        let a = entity(
            "first",
            SketchGeometryDefinition::Line {
                start: Point2::new(0., 0.),
                end: Point2::new(length, 0.),
            },
        );
        let b = entity(
            "second",
            SketchGeometryDefinition::Line {
                start: Point2::new(0., 0.),
                end: Point2::new(0., length),
            },
        );
        let r = exact_counted_dimension_relation(&[&a, &b]);
        println!("Fusion perpendicular lines length{length:e}: {r:?}");
        assert!(matches!(
            r,
            Some(SketchConstraintDefinitionInput::Perpendicular { .. })
        ));
    }
}
