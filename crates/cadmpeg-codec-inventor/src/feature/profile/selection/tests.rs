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
        ProfileIndex::new(&[], &[], &[], &[], entities)
            .whole_piece(&frame, &sketch, edge)
            .map(|piece| piece.entity.id().clone())
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

mod selected_path {
    use super::*;
    use crate::feature::profile::{EdgeName, ProfileBody, ProfileWire};
    use crate::feature::{
        PmDcKernelEntityLink, PmDcKernelEntityLinkPayload, KERNEL_ENTITY_LINK_TYPE,
    };
    use crate::pmdc::PmDcReference;
    use crate::record_identity::{Located, RecordTypeId};
    use cadmpeg_ir::{
        math::{Point2, Vector3},
        sketches::{
            SketchEntity, SketchEntityId, SketchGeometry, SketchId, SketchPlacement, SketchProfiles,
        },
    };

    const NAMING_TYPE: u32 = 38;
    const BODY_ORDINAL: u32 = 30;
    const LINK_ORDINAL: u32 = 31;
    const WIRE_KEY: u32 = 7;
    const SKETCH_ORDINAL: u32 = 2;
    const TOKEN: &str = "segment";

    fn link() -> PmDcKernelEntityLink {
        Located::new(
            PmDcKernelEntityLinkPayload {
                save_version_major: 22,
                header_value: 0,
                header_id: 0,
                selection_source: 0,
                state: 4,
                body: PmDcReference::new(BODY_ORDINAL + 1, true).expect("body reference"),
                entity_id: WIRE_KEY,
                entity_type: NAMING_TYPE,
            },
            RecordTypeId::from_bytes(KERNEL_ENTITY_LINK_TYPE),
            cadmpeg_ir::identity_key!("segment"),
            LINK_ORDINAL,
        )
    }

    fn line(sketch: &SketchId, name: &str) -> SketchEntity {
        SketchEntity::new(
            SketchEntityId::mint(format!("test:design:sketch-entity#{name}")).expect("id"),
            sketch.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Line {
                start: Point2::new(0.0, 0.0),
                end: Point2::new(30.0, 0.0),
            })
            .expect("line"),
        )
    }

    /// Resolves a one-edge path wire against the line `named` and, when
    /// `twin` holds, a coincident line `unnamed`. The edge name joins to
    /// `named` when `join_names` holds.
    fn resolve(join_names: bool, twin: bool) -> Option<SelectedPath> {
        let id = SketchId::mint("test:design:sketch#path").expect("id");
        let sketch = Sketch {
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
        };
        let mut entities = vec![line(&id, "named")];
        if twin {
            entities.push(line(&id, "unnamed"));
        }
        let body = ProfileBody {
            segment_token: cadmpeg_ir::identity_key!("segment"),
            record_ordinal: BODY_ORDINAL,
            wires: vec![ProfileWire {
                key: u64::from(WIRE_KEY),
                edges: vec![ProfileEdge {
                    name: Some(EdgeName {
                        naming_type: i64::from(NAMING_TYPE),
                        associative_id: 5,
                    }),
                    start: Point3::new(0.0, 0.0, 0.0),
                    end: Point3::new(30.0, 0.0, 0.0),
                    middle: None,
                    spline: None,
                }],
            }],
        };
        let links = [link()];
        let bodies = [body];
        let mut index = ProfileIndex::new(&bodies, &links, &[], &[], &entities);
        if join_names {
            index.curves.insert(
                (TOKEN, SKETCH_ORDINAL, 5),
                Some(entities[0].id().as_str().to_owned()),
            );
        }
        index.selected_path(
            TOKEN,
            LINK_ORDINAL,
            "native".to_owned(),
            &[(SKETCH_ORDINAL, &sketch)],
        )
    }

    fn selected(path: &SelectedPath) -> Option<&[SketchEntityId]> {
        match &path.reference {
            PathRef::SketchCurves { curves, .. } => Some(curves.as_slice()),
            _ => None,
        }
    }

    #[test]
    fn a_named_wire_joins_by_name_beside_a_coincident_unnamed_curve() {
        let path = resolve(true, true).expect("path");
        let named = SketchEntityId::mint("test:design:sketch-entity#named").expect("id");
        assert_eq!(selected(&path), Some([named].as_slice()));
        assert_eq!(path.names, [5]);
        assert_eq!(
            path.curves,
            [PathCurve::Line {
                start: Point3::new(0.0, 0.0, 0.0),
                end: Point3::new(30.0, 0.0, 0.0),
            }]
        );
    }

    #[test]
    fn a_wire_without_curve_names_joins_by_geometry() {
        let path = resolve(false, false).expect("path");
        let named = SketchEntityId::mint("test:design:sketch-entity#named").expect("id");
        assert_eq!(selected(&path), Some([named].as_slice()));
        // Two curves with the extent of the edge leave the join without a
        // unique curve.
        let ambiguous = resolve(false, true).expect("path");
        assert!(selected(&ambiguous).is_none());
        assert!(ambiguous.curves.is_empty());
    }
}
