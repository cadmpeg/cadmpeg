// SPDX-License-Identifier: Apache-2.0

use std::f64::consts::{FRAC_PI_2, PI, TAU};

use cadmpeg_ir::math::Point2;
use cadmpeg_ir::scalar::{Angle, Length};
use cadmpeg_ir::sketches::{
    SketchEntity, SketchEntityId, SketchGeometry, SketchGeometryDefinition, SketchId,
};

use super::super::closed_sketch_profiles;
use crate::loss::F3dLossCode;

fn line_entity(sketch: &SketchId, id: &str, start: (f64, f64), end: (f64, f64)) -> SketchEntity {
    SketchEntity::new(
        SketchEntityId::mint(id).expect("fixture edge ID"),
        sketch.clone(),
        SketchGeometry::try_from(SketchGeometryDefinition::Line {
            start: Point2::new(start.0, start.1),
            end: Point2::new(end.0, end.1),
        })
        .expect("valid line geometry"),
    )
}

fn arc_entity(
    sketch: &SketchId,
    id: &str,
    center: (f64, f64),
    radius: f64,
    start_angle: f64,
    end_angle: f64,
) -> SketchEntity {
    SketchEntity::new(
        SketchEntityId::mint(id).expect("fixture edge ID"),
        sketch.clone(),
        SketchGeometry::try_from(SketchGeometryDefinition::Arc {
            center: Point2::new(center.0, center.1),
            radius: Length::new(radius).expect("valid radius"),
            start_angle: Angle::new(start_angle).expect("valid start angle"),
            end_angle: Angle::new(end_angle).expect("valid end angle"),
        })
        .expect("valid arc geometry"),
    )
}

#[test]
fn branched_arc_loop() {
    let sketch = SketchId::mint("f3d:model:sketch#branched-arc-loop").expect("valid sketch ID");

    // Two quarter arcs meeting at (0.0, 10.0) with a shared divider line to (0.0, 0.0)
    // and two bottom lines to complete two adjacent closed cells.
    let entities = vec![
        arc_entity(
            &sketch,
            "synthetic:test:id#arc-right",
            (0.0, 0.0),
            10.0,
            0.0,
            FRAC_PI_2,
        ),
        arc_entity(
            &sketch,
            "synthetic:test:id#arc-left",
            (0.0, 0.0),
            10.0,
            FRAC_PI_2,
            PI,
        ),
        line_entity(
            &sketch,
            "synthetic:test:id#divider",
            (0.0, 10.0),
            (0.0, 0.0),
        ),
        line_entity(
            &sketch,
            "synthetic:test:id#bottom-right",
            (0.0, 0.0),
            (10.0, 0.0),
        ),
        line_entity(
            &sketch,
            "synthetic:test:id#bottom-left",
            (-10.0, 0.0),
            (0.0, 0.0),
        ),
    ];

    let profiles = crate::test_support::with_decode_context(|decode_ctx| {
        closed_sketch_profiles(decode_ctx, &sketch, &entities, 1.0e-6)
    })
    .expect("closed sketch profiles");

    assert_eq!(profiles.len(), 2);
    assert!(profiles.iter().all(|profile| profile.len() == 3));
    assert!(profiles.iter().all(|profile| profile
        .iter()
        .any(|entity_use| entity_use.entity.as_str() == "synthetic:test:id#divider")));
    assert!(profiles.losses.is_empty());
}

#[test]
fn duplicate_shared_wall_yielding_two_profiles() {
    let sketch = SketchId::mint("f3d:model:sketch#duplicate-wall").expect("valid sketch ID");

    // Two adjacent squares sharing an interior wall at u = 1.0.
    // The shared wall is included twice as two distinct entity IDs with identical geometry.
    let entities = vec![
        line_entity(
            &sketch,
            "synthetic:test:id#left-bottom",
            (0.0, 0.0),
            (1.0, 0.0),
        ),
        line_entity(
            &sketch,
            "synthetic:test:id#shared-wall-1",
            (1.0, 0.0),
            (1.0, 1.0),
        ),
        line_entity(
            &sketch,
            "synthetic:test:id#left-top",
            (1.0, 1.0),
            (0.0, 1.0),
        ),
        line_entity(
            &sketch,
            "synthetic:test:id#left-outer",
            (0.0, 1.0),
            (0.0, 0.0),
        ),
        line_entity(
            &sketch,
            "synthetic:test:id#shared-wall-2",
            (1.0, 0.0),
            (1.0, 1.0),
        ),
        line_entity(
            &sketch,
            "synthetic:test:id#right-bottom",
            (1.0, 0.0),
            (2.0, 0.0),
        ),
        line_entity(
            &sketch,
            "synthetic:test:id#right-outer",
            (2.0, 0.0),
            (2.0, 1.0),
        ),
        line_entity(
            &sketch,
            "synthetic:test:id#right-top",
            (2.0, 1.0),
            (1.0, 1.0),
        ),
    ];

    let profiles = crate::test_support::with_decode_context(|decode_ctx| {
        closed_sketch_profiles(decode_ctx, &sketch, &entities, 1.0e-6)
    })
    .expect("closed sketch profiles");

    assert_eq!(profiles.len(), 2);
    assert_eq!(profiles[0].len(), 4);
    assert_eq!(profiles[1].len(), 4);

    let has_wall_1 = |profile: &[cadmpeg_ir::sketches::SketchEntityUse]| {
        profile
            .iter()
            .any(|u| u.entity.as_str() == "synthetic:test:id#shared-wall-1")
    };
    let has_wall_2 = |profile: &[cadmpeg_ir::sketches::SketchEntityUse]| {
        profile
            .iter()
            .any(|u| u.entity.as_str() == "synthetic:test:id#shared-wall-2")
    };

    assert!(
        (has_wall_1(&profiles[0]) && has_wall_2(&profiles[1]))
            || (has_wall_2(&profiles[0]) && has_wall_1(&profiles[1]))
    );
    assert!(profiles.losses.is_empty());
}

#[test]
fn dangling_spur_pruned_to_one_loop() {
    let sketch = SketchId::mint("f3d:model:sketch#dangling-spur").expect("valid sketch ID");

    // A closed triangle plus a dangling spur extending from vertex (1.0, 0.0) to (2.0, 0.0).
    let entities = vec![
        line_entity(
            &sketch,
            "synthetic:test:id#tri-bottom",
            (0.0, 0.0),
            (1.0, 0.0),
        ),
        line_entity(
            &sketch,
            "synthetic:test:id#tri-hypot",
            (1.0, 0.0),
            (0.0, 1.0),
        ),
        line_entity(
            &sketch,
            "synthetic:test:id#tri-left",
            (0.0, 1.0),
            (0.0, 0.0),
        ),
        line_entity(
            &sketch,
            "synthetic:test:id#dangling-spur",
            (1.0, 0.0),
            (2.0, 0.0),
        ),
    ];

    let profiles = crate::test_support::with_decode_context(|decode_ctx| {
        closed_sketch_profiles(decode_ctx, &sketch, &entities, 1.0e-6)
    })
    .expect("closed sketch profiles");

    assert_eq!(profiles.len(), 1);
    assert_eq!(profiles[0].len(), 3);
    assert!(!profiles[0]
        .iter()
        .any(|u| u.entity.as_str() == "synthetic:test:id#dangling-spur"));

    assert_eq!(profiles.losses.len(), 1);
    assert_eq!(
        profiles.losses[0].code,
        F3dLossCode::SketchCurveUnprofiled.kind()
    );
    assert!(profiles.losses[0]
        .message
        .contains("synthetic:test:id#dangling-spur"));
}

#[test]
fn bridge_input_producing_only_closed_loops() {
    let sketch = SketchId::mint("f3d:model:sketch#bridge-input").expect("valid sketch ID");

    // Two independent closed triangles joined by an open bridge segment between them.
    let entities = vec![
        // Triangle 1
        line_entity(
            &sketch,
            "synthetic:test:id#t1-bottom",
            (0.0, 0.0),
            (1.0, 0.0),
        ),
        line_entity(
            &sketch,
            "synthetic:test:id#t1-hypot",
            (1.0, 0.0),
            (0.0, 1.0),
        ),
        line_entity(&sketch, "synthetic:test:id#t1-left", (0.0, 1.0), (0.0, 0.0)),
        // Bridge
        line_entity(
            &sketch,
            "synthetic:test:id#bridge-edge",
            (1.0, 0.0),
            (3.0, 0.0),
        ),
        // Triangle 2
        line_entity(
            &sketch,
            "synthetic:test:id#t2-bottom",
            (3.0, 0.0),
            (4.0, 0.0),
        ),
        line_entity(
            &sketch,
            "synthetic:test:id#t2-hypot",
            (4.0, 0.0),
            (3.0, 1.0),
        ),
        line_entity(&sketch, "synthetic:test:id#t2-left", (3.0, 1.0), (3.0, 0.0)),
    ];

    let profiles = crate::test_support::with_decode_context(|decode_ctx| {
        closed_sketch_profiles(decode_ctx, &sketch, &entities, 1.0e-6)
    })
    .expect("closed sketch profiles");

    assert_eq!(profiles.len(), 2);
    assert_eq!(profiles[0].len(), 3);
    assert_eq!(profiles[1].len(), 3);
    assert!(!profiles.iter().any(|p| p
        .iter()
        .any(|u| u.entity.as_str() == "synthetic:test:id#bridge-edge")));

    assert_eq!(profiles.losses.len(), 1);
    assert_eq!(
        profiles.losses[0].code,
        F3dLossCode::SketchCurveUnprofiled.kind()
    );
    assert!(profiles.losses[0]
        .message
        .contains("synthetic:test:id#bridge-edge"));
}

#[test]
fn complementary_half_arcs_not_deduplicated() {
    let sketch = SketchId::mint("f3d:model:sketch#complementary-arcs").expect("valid sketch ID");

    // Two 180-degree half-arcs of radius 2.0 around (0.0, 0.0) sharing endpoints at (-2.0, 0.0)
    // and (2.0, 0.0), divided by a diameter line.
    let entities = vec![
        arc_entity(
            &sketch,
            "synthetic:test:id#upper-arc",
            (0.0, 0.0),
            2.0,
            0.0,
            PI,
        ),
        arc_entity(
            &sketch,
            "synthetic:test:id#lower-arc",
            (0.0, 0.0),
            2.0,
            PI,
            TAU,
        ),
        line_entity(
            &sketch,
            "synthetic:test:id#diameter",
            (-2.0, 0.0),
            (2.0, 0.0),
        ),
    ];

    let profiles = crate::test_support::with_decode_context(|decode_ctx| {
        closed_sketch_profiles(decode_ctx, &sketch, &entities, 1.0e-6)
    })
    .expect("closed sketch profiles");

    assert_eq!(profiles.len(), 2);
    assert_eq!(profiles[0].len(), 2);
    assert_eq!(profiles[1].len(), 2);

    let has_upper = |profile: &[cadmpeg_ir::sketches::SketchEntityUse]| {
        profile
            .iter()
            .any(|u| u.entity.as_str() == "synthetic:test:id#upper-arc")
    };
    let has_lower = |profile: &[cadmpeg_ir::sketches::SketchEntityUse]| {
        profile
            .iter()
            .any(|u| u.entity.as_str() == "synthetic:test:id#lower-arc")
    };

    assert!(
        (has_upper(&profiles[0]) && has_lower(&profiles[1]))
            || (has_lower(&profiles[0]) && has_upper(&profiles[1]))
    );
    assert!(profiles.losses.is_empty());
}
