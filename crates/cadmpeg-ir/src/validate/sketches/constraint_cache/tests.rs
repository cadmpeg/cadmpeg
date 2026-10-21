// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::features::{DesignParameter, ParameterValue};
use crate::scalar::{Angle, Length};
use crate::sketches::SketchDistanceMeasurement;

fn id(name: &str) -> SketchEntityId {
    format!("test:model:entity#{name}").try_into().unwrap()
}

fn line(start: Point2, end: Point2) -> SketchGeometry {
    Geometry::Line { start, end }.try_into().unwrap()
}

fn circle(center: Point2, radius: f64) -> SketchGeometry {
    Geometry::Circle {
        center,
        radius: Length::new(radius).unwrap(),
    }
    .try_into()
    .unwrap()
}

fn check_with(
    ir: &CadIr,
    definition: &Constraint,
    entities: &[(&SketchEntityId, &SketchGeometry)],
) -> Option<&'static str> {
    let geometry = BorrowedIdentities::build(|add| {
        for (id, geometry) in entities {
            add(id.as_str(), geometry);
        }
    });
    check(ir, definition, &geometry)
}

#[test]
fn concentric_coradial_and_tangent_constraints_measure_distinct_facts() {
    let ir = CadIr::empty();
    let first = id("first");
    let second = id("second");
    let outer = circle(Point2::new(0.0, 0.0), 2.0);
    let inner = circle(Point2::new(0.0, 0.0), 1.0);
    let displaced = circle(Point2::new(3.0, 0.0), 1.0);
    let tangent_line = line(Point2::new(-3.0, 2.0), Point2::new(3.0, 2.0));
    let crossing_line = line(Point2::new(-3.0, 1.0), Point2::new(3.0, 1.0));
    let operands = || (first.clone(), second.clone());
    let (first_id, second_id) = operands();
    assert!(check_with(
        &ir,
        &Constraint::Concentric {
            first: first_id,
            second: second_id
        },
        &[(&first, &outer), (&second, &inner)]
    )
    .is_none());
    let (first_id, second_id) = operands();
    assert!(check_with(
        &ir,
        &Constraint::Coradial {
            first: first_id,
            second: second_id
        },
        &[(&first, &outer), (&second, &inner)]
    )
    .is_some());
    for other in [&displaced, &tangent_line] {
        let (first_id, second_id) = operands();
        assert!(check_with(
            &ir,
            &Constraint::Tangent {
                first: first_id,
                second: second_id
            },
            &[(&first, &outer), (&second, other)]
        )
        .is_none());
    }
    let (first_id, second_id) = operands();
    assert!(check_with(
        &ir,
        &Constraint::Tangent {
            first: first_id,
            second: second_id
        },
        &[(&first, &outer), (&second, &crossing_line)]
    )
    .is_some());
    let (first_id, second_id) = operands();
    assert!(check_with(
        &ir,
        &Constraint::Curvature {
            first: first_id,
            second: second_id
        },
        &[(&first, &outer), (&second, &tangent_line)]
    )
    .is_some());
}

#[test]
fn tangent_loci_check_contact_position_and_local_direction() {
    let ir = CadIr::empty();
    let first = id("first");
    let second = id("second");
    let arc: SketchGeometry = Geometry::Arc {
        center: Point2::new(0.0, 0.0),
        radius: Length::new(1.0).unwrap(),
        start_angle: Angle::new(0.0).unwrap(),
        end_angle: Angle::new(std::f64::consts::FRAC_PI_2).unwrap(),
    }
    .try_into()
    .unwrap();
    let tangent = line(Point2::new(1.0, 0.0), Point2::new(1.0, 2.0));
    let crossing = line(Point2::new(1.0, 0.0), Point2::new(2.0, 0.0));
    let definition = || Constraint::TangentLoci {
        first: SketchLocus::Start(first.clone()),
        second: SketchLocus::Start(second.clone()),
    };
    assert!(check_with(&ir, &definition(), &[(&first, &arc), (&second, &tangent)]).is_none());
    assert!(check_with(&ir, &definition(), &[(&first, &arc), (&second, &crossing)]).is_some());
}

#[test]
fn repeated_distance_reads_the_canonical_parameter_and_per_measurement_axis() {
    let mut ir = CadIr::empty();
    let first = id("first");
    let second = id("second");
    let point = |u, v| -> SketchGeometry {
        Geometry::Point {
            position: Point2::new(u, v),
        }
        .try_into()
        .unwrap()
    };
    let origin = point(0.0, 0.0);
    let end = point(3.0, 4.0);
    let parameter = "test:model:parameter#distance".try_into().unwrap();
    ir.model.parameters.push(DesignParameter {
        id: parameter,
        owner: None,
        ordinal: 0,
        name: "distance".into(),
        expression: ParameterValue::Length(Length::new(5.0).unwrap()).literal_expression(),
        driving: true,
        display: None,
        value: Some(ParameterValue::Length(Length::new(5.0).unwrap())),
        properties: std::collections::BTreeMap::default(),
        pmi: None,
        native_ref: None,
    });
    let definition = Constraint::RepeatedDistance {
        measurements: vec![SketchDistanceMeasurement::Distance {
            first: SketchLocus::Entity(first.clone()),
            second: SketchLocus::Entity(second.clone()),
        }],
        parameter: Dimension::Parameter(ir.model.parameters[0].id.clone()),
    };
    let entities = [(&first, &origin), (&second, &end)];
    assert!(check_with(&ir, &definition, &entities).is_none());
    ir.model.parameters[0].value = Some(ParameterValue::Length(Length::new(4.0).unwrap()));
    assert!(check_with(&ir, &definition, &entities).is_some());
    ir.model.parameters[0].value = None;
    assert!(check_with(&ir, &definition, &entities).is_none());
}

#[test]
fn cache_checks_use_the_declared_tolerance_and_do_not_measure_arc_chords_as_lines() {
    let mut ir = CadIr::empty();
    ir.tolerances.linear = crate::scalar::PositiveLength::new(1.0e-12).unwrap();
    assert!(!close(&ir, 1.0, 1.0 + 1.0e-10));
    let entity = id("arc");
    let arc: SketchGeometry = Geometry::Arc {
        center: Point2::new(0.0, 0.0),
        radius: Length::new(1.0).unwrap(),
        start_angle: Angle::new(0.0).unwrap(),
        end_angle: Angle::new(std::f64::consts::FRAC_PI_2).unwrap(),
    }
    .try_into()
    .unwrap();
    assert!(check_with(
        &ir,
        &Constraint::Horizontal {
            entity: entity.clone()
        },
        &[(&entity, &arc)]
    )
    .is_none());
}

#[test]
fn circular_pattern_distinguishes_periodic_span_from_inclusive_span_and_spacing() {
    use crate::features::{CircularPatternDistribution, PatternAngle};
    use crate::sketches::{SketchCircularPattern, SketchCircularPatternInstance};
    let ir = CadIr::empty();
    let center = id("center");
    let seed = id("seed");
    let first = id("first");
    let second = id("second");
    let point = |u, v| -> SketchGeometry {
        Geometry::Point {
            position: Point2::new(u, v),
        }
        .try_into()
        .unwrap()
    };
    let origin = point(0.0, 0.0);
    let initial = point(1.0, 0.0);
    let first_point = point(-0.5, 3.0_f64.sqrt() / 2.0);
    let second_point = point(-0.5, -3.0_f64.sqrt() / 2.0);
    let instances = vec![
        SketchCircularPatternInstance {
            angle: crate::scalar::NonZeroAngle::new(std::f64::consts::TAU / 3.0).unwrap(),
            entities: vec![first.clone()],
        },
        SketchCircularPatternInstance {
            angle: crate::scalar::NonZeroAngle::new(2.0 * std::f64::consts::TAU / 3.0).unwrap(),
            entities: vec![second.clone()],
        },
    ];
    let pattern = |angle| Constraint::CircularPattern {
        pattern: SketchCircularPattern::new(
            center.clone(),
            angle,
            Dimension::Literal(3),
            vec![seed.clone()],
            instances.clone(),
        )
        .unwrap(),
    };
    let geometry = [
        (&center, &origin),
        (&seed, &initial),
        (&first, &first_point),
        (&second, &second_point),
    ];
    let angle = Dimension::Literal(Angle::new(std::f64::consts::TAU).unwrap());
    assert!(check_with(
        &ir,
        &pattern(PatternAngle::Span {
            angle: angle.clone(),
            distribution: CircularPatternDistribution::Periodic,
        }),
        &geometry
    )
    .is_none());
    assert!(check_with(
        &ir,
        &pattern(PatternAngle::Span {
            angle: angle.clone(),
            distribution: CircularPatternDistribution::Inclusive,
        }),
        &geometry
    )
    .is_some());
    assert!(check_with(&ir, &pattern(PatternAngle::Spacing { angle }), &geometry).is_some());
    let moved = point(0.0, 1.0);
    let wrong_geometry = [
        (&center, &origin),
        (&seed, &initial),
        (&first, &moved),
        (&second, &second_point),
    ];
    assert!(check_with(
        &ir,
        &pattern(PatternAngle::Span {
            angle: Dimension::Literal(Angle::new(std::f64::consts::TAU).unwrap()),
            distribution: CircularPatternDistribution::Periodic,
        }),
        &wrong_geometry
    )
    .is_some());
}
