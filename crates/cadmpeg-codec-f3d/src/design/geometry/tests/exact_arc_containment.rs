// SPDX-License-Identifier: Apache-2.0

use super::*;

const EPS_EXACT_ARC_REGION: f64 = 1.0e-6;

fn sketch_id() -> SketchId {
    SketchId::mint("synthetic:test:id#exact-arc").unwrap()
}

fn entity(name: &str, definition: SketchGeometryDefinition) -> SketchEntity {
    SketchEntity::new(
        SketchEntityId::mint(format!("synthetic:test:id#exact-arc-{name}")).unwrap(),
        sketch_id(),
        SketchGeometry::try_from(definition).unwrap(),
    )
}

fn line(name: &str, start: Point2, end: Point2) -> SketchEntity {
    entity(name, SketchGeometryDefinition::Line { start, end })
}

fn arc(name: &str, center: Point2, radius: f64, start_angle: f64, end_angle: f64) -> SketchEntity {
    entity(
        name,
        SketchGeometryDefinition::Arc {
            center,
            radius: Length::new(radius).unwrap(),
            start_angle: Angle::new(start_angle).unwrap(),
            end_angle: Angle::new(end_angle).unwrap(),
        },
    )
}

fn circle(name: &str, center: Point2, radius: f64) -> SketchEntity {
    entity(
        name,
        SketchGeometryDefinition::Circle {
            center,
            radius: Length::new(radius).unwrap(),
        },
    )
}

fn quadratic_nurbs(name: &str, points: [Point2; 3], weights: Option<Vec<f64>>) -> SketchEntity {
    entity(
        name,
        SketchGeometryDefinition::Nurbs {
            curve: PcurveNurbs::from_lanes(
                2,
                vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
                points.to_vec(),
                weights,
                false,
            )
            .unwrap(),
        },
    )
}

/// A 10 x 10 square whose top side is a rational NURBS that rises to about
/// v = 10.86 at u = 5.
fn nurbs_topped_square() -> Vec<SketchEntity> {
    vec![
        line("bottom", Point2::new(0.0, 0.0), Point2::new(10.0, 0.0)),
        line("right", Point2::new(10.0, 0.0), Point2::new(10.0, 10.0)),
        quadratic_nurbs(
            "top",
            [
                Point2::new(10.0, 10.0),
                Point2::new(5.0, 12.0),
                Point2::new(0.0, 10.0),
            ],
            Some(vec![1.0, 0.75, 1.0]),
        ),
        line("left", Point2::new(0.0, 10.0), Point2::new(0.0, 0.0)),
    ]
}

fn sketch(profiles: &[&[SketchEntity]]) -> Sketch {
    let profiles = profiles
        .iter()
        .map(|profile| {
            profile
                .iter()
                .map(|entity| SketchEntityUse {
                    entity: entity.id().clone(),
                    reversed: false,
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    Sketch {
        id: sketch_id(),
        name: None,
        configuration: None,
        visible: None,
        placement: cadmpeg_ir::sketches::SketchPlacement::try_resolved(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .unwrap(),
        profiles: cadmpeg_ir::sketches::SketchProfiles::try_from(profiles).unwrap(),
        native_ref: None,
    }
}

/// Resolve the region at `point` and require that no local cap skipped work.
fn region_at(
    sketch: &Sketch,
    entities: &[SketchEntity],
    point: Point2,
) -> Option<SketchProfileRegion> {
    let work = local_profile_work();
    let region = crate::test_support::with_decode_context(|decode_ctx| {
        region_containing_points(
            sketch,
            entities,
            &[Point3::new(point.u, point.v, 0.0)],
            EPS_EXACT_ARC_REGION,
            &work,
            decode_ctx,
        )
    })
    .unwrap();
    work.finish_selection();
    let losses = work.losses();
    assert!(losses.is_empty(), "unexpected losses {losses:?}");
    region
}

fn loops(outer: u32, holes: Vec<u32>) -> SketchProfileRegion {
    SketchProfileRegion::loops(outer, holes).unwrap()
}

#[test]
fn nurbs_bounded_profile_with_circular_hole_resolves_region_and_hole() {
    let outline = nurbs_topped_square();
    let hole = [circle("hole", Point2::new(5.0, 5.0), 2.0)];
    let sketch = sketch(&[&outline, &hole]);
    let entities = [outline.as_slice(), &hole].concat();

    assert_eq!(
        region_at(&sketch, &entities, Point2::new(1.0, 1.0)),
        Some(loops(0, vec![1]))
    );
    assert_eq!(
        region_at(&sketch, &entities, Point2::new(5.0, 5.0)),
        Some(loops(1, Vec::new()))
    );
}

#[test]
fn circle_around_nurbs_bounded_island_resolves_ring_and_island() {
    let rim = [circle("rim", Point2::new(5.0, 5.0), 20.0)];
    let island = nurbs_topped_square();
    let sketch = sketch(&[&rim, &island]);
    let entities = [rim.as_slice(), &island].concat();

    assert_eq!(
        region_at(&sketch, &entities, Point2::new(5.0, 20.0)),
        Some(loops(0, vec![1]))
    );
    assert_eq!(
        region_at(&sketch, &entities, Point2::new(1.0, 1.0)),
        Some(loops(1, Vec::new()))
    );
}

#[test]
fn nurbs_bounded_profile_with_slot_hole_resolves_region_and_hole() {
    use std::f64::consts::FRAC_PI_2;

    let outline = nurbs_topped_square();
    let slot = [
        line("slot-low", Point2::new(3.0, 4.0), Point2::new(7.0, 4.0)),
        arc(
            "slot-right",
            Point2::new(7.0, 5.0),
            1.0,
            -FRAC_PI_2,
            FRAC_PI_2,
        ),
        line("slot-high", Point2::new(7.0, 6.0), Point2::new(3.0, 6.0)),
        arc(
            "slot-left",
            Point2::new(3.0, 5.0),
            1.0,
            FRAC_PI_2,
            3.0 * FRAC_PI_2,
        ),
    ];
    let sketch = sketch(&[&outline, &slot]);
    let entities = [outline.as_slice(), &slot].concat();

    assert_eq!(
        region_at(&sketch, &entities, Point2::new(1.0, 1.0)),
        Some(loops(0, vec![1]))
    );
    assert_eq!(
        region_at(&sketch, &entities, Point2::new(5.0, 5.0)),
        Some(loops(1, Vec::new()))
    );
    assert_eq!(
        region_at(&sketch, &entities, Point2::new(7.5, 5.0)),
        Some(loops(1, Vec::new()))
    );
}

/// The arc side of the C-shaped loop is about 60 800 long, so a tube cover at
/// the NURBS target error of the loop would need several hundred thousand
/// tubes.
#[test]
fn nurbs_bounded_profile_with_large_arc_side_resolves_region_and_hole() {
    const RADIUS: f64 = 10_000.0;
    const GAP: f64 = 0.1;
    let end_angle = std::f64::consts::TAU - GAP;
    let on_rim = |angle: f64| Point2::new(RADIUS * angle.cos(), RADIUS * angle.sin());
    let outline = [
        arc("c-arc", Point2::new(0.0, 0.0), RADIUS, GAP, end_angle),
        quadratic_nurbs(
            "c-closing",
            [on_rim(end_angle), Point2::new(10_100.0, 0.0), on_rim(GAP)],
            None,
        ),
    ];
    let hole = [circle("c-hole", Point2::new(0.0, 0.0), 100.0)];
    let sketch = sketch(&[&outline, &hole]);
    let entities = [outline.as_slice(), &hole].concat();

    assert_eq!(
        region_at(&sketch, &entities, Point2::new(5_000.0, 0.0)),
        Some(loops(0, vec![1]))
    );
    assert_eq!(
        region_at(&sketch, &entities, Point2::new(0.0, 0.0)),
        Some(loops(1, Vec::new()))
    );
}

/// Each half of the rim is about 31 400 long, so covering it with tubes at
/// the arrangement's NURBS target error of about 0.1 would take some 314 000
/// tubes.
#[test]
fn arrangement_certifies_a_large_circle_around_a_nurbs_profile_exactly() {
    let rim = [circle("far-rim", Point2::new(5.0, 5.0), 10_000.0)];
    let island = nurbs_topped_square();
    let sketch = sketch(&[&rim, &island]);
    let entities = [rim.as_slice(), &island].concat();
    let work = local_profile_work();
    let faces = crate::test_support::with_decode_context(|decode_ctx| {
        sketch_arrangement_faces(&sketch, &entities, EPS_EXACT_ARC_REGION, &work, decode_ctx)
    })
    .unwrap();

    // The disc of the rim and the square.
    assert_eq!(faces.map(|faces| faces.len()), Some(2));
    work.finish_selection();
    assert!(work.losses().is_empty());
}
