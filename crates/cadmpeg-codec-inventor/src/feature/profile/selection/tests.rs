// SPDX-License-Identifier: Apache-2.0

use super::*;

fn edge(start: f64, end: f64) -> ProfileEdge {
    ProfileEdge {
        name: None,
        start: Point3::new(start, 0.0, 0.0),
        end: Point3::new(end, 0.0, 0.0),
        middle: None,
        spline: None,
    }
}

#[test]
fn complete_open_path_retains_its_endpoints_with_reversed_edge_carriers() {
    let endpoints = path_endpoints(&[edge(1.0, 0.0), edge(1.0, 2.0), edge(3.0, 2.0)])
        .expect("connected open path");
    assert_eq!(
        endpoints,
        [Point3::new(0.0, 0.0, 0.0), Point3::new(3.0, 0.0, 0.0)]
    );
    assert!(path_endpoints(&[]).is_none());
    assert!(path_endpoints(&[edge(0.0, 1.0), edge(2.0, 3.0)]).is_none());
    assert!(path_endpoints(&[edge(0.0, 1.0), edge(1.0, 0.0)]).is_none());
}

#[test]
fn whole_sketch_requires_one_complete_closed_profile_and_resolved_plane() {
    use cadmpeg_ir::{
        features::SketchProfileRegion,
        math::{Point2, Vector3},
        sketches::{
            SketchEntity, SketchEntityId, SketchEntityUse, SketchGeometry, SketchId,
            SketchPlacement, SketchProfiles,
        },
    };
    let id = SketchId::mint("test:design:sketch#wall").expect("id");
    let corners = [
        Point2::new(0.0, 0.0),
        Point2::new(6.0, 0.0),
        Point2::new(6.0, 4.0),
        Point2::new(0.0, 4.0),
    ];
    let entities = (0..4)
        .map(|side| {
            SketchEntity::new(
                SketchEntityId::mint(format!("test:design:sketch-entity#{side}")).expect("id"),
                id.clone(),
                SketchGeometry::try_from(SketchGeometryDefinition::Line {
                    start: corners[side],
                    end: corners[(side + 1) % 4],
                })
                .expect("line"),
            )
        })
        .collect::<Vec<_>>();
    let mut sketch = Sketch {
        id: id.clone(),
        name: None,
        configuration: None,
        visible: None,
        placement: SketchPlacement::try_resolved(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .expect("plane"),
        profiles: SketchProfiles::try_from(vec![entities
            .iter()
            .map(|entity| SketchEntityUse {
                entity: entity.id().clone(),
                reversed: false,
            })
            .collect()])
        .expect("profile"),
        native_ref: None,
    };
    let index = ProfileIndex::new(&[], &[], &[], &[], &entities);
    assert_eq!(
        index.whole_sketch("test", 0, &sketch),
        Some(
            PlanarProfileRef::sketch_regions(
                id,
                vec![SketchProfileRegion::loops(0, Vec::new()).expect("region")]
            )
            .expect("profile")
        )
    );
    let complete = sketch.clone();
    sketch
        .profiles
        .edit(|profiles| {
            profiles[0].pop();
        })
        .expect("profile");
    assert!(index.whole_sketch("test", 0, &sketch).is_none());
    let mut open = entities.clone();
    open[3].geometry = SketchGeometry::try_from(SketchGeometryDefinition::Line {
        start: corners[3],
        end: Point2::new(1.0, 0.0),
    })
    .expect("line");
    assert!(ProfileIndex::new(&[], &[], &[], &[], &open)
        .whole_sketch("test", 0, &complete)
        .is_none());
}
