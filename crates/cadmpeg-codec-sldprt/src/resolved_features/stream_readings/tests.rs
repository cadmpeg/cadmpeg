// SPDX-License-Identifier: Apache-2.0

use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::scalar::{Angle, Length};
use cadmpeg_ir::sketches::{
    EllipseRadii, Sketch, SketchEntity, SketchEntityId, SketchEntityUse, SketchGeometry,
    SketchGeometryDefinition, SketchId, SketchPlacement, SketchProfiles,
};

use super::{marker_reading, EntityIndex, MarkerReading, SolidEdges, UnboundReason};

fn sketch(id: &str, loops: &[&[&SketchEntity]]) -> Sketch {
    Sketch {
        id: SketchId::mint(format!("synthetic:test:id#{id}")).unwrap(),
        name: None,
        configuration: None,
        visible: None,
        placement: SketchPlacement::try_resolved(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .unwrap(),
        profiles: SketchProfiles::try_from(
            loops
                .iter()
                .map(|entities| {
                    entities
                        .iter()
                        .map(|entity| SketchEntityUse {
                            entity: entity.id().clone(),
                            reversed: false,
                        })
                        .collect::<Vec<_>>()
                })
                .collect::<Vec<_>>(),
        )
        .unwrap(),
        native_ref: None,
    }
}

fn entity(id: &str, geometry: SketchGeometryDefinition) -> SketchEntity {
    SketchEntity::new(
        SketchEntityId::mint(format!("synthetic:test:id#{id}")).unwrap(),
        SketchId::mint("synthetic:test:id#owner").unwrap(),
        SketchGeometry::try_from(geometry).unwrap(),
    )
}

fn circle(id: &str, center: [f64; 2], radius: f64) -> SketchEntity {
    entity(
        id,
        SketchGeometryDefinition::Circle {
            center: Point2::new(center[0], center[1]),
            radius: Length::new(radius).unwrap(),
        },
    )
}

fn half(id: &str, radius: f64, start: f64) -> SketchEntity {
    entity(
        id,
        SketchGeometryDefinition::Arc {
            center: Point2::new(0.0, 0.0),
            radius: Length::new(radius).unwrap(),
            start_angle: Angle::new(start).unwrap(),
            end_angle: Angle::new((start + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU))
                .unwrap(),
        },
    )
}

fn square(id: &str, side: f64) -> [SketchEntity; 4] {
    rectangle(id, side, side)
}

fn rectangle(id: &str, width: f64, height: f64) -> [SketchEntity; 4] {
    let corners = [[0.0, 0.0], [width, 0.0], [width, height], [0.0, height]];
    std::array::from_fn(|index| {
        let [start, end] = [corners[index], corners[(index + 1) % 4]];
        entity(
            &format!("{id}-{index}"),
            SketchGeometryDefinition::Line {
                start: Point2::new(start[0], start[1]),
                end: Point2::new(end[0], end[1]),
            },
        )
    })
}

/// `sketch` placed at `origin` with `normal` and `u_axis`.
fn placed(mut sketch: Sketch, origin: [f64; 3], normal: [f64; 3], u_axis: [f64; 3]) -> Sketch {
    sketch.placement = placement(origin, normal, u_axis);
    sketch
}

fn placement(origin: [f64; 3], normal: [f64; 3], u_axis: [f64; 3]) -> SketchPlacement {
    SketchPlacement::try_resolved(
        Point3::from(origin),
        Vector3::from(normal),
        Vector3::from(u_axis),
    )
    .unwrap()
}

/// `marker_reading` for a marker reading whose entities are all planar and
/// that no revolve takes.
fn reading(marker: &Sketch, stream: &Sketch, entities: &EntityIndex<'_>) -> MarkerReading {
    marker_reading(
        marker,
        stream,
        entities,
        true,
        false,
        &SolidEdges::default(),
    )
}

/// A marker reading of another loop binds in place of a stream reading of
/// one circle in its plane; a marker reading that repeats the circle, as two
/// half arcs placed elsewhere, does not, nor does one with an entity that
/// is not planar.
#[test]
fn a_marker_reading_binds_in_place_of_a_lone_circle_it_does_not_repeat() {
    let whole = circle("whole", [5.0, 5.0], 10.0);
    let upper = half("upper", 10.0, 0.0);
    let lower = half("lower", 10.0, std::f64::consts::PI);
    let sides = square("side", 3.0);
    let entities = EntityIndex::new(sides.iter().chain([&whole, &upper, &lower]));
    let stream = sketch("stream", &[&[&whole]]);
    let split = sketch("split", &[&[&upper, &lower]]);
    let boxed = sketch("box", &[&sides.iter().collect::<Vec<_>>()]);
    assert_eq!(reading(&split, &stream, &entities), MarkerReading::Stays);
    assert_eq!(
        reading(&boxed, &stream, &entities),
        MarkerReading::Binds(stream.placement)
    );
    assert_eq!(
        marker_reading(
            &boxed,
            &stream,
            &entities,
            false,
            true,
            &SolidEdges::default()
        ),
        MarkerReading::Stays
    );
}

/// A stream reading of anything but one circle keeps its binding, whatever
/// the marker reading holds.
#[test]
fn a_stream_reading_that_is_not_one_circle_keeps_its_binding() {
    let outer = circle("outer", [0.0, 0.0], 9.0);
    let inner = circle("inner", [0.0, 0.0], 4.0);
    let sides = square("side", 3.0);
    let entities = EntityIndex::new(sides.iter().chain([&outer, &inner]));
    let ring = sketch("ring", &[&[&outer], &[&inner]]);
    let boxed = sketch("box", &[&sides.iter().collect::<Vec<_>>()]);
    let stream_box = sketch("stream-box", &[&sides.iter().collect::<Vec<_>>()]);
    assert_eq!(reading(&ring, &stream_box, &entities), MarkerReading::Stays);
    assert_eq!(reading(&boxed, &ring, &entities), MarkerReading::Stays);
}

/// A marker reading whose normal is parallel to the stream circle's binds on
/// the stream sketch's plane, through its own origin projected onto that
/// plane. Where the normals have the same sense it keeps its own normal and
/// u axis; where they oppose, it takes the stream sketch's.
#[test]
fn a_parallel_marker_reading_binds_on_the_stream_plane() {
    let whole = circle("whole", [0.0, 0.0], 10.0);
    let sides = square("side", 3.0);
    let entities = EntityIndex::new(sides.iter().chain([&whole]));
    let boxed = placed(
        sketch("box", &[&sides.iter().collect::<Vec<_>>()]),
        [1.0, 2.0, 0.0],
        [0.0, 0.0, 1.0],
        [1.0, 0.0, 0.0],
    );
    let above = placed(
        sketch("above", &[&[&whole]]),
        [30.0, -4.0, 20.0],
        [0.0, 0.0, 1.0],
        [1.0, 0.0, 0.0],
    );
    assert_eq!(
        reading(&boxed, &above, &entities),
        MarkerReading::Binds(placement(
            [1.0, 2.0, 20.0],
            [0.0, 0.0, 1.0],
            [1.0, 0.0, 0.0]
        ))
    );
    let below = placed(
        sketch("below", &[&[&whole]]),
        [0.0, 0.0, -18.5],
        [0.0, 0.0, -1.0],
        [-1.0, 0.0, 0.0],
    );
    assert_eq!(
        reading(&boxed, &below, &entities),
        MarkerReading::Binds(placement(
            [1.0, 2.0, -18.5],
            [0.0, 0.0, -1.0],
            [-1.0, 0.0, 0.0]
        ))
    );
}

/// The frame a parallel marker reading of an asymmetric profile takes on
/// the stream plane: with the same normal sense, its own u axis, here a
/// quarter turn from the stream sketch's; with opposed normals, the stream
/// sketch's normal and u axis. The solid carries the profile in that frame
/// and in no other, so it confirms the binding.
#[test]
fn an_asymmetric_parallel_marker_reading_keeps_its_u_axis_only_with_the_same_normal_sense() {
    let whole = circle("whole", [0.0, 0.0], 10.0);
    let sides = rectangle("side", 4.0, 1.0);
    let entities = EntityIndex::new(sides.iter().chain([&whole]));
    let bar = sketch("bar", &[&sides.iter().collect::<Vec<_>>()]);
    let read = |marker: &Sketch, stream: &Sketch, solid: &SolidEdges| {
        marker_reading(marker, stream, &entities, true, false, solid)
    };

    let turned = placed(
        bar.clone(),
        [1.0, 2.0, 0.0],
        [0.0, 0.0, 1.0],
        [0.0, 1.0, 0.0],
    );
    let above = placed(
        sketch("above", &[&[&whole]]),
        [30.0, -4.0, 20.0],
        [0.0, 0.0, 1.0],
        [1.0, 0.0, 0.0],
    );
    let own_frame = polygon_edges(&[
        [1.0, 2.0, 20.0],
        [1.0, 6.0, 20.0],
        [0.0, 6.0, 20.0],
        [0.0, 2.0, 20.0],
    ]);
    let rim_above = ([30.0, -4.0, 20.0], [0.0, 0.0, 1.0], 10.0);
    assert_eq!(
        read(&turned, &above, &solid(&own_frame, &[rim_above])),
        MarkerReading::Binds(placement(
            [1.0, 2.0, 20.0],
            [0.0, 0.0, 1.0],
            [0.0, 1.0, 0.0]
        ))
    );

    let upward = placed(bar, [1.0, 2.0, 0.0], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]);
    let below = placed(
        sketch("below", &[&[&whole]]),
        [0.0, 0.0, -18.5],
        [0.0, 0.0, -1.0],
        [-1.0, 0.0, 0.0],
    );
    let stream_frame = polygon_edges(&[
        [1.0, 2.0, -18.5],
        [-3.0, 2.0, -18.5],
        [-3.0, 3.0, -18.5],
        [1.0, 3.0, -18.5],
    ]);
    let rim_below = ([0.0, 0.0, -18.5], [0.0, 0.0, 1.0], 10.0);
    assert_eq!(
        read(&upward, &below, &solid(&stream_frame, &[rim_below])),
        MarkerReading::Binds(placement(
            [1.0, 2.0, -18.5],
            [0.0, 0.0, -1.0],
            [-1.0, 0.0, 0.0]
        ))
    );
}

/// A marker reading that a revolve takes as its profile binds on its own
/// placement also where its normal is parallel to the stream circle's, with
/// the same sense or the opposed one, and whatever the solid carries.
#[test]
fn a_parallel_revolve_profile_binds_on_its_own_placement() {
    let whole = circle("whole", [0.0, 0.0], 10.0);
    let sides = square("side", 3.0);
    let entities = EntityIndex::new(sides.iter().chain([&whole]));
    let boxed = placed(
        sketch("box", &[&sides.iter().collect::<Vec<_>>()]),
        [1.0, 2.0, 0.0],
        [0.0, 0.0, 1.0],
        [1.0, 0.0, 0.0],
    );
    let rim = ([0.0, 0.0, -18.5], [0.0, 0.0, 1.0], 10.0);
    for (normal, u_axis) in [
        ([0.0, 0.0, 1.0], [1.0, 0.0, 0.0]),
        ([0.0, 0.0, -1.0], [-1.0, 0.0, 0.0]),
    ] {
        let stream = placed(
            sketch("stream", &[&[&whole]]),
            [0.0, 0.0, -18.5],
            normal,
            u_axis,
        );
        for edges in [SolidEdges::default(), solid(&[], &[rim])] {
            assert_eq!(
                marker_reading(&boxed, &stream, &entities, true, true, &edges),
                MarkerReading::Binds(boxed.placement)
            );
        }
    }
}

/// A marker reading whose normal is not parallel to the stream circle's, or
/// whose placement is unresolved, leaves the stream circle bound, unless a
/// revolve takes it as its profile; then it binds with its own placement.
#[test]
fn a_marker_reading_on_another_plane_leaves_the_stream_bound_unless_a_revolve_takes_it() {
    let whole = circle("whole", [0.0, 0.0], 10.0);
    let sides = square("side", 3.0);
    let entities = EntityIndex::new(sides.iter().chain([&whole]));
    let boxed = sketch("box", &[&sides.iter().collect::<Vec<_>>()]);
    let side = placed(
        sketch("side-circle", &[&[&whole]]),
        [0.0, 5.0, 0.0],
        [0.0, 1.0, 0.0],
        [1.0, 0.0, 0.0],
    );
    assert_eq!(
        reading(&boxed, &side, &entities),
        MarkerReading::Unbound(UnboundReason::OtherPlane)
    );
    assert_eq!(
        marker_reading(&boxed, &side, &entities, true, true, &SolidEdges::default()),
        MarkerReading::Binds(boxed.placement)
    );
    let mut unplaced = boxed.clone();
    unplaced.placement = SketchPlacement::Unresolved {};
    assert_eq!(
        reading(&unplaced, &side, &entities),
        MarkerReading::Unbound(UnboundReason::PlacementUnresolved)
    );
    assert_eq!(
        marker_reading(
            &unplaced,
            &side,
            &entities,
            true,
            true,
            &SolidEdges::default()
        ),
        MarkerReading::Binds(SketchPlacement::Unresolved {})
    );
}

/// A marker reading that holds the stream circle's loop among other loops,
/// a plate whose hole is the stream circle once on the stream plane, is
/// decided by the plane and the solid like any other: it binds where the
/// solid carries every loop or where the solid gives no verdict, and leaves
/// the stream circle bound where the solid carries the circle alone.
#[test]
fn a_marker_reading_that_holds_the_circle_among_other_loops_binds_where_the_solid_carries_it() {
    let hole = circle("hole", [8.0, 8.0], 2.0);
    let repeat = circle("repeat", [1.0, 1.0], 2.0);
    let sides = square("side", 20.0);
    let entities = EntityIndex::new(sides.iter().chain([&hole, &repeat]));
    let stream = placed(
        sketch("stream", &[&[&repeat]]),
        [7.0, 7.0, 5.0],
        [0.0, 0.0, 1.0],
        [1.0, 0.0, 0.0],
    );
    let plate = sketch("plate", &[&sides.iter().collect::<Vec<_>>(), &[&hole]]);
    let read = |solid: &SolidEdges| marker_reading(&plate, &stream, &entities, true, false, solid);
    let moved = MarkerReading::Binds(placement([0.0, 0.0, 5.0], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]));
    let hole_edge = ([8.0, 8.0, 5.0], [0.0, 0.0, 1.0], 2.0);
    let rim = polygon_edges(&[
        [0.0, 0.0, 5.0],
        [20.0, 0.0, 5.0],
        [20.0, 20.0, 5.0],
        [0.0, 20.0, 5.0],
    ]);
    assert_eq!(read(&solid(&rim, &[hole_edge])), moved);
    assert_eq!(read(&SolidEdges::default()), moved);
    assert_eq!(
        read(&solid(&[], &[hole_edge])),
        MarkerReading::Unbound(UnboundReason::OffSolid)
    );
}

/// A loop curve the solid's edges cannot test, an ellipse, gives no
/// verdict: beside a circle on an edge, the marker reading binds as it does
/// without a solid. A curve of another loop on no edge still decides, also
/// where the ellipse's loop comes first.
#[test]
fn an_ellipse_loop_beside_a_circle_on_an_edge_gives_no_verdict() {
    let whole = circle("whole", [0.0, 0.0], 10.0);
    let small = circle("small", [5.0, 5.0], 1.0);
    let oval = entity(
        "oval",
        SketchGeometryDefinition::Ellipse {
            center: Point2::new(12.0, 5.0),
            major_angle: Angle::new(0.0).unwrap(),
            radii: EllipseRadii {
                major_radius: Length::new(2.0).unwrap(),
                minor_radius: Length::new(1.0).unwrap(),
            },
            bounds: None,
        },
    );
    let entities = EntityIndex::new([&whole, &small, &oval]);
    let marker = sketch("marker", &[&[&oval], &[&small]]);
    let above = placed(
        sketch("above", &[&[&whole]]),
        [0.0, 0.0, 20.0],
        [0.0, 0.0, 1.0],
        [1.0, 0.0, 0.0],
    );
    let read = |solid: &SolidEdges| marker_reading(&marker, &above, &entities, true, false, solid);
    let rim = ([0.0, 0.0, 20.0], [0.0, 0.0, 1.0], 10.0);
    let small_edge = ([5.0, 5.0, 20.0], [0.0, 0.0, 1.0], 1.0);
    let moved = MarkerReading::Binds(placement(
        [0.0, 0.0, 20.0],
        [0.0, 0.0, 1.0],
        [1.0, 0.0, 0.0],
    ));
    assert_eq!(read(&solid(&[], &[rim, small_edge])), moved);
    assert_eq!(
        read(&solid(&[], &[rim])),
        MarkerReading::Unbound(UnboundReason::OffSolid)
    );
}

/// A solid of the given line edges, each from its two ends, and circular
/// edges, each from its centre, axis and radius, built from its topology.
pub(crate) fn solid(
    lines: &[([f64; 3], [f64; 3])],
    circles: &[([f64; 3], [f64; 3], f64)],
) -> SolidEdges {
    use cadmpeg_ir::features::geometry::FinitePoint3;
    use cadmpeg_ir::geometry::analytic::{CircleCurve, LineCurve};
    use cadmpeg_ir::geometry::{Curve, CurveGeometry, SolvedCurveGeometry};
    use cadmpeg_ir::ids::{CurveId, EdgeId, PointId, VertexId};
    use cadmpeg_ir::topology::{Edge, EdgeCarrier, EdgeEnds, Point, Vertex};

    let (mut edges, mut vertices, mut points, mut curves) = (vec![], vec![], vec![], vec![]);
    let mut vertex = |name: String, position: [f64; 3]| {
        let point = PointId::mint(format!("test:solid:point#{name}")).unwrap();
        let id = VertexId::mint(format!("test:solid:vertex#{name}")).unwrap();
        points.push(Point::new(
            point.clone(),
            FinitePoint3::new(Point3::from(position)).unwrap(),
            None,
        ));
        vertices.push(Vertex {
            id: id.clone(),
            point,
            tolerance: None,
            native_ref: None,
        });
        id
    };
    let geometries = lines
        .iter()
        .enumerate()
        .map(|(index, (start, end))| {
            let ends = EdgeEnds::new(
                vertex(format!("{index}a"), *start),
                vertex(format!("{index}b"), *end),
            );
            let direction = Point3::from(*end).vector_from(Point3::from(*start));
            let line = LineCurve::try_new(Point3::from(*start), direction.unit().unwrap()).unwrap();
            (Some(ends), SolvedCurveGeometry::Line(line))
        })
        .chain(circles.iter().map(|(center, axis, radius)| {
            let reference = if axis[0] == 0.0 && axis[1] == 0.0 {
                Vector3::new(1.0, 0.0, 0.0)
            } else {
                Vector3::new(0.0, 0.0, 1.0)
            };
            let circle = CircleCurve::try_new(
                Point3::from(*center),
                Vector3::from(*axis),
                reference,
                *radius,
            )
            .unwrap();
            (None, SolvedCurveGeometry::Circle(circle))
        }))
        .collect::<Vec<_>>();
    for (index, (ends, geometry)) in geometries.into_iter().enumerate() {
        let curve = CurveId::mint(format!("test:solid:curve#{index}")).unwrap();
        curves.push(Curve {
            parameter_range: None,
            id: curve.clone(),
            geometry: CurveGeometry::Solved(geometry),
            source_object: None,
            native_ref: None,
        });
        edges.push(Edge {
            id: EdgeId::mint(format!("test:solid:edge#{index}")).unwrap(),
            carrier: EdgeCarrier::unbounded(Some(curve)),
            ends,
            tolerance: None,
            native_ref: None,
        });
    }
    SolidEdges::new(&edges, &vertices, &points, &curves)
}

/// The sides of the closed polygon through `corners`, as line edges.
fn polygon_edges(corners: &[[f64; 3]]) -> Vec<([f64; 3], [f64; 3])> {
    (0..corners.len())
        .map(|index| (corners[index], corners[(index + 1) % corners.len()]))
        .collect()
}

/// The four sides of the 3-unit square at `[1, 2]` on the plane `z`, as
/// line edges.
fn square_edges(z: f64) -> Vec<([f64; 3], [f64; 3])> {
    polygon_edges(&[[1.0, 2.0, z], [4.0, 2.0, z], [4.0, 5.0, z], [1.0, 5.0, z]])
}

/// Where the solid carries the stream circle on its edges and not the
/// marker reading's loops on the stream plane, a parallel marker reading
/// leaves the stream circle bound and joins the model unbound. Where the
/// solid carries the marker loops there, or does not carry the stream
/// circle, it binds on the stream plane as before; a revolve's profile binds
/// on its own placement whatever the solid carries.
#[test]
fn a_parallel_marker_reading_off_the_solid_leaves_a_stream_circle_on_it_bound() {
    let whole = circle("whole", [0.0, 0.0], 10.0);
    let sides = square("side", 3.0);
    let entities = EntityIndex::new(sides.iter().chain([&whole]));
    let boxed = placed(
        sketch("box", &[&sides.iter().collect::<Vec<_>>()]),
        [1.0, 2.0, 0.0],
        [0.0, 0.0, 1.0],
        [1.0, 0.0, 0.0],
    );
    let above = placed(
        sketch("above", &[&[&whole]]),
        [0.0, 0.0, 20.0],
        [0.0, 0.0, 1.0],
        [1.0, 0.0, 0.0],
    );
    let moved = MarkerReading::Binds(placement(
        [1.0, 2.0, 20.0],
        [0.0, 0.0, 1.0],
        [1.0, 0.0, 0.0],
    ));
    let rim = ([0.0, 0.0, 20.0], [0.0, 0.0, -1.0], 10.0);
    let read = |solid: &SolidEdges, revolve: bool| {
        marker_reading(&boxed, &above, &entities, true, revolve, solid)
    };
    let off = MarkerReading::Unbound(UnboundReason::OffSolid);
    assert_eq!(read(&solid(&[], &[rim]), false), off);
    assert_eq!(
        read(&solid(&square_edges(0.0), &[rim]), false),
        off,
        "the witness reads the marker loops on the stream plane, not on their own"
    );
    let mut short = square_edges(20.0);
    short[2] = ([4.0, 5.0, 20.0], [3.0, 5.0, 20.0]);
    assert_eq!(
        read(&solid(&short, &[rim]), false),
        off,
        "a side whose midpoint lies outside the collinear edge is not on it"
    );
    assert_eq!(read(&solid(&square_edges(20.0), &[rim]), false), moved);
    assert_eq!(read(&solid(&square_edges(20.0), &[]), false), moved);
    assert_eq!(read(&SolidEdges::default(), false), moved);
    assert_eq!(
        read(&solid(&[], &[rim]), true),
        MarkerReading::Binds(boxed.placement)
    );
    let wider = ([0.0, 0.0, 20.0], [0.0, 0.0, 1.0], 10.5);
    assert_eq!(
        read(&solid(&[], &[wider]), false),
        moved,
        "a circular edge of another radius does not carry the stream circle"
    );
}
