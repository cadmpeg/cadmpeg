//! Evaluated-geometry matching and locus-fallback tests.

use super::super::*;
use super::marker;
use crate::records::{SketchInputKind, SketchInputLink, SketchRelationKind};
use cadmpeg_ir::features::{
    Angle, DesignParameter, FeatureId, Length, ParameterId, ParameterValue,
};
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::sketches::{
    SketchConstraintDefinition, SketchEntity, SketchEntityId, SketchGeometry, SketchId, SketchLocus,
};
use std::collections::{BTreeMap, HashMap, HashSet};

#[test]
fn binary_relations_require_matching_evaluated_geometry() {
    use SketchRelationKind::{
        Collinear, Concentric, Coradial, Equal, Parallel, Perpendicular, Tangent,
    };
    let sketch = SketchId("sketch".into());
    let entity =
        |id: &str, geometry| SketchEntity::new(SketchEntityId(id.into()), sketch.clone(), geometry);
    let horizontal = entity(
        "horizontal",
        SketchGeometry::Line {
            start: Point2::new(0.0, 0.0),
            end: Point2::new(4.0, 0.0),
        },
    );
    let parallel = entity(
        "parallel",
        SketchGeometry::Line {
            start: Point2::new(0.0, 2.0),
            end: Point2::new(4.0, 2.0),
        },
    );
    let perpendicular = entity(
        "perpendicular",
        SketchGeometry::Line {
            start: Point2::new(0.0, 0.0),
            end: Point2::new(0.0, 4.0),
        },
    );
    let collinear = entity(
        "collinear",
        SketchGeometry::Line {
            start: Point2::new(6.0, 0.0),
            end: Point2::new(10.0, 0.0),
        },
    );
    let circle = |id: &str, u, v, radius| {
        entity(
            id,
            SketchGeometry::Circle {
                center: Point2::new(u, v),
                radius: Length(radius),
            },
        )
    };
    let first_circle = circle("first-circle", 0.0, 2.0, 2.0);
    let equal_circle = circle("equal-circle", 4.0, 2.0, 2.0);
    let concentric_circle = circle("concentric-circle", 0.0, 2.0, 1.0);
    let coradial_circle = circle("coradial-circle", 0.0, 2.0, 2.0);
    let unrelated_circle = circle("unrelated-circle", 8.0, 8.0, 3.0);

    for (kind, first, second) in [
        (Parallel, &horizontal, &parallel),
        (Perpendicular, &horizontal, &perpendicular),
        (Collinear, &horizontal, &collinear),
        (Equal, &first_circle, &equal_circle),
        (Concentric, &first_circle, &concentric_circle),
        (Coradial, &first_circle, &coradial_circle),
        (Tangent, &horizontal, &first_circle),
        (Tangent, &first_circle, &equal_circle),
    ] {
        assert!(binary_relation_matches_evaluated_geometry(
            kind, first, second
        ));
    }
    for kind in [
        Parallel,
        Perpendicular,
        Collinear,
        Equal,
        Concentric,
        Tangent,
        Coradial,
    ] {
        assert!(!binary_relation_matches_evaluated_geometry(
            kind,
            &horizontal,
            &unrelated_circle,
        ));
    }
}

#[test]
fn locus_relations_require_matching_evaluated_geometry() {
    let sketch = SketchId("sketch".into());
    let entity = |id: &str, geometry| {
        SketchEntity::new(SketchEntityId(id.into()), sketch.clone(), geometry)
            .with_construction(true)
    };
    let mut first = entity(
        "first",
        SketchGeometry::Point {
            position: Point2::new(0.0, 0.0),
        },
    );
    let mut second = entity(
        "second",
        SketchGeometry::Point {
            position: Point2::new(0.0, 0.0),
        },
    );
    let line = entity(
        "line",
        SketchGeometry::Line {
            start: Point2::new(-2.0, 0.0),
            end: Point2::new(2.0, 0.0),
        },
    );
    let mut arc = entity(
        "arc",
        SketchGeometry::Arc {
            center: Point2::new(0.0, 0.0),
            radius: Length(1.0),
            start_angle: cadmpeg_ir::features::Angle(0.0),
            end_angle: cadmpeg_ir::features::Angle(std::f64::consts::FRAC_PI_2),
        },
    );
    let symmetric_first = entity(
        "symmetric-first",
        SketchGeometry::Point {
            position: Point2::new(-1.0, 2.0),
        },
    );
    let mut symmetric_second = entity(
        "symmetric-second",
        SketchGeometry::Point {
            position: Point2::new(1.0, 2.0),
        },
    );
    let symmetry_axis = entity(
        "symmetry-axis",
        SketchGeometry::Line {
            start: Point2::new(0.0, -3.0),
            end: Point2::new(0.0, 3.0),
        },
    );
    let mut first_marker = marker("first-marker", None);
    let second_marker = marker("second-marker", None);
    let mut line_marker = marker("line-marker", None);
    line_marker.kind = SketchInputKind::LineOrCircle;
    let mut arc_marker = marker("arc-marker", None);
    arc_marker.kind = SketchInputKind::Arc;
    let symmetric_first_marker = marker("symmetric-first-marker", None);
    let symmetric_second_marker = marker("symmetric-second-marker", None);
    let mut symmetry_axis_marker = marker("symmetry-axis-marker", None);
    symmetry_axis_marker.kind = SketchInputKind::LineOrCircle;
    let mut coincident = marker("coincident", None);
    coincident.kind = SketchInputKind::Relation(SketchRelationKind::Coincident);
    coincident.links = [(&first_marker, 1), (&second_marker, 2)]
        .map(|(marker, local_id)| SketchInputLink {
            local_id,
            entity_ref: marker.id.clone(),
        })
        .to_vec();
    let mut merge_points = coincident.clone();
    merge_points.id = "merge-points".into();
    merge_points.kind = SketchInputKind::Relation(SketchRelationKind::MergePoints);
    let mut midpoint = marker("midpoint", None);
    midpoint.kind = SketchInputKind::Relation(SketchRelationKind::Midpoint);
    midpoint.links = [(&first_marker, 1), (&line_marker, 3)]
        .map(|(marker, local_id)| SketchInputLink {
            local_id,
            entity_ref: marker.id.clone(),
        })
        .to_vec();
    let mut arc_angle = marker("arc-angle", None);
    arc_angle.kind = SketchInputKind::Relation(SketchRelationKind::ArcAngle90);
    arc_angle.links = vec![SketchInputLink {
        local_id: 4,
        entity_ref: arc_marker.id.clone(),
    }];
    let mut symmetric = marker("symmetric", None);
    symmetric.kind = SketchInputKind::Relation(SketchRelationKind::Symmetric);
    symmetric.links = [(&symmetric_first_marker, 5), (&symmetric_second_marker, 6)]
        .map(|(marker, local_id)| SketchInputLink {
            local_id,
            entity_ref: marker.id.clone(),
        })
        .to_vec();
    symmetry_axis_marker.links.push(SketchInputLink {
        local_id: 7,
        entity_ref: symmetric.id.clone(),
    });
    let mut at_intersection = marker("at-intersection", None);
    at_intersection.kind = SketchInputKind::Relation(SketchRelationKind::AtIntersection);
    at_intersection.links = [(&line_marker, 9), (&symmetry_axis_marker, 10)]
        .map(|(marker, local_id)| SketchInputLink {
            local_id,
            entity_ref: marker.id.clone(),
        })
        .to_vec();
    first_marker.links.push(SketchInputLink {
        local_id: 8,
        entity_ref: at_intersection.id.clone(),
    });
    let markers = HashMap::from([
        (first_marker.id.as_str(), &first_marker),
        (second_marker.id.as_str(), &second_marker),
        (line_marker.id.as_str(), &line_marker),
        (arc_marker.id.as_str(), &arc_marker),
        (symmetric_first_marker.id.as_str(), &symmetric_first_marker),
        (
            symmetric_second_marker.id.as_str(),
            &symmetric_second_marker,
        ),
        (symmetry_axis_marker.id.as_str(), &symmetry_axis_marker),
        (coincident.id.as_str(), &coincident),
        (merge_points.id.as_str(), &merge_points),
        (midpoint.id.as_str(), &midpoint),
        (arc_angle.id.as_str(), &arc_angle),
        (symmetric.id.as_str(), &symmetric),
        (at_intersection.id.as_str(), &at_intersection),
    ]);
    let loci = HashMap::from([
        (
            first_marker.id.clone(),
            vec![SketchLocus::Entity(first.id().clone())],
        ),
        (
            second_marker.id.clone(),
            vec![SketchLocus::Entity(second.id().clone())],
        ),
        (
            line_marker.id.clone(),
            vec![SketchLocus::Entity(line.id().clone())],
        ),
        (
            arc_marker.id.clone(),
            vec![SketchLocus::Entity(arc.id().clone())],
        ),
        (
            symmetric_first_marker.id.clone(),
            vec![SketchLocus::Entity(symmetric_first.id().clone())],
        ),
        (
            symmetric_second_marker.id.clone(),
            vec![SketchLocus::Entity(symmetric_second.id().clone())],
        ),
        (
            symmetry_axis_marker.id.clone(),
            vec![SketchLocus::Entity(symmetry_axis.id().clone())],
        ),
    ]);
    assert!(matches!(
        typed_marker_relation_definition_in_sketch(
            &coincident,
            &sketch,
            &[first.clone(), second.clone(), line.clone(), arc.clone()],
            &markers,
            &loci,
        ),
        Some(SketchConstraintDefinition::CoincidentLoci { .. })
    ));
    assert!(matches!(
        typed_marker_relation_definition_in_sketch(
            &merge_points,
            &sketch,
            &[first.clone(), second.clone(), line.clone(), arc.clone()],
            &markers,
            &loci,
        ),
        Some(SketchConstraintDefinition::CoincidentLoci { .. })
    ));
    assert!(matches!(
        typed_marker_relation_definition_in_sketch(
            &midpoint,
            &sketch,
            &[first.clone(), second.clone(), line.clone(), arc.clone()],
            &markers,
            &loci,
        ),
        Some(SketchConstraintDefinition::Midpoint { .. })
    ));
    assert!(matches!(
        typed_marker_relation_definition_in_sketch(
            &arc_angle,
            &sketch,
            &[first.clone(), second.clone(), line.clone(), arc.clone()],
            &markers,
            &loci,
        ),
        Some(SketchConstraintDefinition::ArcAngle { .. })
    ));
    assert_eq!(
        typed_marker_relation_definition_in_sketch(
            &symmetric,
            &sketch,
            &[
                symmetric_first.clone(),
                symmetric_second.clone(),
                symmetry_axis.clone(),
            ],
            &markers,
            &loci,
        ),
        Some(SketchConstraintDefinition::Symmetric {
            first: SketchLocus::Entity(symmetric_first.id().clone()),
            second: SketchLocus::Entity(symmetric_second.id().clone()),
            axis: symmetry_axis.id().clone(),
        })
    );
    assert_eq!(
        typed_marker_relation_definition_in_sketch(
            &at_intersection,
            &sketch,
            &[first.clone(), line.clone(), symmetry_axis.clone()],
            &markers,
            &loci,
        ),
        Some(SketchConstraintDefinition::AtIntersection {
            point: SketchLocus::Entity(first.id().clone()),
            first: line.id().clone(),
            second: symmetry_axis.id().clone(),
        })
    );

    second.geometry = SketchGeometry::Point {
        position: Point2::new(1.0, 0.0),
    };
    let definition = typed_marker_relation_definition_in_sketch(
        &coincident,
        &sketch,
        &[first.clone(), second.clone(), line.clone(), arc.clone()],
        &markers,
        &loci,
    )
    .expect("typed coincident relation");
    assert!(matches!(
        definition,
        SketchConstraintDefinition::CoincidentLoci { .. }
    ));
    assert!(marker_relation_is_inactive(
        &coincident,
        &definition,
        &[first.clone(), second.clone(), line.clone(), arc.clone()],
    ));
    first.clone_from(&entity(
        "first",
        SketchGeometry::Point {
            position: Point2::new(1.0, 0.0),
        },
    ));
    assert!(matches!(
        typed_marker_relation_definition_in_sketch(
            &at_intersection,
            &sketch,
            &[first.clone(), line.clone(), symmetry_axis.clone()],
            &markers,
            &loci,
        ),
        Some(SketchConstraintDefinition::Native { .. })
    ));
    assert!(matches!(
        typed_marker_relation_definition_in_sketch(
            &midpoint,
            &sketch,
            &[first.clone(), second.clone(), line.clone(), arc.clone()],
            &markers,
            &loci,
        ),
        Some(SketchConstraintDefinition::Native { .. })
    ));
    arc.geometry = SketchGeometry::Arc {
        center: Point2::new(0.0, 0.0),
        radius: Length(1.0),
        start_angle: cadmpeg_ir::features::Angle(0.0),
        end_angle: cadmpeg_ir::features::Angle(std::f64::consts::PI),
    };
    assert!(matches!(
        typed_marker_relation_definition_in_sketch(
            &arc_angle,
            &sketch,
            &[first.clone(), second.clone(), line.clone(), arc.clone()],
            &markers,
            &loci,
        ),
        Some(SketchConstraintDefinition::Native { .. })
    ));
    symmetric_second.geometry = SketchGeometry::Point {
        position: Point2::new(2.0, 2.0),
    };
    assert!(matches!(
        typed_marker_relation_definition_in_sketch(
            &symmetric,
            &sketch,
            &[symmetric_first, symmetric_second, symmetry_axis],
            &markers,
            &loci,
        ),
        Some(SketchConstraintDefinition::Native { .. })
    ));
}

#[test]
fn distance_pair_fallback_requires_one_pair_in_the_complete_sketch() {
    let sketch = SketchId("sketch".into());
    let point = |id: &str, u: f64, v: f64| {
        SketchEntity::new(
            SketchEntityId(id.into()),
            sketch.clone(),
            SketchGeometry::Point {
                position: Point2::new(u, v),
            },
        )
    };
    let parameter = DesignParameter {
        id: ParameterId("distance".into()),
        owner: Some(FeatureId("feature".into())),
        ordinal: 0,
        name: "D1".into(),
        expression: "5mm".into(),
        display: None,
        value: Some(ParameterValue::Length(Length(5.0))),
        dependencies: Vec::new(),
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: None,
    };
    let first = point("first", 0.0, 0.0);
    let coincident_first = point("z-coincident-first", 0.0, 0.0);
    let second = point("second", 3.0, 4.0);
    let unrelated = point("unrelated", 20.0, 20.0);
    assert_eq!(
        unique_profile_distance_loci_pair(
            &sketch,
            &parameter,
            &[
                first.clone(),
                coincident_first,
                second.clone(),
                unrelated.clone(),
            ],
        ),
        Some((
            SketchLocus::Entity(first.id().clone()),
            SketchLocus::Entity(second.id().clone()),
        ))
    );

    let ambiguous = point("ambiguous", 23.0, 24.0);
    assert_eq!(
        unique_profile_distance_loci_pair(
            &sketch,
            &parameter,
            &[first, second, unrelated, ambiguous],
        ),
        None
    );
}

#[test]
fn axis_distance_fallback_requires_one_pair_in_the_complete_sketch() {
    let sketch = SketchId("sketch".into());
    let point = |id: &str, u: f64, v: f64| {
        SketchEntity::new(
            SketchEntityId(id.into()),
            sketch.clone(),
            SketchGeometry::Point {
                position: Point2::new(u, v),
            },
        )
    };
    let first = point("first", 0.0, 0.0);
    let second = point("second", 5.0, 20.0);
    let unrelated = point("unrelated", 100.0, 100.0);
    let parameter = DesignParameter {
        id: ParameterId("distance".into()),
        owner: Some(FeatureId("feature".into())),
        ordinal: 0,
        name: "D1".into(),
        expression: "5mm".into(),
        display: None,
        value: Some(ParameterValue::Length(Length(5.0))),
        dependencies: Vec::new(),
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: None,
    };
    let first_locus = SketchLocus::Entity(first.id().clone());
    let second_locus = SketchLocus::Entity(second.id().clone());
    let entities = [first.clone(), second.clone(), unrelated.clone()];
    assert_eq!(
        unique_profile_axis_distance_locus(&sketch, &first_locus, &parameter, &entities, true,),
        Some(second_locus.clone())
    );
    assert_eq!(
        unique_profile_axis_distance_pair(&sketch, &parameter, &entities, true),
        Some((first_locus, second_locus))
    );

    let ambiguous = point("ambiguous", 10.0, 30.0);
    assert_eq!(
        unique_profile_axis_distance_pair(
            &sketch,
            &parameter,
            &[first, second, unrelated, ambiguous],
            true,
        ),
        None
    );
}

#[test]
fn line_distance_fallback_requires_one_parallel_pair_in_the_complete_sketch() {
    let sketch = SketchId("sketch".into());
    let line = |id: &str, start: Point2, end: Point2| {
        SketchEntity::new(
            SketchEntityId(id.into()),
            sketch.clone(),
            SketchGeometry::Line { start, end },
        )
    };
    let first = line("first", Point2::new(0.0, 0.0), Point2::new(10.0, 0.0));
    let second = line("second", Point2::new(0.0, 5.0), Point2::new(10.0, 5.0));
    let unrelated = line(
        "unrelated",
        Point2::new(20.0, 20.0),
        Point2::new(21.0, 21.0),
    );
    let parameter = DesignParameter {
        id: ParameterId("distance".into()),
        owner: Some(FeatureId("feature".into())),
        ordinal: 0,
        name: "D1".into(),
        expression: "5mm".into(),
        display: None,
        value: Some(ParameterValue::Length(Length(5.0))),
        dependencies: Vec::new(),
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: None,
    };
    let entities = [first.clone(), second.clone(), unrelated.clone()];
    assert_eq!(
        unique_profile_line_distance_entity(&sketch, first.id(), &parameter, &entities),
        Some(second.id().clone())
    );
    assert_eq!(
        unique_profile_line_distance_pair(&sketch, &parameter, &entities),
        Some((first.id().clone(), second.id().clone()))
    );

    let wrong = line("wrong", Point2::new(0.0, 2.0), Point2::new(10.0, 2.0));
    assert_eq!(
        unique_repaired_profile_line_distance_pair(
            &sketch,
            first.id(),
            wrong.id(),
            &parameter,
            &[
                first.clone(),
                wrong.clone(),
                second.clone(),
                unrelated.clone(),
            ],
        ),
        Some((first.id().clone(), second.id().clone()))
    );

    let other_solved = line(
        "other-solved",
        Point2::new(0.0, -5.0),
        Point2::new(10.0, -5.0),
    );
    assert_eq!(
        unique_repaired_profile_line_distance_pair(
            &sketch,
            first.id(),
            wrong.id(),
            &parameter,
            &[first.clone(), wrong.clone(), second.clone(), other_solved,],
        ),
        None
    );

    let unrelated_first = line(
        "unrelated-first",
        Point2::new(20.0, 20.0),
        Point2::new(30.0, 20.0),
    );
    let unrelated_second = line(
        "unrelated-second",
        Point2::new(20.0, 25.0),
        Point2::new(30.0, 25.0),
    );
    assert_eq!(
        unique_repaired_profile_line_distance_pair(
            &sketch,
            first.id(),
            wrong.id(),
            &parameter,
            &[
                first.clone(),
                wrong.clone(),
                unrelated_first,
                unrelated_second,
            ],
        ),
        None
    );

    let ambiguous = line("ambiguous", Point2::new(0.0, 10.0), Point2::new(10.0, 10.0));
    assert_eq!(
        unique_profile_line_distance_pair(
            &sketch,
            &parameter,
            &[first, second, unrelated, ambiguous],
        ),
        None
    );
}

#[test]
fn line_angle_fallback_requires_one_pair_in_the_complete_sketch() {
    let sketch = SketchId("sketch".into());
    let line = |id: &str, start: Point2, end: Point2| {
        SketchEntity::new(
            SketchEntityId(id.into()),
            sketch.clone(),
            SketchGeometry::Line { start, end },
        )
    };
    let horizontal = line("horizontal", Point2::new(0.0, 0.0), Point2::new(10.0, 0.0));
    let vertical = line("vertical", Point2::new(0.0, 0.0), Point2::new(0.0, 10.0));
    let diagonal = line("diagonal", Point2::new(20.0, 20.0), Point2::new(21.0, 21.0));
    let parameter = DesignParameter {
        id: ParameterId("angle".into()),
        owner: Some(FeatureId("feature".into())),
        ordinal: 0,
        name: "D1".into(),
        expression: "90deg".into(),
        display: None,
        value: Some(ParameterValue::Angle(Angle(std::f64::consts::FRAC_PI_2))),
        dependencies: Vec::new(),
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: None,
    };
    let entities = [horizontal.clone(), vertical.clone(), diagonal.clone()];
    assert_eq!(
        unique_profile_line_angle_entity(&sketch, horizontal.id(), &parameter, &entities),
        Some(vertical.id().clone())
    );
    assert_eq!(
        unique_profile_line_angle_pair(&sketch, &parameter, &entities),
        Some((horizontal.id().clone(), vertical.id().clone()))
    );

    let wrong = line(
        "wrong",
        Point2::new(0.0, 0.0),
        Point2::new(3.0_f64.sqrt(), 1.0),
    );
    assert_eq!(
        unique_repaired_profile_line_angle_pair(
            &sketch,
            horizontal.id(),
            wrong.id(),
            &parameter,
            &[
                horizontal.clone(),
                wrong.clone(),
                vertical.clone(),
                diagonal.clone(),
            ],
        ),
        Some((horizontal.id().clone(), vertical.id().clone()))
    );

    let ambiguous = line("ambiguous", Point2::new(5.0, 0.0), Point2::new(5.0, 10.0));
    assert_eq!(
        unique_repaired_profile_line_angle_pair(
            &sketch,
            horizontal.id(),
            wrong.id(),
            &parameter,
            &[
                horizontal.clone(),
                wrong.clone(),
                vertical.clone(),
                ambiguous.clone(),
            ],
        ),
        None
    );

    let unrelated_first = line(
        "unrelated-first",
        Point2::new(0.0, 0.0),
        Point2::new(0.5, 3.0_f64.sqrt() * 0.5),
    );
    let unrelated_second = line(
        "unrelated-second",
        Point2::new(0.0, 0.0),
        Point2::new(-3.0_f64.sqrt() * 0.5, 0.5),
    );
    assert_eq!(
        unique_repaired_profile_line_angle_pair(
            &sketch,
            horizontal.id(),
            wrong.id(),
            &parameter,
            &[
                horizontal.clone(),
                wrong.clone(),
                unrelated_first,
                unrelated_second,
            ],
        ),
        None
    );
    assert_eq!(
        unique_profile_line_angle_pair(
            &sketch,
            &parameter,
            &[horizontal, vertical, diagonal, ambiguous],
        ),
        None
    );
}

#[test]
fn point_line_fallback_requires_one_pair_in_the_complete_sketch() {
    let sketch = SketchId("sketch".into());
    let point = SketchEntity::new(
        SketchEntityId("point".into()),
        sketch.clone(),
        SketchGeometry::Point {
            position: Point2::new(0.0, 5.0),
        },
    );
    let line = |id: &str, start: Point2, end: Point2| {
        SketchEntity::new(
            SketchEntityId(id.into()),
            sketch.clone(),
            SketchGeometry::Line { start, end },
        )
    };
    let horizontal = line("horizontal", Point2::new(0.0, 0.0), Point2::new(10.0, 0.0));
    let unrelated = line(
        "unrelated",
        Point2::new(100.0, 20.0),
        Point2::new(100.0, 30.0),
    );
    let parameter = DesignParameter {
        id: ParameterId("distance".into()),
        owner: Some(FeatureId("feature".into())),
        ordinal: 0,
        name: "D1".into(),
        expression: "5mm".into(),
        display: None,
        value: Some(ParameterValue::Length(Length(5.0))),
        dependencies: Vec::new(),
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: None,
    };
    let point_locus = SketchLocus::Entity(point.id().clone());
    let entities = [point.clone(), horizontal.clone(), unrelated.clone()];
    assert_eq!(
        unique_profile_point_line_entity(&sketch, &point_locus, &parameter, &entities),
        Some(horizontal.id().clone())
    );
    assert_eq!(
        unique_profile_line_point_locus(&sketch, horizontal.id(), &parameter, &entities),
        Some(point_locus.clone())
    );
    assert_eq!(
        unique_profile_point_line_pair(&sketch, &parameter, &entities),
        Some((point_locus, horizontal.id().clone()))
    );

    let wrong = line("wrong", Point2::new(0.0, 2.0), Point2::new(10.0, 2.0));
    assert_eq!(
        unique_repaired_profile_point_line_pair(
            &sketch,
            &SketchLocus::Entity(point.id().clone()),
            wrong.id(),
            &parameter,
            &[
                point.clone(),
                wrong.clone(),
                horizontal.clone(),
                unrelated.clone(),
            ],
        ),
        Some((
            SketchLocus::Entity(point.id().clone()),
            horizontal.id().clone(),
        ))
    );

    let ambiguous = line("ambiguous", Point2::new(0.0, 10.0), Point2::new(10.0, 10.0));
    assert_eq!(
        unique_repaired_profile_point_line_pair(
            &sketch,
            &SketchLocus::Entity(point.id().clone()),
            wrong.id(),
            &parameter,
            &[
                point.clone(),
                wrong.clone(),
                horizontal.clone(),
                ambiguous.clone(),
            ],
        ),
        None
    );

    let unrelated_point = SketchEntity::new(
        SketchEntityId("unrelated-point".into()),
        point.sketch.clone(),
        SketchGeometry::Point {
            position: Point2::new(20.0, 25.0),
        },
    )
    .with_construction(point.construction)
    .with_native_ref(point.native_ref.clone())
    .with_geometry_ref(point.geometry_ref.clone())
    .with_endpoint_refs(point.endpoint_refs.clone());
    let unrelated_line = line(
        "unrelated-line",
        Point2::new(20.0, 20.0),
        Point2::new(30.0, 20.0),
    );
    assert_eq!(
        unique_repaired_profile_point_line_pair(
            &sketch,
            &SketchLocus::Entity(point.id().clone()),
            wrong.id(),
            &parameter,
            &[
                point.clone(),
                wrong.clone(),
                unrelated_point,
                unrelated_line,
            ],
        ),
        None
    );
    assert_eq!(
        unique_profile_point_line_pair(
            &sketch,
            &parameter,
            &[point, horizontal, unrelated, ambiguous],
        ),
        None
    );
}

#[test]
fn axis_relation_fallback_requires_one_aligned_locus_in_the_complete_sketch() {
    let sketch = SketchId("sketch".into());
    let point = |id: &str, u: f64, v: f64| {
        SketchEntity::new(
            SketchEntityId(id.into()),
            sketch.clone(),
            SketchGeometry::Point {
                position: Point2::new(u, v),
            },
        )
    };
    let first_entity = point("first-entity", 1.0, 2.0);
    let second_entity = point("second-entity", 4.0, 2.0);
    let unrelated = point("unrelated", 8.0, 9.0);
    let first = marker("first-marker", Some([0.001, 0.002]));
    let second = marker("second-marker", None);
    let collision = marker("collision-marker", Some([8.0, 9.0]));
    let mut relation = marker("relation", None);
    relation.kind = SketchInputKind::Relation(SketchRelationKind::Horizontal);
    relation.local_id = Some(7);
    relation.object_index = Some(7);
    relation.links = vec![
        SketchInputLink {
            local_id: 7,
            entity_ref: collision.id.clone(),
        },
        SketchInputLink {
            local_id: 1,
            entity_ref: first.id.clone(),
        },
        SketchInputLink {
            local_id: 2,
            entity_ref: second.id.clone(),
        },
    ];
    let markers = HashMap::from([
        (first.id.as_str(), &first),
        (second.id.as_str(), &second),
        (collision.id.as_str(), &collision),
    ]);
    let loci = HashMap::from([(
        first.id.clone(),
        vec![SketchLocus::Entity(first_entity.id().clone())],
    )]);
    assert_eq!(
        unique_axis_aligned_linked_loci(
            &relation,
            &sketch,
            &[
                first_entity.clone(),
                second_entity.clone(),
                unrelated.clone()
            ],
            &markers,
            &loci,
            true,
        ),
        Some(vec![
            SketchLocus::Entity(first_entity.id().clone()),
            SketchLocus::Entity(second_entity.id().clone()),
        ])
    );

    let ambiguous = point("ambiguous", 6.0, 2.0);
    assert_eq!(
        unique_axis_aligned_linked_loci(
            &relation,
            &sketch,
            &[first_entity, second_entity, unrelated, ambiguous],
            &markers,
            &loci,
            true,
        ),
        None
    );
}

#[test]
fn fixed_relation_ignores_self_identifying_geometry_link() {
    let mut relation = marker("fixed", None);
    relation.kind = SketchInputKind::Relation(SketchRelationKind::Fixed);
    relation.local_id = Some(7);
    relation.object_index = Some(7);
    relation.links = vec![
        SketchInputLink {
            local_id: 7,
            entity_ref: "collision".into(),
        },
        SketchInputLink {
            local_id: 2,
            entity_ref: "point".into(),
        },
    ];
    let mut collision = marker("collision", Some([3.0, 4.0]));
    collision.kind = SketchInputKind::Point;
    let mut point = marker("point", Some([1.0, 2.0]));
    point.kind = SketchInputKind::Point;
    let markers = HashMap::from([
        (relation.id.as_str(), &relation),
        (collision.id.as_str(), &collision),
        (point.id.as_str(), &point),
    ]);
    let point_id = SketchEntityId("point-entity".into());
    let loci = HashMap::from([(
        point.id.clone(),
        vec![SketchLocus::Entity(point_id.clone())],
    )]);
    let point_entity = SketchEntity::new(
        point_id.clone(),
        SketchId("sketch".into()),
        SketchGeometry::Point {
            position: Point2::new(1.0, 2.0),
        },
    )
    .with_native_ref(Some(point.id.clone()));

    assert_eq!(
        typed_marker_relation_definition_in_sketch(
            &relation,
            &SketchId("sketch".into()),
            std::slice::from_ref(&point_entity),
            &markers,
            &loci,
        ),
        Some(SketchConstraintDefinition::Fixed { entity: point_id })
    );
}

#[test]
fn relation_line_identity_ignores_self_identifying_geometry_link() {
    let sketch = SketchId("sketch".into());
    let line_id = SketchEntityId("line".into());
    let first_id = SketchEntityId("first".into());
    let second_id = SketchEntityId("second".into());
    let line = SketchEntity::new(
        line_id.clone(),
        sketch.clone(),
        SketchGeometry::Line {
            start: Point2::new(0.0, 0.0),
            end: Point2::new(2.0, 0.0),
        },
    );
    let point_entity = |id: SketchEntityId, position: Point2| {
        SketchEntity::new(id, sketch.clone(), SketchGeometry::Point { position })
            .with_construction(true)
    };
    let first_entity = point_entity(first_id.clone(), Point2::new(0.0, 0.0));
    let second_entity = point_entity(second_id.clone(), Point2::new(2.0, 0.0));
    let mut relation = marker("relation", None);
    relation.kind = SketchInputKind::Relation(SketchRelationKind::Distance);
    relation.local_id = Some(7);
    relation.object_index = Some(7);
    relation.links = vec![
        SketchInputLink {
            local_id: 7,
            entity_ref: "collision".into(),
        },
        SketchInputLink {
            local_id: 1,
            entity_ref: "first-marker".into(),
        },
        SketchInputLink {
            local_id: 2,
            entity_ref: "second-marker".into(),
        },
    ];
    let collision = marker("collision", Some([8.0, 9.0]));
    let first_marker = marker("first-marker", Some([0.0, 0.0]));
    let second_marker = marker("second-marker", Some([2.0, 0.0]));
    let markers = HashMap::from([
        (relation.id.as_str(), &relation),
        (collision.id.as_str(), &collision),
        (first_marker.id.as_str(), &first_marker),
        (second_marker.id.as_str(), &second_marker),
    ]);
    let loci = HashMap::from([
        (first_marker.id.clone(), vec![SketchLocus::Entity(first_id)]),
        (
            second_marker.id.clone(),
            vec![SketchLocus::Entity(second_id)],
        ),
    ]);

    assert_eq!(
        single_marker_line_entity(
            &relation.id,
            &markers,
            &loci,
            &[line, first_entity, second_entity],
        ),
        Some(line_id)
    );
}

#[test]
fn linked_locus_disambiguates_a_coordinate_collision() {
    let mut ambiguous = marker("ambiguous", None);
    ambiguous.links = vec![SketchInputLink {
        local_id: 2,
        entity_ref: "linked".into(),
    }];
    let linked = marker("linked", None);
    let markers = HashMap::from([
        (ambiguous.id.as_str(), &ambiguous),
        (linked.id.as_str(), &linked),
    ]);
    let expected = SketchLocus::Start(SketchEntityId("line-a".into()));
    let loci = HashMap::from([
        (
            ambiguous.id.clone(),
            vec![
                expected.clone(),
                SketchLocus::End(SketchEntityId("line-b".into())),
            ],
        ),
        (linked.id.clone(), vec![expected.clone()]),
    ]);

    assert_eq!(
        resolved_marker_locus(&ambiguous.id, &markers, &loci, &mut HashSet::new()),
        Some(expected)
    );
    assert_eq!(
        marker_entities(&ambiguous.id, &markers, &loci),
        vec![SketchEntityId("line-a".into())]
    );
}

#[test]
fn point_handle_does_not_inherit_a_constraint_sibling_locus() {
    let mut point = marker("point", None);
    point.links = vec![SketchInputLink {
        local_id: 0,
        entity_ref: "relation".into(),
    }];
    let mut relation = marker("relation", None);
    relation.kind = SketchInputKind::Relation(SketchRelationKind::Distance);
    relation.links = vec![
        SketchInputLink {
            local_id: 1,
            entity_ref: point.id.clone(),
        },
        SketchInputLink {
            local_id: 3,
            entity_ref: "known".into(),
        },
    ];
    let known = marker("known", None);
    let markers = HashMap::from([
        (point.id.as_str(), &point),
        (relation.id.as_str(), &relation),
        (known.id.as_str(), &known),
    ]);
    let loci = HashMap::from([(
        known.id.clone(),
        vec![SketchLocus::Start(SketchEntityId("line".into()))],
    )]);

    assert_eq!(
        resolved_marker_locus(&point.id, &markers, &loci, &mut HashSet::new()),
        None
    );
}
