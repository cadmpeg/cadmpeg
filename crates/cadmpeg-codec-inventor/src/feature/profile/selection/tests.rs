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

mod whole_curve {
    use super::*;
    use cadmpeg_ir::{
        math::{Point2, Vector3},
        sketches::{
            SketchEntity, SketchEntityId, SketchGeometry, SketchId, SketchPlacement, SketchProfiles,
        },
    };

    fn sketch(id: &SketchId) -> Sketch {
        Sketch {
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
            profiles: SketchProfiles::default(),
            native_ref: None,
        }
    }

    fn line(sketch: &SketchId, name: &str, start: (f64, f64), end: (f64, f64)) -> SketchEntity {
        SketchEntity::new(
            SketchEntityId::mint(format!("test:design:sketch-entity#{name}")).expect("id"),
            sketch.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Line {
                start: Point2::new(start.0, start.1),
                end: Point2::new(end.0, end.1),
            })
            .expect("line"),
        )
    }

    fn run(entities: &[SketchEntity], edge: &ProfileEdge) -> Option<SketchEntityId> {
        let id = SketchId::mint("test:design:sketch#path").expect("id");
        let sketch = sketch(&id);
        let frame = SketchFrame::new(&sketch).expect("frame");
        ProfileIndex::new(&[], &[], &[], &[], entities).whole_curve(&frame, &sketch, edge)
    }

    fn path_edge(start: (f64, f64, f64), end: (f64, f64, f64)) -> ProfileEdge {
        ProfileEdge {
            name: None,
            start: Point3::new(start.0, start.1, start.2),
            end: Point3::new(end.0, end.1, end.2),
            middle: None,
            spline: None,
        }
    }

    #[test]
    fn edge_running_along_exactly_one_whole_curve_selects_it() {
        let id = SketchId::mint("test:design:sketch#path").expect("id");
        let entities = [
            line(&id, "first", (0.0, 0.0), (30.0, 0.0)),
            line(&id, "second", (30.0, 0.0), (30.0, 20.0)),
        ];
        let selected = run(&entities, &path_edge((30.0, 20.0, 0.0), (30.0, 0.0, 0.0)));
        assert_eq!(selected.as_ref(), Some(entities[1].id()));
    }

    #[test]
    fn partial_ambiguous_construction_and_off_plane_edges_select_nothing() {
        let id = SketchId::mint("test:design:sketch#path").expect("id");
        let long = line(&id, "long", (0.0, 0.0), (30.0, 0.0));
        let edge = path_edge((0.0, 0.0, 0.0), (30.0, 0.0, 0.0));
        assert!(run(std::slice::from_ref(&long), &edge).is_some());
        // The edge covers only part of the curve.
        assert!(run(
            std::slice::from_ref(&long),
            &path_edge((0.0, 0.0, 0.0), (20.0, 0.0, 0.0))
        )
        .is_none());
        // Two curves with the same extent leave the selection ambiguous.
        let twin = line(&id, "twin", (30.0, 0.0), (0.0, 0.0));
        assert!(run(&[long.clone(), twin], &edge).is_none());
        // A construction curve is not a path member.
        assert!(run(&[long.clone().with_construction(true)], &edge).is_none());
        // An edge off the sketch plane has no sketch coordinates.
        assert!(run(
            std::slice::from_ref(&long),
            &path_edge((0.0, 0.0, 5.0), (30.0, 0.0, 5.0))
        )
        .is_none());
        // A curve of another sketch is ignored.
        let other = SketchId::mint("test:design:sketch#other").expect("id");
        assert!(run(&[line(&other, "far", (0.0, 0.0), (30.0, 0.0))], &edge).is_none());
    }
}
