// SPDX-License-Identifier: Apache-2.0

use super::super::MAX_SUBDIVISIONS;
use super::*;
use crate::loss::F3dLossCode;

const EPS_DENSIFICATION_REGION: f64 = 1.0e-6;

fn densification_loss_message(selections: usize) -> String {
    format!(
        "{selections} extrude profile selection(s) skipped a certified profile test because a curve span needed more than MAX_SUBDIVISIONS ({MAX_SUBDIVISIONS}) tubes."
    )
}

fn assert_one_densification_loss(work: &ProfileGeometryWork) {
    let losses = work.losses();
    let [loss] = losses.as_slice() else {
        panic!("expected one loss, got {losses:?}");
    };
    assert_eq!(
        loss.code,
        F3dLossCode::FeatureProfileDensificationCapped.kind()
    );
    assert_eq!(loss.message, densification_loss_message(1));
}

#[test]
fn subdivision_count_skips_a_span_past_max_subdivisions() {
    let work = super::local_profile_work();
    assert_eq!(
        super::super::subdivision_count(MAX_SUBDIVISIONS, 1.0, &work),
        Some(100_000)
    );
    work.finish_selection();
    assert!(work.losses().is_empty());

    assert_eq!(
        super::super::subdivision_count(MAX_SUBDIVISIONS + 1.0, 1.0, &work),
        None
    );
    work.finish_selection();
    assert_one_densification_loss(&work);
}

#[test]
fn certified_arc_tubes_skip_an_arc_past_max_subdivisions() {
    let work = super::local_profile_work();
    let radius = positive_radius(1.0);
    let within = super::super::certified_arc_tubes(
        Point2::new(0.0, 0.0),
        radius,
        0.0,
        MAX_SUBDIVISIONS,
        1.0,
        &work,
    );
    assert_eq!(within.map(|tubes| tubes.len()), Some(100_000));
    let past = super::super::certified_arc_tubes(
        Point2::new(0.0, 0.0),
        radius,
        0.0,
        MAX_SUBDIVISIONS + 1.0,
        1.0,
        &work,
    );
    assert!(past.is_none());
    work.finish_selection();
    assert_one_densification_loss(&work);
}

/// A triangle bounded by a NURBS and a line, so its boundary is a certified
/// loop, and optionally a circle inside it.
fn spline_triangle_sketch(with_circle: bool) -> (Sketch, Vec<SketchEntity>) {
    let sketch_id = SketchId::mint("synthetic:test:id#densification").unwrap();
    let spline_id = SketchEntityId::mint("synthetic:test:id#densification-spline").unwrap();
    let closing_id = SketchEntityId::mint("synthetic:test:id#densification-closing").unwrap();
    let circle_id = SketchEntityId::mint("synthetic:test:id#densification-circle").unwrap();
    let spline = PcurveNurbs::from_lanes(
        1,
        vec![0.0, 0.0, 0.5, 1.0, 1.0],
        vec![
            Point2::new(0.0, 0.0),
            Point2::new(4.0, 0.0),
            Point2::new(4.0, 4.0),
        ],
        None,
        false,
    )
    .unwrap();
    let mut entities = vec![
        SketchEntity::new(
            spline_id.clone(),
            sketch_id.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Nurbs { curve: spline }).unwrap(),
        ),
        SketchEntity::new(
            closing_id.clone(),
            sketch_id.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Line {
                start: Point2::new(4.0, 4.0),
                end: Point2::new(0.0, 0.0),
            })
            .unwrap(),
        ),
    ];
    let mut profiles = vec![vec![
        SketchEntityUse {
            entity: spline_id,
            reversed: false,
        },
        SketchEntityUse {
            entity: closing_id,
            reversed: false,
        },
    ]];
    if with_circle {
        entities.push(SketchEntity::new(
            circle_id.clone(),
            sketch_id.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Circle {
                center: Point2::new(3.0, 1.0),
                radius: Length::new(0.5).unwrap(),
            })
            .unwrap(),
        ));
        profiles.push(vec![SketchEntityUse {
            entity: circle_id,
            reversed: false,
        }]);
    }
    let sketch = Sketch {
        id: sketch_id,
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
    };
    (sketch, entities)
}

#[test]
fn region_containing_points_resolves_a_certified_loop_within_max_subdivisions() {
    let (sketch, entities) = spline_triangle_sketch(false);
    let work = super::local_profile_work();
    let region = crate::test_support::with_decode_context(|decode_ctx| {
        super::region_containing_points(
            &sketch,
            &entities,
            &[Point3::new(3.0, 1.0, 0.0)],
            EPS_DENSIFICATION_REGION,
            &work,
            decode_ctx,
        )
    })
    .unwrap();
    assert_eq!(
        region,
        Some(SketchProfileRegion::loops(0, Vec::new()).unwrap())
    );
    work.finish_selection();
    assert!(work.losses().is_empty());
}

#[test]
fn region_containing_points_skips_when_a_containment_test_passes_max_subdivisions() {
    let (sketch, entities) = spline_triangle_sketch(true);
    let work = super::local_profile_work();
    let region = crate::test_support::with_decode_context(|decode_ctx| {
        super::region_containing_points(
            &sketch,
            &entities,
            &[Point3::new(1.0, 0.5, 0.0)],
            EPS_DENSIFICATION_REGION,
            &work,
            decode_ctx,
        )
    })
    .unwrap();
    assert_eq!(region, None);
    work.finish_selection();
    assert_one_densification_loss(&work);
}
