// SPDX-License-Identifier: Apache-2.0

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::sketches::{
    Sketch, SketchEntity, SketchEntityId, SketchEntityUse, SketchGeometry,
    SketchGeometryDefinition, SketchId, SketchPlacement,
};

fn sketch(id: &SketchId, entity: &SketchEntityId) -> Sketch {
    Sketch {
        id: id.clone(),
        name: None,
        configuration: None,
        visible: None,
        placement: SketchPlacement::Unresolved {},
        profiles: cadmpeg_ir::sketches::SketchProfiles::try_from(vec![vec![SketchEntityUse {
            entity: entity.clone(),
            reversed: false,
        }]])
        .expect("valid test fixture"),
        native_ref: None,
    }
}

fn line_entity(id: &SketchEntityId, sketch: &SketchId, end: [f64; 2]) -> SketchEntity {
    SketchEntity::new(
        id.clone(),
        sketch.clone(),
        SketchGeometry::try_from(SketchGeometryDefinition::Line {
            start: Point2::new(0.0, 0.0),
            end: Point2::new(end[0], end[1]),
        })
        .expect("valid test fixture"),
    )
}

#[test]
fn profile_joins_reject_duplicate_sketch_ids() {
    let sketch_id = SketchId::mint("creo:model:sketch#7".to_string()).expect("valid test fixture");
    let entity_id = SketchEntityId::mint("creo:featdefs:sketch_entity#7:1".to_string())
        .expect("valid test fixture");
    let mut ir = CadIr::empty();
    ir.model.sketches.extend([
        sketch(&sketch_id, &entity_id),
        sketch(&sketch_id, &entity_id),
    ]);
    ir.model
        .sketch_entities
        .push(line_entity(&entity_id, &sketch_id, [1.0, 0.0]));

    assert!(super::connected_sketch_profile_vertices(&ir, &sketch_id).is_empty());
    assert!(super::resolved_sketch_profiles(&ir, &sketch_id, 1).is_none());
}

#[test]
fn profile_joins_reject_duplicate_sketch_entity_ids() {
    let sketch_id = SketchId::mint("creo:model:sketch#7".to_string()).expect("valid test fixture");
    let entity_id = SketchEntityId::mint("creo:featdefs:sketch_entity#7:1".to_string())
        .expect("valid test fixture");
    let mut ir = CadIr::empty();
    ir.model.sketches.push(sketch(&sketch_id, &entity_id));
    ir.model.sketch_entities.extend([
        line_entity(&entity_id, &sketch_id, [1.0, 0.0]),
        line_entity(&entity_id, &sketch_id, [0.0, 1.0]),
    ]);

    assert!(super::connected_sketch_profile_vertices(&ir, &sketch_id).is_empty());
    assert!(super::resolved_sketch_profiles(&ir, &sketch_id, 1).is_none());
}

#[test]
fn two_refused_circular_pcurves_state_two_records_each_naming_its_instance() {
    let mut refusal = crate::lane_refusal::LaneRefusals::new();
    let first = super::circular_pcurve(
        [f64::MAX, f64::MAX],
        f64::MAX,
        0.0,
        std::f64::consts::TAU,
        &"extrusion feature 11 cap",
        &mut refusal,
    );
    let second = super::circular_pcurve(
        [f64::MAX, f64::MAX],
        f64::MAX,
        0.0,
        std::f64::consts::TAU,
        &"extrusion feature 12 cap",
        &mut refusal,
    );
    assert!(first.is_none(), "the refused arc states no pcurve");
    assert!(second.is_none(), "the refused arc states no pcurve");
    let records = refusal.take_records();
    assert_eq!(records.len(), 2, "one record per refused arc: {records:?}");
    assert!(
        records[0].starts_with("creo circular pcurve record for extrusion feature 11 cap: "),
        "the record names the instance that stated the lanes: {}",
        records[0]
    );
    assert!(
        records[1].starts_with("creo circular pcurve record for extrusion feature 12 cap: "),
        "the record names the instance that stated the lanes: {}",
        records[1]
    );
}

#[test]
fn large_profile_circle_intersections_stay_finite() {
    let r = 1e200;
    let arc = ([0., 0.], r, 0., std::f64::consts::TAU);
    assert!(super::line_arc_intersect(
        [[-2. * r, 0.], [2. * r, 0.]],
        arc,
        1e-9
    ));
    assert!(super::arcs_intersect(
        arc,
        ([r, 0.], r, 0., std::f64::consts::TAU),
        1e-9
    ));
    assert!(!super::arcs_intersect(
        arc,
        ([3. * r, 0.], r, 0., std::f64::consts::TAU),
        1e-9
    ));
}

#[test]
fn small_segment_crossings_do_not_depend_on_cross_product_units() {
    const DISTANCE_TOLERANCE: f64 = 1e-9;
    for scale in [1e-5, 1.0, 1e200] {
        assert!(super::segments_intersect(
            [[-scale, 0.0], [scale, 0.0]],
            [[0.0, -scale], [0.0, scale]],
            DISTANCE_TOLERANCE
        ));
    }
    assert!(!super::segments_intersect(
        [[0.0, 0.0], [1e-5, 0.0]],
        [[0.0, 1e-5], [1e-5, 1e-5]],
        DISTANCE_TOLERANCE
    ));
}

#[test]
fn numerical_ranges_profile_arc_tolerance_is_a_length_at_both_ends() {
    const RADIUS: f64 = 1e-6;
    const TOLERANCE: f64 = 1e-9;
    for sweep in [-1.0_f64, 1.0] {
        for angle in [-0.0005 * sweep, 1.0005 * sweep] {
            let point = [RADIUS * angle.cos(), RADIUS * angle.sin()];
            assert!(super::point_on_profile_arc(
                point,
                ([0., 0.], RADIUS, 0., sweep),
                TOLERANCE
            ));
        }
        let angle = 1.01 * sweep;
        assert!(!super::point_on_profile_arc(
            [RADIUS * angle.cos(), RADIUS * angle.sin()],
            ([0., 0.], RADIUS, 0., sweep),
            TOLERANCE
        ));
    }
}

#[test]
fn audit_regression_line_arc_endpoint_tolerance_has_length_units() {
    let line = [[0., 0.], [1000., 0.]];
    let arc = |center| ([center, 0.], 0.1, 0., std::f64::consts::TAU);
    assert!(!super::line_arc_intersect(line, arc(1000.5), 0.001));
    assert!(super::line_arc_intersect(line, arc(1000.1005), 0.001));
    assert!(super::line_arc_intersect(line, arc(999.5), 0.001));
}
