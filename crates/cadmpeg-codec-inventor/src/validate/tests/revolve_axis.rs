// SPDX-License-Identifier: Apache-2.0
//! Validation of a neutral revolve profile against its axis.

use std::collections::HashMap;

use cadmpeg_ir::features::{FeatureId, RevolveConstruction};
use cadmpeg_ir::sketches::{Sketch, SketchEntity};
use serde_json::json;

use crate::validate::revolve_axis_finding;

/// A square loop in the plane whose u axis is -X and whose v axis is +Z, with u
/// in `[left, 4]` and v in `[0, 5]`.
fn square(left: f64) -> (Sketch, Vec<SketchEntity>) {
    let corners = [[left, 0.0], [4.0, 0.0], [4.0, 5.0], [left, 5.0]];
    let entities = (0..4)
        .map(|index| {
            let (start, end) = (corners[index], corners[(index + 1) % 4]);
            serde_json::from_value(json!({
                "id": format!("inventor:test:entity#{index}"),
                "sketch": "inventor:test:sketch#1",
                "construction": false,
                "geometry": {
                    "kind": "line",
                    "start": {"u": start[0], "v": start[1]},
                    "end": {"u": end[0], "v": end[1]},
                },
            }))
            .expect("line entity")
        })
        .collect();
    let chain = (0..4)
        .map(|index| json!({"entity": format!("inventor:test:entity#{index}"), "reversed": false}))
        .collect::<Vec<_>>();
    let sketch = serde_json::from_value(json!({
        "id": "inventor:test:sketch#1",
        "placement": {"kind": "resolved", "frame": {
            "origin": {"x": 0.0, "y": 0.0, "z": 0.0},
            "normal": {"x": 0.0, "y": 1.0, "z": 0.0},
            "u_axis": {"x": -1.0, "y": 0.0, "z": 0.0},
        }},
        "profiles": [chain],
    }))
    .expect("sketch");
    (sketch, entities)
}

/// A full revolution of the square about the sketch v axis (world Z).
fn construction() -> RevolveConstruction {
    RevolveConstruction::Resolved {
        profile: serde_json::from_value(json!({
            "kind": "sketch_regions",
            "value": {
                "sketch": "inventor:test:sketch#1",
                "regions": [{"region": "loops", "outer": 0}],
            },
        }))
        .expect("profile"),
        axis: serde_json::from_value(json!({
            "origin": {"x": 0.0, "y": 0.0, "z": 0.0},
            "direction": {"x": 0.0, "y": 0.0, "z": 1.0},
        }))
        .expect("axis"),
        extent: cadmpeg_ir::features::RevolveExtent::OneSided {
            termination: cadmpeg_ir::features::AngularTermination::Angle {
                angle: cadmpeg_ir::Dimension::Literal(
                    cadmpeg_ir::scalar::PositiveAngle::new(std::f64::consts::TAU)
                        .expect("full turn"),
                ),
            },
        },
        solid: Some(true),
        face_maker: None,
        fuse_order: None,
        allow_multi_profile_faces: None,
    }
}

fn finding_for(left: f64) -> Option<cadmpeg_ir::report::check::Finding> {
    let (sketch, entities) = square(left);
    let sketches = HashMap::from([(sketch.id.as_str(), &sketch)]);
    let entities = entities
        .iter()
        .map(|entity| (entity.id().as_str(), entity))
        .collect::<HashMap<_, _>>();
    let feature = FeatureId::mint("inventor:design:feature#revolve").expect("feature id");
    revolve_axis_finding(&feature, &construction(), &sketches, &entities)
}

#[test]
fn validator_reports_a_neutral_revolve_whose_profile_crosses_its_axis() {
    let finding = finding_for(-1.0).expect("crossing finding");
    assert_eq!(
        finding.message,
        "Inventor neutral revolve profile has points on both sides of its axis"
    );
    assert_eq!(
        finding.entity.as_deref(),
        Some("inventor:design:feature#revolve")
    );
}

#[test]
fn validator_accepts_a_neutral_revolve_with_its_profile_on_one_side() {
    // An edge on the axis belongs to neither side.
    assert!(finding_for(0.0).is_none());
    assert!(finding_for(1.0).is_none());
}
