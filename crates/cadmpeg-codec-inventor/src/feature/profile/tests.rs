use std::collections::HashMap;

use cadmpeg_ir::attributes::AttributeValue;
use cadmpeg_ir::features::{PlanarProfileRef, SketchProfileBoundaryUse, SketchProfileRegion};
use cadmpeg_ir::geometry::DirectedParameterRange;
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::scalar::{Angle, Length};
use cadmpeg_ir::sketches::{
    Sketch, SketchEntity, SketchEntityId, SketchEntityUse, SketchGeometry,
    SketchGeometryDefinition, SketchId, SketchPlacement, SketchProfiles,
};

use super::{
    chord_turn, edge_name, encloses, EdgeName, LoopCurve, ProfileBody, ProfileEdge, ProfileIndex,
    ProfileWire,
};
use crate::feature::{PmDcKernelEntityLink, PmDcKernelEntityLinkPayload, KERNEL_ENTITY_LINK_TYPE};
use crate::pmdc::PmDcReference;
use crate::record_identity::{Located, RecordTypeId};

const NAMING_TYPE: u32 = 38;
const SKETCH_ORDINAL: u32 = 2;
const BODY_ORDINAL: u32 = 30;

fn naming_values(naming_type: i64, associative_id: i64, topology: i64) -> Vec<AttributeValue> {
    let mut values = vec![
        AttributeValue::Reference("sat:brep:entity#-1".into()),
        AttributeValue::Integer(-1),
        AttributeValue::Reference("sat:brep:entity#-1".into()),
        AttributeValue::Reference("sat:brep:entity#-1".into()),
        AttributeValue::Reference("sat:brep:entity#9".into()),
    ];
    values.extend([naming_type, 0, associative_id, 0, topology].map(AttributeValue::Integer));
    values
}

#[test]
fn edge_naming_attribute_yields_its_naming_type_and_associative_id() {
    assert_eq!(
        edge_name(&naming_values(38, 14, 2)),
        Some(EdgeName {
            naming_type: 38,
            associative_id: 14,
        })
    );
    // A vertex name and a truncated attribute name no sketch curve.
    assert_eq!(edge_name(&naming_values(38, 14, 1)), None);
    assert_eq!(edge_name(&naming_values(38, 14, 2)[..9]), None);
}

fn sketch_id() -> SketchId {
    SketchId::mint("inventor:design:sketch#segment-2").expect("sketch id")
}

fn entity(ordinal: u32, definition: SketchGeometryDefinition) -> SketchEntity {
    SketchEntity::new(
        SketchEntityId::mint(format!("inventor:design:sketch-entity#segment-{ordinal}"))
            .expect("entity id"),
        sketch_id(),
        SketchGeometry::try_from(definition).expect("sketch geometry"),
    )
    .with_native_ref(Some(format!(
        "inventor:pmdc:sketch-entity#segment-{ordinal}"
    )))
}

/// A 20 mm square from the origin with a 3 mm circle at its center, on the
/// model XY plane. Square sides are entities 11 through 14; the circle is 15.
fn square_with_hole() -> (Sketch, Vec<SketchEntity>) {
    let corners = [(0.0, 0.0), (20.0, 0.0), (20.0, 20.0), (0.0, 20.0)];
    let mut entities = (0..4)
        .map(|side| {
            let (start, end) = (corners[side], corners[(side + 1) % 4]);
            entity(
                11 + u32::try_from(side).expect("side fits"),
                SketchGeometryDefinition::Line {
                    start: Point2::new(start.0, start.1),
                    end: Point2::new(end.0, end.1),
                },
            )
        })
        .collect::<Vec<_>>();
    entities.push(entity(
        15,
        SketchGeometryDefinition::Circle {
            center: Point2::new(10.0, 10.0),
            radius: Length::new(3.0).expect("radius"),
        },
    ));
    let uses = |range: std::ops::Range<usize>| {
        entities[range]
            .iter()
            .map(|entity| SketchEntityUse {
                entity: entity.id().clone(),
                reversed: false,
            })
            .collect::<Vec<_>>()
    };
    let sketch = Sketch {
        id: sketch_id(),
        name: None,
        configuration: None,
        visible: None,
        placement: SketchPlacement::try_resolved(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .expect("placement"),
        profiles: SketchProfiles::try_from(vec![uses(0..4), uses(4..5)]).expect("profiles"),
        native_ref: Some("inventor:pmdc:sketch#segment-2".into()),
    };
    (sketch, entities)
}

fn edge(associative_id: i64, start: (f64, f64), end: (f64, f64)) -> ProfileEdge {
    ProfileEdge {
        name: Some(EdgeName {
            naming_type: i64::from(NAMING_TYPE),
            associative_id,
        }),
        start: Point3::new(start.0, start.1, 0.0),
        end: Point3::new(end.0, end.1, 0.0),
        middle: None,
    }
}

/// An edge whose carrier passes through `middle` halfway along it.
fn edge_through(
    associative_id: i64,
    start: (f64, f64),
    middle: (f64, f64),
    end: (f64, f64),
) -> ProfileEdge {
    ProfileEdge {
        middle: Some(Point3::new(middle.0, middle.1, 0.0)),
        ..edge(associative_id, start, end)
    }
}

fn kernel_link(ordinal: u32, entity_id: u32) -> PmDcKernelEntityLink {
    Located::new(
        PmDcKernelEntityLinkPayload {
            save_version_major: 22,
            header_value: 0,
            header_id: 0,
            selection_source: 0,
            state: 4,
            body: PmDcReference::new(BODY_ORDINAL + 1, true).expect("body reference"),
            entity_id,
            entity_type: NAMING_TYPE,
        },
        RecordTypeId::from_bytes(KERNEL_ENTITY_LINK_TYPE),
        cadmpeg_ir::identity_key!("segment"),
        ordinal,
    )
}

/// Profile wires for the square (key 7) and the circle (key 8). The square
/// wire starts its sides at `square_start`.
fn profile_body(square_start: (f64, f64)) -> ProfileBody {
    ProfileBody {
        segment_token: cadmpeg_ir::identity_key!("segment"),
        record_ordinal: BODY_ORDINAL,
        wires: vec![
            ProfileWire {
                key: 7,
                edges: vec![
                    edge(1, square_start, (20.0, 0.0)),
                    edge(2, (20.0, 0.0), (20.0, 20.0)),
                    edge(3, (20.0, 20.0), (0.0, 20.0)),
                    edge(4, (0.0, 20.0), (0.0, 0.0)),
                ],
            },
            ProfileWire {
                key: 8,
                // The circle runs through two half edges.
                edges: vec![
                    edge(5, (13.0, 10.0), (7.0, 10.0)),
                    edge(5, (7.0, 10.0), (13.0, 10.0)),
                ],
            },
        ],
    }
}

fn index<'a>(
    bodies: &'a [ProfileBody],
    links: &'a [PmDcKernelEntityLink],
    entities: &'a [SketchEntity],
) -> ProfileIndex<'a> {
    let mut index = ProfileIndex::new(bodies, links, &[], &[], entities);
    index.curves = (1..=8)
        .map(|id| {
            (
                ("segment", SKETCH_ORDINAL, id),
                Some(format!("inventor:pmdc:sketch-entity#segment-{}", 10 + id)),
            )
        })
        .collect::<HashMap<_, _>>();
    index
}

#[test]
fn a_loop_inside_another_selected_loop_is_a_hole_of_its_region() {
    let (sketch, entities) = square_with_hole();
    let bodies = [profile_body((0.0, 0.0))];
    let links = [kernel_link(20, 7), kernel_link(21, 8)];
    let index = index(&bodies, &links, &entities);
    let profile = index
        .regions("segment", SKETCH_ORDINAL, &sketch, &[20, 21])
        .expect("resolved profile");
    let PlanarProfileRef::SketchRegions { regions, .. } = profile else {
        panic!("profile regions");
    };
    assert_eq!(
        regions.as_slice(),
        [SketchProfileRegion::loops(0, vec![1]).expect("region")]
    );
    // The circle alone selects the disc.
    let disc = index
        .regions("segment", SKETCH_ORDINAL, &sketch, &[21])
        .expect("resolved disc");
    let PlanarProfileRef::SketchRegions { regions, .. } = disc else {
        panic!("profile regions");
    };
    assert_eq!(
        regions.as_slice(),
        [SketchProfileRegion::loops(1, Vec::new()).expect("region")]
    );
}

#[test]
fn a_wire_that_does_not_close_leaves_the_selection_unresolved() {
    let (sketch, entities) = square_with_hole();
    let bodies = [profile_body((5.0, 0.0))];
    let links = [kernel_link(20, 7)];
    let index = index(&bodies, &links, &entities);
    assert!(index
        .regions("segment", SKETCH_ORDINAL, &sketch, &[20])
        .is_none());
}

/// The winding number of a closed loop about `point`, from a dense polyline
/// through its curves.
fn polyline_winding(curves: &[LoopCurve], point: Point2) -> f64 {
    let mut turns = 0.0;
    for curve in curves {
        let samples = match *curve {
            LoopCurve::Arc {
                center,
                radius,
                start_angle,
                sweep,
            } => (0..=4000)
                .map(|step| {
                    let angle = sweep.mul_add(f64::from(step) / 4000.0, start_angle);
                    Point2::new(
                        radius.mul_add(angle.cos(), center.u),
                        radius.mul_add(angle.sin(), center.v),
                    )
                })
                .collect::<Vec<_>>(),
            _ => {
                let (start, end) = curve.ends();
                vec![start, end]
            }
        };
        for pair in samples.windows(2) {
            turns += chord_turn(pair[0], pair[1], point);
        }
    }
    turns / std::f64::consts::TAU
}

#[test]
fn an_arc_turns_by_its_chord_and_the_segment_it_bounds() {
    use std::f64::consts::{FRAC_PI_2, FRAC_PI_4, PI};
    let arc = |center: (f64, f64), radius: f64, start_angle: f64, sweep: f64| LoopCurve::Arc {
        center: Point2::new(center.0, center.1),
        radius,
        start_angle,
        sweep,
    };
    let radius = 0.5_f64.sqrt();
    // From (1, 0) over the top to (0, 0), on the circle about (0.5, -0.5).
    let upper = arc((0.5, -0.5), radius, FRAC_PI_4, FRAC_PI_2);
    let loops = [
        // A lens: back along the circle about (0.5, 0.5).
        vec![upper, arc((0.5, 0.5), radius, -3.0 * FRAC_PI_4, FRAC_PI_2)],
        // A crescent: back the long way around that circle.
        vec![upper, arc((0.5, 0.5), radius, -3.0 * FRAC_PI_4, -1.5 * PI)],
    ];
    for curves in &loops {
        for i in 0..=40 {
            for j in 0..=40 {
                let point = Point2::new(
                    f64::from(i).mul_add(0.05, -0.5),
                    f64::from(j).mul_add(0.05, -1.0),
                );
                let near_curve = [(0.5, -0.5), (0.5, 0.5)].iter().any(|center| {
                    ((point.u - center.0).hypot(point.v - center.1) - radius).abs() < 1.0e-2
                });
                if near_curve {
                    continue;
                }
                assert_eq!(
                    encloses(curves, point),
                    Some(polyline_winding(curves, point).round() != 0.0),
                    "{point:?}"
                );
            }
        }
    }
    // A point inside the circle of an arc but on the far side of its chord.
    let crescent = [
        arc((0.5, 0.726), 0.882, -2.174, 1.206),
        arc((0.5, -0.417), 0.651, 0.695, -4.531),
    ];
    assert_eq!(encloses(&crescent, Point2::new(0.898, 0.053)), Some(false));
}

#[test]
fn a_ring_along_parts_of_curves_is_a_trimmed_region() {
    // A divider from (10, 0) to (10, 20) splits the square; the selection is
    // its left half.
    let (sketch, mut entities) = square_with_hole();
    entities.push(entity(
        16,
        SketchGeometryDefinition::Line {
            start: Point2::new(10.0, 0.0),
            end: Point2::new(10.0, 20.0),
        },
    ));
    let bodies = [ProfileBody {
        segment_token: cadmpeg_ir::identity_key!("segment"),
        record_ordinal: BODY_ORDINAL,
        wires: vec![ProfileWire {
            key: 9,
            edges: vec![
                edge(1, (0.0, 0.0), (10.0, 0.0)),
                edge(6, (10.0, 0.0), (10.0, 20.0)),
                edge(3, (10.0, 20.0), (0.0, 20.0)),
                edge(4, (0.0, 20.0), (0.0, 0.0)),
            ],
        }],
    }];
    let links = [kernel_link(20, 9)];
    let index = index(&bodies, &links, &entities);
    let profile = index
        .regions("segment", SKETCH_ORDINAL, &sketch, &[20])
        .expect("resolved profile");
    let PlanarProfileRef::SketchRegions { regions, .. } = profile else {
        panic!("profile regions");
    };
    let boundary_use = |ordinal: u32, range: [f64; 2]| SketchProfileBoundaryUse {
        entity: SketchEntityId::mint(format!("inventor:design:sketch-entity#segment-{ordinal}"))
            .expect("entity id"),
        parameter_range: DirectedParameterRange::new(range).expect("range"),
        reversed: false,
    };
    assert_eq!(
        regions.as_slice(),
        [SketchProfileRegion::trimmed(
            vec![
                boundary_use(11, [0.0, 0.5]),
                boundary_use(16, [0.0, 1.0]),
                boundary_use(13, [0.5, 1.0]),
                boundary_use(14, [0.0, 1.0]),
            ],
            Vec::new(),
        )
        .expect("region")]
    );
}

fn boundary_use(ordinal: u32, range: [f64; 2], reversed: bool) -> SketchProfileBoundaryUse {
    SketchProfileBoundaryUse {
        entity: SketchEntityId::mint(format!("inventor:design:sketch-entity#segment-{ordinal}"))
            .expect("entity id"),
        parameter_range: DirectedParameterRange::new(range).expect("range"),
        reversed,
    }
}

fn trimmed_boundary(
    sketch: &Sketch,
    entities: &[SketchEntity],
    edges: Vec<ProfileEdge>,
) -> Vec<SketchProfileBoundaryUse> {
    let bodies = [ProfileBody {
        segment_token: cadmpeg_ir::identity_key!("segment"),
        record_ordinal: BODY_ORDINAL,
        wires: vec![ProfileWire { key: 9, edges }],
    }];
    let links = [kernel_link(20, 9)];
    let index = index(&bodies, &links, entities);
    let profile = index
        .regions("segment", SKETCH_ORDINAL, sketch, &[20])
        .expect("resolved profile");
    let PlanarProfileRef::SketchRegions { regions, .. } = profile else {
        panic!("profile regions");
    };
    let [region] = regions.as_slice() else {
        panic!("one region");
    };
    let SketchProfileRegion::Trimmed {
        outer_boundary,
        hole_boundaries,
    } = region
    else {
        panic!("trimmed region");
    };
    assert!(hole_boundaries.is_empty());
    outer_boundary.to_vec()
}

#[test]
fn an_arc_piece_entered_at_its_start_angle_runs_forward() {
    use std::f64::consts::FRAC_PI_2;
    // The right half of a circle about (10, 10) from (10, 0) to (10, 20),
    // closed below the middle by a radius along v = 10 and the lower half of
    // the diameter. The wire enters the arc a hair below its start angle.
    let (sketch, mut entities) = square_with_hole();
    entities.extend([
        entity(
            16,
            SketchGeometryDefinition::Arc {
                center: Point2::new(10.0, 10.0),
                radius: Length::new(10.0).expect("radius"),
                start_angle: Angle::new(-FRAC_PI_2).expect("angle"),
                end_angle: Angle::new(FRAC_PI_2).expect("angle"),
            },
        ),
        entity(
            17,
            SketchGeometryDefinition::Line {
                start: Point2::new(10.0, 10.0),
                end: Point2::new(20.0, 10.0),
            },
        ),
        entity(
            18,
            SketchGeometryDefinition::Line {
                start: Point2::new(10.0, 0.0),
                end: Point2::new(10.0, 20.0),
            },
        ),
    ]);
    let boundary = trimmed_boundary(
        &sketch,
        &entities,
        vec![
            edge(6, (10.0 - 1.0e-12, 0.0), (20.0, 10.0)),
            edge(7, (20.0, 10.0), (10.0, 10.0)),
            edge(8, (10.0, 10.0), (10.0, 0.0)),
        ],
    );
    assert_eq!(
        boundary,
        [
            boundary_use(16, [-FRAC_PI_2, 0.0], false),
            boundary_use(17, [0.0, 1.0], true),
            boundary_use(18, [0.0, 0.5], true),
        ]
    );
}

#[test]
fn a_ring_along_part_of_a_circle_runs_through_the_edge_middle() {
    use std::f64::consts::{FRAC_PI_2, PI};
    // The right half of the 3 mm circle, closed by its vertical diameter.
    let (sketch, mut entities) = square_with_hole();
    entities.push(entity(
        16,
        SketchGeometryDefinition::Line {
            start: Point2::new(10.0, 7.0),
            end: Point2::new(10.0, 13.0),
        },
    ));
    let boundary = trimmed_boundary(
        &sketch,
        &entities,
        vec![
            edge_through(5, (10.0, 7.0), (13.0, 10.0), (10.0, 13.0)),
            edge(6, (10.0, 13.0), (10.0, 7.0)),
        ],
    );
    assert_eq!(boundary.len(), 2);
    let range = boundary[0].parameter_range.endpoints();
    assert!((range[0] - 3.0 * FRAC_PI_2).abs() < 1.0e-12, "{range:?}");
    assert!((range[1] - range[0] - PI).abs() < 1.0e-12, "{range:?}");
    assert!(!boundary[0].reversed);
    assert_eq!(boundary[1], boundary_use(16, [0.0, 1.0], true));
}
