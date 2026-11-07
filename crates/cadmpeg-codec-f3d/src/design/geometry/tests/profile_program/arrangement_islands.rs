// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn a_full_ellipse_leaves_counted_members_without_a_region() {
    let id = SketchId::mint("synthetic:test:id#ellipse-island").unwrap();
    let outer = rectangle(&id, "outer", (0.0, 0.0), (40.0, 30.0));
    let sketch = sketch_with_loops(id.clone(), &[&outer]);
    let mut entities = outer.clone();
    entities.push(SketchEntity::new(
        SketchEntityId::mint("synthetic:test:id#ellipse").unwrap(),
        id,
        SketchGeometry::try_from(SketchGeometryDefinition::Ellipse {
            center: Point2::new(20.0, 15.0),
            major_angle: cadmpeg_ir::scalar::Angle::new(0.0).unwrap(),
            radii: cadmpeg_ir::sketches::EllipseRadii {
                major_radius: Length::new(6.0).unwrap(),
                minor_radius: Length::new(3.0).unwrap(),
            },
            bounds: None,
        })
        .unwrap(),
    ));
    assert_eq!(
        member_regions(&sketch, &entities, &[ring(&outer, false)]),
        None
    );
}

/// A full ellipse of `sketch` centred at `center`, with radii 6 and 3.
fn full_ellipse(sketch: &SketchId, center: (f64, f64)) -> SketchEntity {
    SketchEntity::new(
        SketchEntityId::mint("synthetic:test:id#far-ellipse").unwrap(),
        sketch.clone(),
        SketchGeometry::try_from(SketchGeometryDefinition::Ellipse {
            center: Point2::new(center.0, center.1),
            major_angle: cadmpeg_ir::scalar::Angle::new(0.0).unwrap(),
            radii: cadmpeg_ir::sketches::EllipseRadii {
                major_radius: Length::new(6.0).unwrap(),
                minor_radius: Length::new(3.0).unwrap(),
            },
            bounds: None,
        })
        .unwrap(),
    )
}

#[test]
fn a_full_ellipse_clear_of_the_selected_face_keeps_the_selection() {
    let id = SketchId::mint("synthetic:test:id#ellipse-far").unwrap();
    let outer = rectangle(&id, "outer", (0.0, 0.0), (40.0, 30.0));
    let sketch = sketch_with_loops(id.clone(), &[&outer]);
    let mut far = outer.clone();
    far.push(full_ellipse(&id, (100.0, 100.0)));
    let (exterior, holes) = single_region(&sketch, &far, &[ring(&outer, false)]);
    assert_eq!(exterior.len(), 4);
    assert!(holes.is_empty());
    let mut near = outer.clone();
    near.push(full_ellipse(&id, (45.0, 15.0)));
    assert_eq!(
        member_regions(&sketch, &near, &[ring(&outer, false)]),
        None,
        "the ellipse box meets the face box"
    );
    let mut unbounded = outer.clone();
    unbounded.push(SketchEntity::new(
        SketchEntityId::mint("synthetic:test:id#far-reference").unwrap(),
        id,
        SketchGeometry::try_from(SketchGeometryDefinition::ReferenceLine {
            origin: Point2::new(100.0, 100.0),
            direction: Point2::new(1.0, 0.0),
        })
        .unwrap(),
    ));
    assert_eq!(
        member_regions(&sketch, &unbounded, &[ring(&outer, false)]),
        None,
        "a reference line has no bounds"
    );
}

/// A full ellipse of `sketch` centred at `center`, with radii 3e-5 and 2e-5.
fn tiny_ellipse(sketch: &SketchId, center: (f64, f64)) -> SketchEntity {
    SketchEntity::new(
        SketchEntityId::mint("synthetic:test:id#tiny-ellipse").unwrap(),
        sketch.clone(),
        SketchGeometry::try_from(SketchGeometryDefinition::Ellipse {
            center: Point2::new(center.0, center.1),
            major_angle: cadmpeg_ir::scalar::Angle::new(0.0).unwrap(),
            radii: cadmpeg_ir::sketches::EllipseRadii {
                major_radius: Length::new(3.0e-5).unwrap(),
                minor_radius: Length::new(2.0e-5).unwrap(),
            },
            bounds: None,
        })
        .unwrap(),
    )
}

#[test]
fn an_ellipse_beyond_the_sampled_bounds_of_a_curved_face_voids_its_selection() {
    // A radius-10 arc over 180 degrees, closed by its chord in two halves.
    // The arc's extreme point lies midway between two polyline vertices, so
    // the polyline box stops 1.9e-4 short of the arc. The ellipse box starts
    // 3e-5 from the extreme point.
    let id = SketchId::mint("synthetic:test:id#d-face").unwrap();
    let point = |degrees: f64| {
        let angle = degrees.to_radians();
        (10.0 * angle.cos(), 10.0 * angle.sin())
    };
    for (start, extreme) in [(-80.507_812_5, (10.0, 0.0)), (99.492_187_5, (-10.0, 0.0))] {
        let arc = SketchEntity::new(
            SketchEntityId::mint("synthetic:test:id#d-arc").unwrap(),
            id.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Arc {
                center: Point2::new(0.0, 0.0),
                radius: Length::new(10.0).unwrap(),
                start_angle: Angle::new(f64::to_radians(start)).unwrap(),
                end_angle: Angle::new(f64::to_radians(start + 180.0)).unwrap(),
            })
            .unwrap(),
        );
        let (top, bottom) = (point(start + 180.0), point(start));
        let middle = (
            f64::midpoint(top.0, bottom.0),
            f64::midpoint(top.1, bottom.1),
        );
        let upper = line_entity(&id, "d-upper", top, middle);
        let lower = line_entity(&id, "d-lower", middle, bottom);
        let curves = vec![arc.clone(), upper.clone(), lower.clone()];
        let sketch = sketch_with_loops(id.clone(), &[&curves]);
        let ring = [vec![(&arc, false), (&upper, false), (&lower, false)]];
        let (exterior, holes) = single_region(&sketch, &curves, &ring);
        assert_eq!(exterior.len(), 3, "arc from {start} degrees");
        assert!(holes.is_empty());
        let mut far = curves.clone();
        far.push(tiny_ellipse(&id, (3.0 * extreme.0, 0.0)));
        assert!(
            member_regions(&sketch, &far, &ring).is_some(),
            "arc from {start} degrees"
        );
        let mut beyond = curves.clone();
        beyond.push(tiny_ellipse(&id, extreme));
        assert_eq!(
            member_regions(&sketch, &beyond, &ring),
            None,
            "arc from {start} degrees"
        );
    }
}

#[test]
fn the_innermost_container_holds_the_island_when_the_polylines_decide_it() {
    // Two concentric circles around a small square: the square lies in both
    // disks, and the inner disk lies inside the outer one.
    let id = SketchId::mint("synthetic:test:id#nested-disks").unwrap();
    let circle = |name: &str, radius: f64| {
        SketchEntity::new(
            SketchEntityId::mint(format!("synthetic:test:id#{name}")).unwrap(),
            id.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Circle {
                center: Point2::new(0.0, 0.0),
                radius: Length::new(radius).unwrap(),
            })
            .unwrap(),
        )
    };
    let square = rectangle(&id, "core", (-1.0, -1.0), (1.0, 1.0));
    let inner_holes = |inner_radius: f64, outer_radius: f64| {
        let inner = circle("inner-disk", inner_radius);
        let outer = circle("outer-disk", outer_radius);
        let mut entities = vec![inner.clone(), outer.clone()];
        entities.extend(square.iter().cloned());
        let sketch = sketch_with_loops(
            id.clone(),
            &[
                std::slice::from_ref(&inner),
                std::slice::from_ref(&outer),
                &square,
            ],
        );
        let faces = crate::test_support::with_decode_context(|ctx| {
            super::super::super::curve_arrangement_faces(
                &sketch,
                &entities,
                PROFILE_PROGRAM_TOLERANCE,
                &ProfileGeometryWork::new(),
                ctx,
                true,
            )
        })
        .unwrap()
        .expect("the arrangement");
        faces
            .iter()
            .find(|face| face.ring.curves().any(|curve| curve == inner.id()))
            .expect("the inner disk")
            .holes
            .as_ref()
            .map(Vec::len)
    };
    assert_eq!(inner_holes(10.0, 13.0), Some(1));
    assert_eq!(inner_holes(10.0, 10.1), Some(1), "a thin wall still nests");
    // Radius 1e4: each circle's polyline sagitta is about 0.19, more than
    // the 0.15 wall, so the polylines cannot order the two disks.
    assert_eq!(inner_holes(1.0e4, 1.0e4 + 0.15), None);

    // The outer circle's centre moves 100 left, so the wall stays 0.15 on
    // the right, where the inner root lies. A spur gives the outer circle a
    // first node on its far left: that root lies clear outside the inner
    // disk, which orders the pair.
    let inner = circle("inner-disk", 1.0e4);
    let outer = SketchEntity::new(
        SketchEntityId::mint("synthetic:test:id#outer-disk").unwrap(),
        id.clone(),
        SketchGeometry::try_from(SketchGeometryDefinition::Circle {
            center: Point2::new(-100.0, 0.0),
            radius: Length::new(1.0e4 + 100.15).unwrap(),
        })
        .unwrap(),
    );
    let spur = line_entity(&id, "spur", (-10_200.15, 0.0), (-10_205.0, 0.0));
    let mut entities = vec![spur.clone(), inner.clone(), outer.clone()];
    entities.extend(square.iter().cloned());
    let sketch = sketch_with_loops(
        id.clone(),
        &[
            std::slice::from_ref(&inner),
            std::slice::from_ref(&outer),
            &square,
        ],
    );
    let faces = crate::test_support::with_decode_context(|ctx| {
        super::super::super::curve_arrangement_faces(
            &sketch,
            &entities,
            PROFILE_PROGRAM_TOLERANCE,
            &ProfileGeometryWork::new(),
            ctx,
            true,
        )
    })
    .unwrap()
    .expect("the arrangement");
    let inner_face = faces
        .iter()
        .find(|face| face.ring.curves().any(|curve| curve == inner.id()))
        .expect("the inner disk");
    assert_eq!(inner_face.holes.as_ref().map(Vec::len), Some(1));
}

#[test]
fn an_island_within_the_polyline_deviation_of_a_face_leaves_its_islands_unknown() {
    // A radius-1e4 disk: its polyline sagitta is about 0.19.
    let id = SketchId::mint("synthetic:test:id#near-island").unwrap();
    let disk = SketchEntity::new(
        SketchEntityId::mint("synthetic:test:id#disk").unwrap(),
        id.clone(),
        SketchGeometry::try_from(SketchGeometryDefinition::Circle {
            center: Point2::new(0.0, 0.0),
            radius: Length::new(1.0e4).unwrap(),
        })
        .unwrap(),
    );
    let faces = |island: &[SketchEntity]| {
        let mut entities = vec![disk.clone()];
        entities.extend(island.iter().cloned());
        let sketch = sketch_with_loops(id.clone(), &[std::slice::from_ref(&disk), island]);
        crate::test_support::with_decode_context(|ctx| {
            super::super::super::curve_arrangement_faces(
                &sketch,
                &entities,
                PROFILE_PROGRAM_TOLERANCE,
                &ProfileGeometryWork::new(),
                ctx,
                true,
            )
        })
        .unwrap()
        .expect("the arrangement")
    };
    let disk_holes = |faces: &[super::super::super::SketchArrangementFace]| {
        faces
            .iter()
            .find(|face| face.ring.curves().any(|curve| curve == disk.id()))
            .expect("the disk face")
            .holes
            .as_ref()
            .map(Vec::len)
    };
    let centred = rectangle(&id, "centred", (-1.0, -1.0), (1.0, 1.0));
    assert_eq!(disk_holes(&faces(&centred)), Some(1));
    let near = rectangle(&id, "near", (9_999.85, -0.005), (9_999.86, 0.005));
    assert_eq!(disk_holes(&faces(&near)), None);
    let clear = rectangle(&id, "clear", (9_999.5, -0.005), (9_999.6, 0.005));
    assert_eq!(
        disk_holes(&faces(&clear)),
        Some(1),
        "0.4 inside the boundary, beyond the 0.19 sagitta"
    );

    // Outside the disk, near its boundary, inside a larger square: the
    // square contains the island, and the disk cannot decide it.
    let frame = rectangle(&id, "frame", (-2.0e4, -2.0e4), (2.0e4, 2.0e4));
    let outside = rectangle(&id, "outside", (10_000.14, -0.005), (10_000.15, 0.005));
    let mut framed = frame.clone();
    framed.extend(outside.iter().cloned());
    let arranged = faces(&framed);
    let frame_face = arranged
        .iter()
        .find(|face| face.ring.curves().any(|curve| curve == frame[0].id()))
        .expect("the frame face");
    assert_eq!(frame_face.holes.as_ref().map(Vec::len), None);
    assert_eq!(disk_holes(&arranged), None);

    let square = rectangle(&id, "square", (0.0, 0.0), (20.0, 20.0));
    let island = rectangle(&id, "beside", (19.98, 5.0), (19.99, 6.0));
    let mut entities = square.clone();
    entities.extend(island.iter().cloned());
    let sketch = sketch_with_loops(id.clone(), &[&square, &island]);
    let faces = crate::test_support::with_decode_context(|ctx| {
        super::super::super::curve_arrangement_faces(
            &sketch,
            &entities,
            PROFILE_PROGRAM_TOLERANCE,
            &ProfileGeometryWork::new(),
            ctx,
            true,
        )
    })
    .unwrap()
    .expect("the arrangement");
    let square_face = faces
        .iter()
        .find(|face| face.ring.curves().any(|curve| curve == square[0].id()))
        .expect("the square face");
    assert_eq!(
        square_face.holes.as_ref().map(Vec::len),
        Some(1),
        "a straight side has no polyline deviation"
    );
}

#[test]
fn an_ellipse_clear_of_the_exact_face_box_keeps_the_selection() {
    // The upper half of a radius-10 disk, and an ellipse 0.03 beyond the
    // disk on the x axis.
    let id = SketchId::mint("synthetic:test:id#upper-half").unwrap();
    let disk = SketchEntity::new(
        SketchEntityId::mint("synthetic:test:id#half-disk").unwrap(),
        id.clone(),
        SketchGeometry::try_from(SketchGeometryDefinition::Circle {
            center: Point2::new(0.0, 0.0),
            radius: Length::new(10.0).unwrap(),
        })
        .unwrap(),
    );
    let diameter = line_entity(&id, "diameter", (-10.0, 0.0), (10.0, 0.0));
    let ellipse = |center: (f64, f64)| {
        SketchEntity::new(
            SketchEntityId::mint("synthetic:test:id#clear-ellipse").unwrap(),
            id.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Ellipse {
                center: Point2::new(center.0, center.1),
                major_angle: cadmpeg_ir::scalar::Angle::new(0.0).unwrap(),
                radii: cadmpeg_ir::sketches::EllipseRadii {
                    major_radius: Length::new(0.01).unwrap(),
                    minor_radius: Length::new(0.005).unwrap(),
                },
                bounds: None,
            })
            .unwrap(),
        )
    };
    let curves = vec![disk.clone(), diameter.clone()];
    let sketch = sketch_with_loops(id.clone(), &[&curves]);
    let ring = [vec![(&disk, false), (&diameter, false)]];
    let (exterior, _) = single_region(&sketch, &curves, &ring);
    assert_eq!(exterior.len(), 2);
    // Beside the disk, and inside the lower half, which the selected upper
    // half's box excludes.
    for center in [(10.04, 0.0), (0.0, -5.0)] {
        let mut with_ellipse = curves.clone();
        with_ellipse.push(ellipse(center));
        assert!(
            member_regions(&sketch, &with_ellipse, &ring).is_some(),
            "ellipse at {center:?}"
        );
    }
}

#[test]
fn a_point_beside_an_exact_edge_of_a_face_with_a_curved_edge_selects_the_face() {
    // A 100 x 10 face whose top side is a NURBS: its polyline band is wide,
    // and the bottom line's band is zero.
    use cadmpeg_ir::geometry::pcurve::{PcurveNurbs, PcurveNurbsPoles};

    let id = SketchId::mint("synthetic:test:id#nurbs-top").unwrap();
    let top = SketchEntity::new(
        SketchEntityId::mint("synthetic:test:id#nurbs-top-curve").unwrap(),
        id.clone(),
        SketchGeometry::try_from(SketchGeometryDefinition::Nurbs {
            curve: PcurveNurbs::new(
                2,
                vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
                PcurveNurbsPoles::from_lanes(
                    vec![
                        Point2::new(100.0, 10.0),
                        Point2::new(50.0, 12.0),
                        Point2::new(0.0, 10.0),
                    ],
                    None,
                )
                .unwrap(),
                false,
            )
            .unwrap(),
        })
        .unwrap(),
    );
    let entities = vec![
        line_entity(&id, "nurbs-bottom", (0.0, 0.0), (100.0, 0.0)),
        line_entity(&id, "nurbs-right", (100.0, 0.0), (100.0, 10.0)),
        top,
        line_entity(&id, "nurbs-left", (0.0, 10.0), (0.0, 0.0)),
    ];
    let sketch = sketch_with_loops(id, &[&entities]);
    let region = crate::test_support::with_decode_context(|ctx| {
        super::super::super::arrangement_region_containing_points(
            &sketch,
            &entities,
            &[Point2::new(50.0, 0.1)],
            PROFILE_PROGRAM_TOLERANCE,
            &ProfileGeometryWork::new(),
            ctx,
        )
    })
    .unwrap();
    assert!(region.is_some());
}

#[test]
fn a_point_between_a_curved_boundary_and_its_polyline_selects_no_face() {
    // A 20 x 20 square split by an arc about (0, 10) from (10, 0) to
    // (10, 20) that bulges to x = 14.14. The arc polyline has vertices at 0
    // and pi / 512 rad; a point at half that angle, 3e-5 inside the arc,
    // lies in the left face but beyond the chord, where the right face's
    // polyline holds it.
    let id = SketchId::mint("synthetic:test:id#bulge").unwrap();
    let radius = 200.0_f64.sqrt();
    let arc = SketchEntity::new(
        SketchEntityId::mint("synthetic:test:id#bulge-arc").unwrap(),
        id.clone(),
        SketchGeometry::try_from(SketchGeometryDefinition::Arc {
            center: Point2::new(0.0, 10.0),
            radius: Length::new(radius).unwrap(),
            start_angle: Angle::new(-std::f64::consts::FRAC_PI_4).unwrap(),
            end_angle: Angle::new(std::f64::consts::FRAC_PI_4).unwrap(),
        })
        .unwrap(),
    );
    let mut entities = rectangle(&id, "frame", (0.0, 0.0), (20.0, 20.0));
    entities.push(arc.clone());
    let sketch = sketch_with_loops(id, &[&entities]);
    let region = |point: Point2| {
        crate::test_support::with_decode_context(|ctx| {
            super::super::super::arrangement_region_containing_points(
                &sketch,
                &entities,
                &[point],
                PROFILE_PROGRAM_TOLERANCE,
                &ProfileGeometryWork::new(),
                ctx,
            )
        })
        .unwrap()
    };
    assert!(
        region(Point2::new(5.0, 10.0)).is_some(),
        "inside the left face"
    );
    assert!(
        region(Point2::new(17.0, 10.0)).is_some(),
        "inside the right face"
    );
    let half_step = std::f64::consts::PI / 1024.0;
    let inside = radius - 3.0e-5;
    assert_eq!(
        region(Point2::new(
            inside * half_step.cos(),
            10.0 + inside * half_step.sin()
        )),
        None
    );
}
