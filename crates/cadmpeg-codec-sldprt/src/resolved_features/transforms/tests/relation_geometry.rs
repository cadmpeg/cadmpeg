//! Evaluated-geometry matching and locus-fallback tests.

use super::super::*;
use super::marker;
use crate::records::{SketchInputKind, SketchInputLink, SketchRelationKind};
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::sketches::{
    SketchConstraintDefinitionInput, SketchEntity, SketchEntityId, SketchGeometry,
    SketchGeometryDefinition, SketchId, SketchLocus,
};
use cadmpeg_ir::{
    features::{DesignParameter, FeatureId, ParameterId, ParameterValue},
    scalar::{Angle, Length},
};
use std::collections::{BTreeMap, HashMap, HashSet};

#[test]
fn binary_relations_require_matching_evaluated_geometry() {
    use SketchRelationKind::{
        Collinear, Concentric, Coradial, Equal, Parallel, Perpendicular, Tangent,
    };
    let sketch = SketchId::mint("synthetic:test:id#sketch").unwrap();
    let entity = |id: &str, geometry| {
        SketchEntity::new(SketchEntityId::mint(id).unwrap(), sketch.clone(), geometry)
    };
    let horizontal = entity(
        "synthetic:test:id#horizontal",
        SketchGeometry::try_from(SketchGeometryDefinition::Line {
            start: Point2::new(0.0, 0.0),
            end: Point2::new(4.0, 0.0),
        })
        .unwrap(),
    );
    let parallel = entity(
        "synthetic:test:id#parallel",
        SketchGeometry::try_from(SketchGeometryDefinition::Line {
            start: Point2::new(0.0, 2.0),
            end: Point2::new(4.0, 2.0),
        })
        .unwrap(),
    );
    let perpendicular = entity(
        "synthetic:test:id#perpendicular",
        SketchGeometry::try_from(SketchGeometryDefinition::Line {
            start: Point2::new(0.0, 0.0),
            end: Point2::new(0.0, 4.0),
        })
        .unwrap(),
    );
    let collinear = entity(
        "synthetic:test:id#collinear",
        SketchGeometry::try_from(SketchGeometryDefinition::Line {
            start: Point2::new(6.0, 0.0),
            end: Point2::new(10.0, 0.0),
        })
        .unwrap(),
    );
    let circle = |id: &str, u, v, radius| {
        entity(
            id,
            SketchGeometry::try_from(SketchGeometryDefinition::Circle {
                center: Point2::new(u, v),
                radius: Length::new(radius).unwrap(),
            })
            .unwrap(),
        )
    };
    let first_circle = circle("synthetic:test:id#first-circle", 0.0, 2.0, 2.0);
    let equal_circle = circle("synthetic:test:id#equal-circle", 4.0, 2.0, 2.0);
    let concentric_circle = circle("synthetic:test:id#concentric-circle", 0.0, 2.0, 1.0);
    let coradial_circle = circle("synthetic:test:id#coradial-circle", 0.0, 2.0, 2.0);
    let unrelated_circle = circle("synthetic:test:id#unrelated-circle", 8.0, 8.0, 3.0);

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
    let sketch = SketchId::mint("synthetic:test:id#sketch").unwrap();
    let entity = |id: &str, geometry| {
        SketchEntity::new(SketchEntityId::mint(id).unwrap(), sketch.clone(), geometry)
            .with_construction(true)
    };
    let mut first = entity(
        "synthetic:test:id#first",
        SketchGeometry::try_from(SketchGeometryDefinition::Point {
            position: Point2::new(0.0, 0.0),
        })
        .unwrap(),
    );
    let mut second = entity(
        "synthetic:test:id#second",
        SketchGeometry::try_from(SketchGeometryDefinition::Point {
            position: Point2::new(0.0, 0.0),
        })
        .unwrap(),
    );
    let line = entity(
        "synthetic:test:id#line",
        SketchGeometry::try_from(SketchGeometryDefinition::Line {
            start: Point2::new(-2.0, 0.0),
            end: Point2::new(2.0, 0.0),
        })
        .unwrap(),
    );
    let mut arc = entity(
        "synthetic:test:id#arc",
        SketchGeometry::try_from(SketchGeometryDefinition::Arc {
            center: Point2::new(0.0, 0.0),
            radius: Length::new(1.0).unwrap(),
            start_angle: cadmpeg_ir::scalar::Angle::new(0.0).unwrap(),
            end_angle: cadmpeg_ir::scalar::Angle::new(std::f64::consts::FRAC_PI_2).unwrap(),
        })
        .unwrap(),
    );
    let symmetric_first = entity(
        "synthetic:test:id#symmetric-first",
        SketchGeometry::try_from(SketchGeometryDefinition::Point {
            position: Point2::new(-1.0, 2.0),
        })
        .unwrap(),
    );
    let mut symmetric_second = entity(
        "synthetic:test:id#symmetric-second",
        SketchGeometry::try_from(SketchGeometryDefinition::Point {
            position: Point2::new(1.0, 2.0),
        })
        .unwrap(),
    );
    let symmetry_axis = entity(
        "synthetic:test:id#symmetry-axis",
        SketchGeometry::try_from(SketchGeometryDefinition::Line {
            start: Point2::new(0.0, -3.0),
            end: Point2::new(0.0, 3.0),
        })
        .unwrap(),
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
    coincident.links = crate::records::SketchInputLinks::new(
        0,
        [(&first_marker, 1), (&second_marker, 2)]
            .map(|(marker, local_id)| SketchInputLink {
                local_id,
                entity_ref: marker.id.clone(),
            })
            .to_vec(),
    );
    let mut merge_points = coincident.clone();
    merge_points.id = "merge-points".into();
    merge_points.kind = SketchInputKind::Relation(SketchRelationKind::MergePoints);
    let mut midpoint = marker("midpoint", None);
    midpoint.kind = SketchInputKind::Relation(SketchRelationKind::Midpoint);
    midpoint.links = crate::records::SketchInputLinks::new(
        0,
        [(&first_marker, 1), (&line_marker, 3)]
            .map(|(marker, local_id)| SketchInputLink {
                local_id,
                entity_ref: marker.id.clone(),
            })
            .to_vec(),
    );
    let mut arc_angle = marker("arc-angle", None);
    arc_angle.kind = SketchInputKind::Relation(SketchRelationKind::ArcAngle90);
    arc_angle.links = crate::records::SketchInputLinks::new(
        0,
        vec![SketchInputLink {
            local_id: 4,
            entity_ref: arc_marker.id.clone(),
        }],
    );
    let mut symmetric = marker("symmetric", None);
    symmetric.kind = SketchInputKind::Relation(SketchRelationKind::Symmetric);
    symmetric.links = crate::records::SketchInputLinks::new(
        0,
        [(&symmetric_first_marker, 5), (&symmetric_second_marker, 6)]
            .map(|(marker, local_id)| SketchInputLink {
                local_id,
                entity_ref: marker.id.clone(),
            })
            .to_vec(),
    );
    symmetry_axis_marker.links = crate::records::SketchInputLinks::new(
        0,
        symmetry_axis_marker
            .links()
            .iter()
            .cloned()
            .chain(std::iter::once(SketchInputLink {
                local_id: 7,
                entity_ref: symmetric.id.clone(),
            }))
            .collect(),
    );
    let mut at_intersection = marker("at-intersection", None);
    at_intersection.kind = SketchInputKind::Relation(SketchRelationKind::AtIntersection);
    at_intersection.links = crate::records::SketchInputLinks::new(
        0,
        [(&line_marker, 9), (&symmetry_axis_marker, 10)]
            .map(|(marker, local_id)| SketchInputLink {
                local_id,
                entity_ref: marker.id.clone(),
            })
            .to_vec(),
    );
    first_marker.links = crate::records::SketchInputLinks::new(
        0,
        first_marker
            .links()
            .iter()
            .cloned()
            .chain(std::iter::once(SketchInputLink {
                local_id: 8,
                entity_ref: at_intersection.id.clone(),
            }))
            .collect(),
    );
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
        Some(SketchConstraintDefinitionInput::CoincidentLoci { .. })
    ));
    assert!(matches!(
        typed_marker_relation_definition_in_sketch(
            &merge_points,
            &sketch,
            &[first.clone(), second.clone(), line.clone(), arc.clone()],
            &markers,
            &loci,
        ),
        Some(SketchConstraintDefinitionInput::CoincidentLoci { .. })
    ));
    assert!(matches!(
        typed_marker_relation_definition_in_sketch(
            &midpoint,
            &sketch,
            &[first.clone(), second.clone(), line.clone(), arc.clone()],
            &markers,
            &loci,
        ),
        Some(SketchConstraintDefinitionInput::Midpoint { .. })
    ));
    assert!(matches!(
        typed_marker_relation_definition_in_sketch(
            &arc_angle,
            &sketch,
            &[first.clone(), second.clone(), line.clone(), arc.clone()],
            &markers,
            &loci,
        ),
        Some(SketchConstraintDefinitionInput::ArcAngle { .. })
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
        Some(SketchConstraintDefinitionInput::Symmetric {
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
        Some(SketchConstraintDefinitionInput::AtIntersection {
            point: SketchLocus::Entity(first.id().clone()),
            first: line.id().clone(),
            second: symmetry_axis.id().clone(),
        })
    );

    second.geometry = SketchGeometry::try_from(SketchGeometryDefinition::Point {
        position: Point2::new(1.0, 0.0),
    })
    .unwrap();
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
        SketchConstraintDefinitionInput::CoincidentLoci { .. }
    ));
    assert!(marker_relation_is_inactive(
        &coincident,
        &definition,
        &[first.clone(), second.clone(), line.clone(), arc.clone()],
    ));
    first.clone_from(&entity(
        "synthetic:test:id#first",
        SketchGeometry::try_from(SketchGeometryDefinition::Point {
            position: Point2::new(1.0, 0.0),
        })
        .unwrap(),
    ));
    assert!(matches!(
        typed_marker_relation_definition_in_sketch(
            &at_intersection,
            &sketch,
            &[first.clone(), line.clone(), symmetry_axis.clone()],
            &markers,
            &loci,
        ),
        Some(SketchConstraintDefinitionInput::Native { .. })
    ));
    assert!(matches!(
        typed_marker_relation_definition_in_sketch(
            &midpoint,
            &sketch,
            &[first.clone(), second.clone(), line.clone(), arc.clone()],
            &markers,
            &loci,
        ),
        Some(SketchConstraintDefinitionInput::Native { .. })
    ));
    arc.geometry = SketchGeometry::try_from(SketchGeometryDefinition::Arc {
        center: Point2::new(0.0, 0.0),
        radius: Length::new(1.0).unwrap(),
        start_angle: cadmpeg_ir::scalar::Angle::new(0.0).unwrap(),
        end_angle: cadmpeg_ir::scalar::Angle::new(std::f64::consts::PI).unwrap(),
    })
    .unwrap();
    assert!(matches!(
        typed_marker_relation_definition_in_sketch(
            &arc_angle,
            &sketch,
            &[first.clone(), second.clone(), line.clone(), arc.clone()],
            &markers,
            &loci,
        ),
        Some(SketchConstraintDefinitionInput::Native { .. })
    ));
    symmetric_second.geometry = SketchGeometry::try_from(SketchGeometryDefinition::Point {
        position: Point2::new(2.0, 2.0),
    })
    .unwrap();
    assert!(matches!(
        typed_marker_relation_definition_in_sketch(
            &symmetric,
            &sketch,
            &[symmetric_first, symmetric_second, symmetry_axis],
            &markers,
            &loci,
        ),
        Some(SketchConstraintDefinitionInput::Native { .. })
    ));
}

#[test]
fn distance_pair_fallback_requires_one_pair_in_the_complete_sketch() {
    let sketch = SketchId::mint("synthetic:test:id#sketch").unwrap();
    let point = |id: &str, u: f64, v: f64| {
        SketchEntity::new(
            SketchEntityId::mint(id).unwrap(),
            sketch.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Point {
                position: Point2::new(u, v),
            })
            .unwrap(),
        )
    };
    let parameter = DesignParameter {
        id: ParameterId::mint("synthetic:test:id#distance").expect("identity grammar"),
        owner: Some(FeatureId::mint("synthetic:test:id#feature").expect("identity grammar")),
        ordinal: 0,
        name: "D1".into(),
        expression: "5mm".into(),
        display: None,
        value: Some(ParameterValue::Length(Length::new(5.0).unwrap())),
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: None,
    };
    let first = point("synthetic:test:id#first", 0.0, 0.0);
    let coincident_first = point("synthetic:test:id#z-coincident-first", 0.0, 0.0);
    let second = point("synthetic:test:id#second", 3.0, 4.0);
    let unrelated = point("synthetic:test:id#unrelated", 20.0, 20.0);
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

    let ambiguous = point("synthetic:test:id#ambiguous", 23.0, 24.0);
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
    let sketch = SketchId::mint("synthetic:test:id#sketch").unwrap();
    let point = |id: &str, u: f64, v: f64| {
        SketchEntity::new(
            SketchEntityId::mint(id).unwrap(),
            sketch.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Point {
                position: Point2::new(u, v),
            })
            .unwrap(),
        )
    };
    let first = point("synthetic:test:id#first", 0.0, 0.0);
    let second = point("synthetic:test:id#second", 5.0, 20.0);
    let unrelated = point("synthetic:test:id#unrelated", 100.0, 100.0);
    let parameter = DesignParameter {
        id: ParameterId::mint("synthetic:test:id#distance").expect("identity grammar"),
        owner: Some(FeatureId::mint("synthetic:test:id#feature").expect("identity grammar")),
        ordinal: 0,
        name: "D1".into(),
        expression: "5mm".into(),
        display: None,
        value: Some(ParameterValue::Length(Length::new(5.0).unwrap())),
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: None,
    };
    let first_locus = SketchLocus::Entity(first.id().clone());
    let second_locus = SketchLocus::Entity(second.id().clone());
    let entities = [first.clone(), second.clone(), unrelated.clone()];
    assert_eq!(
        unique_profile_axis_distance_locus(
            &sketch,
            &first_locus,
            &parameter,
            &entities,
            ProfileAxis::U,
        ),
        Some(second_locus.clone())
    );
    assert_eq!(
        unique_profile_axis_distance_pair(&sketch, &parameter, &entities, ProfileAxis::U),
        Some((first_locus, second_locus))
    );

    let ambiguous = point("synthetic:test:id#ambiguous", 10.0, 30.0);
    assert_eq!(
        unique_profile_axis_distance_pair(
            &sketch,
            &parameter,
            &[first, second, unrelated, ambiguous],
            ProfileAxis::U,
        ),
        None
    );
}

#[test]
fn line_distance_fallback_requires_one_parallel_pair_in_the_complete_sketch() {
    let sketch = SketchId::mint("synthetic:test:id#sketch").unwrap();
    let line = |id: &str, start: Point2, end: Point2| {
        SketchEntity::new(
            SketchEntityId::mint(id).unwrap(),
            sketch.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Line { start, end }).unwrap(),
        )
    };
    let first = line(
        "synthetic:test:id#first",
        Point2::new(0.0, 0.0),
        Point2::new(10.0, 0.0),
    );
    let second = line(
        "synthetic:test:id#second",
        Point2::new(0.0, 5.0),
        Point2::new(10.0, 5.0),
    );
    let unrelated = line(
        "synthetic:test:id#unrelated",
        Point2::new(20.0, 20.0),
        Point2::new(21.0, 21.0),
    );
    let parameter = DesignParameter {
        id: ParameterId::mint("synthetic:test:id#distance").expect("identity grammar"),
        owner: Some(FeatureId::mint("synthetic:test:id#feature").expect("identity grammar")),
        ordinal: 0,
        name: "D1".into(),
        expression: "5mm".into(),
        display: None,
        value: Some(ParameterValue::Length(Length::new(5.0).unwrap())),
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
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

    let wrong = line(
        "synthetic:test:id#wrong",
        Point2::new(0.0, 2.0),
        Point2::new(10.0, 2.0),
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
                second.clone(),
                unrelated.clone(),
            ],
        ),
        Some((first.id().clone(), second.id().clone()))
    );

    let other_solved = line(
        "synthetic:test:id#other-solved",
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
        "synthetic:test:id#unrelated-first",
        Point2::new(20.0, 20.0),
        Point2::new(30.0, 20.0),
    );
    let unrelated_second = line(
        "synthetic:test:id#unrelated-second",
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

    let ambiguous = line(
        "synthetic:test:id#ambiguous",
        Point2::new(0.0, 10.0),
        Point2::new(10.0, 10.0),
    );
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
    let sketch = SketchId::mint("synthetic:test:id#sketch").unwrap();
    let line = |id: &str, start: Point2, end: Point2| {
        SketchEntity::new(
            SketchEntityId::mint(id).unwrap(),
            sketch.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Line { start, end }).unwrap(),
        )
    };
    let horizontal = line(
        "synthetic:test:id#horizontal",
        Point2::new(0.0, 0.0),
        Point2::new(10.0, 0.0),
    );
    let vertical = line(
        "synthetic:test:id#vertical",
        Point2::new(0.0, 0.0),
        Point2::new(0.0, 10.0),
    );
    let diagonal = line(
        "synthetic:test:id#diagonal",
        Point2::new(20.0, 20.0),
        Point2::new(21.0, 21.0),
    );
    let parameter = DesignParameter {
        id: ParameterId::mint("synthetic:test:id#angle").expect("identity grammar"),
        owner: Some(FeatureId::mint("synthetic:test:id#feature").expect("identity grammar")),
        ordinal: 0,
        name: "D1".into(),
        expression: "90deg".into(),
        display: None,
        value: Some(ParameterValue::Angle(
            Angle::new(std::f64::consts::FRAC_PI_2).unwrap(),
        )),
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
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
        "synthetic:test:id#wrong",
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

    let ambiguous = line(
        "synthetic:test:id#ambiguous",
        Point2::new(5.0, 0.0),
        Point2::new(5.0, 10.0),
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
                ambiguous.clone(),
            ],
        ),
        None
    );

    let unrelated_first = line(
        "synthetic:test:id#unrelated-first",
        Point2::new(0.0, 0.0),
        Point2::new(0.5, 3.0_f64.sqrt() * 0.5),
    );
    let unrelated_second = line(
        "synthetic:test:id#unrelated-second",
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
    let sketch = SketchId::mint("synthetic:test:id#sketch").unwrap();
    let point = SketchEntity::new(
        SketchEntityId::mint("synthetic:test:id#point").unwrap(),
        sketch.clone(),
        SketchGeometry::try_from(SketchGeometryDefinition::Point {
            position: Point2::new(0.0, 5.0),
        })
        .unwrap(),
    );
    let line = |id: &str, start: Point2, end: Point2| {
        SketchEntity::new(
            SketchEntityId::mint(id).unwrap(),
            sketch.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Line { start, end }).unwrap(),
        )
    };
    let horizontal = line(
        "synthetic:test:id#horizontal",
        Point2::new(0.0, 0.0),
        Point2::new(10.0, 0.0),
    );
    let unrelated = line(
        "synthetic:test:id#unrelated",
        Point2::new(100.0, 20.0),
        Point2::new(100.0, 30.0),
    );
    let parameter = DesignParameter {
        id: ParameterId::mint("synthetic:test:id#distance").expect("identity grammar"),
        owner: Some(FeatureId::mint("synthetic:test:id#feature").expect("identity grammar")),
        ordinal: 0,
        name: "D1".into(),
        expression: "5mm".into(),
        display: None,
        value: Some(ParameterValue::Length(Length::new(5.0).unwrap())),
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
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

    let wrong = line(
        "synthetic:test:id#wrong",
        Point2::new(0.0, 2.0),
        Point2::new(10.0, 2.0),
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
                horizontal.clone(),
                unrelated.clone(),
            ],
        ),
        Some((
            SketchLocus::Entity(point.id().clone()),
            horizontal.id().clone(),
        ))
    );

    let ambiguous = line(
        "synthetic:test:id#ambiguous",
        Point2::new(0.0, 10.0),
        Point2::new(10.0, 10.0),
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
                horizontal.clone(),
                ambiguous.clone(),
            ],
        ),
        None
    );

    let unrelated_point = SketchEntity::new(
        SketchEntityId::mint("synthetic:test:id#unrelated-point").unwrap(),
        point.sketch.clone(),
        SketchGeometry::try_from(SketchGeometryDefinition::Point {
            position: Point2::new(20.0, 25.0),
        })
        .unwrap(),
    )
    .with_construction(point.construction)
    .with_native_ref(point.native_ref.clone())
    .with_geometry_ref(point.geometry_ref.clone())
    .with_endpoint_refs(point.endpoint_refs.clone());
    let unrelated_line = line(
        "synthetic:test:id#unrelated-line",
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
    let sketch = SketchId::mint("synthetic:test:id#sketch").unwrap();
    let point = |id: &str, u: f64, v: f64| {
        SketchEntity::new(
            SketchEntityId::mint(id).unwrap(),
            sketch.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Point {
                position: Point2::new(u, v),
            })
            .unwrap(),
        )
    };
    let first_entity = point("synthetic:test:id#first-entity", 1.0, 2.0);
    let second_entity = point("synthetic:test:id#second-entity", 4.0, 2.0);
    let unrelated = point("synthetic:test:id#unrelated", 8.0, 9.0);
    let first = marker("first-marker", Some([0.001, 0.002]));
    let second = marker("second-marker", None);
    let collision = marker("collision-marker", Some([8.0, 9.0]));
    let mut relation = marker("relation", None);
    relation.kind = SketchInputKind::Relation(SketchRelationKind::Horizontal);
    relation = relation.with_test_identity(relation.object_index(), Some(7));
    relation = relation.with_test_identity(Some(7), relation.local_id());
    relation.links = crate::records::SketchInputLinks::new(
        0,
        vec![
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
        ],
    );
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
            ProfileAxis::U,
        ),
        Some(vec![
            SketchLocus::Entity(first_entity.id().clone()),
            SketchLocus::Entity(second_entity.id().clone()),
        ])
    );

    let ambiguous = point("synthetic:test:id#ambiguous", 6.0, 2.0);
    assert_eq!(
        unique_axis_aligned_linked_loci(
            &relation,
            &sketch,
            &[first_entity, second_entity, unrelated, ambiguous],
            &markers,
            &loci,
            ProfileAxis::U,
        ),
        None
    );
}

#[test]
fn fixed_relation_ignores_self_identifying_geometry_link() {
    let mut relation = marker("fixed", None);
    relation.kind = SketchInputKind::Relation(SketchRelationKind::Fixed);
    relation = relation.with_test_identity(relation.object_index(), Some(7));
    relation = relation.with_test_identity(Some(7), relation.local_id());
    relation.links = crate::records::SketchInputLinks::new(
        0,
        vec![
            SketchInputLink {
                local_id: 7,
                entity_ref: "collision".into(),
            },
            SketchInputLink {
                local_id: 2,
                entity_ref: "point".into(),
            },
        ],
    );
    let mut collision = marker("collision", Some([3.0, 4.0]));
    collision.kind = SketchInputKind::Point;
    let mut point = marker("point", Some([1.0, 2.0]));
    point.kind = SketchInputKind::Point;
    let markers = HashMap::from([
        (relation.id.as_str(), &relation),
        (collision.id.as_str(), &collision),
        (point.id.as_str(), &point),
    ]);
    let point_id = SketchEntityId::mint("synthetic:test:id#point-entity").unwrap();
    let loci = HashMap::from([(
        point.id.clone(),
        vec![SketchLocus::Entity(point_id.clone())],
    )]);
    let point_entity = SketchEntity::new(
        point_id.clone(),
        SketchId::mint("synthetic:test:id#sketch").unwrap(),
        SketchGeometry::try_from(SketchGeometryDefinition::Point {
            position: Point2::new(1.0, 2.0),
        })
        .unwrap(),
    )
    .with_native_ref(Some(point.id.clone()));

    assert_eq!(
        typed_marker_relation_definition_in_sketch(
            &relation,
            &SketchId::mint("synthetic:test:id#sketch").unwrap(),
            std::slice::from_ref(&point_entity),
            &markers,
            &loci,
        ),
        Some(SketchConstraintDefinitionInput::Fixed { entity: point_id })
    );
}

#[test]
fn relation_line_identity_ignores_self_identifying_geometry_link() {
    let sketch = SketchId::mint("synthetic:test:id#sketch").unwrap();
    let line_id = SketchEntityId::mint("synthetic:test:id#line").unwrap();
    let first_id = SketchEntityId::mint("synthetic:test:id#first").unwrap();
    let second_id = SketchEntityId::mint("synthetic:test:id#second").unwrap();
    let line = SketchEntity::new(
        line_id.clone(),
        sketch.clone(),
        SketchGeometry::try_from(SketchGeometryDefinition::Line {
            start: Point2::new(0.0, 0.0),
            end: Point2::new(2.0, 0.0),
        })
        .unwrap(),
    );
    let point_entity = |id: SketchEntityId, position: Point2| {
        SketchEntity::new(
            id,
            sketch.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Point { position }).unwrap(),
        )
        .with_construction(true)
    };
    let first_entity = point_entity(first_id.clone(), Point2::new(0.0, 0.0));
    let second_entity = point_entity(second_id.clone(), Point2::new(2.0, 0.0));
    let mut relation = marker("relation", None);
    relation.kind = SketchInputKind::Relation(SketchRelationKind::Distance);
    relation = relation.with_test_identity(relation.object_index(), Some(7));
    relation = relation.with_test_identity(Some(7), relation.local_id());
    relation.links = crate::records::SketchInputLinks::new(
        0,
        vec![
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
        ],
    );
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
    ambiguous.links = crate::records::SketchInputLinks::new(
        0,
        vec![SketchInputLink {
            local_id: 2,
            entity_ref: "linked".into(),
        }],
    );
    let linked = marker("linked", None);
    let markers = HashMap::from([
        (ambiguous.id.as_str(), &ambiguous),
        (linked.id.as_str(), &linked),
    ]);
    let expected = SketchLocus::Start(SketchEntityId::mint("synthetic:test:id#line-a").unwrap());
    let loci = HashMap::from([
        (
            ambiguous.id.clone(),
            vec![
                expected.clone(),
                SketchLocus::End(SketchEntityId::mint("synthetic:test:id#line-b").unwrap()),
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
        vec![SketchEntityId::mint("synthetic:test:id#line-a").unwrap()]
    );
}

#[test]
fn point_handle_does_not_inherit_a_constraint_sibling_locus() {
    let mut point = marker("point", None);
    point.links = crate::records::SketchInputLinks::new(
        0,
        vec![SketchInputLink {
            local_id: 0,
            entity_ref: "relation".into(),
        }],
    );
    let mut relation = marker("relation", None);
    relation.kind = SketchInputKind::Relation(SketchRelationKind::Distance);
    relation.links = crate::records::SketchInputLinks::new(
        0,
        vec![
            SketchInputLink {
                local_id: 1,
                entity_ref: point.id.clone(),
            },
            SketchInputLink {
                local_id: 3,
                entity_ref: "known".into(),
            },
        ],
    );
    let known = marker("known", None);
    let markers = HashMap::from([
        (point.id.as_str(), &point),
        (relation.id.as_str(), &relation),
        (known.id.as_str(), &known),
    ]);
    let loci = HashMap::from([(
        known.id.clone(),
        vec![SketchLocus::Start(
            SketchEntityId::mint("synthetic:test:id#line").unwrap(),
        )],
    )]);

    assert_eq!(
        resolved_marker_locus(&point.id, &markers, &loci, &mut HashSet::new()),
        None
    );
}
