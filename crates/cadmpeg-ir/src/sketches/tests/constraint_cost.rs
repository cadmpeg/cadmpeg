// SPDX-License-Identifier: Apache-2.0

use crate::features::ParameterId;
use crate::scalar::{Angle, Length, NonZeroAngle, PositiveAngle};
use crate::sketches::{
    NativeOperandField, OffsetParameter, SketchAxis, SketchCircularPattern,
    SketchCircularPatternInstance, SketchConstraint, SketchConstraintDefinition,
    SketchConstraintDefinitionInput, SketchConstraintId, SketchCoordinateAxis,
    SketchDistanceMeasurement, SketchDistancePair, SketchEntityId, SketchId,
    SketchInternalAlignment, SketchLocus, SketchNativeOperand, SketchOffsetPair,
    SketchPatternDirection, SketchPatternDistance, SketchPatternInstance, SketchPolygon,
    SketchRectangularPattern, SketchSameCoordinate,
};
use crate::transform::Transform;
use cadmpeg_core::decode::cost::DecodeCost;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::text::NonBlankString;
use std::collections::BTreeMap;

const TAG_BYTES: u64 = 1;

fn entity_id(suffix: &str) -> SketchEntityId {
    SketchEntityId::mint(format!("synthetic:test:sketch-entity#{suffix}")).expect("entity identity")
}

fn parameter_id(suffix: &str) -> ParameterId {
    ParameterId::mint(format!("synthetic:test:parameter#{suffix}")).expect("parameter identity")
}

fn entity_bytes(id: &SketchEntityId) -> u64 {
    u64::try_from(id.as_str().len()).expect("short identity length")
}

fn parameter_bytes(id: &ParameterId) -> u64 {
    u64::try_from(id.as_str().len()).expect("short identity length")
}

fn locus_bytes(locus: &SketchLocus) -> u64 {
    TAG_BYTES
        + match locus {
            SketchLocus::Entity(id)
            | SketchLocus::Start(id)
            | SketchLocus::End(id)
            | SketchLocus::Center(id) => entity_bytes(id),
        }
}

fn entity_list_bytes(entities: &[SketchEntityId]) -> u64 {
    entities.iter().map(entity_bytes).sum()
}

fn locus_list_bytes(loci: &[SketchLocus]) -> u64 {
    loci.iter().map(locus_bytes).sum()
}

fn maybe_parameter_bytes(parameter: Option<&ParameterId>) -> u64 {
    TAG_BYTES + parameter.map_or(0, parameter_bytes)
}

fn direction_bytes(direction: &SketchPatternDirection) -> u64 {
    let distance = match direction.distance.as_ref() {
        Some(distance) => TAG_BYTES + TAG_BYTES + parameter_bytes(distance.parameter()),
        None => TAG_BYTES,
    };
    let count_parameter = maybe_parameter_bytes(direction.count_parameter.as_ref());
    2 * u64::try_from(std::mem::size_of::<f64>()).expect("scalar size")
        + u64::try_from(std::mem::size_of::<f64>()).expect("scalar size")
        + distance
        + count_parameter
}

fn distance_pair_bytes(pair: &SketchDistancePair) -> u64 {
    locus_bytes(&pair.first) + locus_bytes(&pair.second)
}

fn measured_distance_bytes(measurement: &SketchDistanceMeasurement) -> u64 {
    match measurement {
        SketchDistanceMeasurement::Distance { first, second }
        | SketchDistanceMeasurement::Horizontal { first, second }
        | SketchDistanceMeasurement::Vertical { first, second } => {
            TAG_BYTES + locus_bytes(first) + locus_bytes(second)
        }
    }
}

fn assert_cost<T: DecodeCost>(ctx: &DecodeContext<'_>, value: &T, expected: u64) {
    assert_eq!(
        value
            .decode_cost(ctx, "test sketch constraint decode cost")
            .unwrap(),
        expected
    );
}

#[test]
fn every_planar_constraint_variant_costs_its_tag_and_active_fields() {
    let policy = DecodePolicy::service();
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root input");
    let e1 = entity_id("one");
    let e2 = entity_id("two");
    let e3 = entity_id("three");
    let e4 = entity_id("four");
    let center = entity_id("center");
    let p1 = parameter_id("one");
    let p2 = parameter_id("two");
    let e1_bytes = entity_bytes(&e1);
    let e2_bytes = entity_bytes(&e2);
    let e3_bytes = entity_bytes(&e3);
    let e4_bytes = entity_bytes(&e4);
    let p1_bytes = parameter_bytes(&p1);
    let p2_bytes = parameter_bytes(&p2);
    let f64_bytes = u64::try_from(std::mem::size_of::<f64>()).expect("scalar size");
    let u32_bytes = u64::try_from(std::mem::size_of::<u32>()).expect("index size");
    let bool_bytes = u64::try_from(std::mem::size_of::<bool>()).expect("flag size");
    let entity_locus = SketchLocus::Entity(e1.clone());
    let start_locus = SketchLocus::Start(e2.clone());
    let end_locus = SketchLocus::End(e3.clone());
    let center_locus = SketchLocus::Center(e4.clone());
    let entity_locus_bytes = locus_bytes(&entity_locus);
    let start_locus_bytes = locus_bytes(&start_locus);
    let end_locus_bytes = locus_bytes(&end_locus);
    let center_locus_bytes = locus_bytes(&center_locus);
    let loci = vec![entity_locus.clone(), start_locus.clone()];
    let loci_bytes = locus_list_bytes(&loci);
    let direction_u = SketchPatternDirection::new(
        [1.0, 0.0],
        Length::new(2.0).unwrap(),
        Some(SketchPatternDistance::Spacing {
            parameter: p1.clone(),
        }),
        Some(p2.clone()),
    )
    .expect("unit pattern direction");
    let direction_v =
        SketchPatternDirection::new([0.0, 1.0], Length::new(3.0).unwrap(), None, None)
            .expect("unit pattern direction");
    let rectangular = SketchRectangularPattern::new(
        [direction_u.clone(), direction_v.clone()],
        vec![vec![SketchPatternInstance {
            entities: vec![e1.clone()],
        }]],
    )
    .expect("one-cell rectangular pattern");
    let circular = SketchCircularPattern::new(
        center.clone(),
        Angle::new(1.25).unwrap(),
        Some(p1.clone()),
        Some(p2.clone()),
        vec![e1.clone()],
        vec![SketchCircularPatternInstance {
            angle: NonZeroAngle::new(0.5).unwrap(),
            entities: vec![e2.clone()],
        }],
    )
    .expect("one-instance circular pattern");
    let polygon = SketchPolygon::try_new(
        vec![e1.clone(), e2.clone(), e3.clone()],
        &ctx,
        "test sketch polygon identities",
    )
    .unwrap()
    .expect("three distinct polygon identities");
    let same_coordinate = SketchSameCoordinate::try_new(
        entity_locus.clone(),
        start_locus.clone(),
        SketchCoordinateAxis::U,
    )
    .expect("distinct same-coordinate loci");
    let offset_pair = SketchOffsetPair {
        source: e1.clone(),
        result: e2.clone(),
        source_reversed: true,
    };
    let offset_parameter = OffsetParameter {
        id: p1.clone(),
        negated: true,
    };
    let distance_pair_a = SketchDistancePair {
        first: entity_locus.clone(),
        second: start_locus.clone(),
    };
    let distance_pair_b = SketchDistancePair {
        first: end_locus.clone(),
        second: center_locus.clone(),
    };
    let measurement = SketchDistanceMeasurement::Horizontal {
        first: entity_locus.clone(),
        second: start_locus.clone(),
    };
    let native_kind = NonBlankString::try_from("native-curve").expect("native kind");
    let operand_kind = NonBlankString::try_from("point-operand").expect("operand kind");
    let operand_field_name = NonBlankString::try_from("endpoint").expect("field name");
    let operand = SketchNativeOperand {
        native_kind: operand_kind,
        field: Some(NativeOperandField {
            name: operand_field_name,
            role: Some(9),
        }),
        object_index: Some(4),
        native_ref: Some("native-object".to_owned()),
    };
    let native_properties = BTreeMap::from([
        ("native-key-a".to_owned(), "value-a".to_owned()),
        ("native-key-b".to_owned(), "value-b".to_owned()),
    ]);
    let native_properties_bytes = native_properties
        .iter()
        .map(|(key, value)| u64::try_from(key.len() + value.len()).unwrap())
        .sum::<u64>();
    let operand_bytes = u64::try_from("point-operand".len()).unwrap()
        + TAG_BYTES
        + u64::try_from("endpoint".len()).unwrap()
        + TAG_BYTES
        + u32_bytes
        + TAG_BYTES
        + u32_bytes
        + TAG_BYTES
        + u64::try_from("native-object".len()).unwrap();
    let group_loci = vec![entity_locus.clone(), center_locus.clone()];
    let group_loci_bytes = locus_list_bytes(&group_loci);
    let text_transforms = vec![Transform::identity()];
    let transform_bytes = 12 * f64_bytes;
    let polygon_bytes = entity_list_bytes(polygon.entities());
    let rectangular_bytes =
        TAG_BYTES + direction_bytes(&direction_u) + direction_bytes(&direction_v) + e1_bytes;
    let circular_bytes = TAG_BYTES
        + entity_bytes(&center)
        + f64_bytes
        + TAG_BYTES
        + p1_bytes
        + TAG_BYTES
        + p2_bytes
        + e1_bytes
        + f64_bytes
        + e2_bytes;
    let offset_pair_bytes = e1_bytes + e2_bytes + bool_bytes;
    let repeated_measurement_bytes = measured_distance_bytes(&measurement);
    let cases: Vec<(SketchConstraintDefinitionInput, u64)> = vec![
        (SketchConstraintDefinitionInput::Disabled {}, TAG_BYTES),
        (
            SketchConstraintDefinitionInput::Coincident {
                entities: vec![e1.clone(), e2.clone()],
            },
            TAG_BYTES + e1_bytes + e2_bytes,
        ),
        (
            SketchConstraintDefinitionInput::Polygon { polygon },
            TAG_BYTES + polygon_bytes,
        ),
        (
            SketchConstraintDefinitionInput::SplineGroup {
                entities: vec![e2.clone(), e3.clone()],
            },
            TAG_BYTES + e2_bytes + e3_bytes,
        ),
        (
            SketchConstraintDefinitionInput::RectangularPattern {
                pattern: rectangular,
            },
            rectangular_bytes,
        ),
        (
            SketchConstraintDefinitionInput::CircularPattern { pattern: circular },
            circular_bytes,
        ),
        (
            SketchConstraintDefinitionInput::TextFrame {
                text: e1.clone(),
                frame: vec![e2.clone(), e3.clone()],
            },
            TAG_BYTES + e1_bytes + e2_bytes + e3_bytes,
        ),
        (
            SketchConstraintDefinitionInput::TextPath {
                text: e1.clone(),
                path: e2.clone(),
                glyph_transforms: text_transforms,
            },
            TAG_BYTES + e1_bytes + e2_bytes + transform_bytes,
        ),
        (
            SketchConstraintDefinitionInput::CoincidentLoci { loci: loci.clone() },
            TAG_BYTES + loci_bytes,
        ),
        (
            SketchConstraintDefinitionInput::SameCoordinate {
                relation: same_coordinate,
            },
            TAG_BYTES + entity_locus_bytes + start_locus_bytes + TAG_BYTES,
        ),
        (
            SketchConstraintDefinitionInput::PointOnObject {
                point: entity_locus.clone(),
                entity: e2.clone(),
            },
            TAG_BYTES + entity_locus_bytes + e2_bytes,
        ),
        (
            SketchConstraintDefinitionInput::Midpoint {
                point: start_locus.clone(),
                entity: e3.clone(),
            },
            TAG_BYTES + start_locus_bytes + e3_bytes,
        ),
        (
            SketchConstraintDefinitionInput::PointCoordinateValues {
                point: entity_locus.clone(),
                values: [Length::new(1.0).unwrap(), Length::new(2.0).unwrap()],
            },
            TAG_BYTES + entity_locus_bytes + 2 * f64_bytes,
        ),
        (
            SketchConstraintDefinitionInput::MidpointCoordinate {
                first: entity_locus.clone(),
                second: start_locus.clone(),
                axis: SketchCoordinateAxis::V,
                value: Length::new(4.0).unwrap(),
            },
            TAG_BYTES + entity_locus_bytes + start_locus_bytes + TAG_BYTES + f64_bytes,
        ),
        (
            SketchConstraintDefinitionInput::Offset {
                pairs: vec![offset_pair],
                distance: Length::new(5.0).unwrap(),
                parameter: Some(offset_parameter),
            },
            TAG_BYTES + offset_pair_bytes + f64_bytes + TAG_BYTES + p1_bytes + bool_bytes,
        ),
        (
            SketchConstraintDefinitionInput::ProjectedCopy {
                source: e1.clone(),
                result: e2.clone(),
            },
            TAG_BYTES + e1_bytes + e2_bytes,
        ),
        (
            SketchConstraintDefinitionInput::AtIntersection {
                point: end_locus.clone(),
                first: e1.clone(),
                second: e2.clone(),
            },
            TAG_BYTES + end_locus_bytes + e1_bytes + e2_bytes,
        ),
        (
            SketchConstraintDefinitionInput::Concentric {
                first: e1.clone(),
                second: e2.clone(),
            },
            TAG_BYTES + e1_bytes + e2_bytes,
        ),
        (
            SketchConstraintDefinitionInput::Coradial {
                first: e2.clone(),
                second: e3.clone(),
            },
            TAG_BYTES + e2_bytes + e3_bytes,
        ),
        (
            SketchConstraintDefinitionInput::Collinear {
                first: e3.clone(),
                second: e4.clone(),
            },
            TAG_BYTES + e3_bytes + e4_bytes,
        ),
        (
            SketchConstraintDefinitionInput::Symmetric {
                first: entity_locus.clone(),
                second: start_locus.clone(),
                axis: e3.clone(),
            },
            TAG_BYTES + entity_locus_bytes + start_locus_bytes + e3_bytes,
        ),
        (
            SketchConstraintDefinitionInput::PointSymmetric {
                first: entity_locus.clone(),
                second: start_locus.clone(),
                center: center_locus.clone(),
            },
            TAG_BYTES + entity_locus_bytes + start_locus_bytes + center_locus_bytes,
        ),
        (
            SketchConstraintDefinitionInput::Horizontal { entity: e1.clone() },
            TAG_BYTES + e1_bytes,
        ),
        (
            SketchConstraintDefinitionInput::Vertical { entity: e2.clone() },
            TAG_BYTES + e2_bytes,
        ),
        (
            SketchConstraintDefinitionInput::Parallel {
                first: e1.clone(),
                second: e2.clone(),
            },
            TAG_BYTES + e1_bytes + e2_bytes,
        ),
        (
            SketchConstraintDefinitionInput::Perpendicular {
                first: e2.clone(),
                second: e3.clone(),
            },
            TAG_BYTES + e2_bytes + e3_bytes,
        ),
        (
            SketchConstraintDefinitionInput::Tangent {
                first: e3.clone(),
                second: e4.clone(),
            },
            TAG_BYTES + e3_bytes + e4_bytes,
        ),
        (
            SketchConstraintDefinitionInput::TangentLoci {
                first: entity_locus.clone(),
                second: start_locus.clone(),
            },
            TAG_BYTES + entity_locus_bytes + start_locus_bytes,
        ),
        (
            SketchConstraintDefinitionInput::Curvature {
                first: e1.clone(),
                second: e3.clone(),
            },
            TAG_BYTES + e1_bytes + e3_bytes,
        ),
        (
            SketchConstraintDefinitionInput::Equal {
                first: e2.clone(),
                second: e4.clone(),
            },
            TAG_BYTES + e2_bytes + e4_bytes,
        ),
        (
            SketchConstraintDefinitionInput::Fixed { entity: e4.clone() },
            TAG_BYTES + e4_bytes,
        ),
        (
            SketchConstraintDefinitionInput::ArcAngle {
                entity: e1.clone(),
                angle: PositiveAngle::new(0.5).unwrap(),
            },
            TAG_BYTES + e1_bytes + f64_bytes,
        ),
        (
            SketchConstraintDefinitionInput::EllipseAngle {
                entity: e2.clone(),
                angle: PositiveAngle::new(0.75).unwrap(),
            },
            TAG_BYTES + e2_bytes + f64_bytes,
        ),
        (
            SketchConstraintDefinitionInput::Distance {
                entities: vec![e1.clone(), e2.clone()],
                parameter: p1.clone(),
            },
            TAG_BYTES + e1_bytes + e2_bytes + p1_bytes,
        ),
        (
            SketchConstraintDefinitionInput::DistanceLoci {
                first: entity_locus.clone(),
                second: start_locus.clone(),
                parameter: p2.clone(),
            },
            TAG_BYTES + entity_locus_bytes + start_locus_bytes + p2_bytes,
        ),
        (
            SketchConstraintDefinitionInput::DistanceLociValue {
                first: entity_locus.clone(),
                second: end_locus.clone(),
                distance: Length::new(6.0).unwrap(),
                parameter: Some(p1.clone()),
            },
            TAG_BYTES + entity_locus_bytes + end_locus_bytes + f64_bytes + TAG_BYTES + p1_bytes,
        ),
        (
            SketchConstraintDefinitionInput::PolarDistance {
                first: entity_locus.clone(),
                second: start_locus.clone(),
                distance: Length::new(7.0).unwrap(),
                angle: Some(Angle::new(0.25).unwrap()),
                distance_parameter: Some(p2.clone()),
            },
            TAG_BYTES
                + entity_locus_bytes
                + start_locus_bytes
                + f64_bytes
                + TAG_BYTES
                + f64_bytes
                + TAG_BYTES
                + p2_bytes,
        ),
        (
            SketchConstraintDefinitionInput::AngleDifference {
                first: 1,
                second: 2,
                difference: 3,
                value: Angle::new(0.5).unwrap(),
            },
            TAG_BYTES + 3 * u32_bytes + f64_bytes,
        ),
        (
            SketchConstraintDefinitionInput::ScalarEquality {
                first: 4,
                second: 5,
            },
            TAG_BYTES + 2 * u32_bytes,
        ),
        (
            SketchConstraintDefinitionInput::EqualDistance {
                first: distance_pair_a.clone(),
                second: distance_pair_b.clone(),
            },
            TAG_BYTES
                + distance_pair_bytes(&distance_pair_a)
                + distance_pair_bytes(&distance_pair_b),
        ),
        (
            SketchConstraintDefinitionInput::HorizontalDistance {
                first: entity_locus.clone(),
                second: start_locus.clone(),
                parameter: p1.clone(),
            },
            TAG_BYTES + entity_locus_bytes + start_locus_bytes + p1_bytes,
        ),
        (
            SketchConstraintDefinitionInput::VerticalDistance {
                first: end_locus.clone(),
                second: center_locus.clone(),
                parameter: p2.clone(),
            },
            TAG_BYTES + end_locus_bytes + center_locus_bytes + p2_bytes,
        ),
        (
            SketchConstraintDefinitionInput::RepeatedDistance {
                measurements: vec![measurement.clone()],
                parameter: p1.clone(),
            },
            TAG_BYTES + repeated_measurement_bytes + p1_bytes,
        ),
        (
            SketchConstraintDefinitionInput::RepeatedLength {
                entities: vec![e1.clone(), e4.clone()],
                parameter: p1.clone(),
            },
            TAG_BYTES + e1_bytes + e4_bytes + p1_bytes,
        ),
        (
            SketchConstraintDefinitionInput::ParallelLineSetDistance {
                first: vec![e1.clone(), e2.clone()],
                second: vec![e3.clone(), e4.clone()],
                parameter: p2.clone(),
            },
            TAG_BYTES + e1_bytes + e2_bytes + e3_bytes + e4_bytes + p2_bytes,
        ),
        (
            SketchConstraintDefinitionInput::Angle {
                first: e1.clone(),
                second: e2.clone(),
                parameter: p1.clone(),
            },
            TAG_BYTES + e1_bytes + e2_bytes + p1_bytes,
        ),
        (
            SketchConstraintDefinitionInput::AngleToAxis {
                entity: e2.clone(),
                axis: SketchAxis::Vertical,
                parameter: p2.clone(),
            },
            TAG_BYTES + e2_bytes + TAG_BYTES + p2_bytes,
        ),
        (
            SketchConstraintDefinitionInput::Radius {
                entity: e3.clone(),
                parameter: p1.clone(),
            },
            TAG_BYTES + e3_bytes + p1_bytes,
        ),
        (
            SketchConstraintDefinitionInput::RepeatedRadius {
                entities: vec![e2.clone(), e4.clone()],
                parameter: p2.clone(),
            },
            TAG_BYTES + e2_bytes + e4_bytes + p2_bytes,
        ),
        (
            SketchConstraintDefinitionInput::Diameter {
                entity: e4.clone(),
                parameter: p2.clone(),
            },
            TAG_BYTES + e4_bytes + p2_bytes,
        ),
        (
            SketchConstraintDefinitionInput::RepeatedDiameter {
                entities: vec![e1.clone(), e3.clone()],
                parameter: p1.clone(),
            },
            TAG_BYTES + e1_bytes + e3_bytes + p1_bytes,
        ),
        (
            SketchConstraintDefinitionInput::SnellsLaw {
                incident: entity_locus.clone(),
                refracted: start_locus.clone(),
                interface: e3.clone(),
                parameter: p1.clone(),
            },
            TAG_BYTES + entity_locus_bytes + start_locus_bytes + e3_bytes + p1_bytes,
        ),
        (
            SketchConstraintDefinitionInput::Weight {
                entity: e1.clone(),
                parameter: p2.clone(),
            },
            TAG_BYTES + e1_bytes + p2_bytes,
        ),
        (
            SketchConstraintDefinitionInput::InternalAlignment {
                helper: e1.clone(),
                parent: e2.clone(),
                alignment: SketchInternalAlignment::BsplineKnotPoint(12),
            },
            TAG_BYTES + e1_bytes + e2_bytes + TAG_BYTES + u32_bytes,
        ),
        (
            SketchConstraintDefinitionInput::Group {
                elements: group_loci.clone(),
            },
            TAG_BYTES + group_loci_bytes,
        ),
        (
            SketchConstraintDefinitionInput::Text {
                elements: group_loci,
                text: "label".to_owned(),
                font: Some("font".to_owned()),
                is_text_height: true,
            },
            TAG_BYTES + group_loci_bytes + 5 + TAG_BYTES + 4 + bool_bytes,
        ),
        (
            SketchConstraintDefinitionInput::Native {
                native_kind: native_kind.clone(),
                native_state: Some(17),
                native_flags: Some(3),
                native_properties,
                entities: vec![e1.clone(), e2.clone()],
                parameter: Some(p2.clone()),
                operands: vec![operand],
            },
            TAG_BYTES
                + u64::try_from(native_kind.as_str().len()).unwrap()
                + TAG_BYTES
                + u64::try_from(std::mem::size_of::<u64>()).unwrap()
                + TAG_BYTES
                + u64::try_from(std::mem::size_of::<u64>()).unwrap()
                + native_properties_bytes
                + e1_bytes
                + e2_bytes
                + TAG_BYTES
                + p2_bytes
                + operand_bytes,
        ),
    ];

    assert_eq!(
        cases.len(),
        57,
        "every planar constraint input variant is covered"
    );
    for (kind, expected) in cases {
        assert_cost(&ctx, &kind, expected);
    }
}

#[test]
fn auxiliary_constraint_types_cost_all_active_enum_fields() {
    let policy = DecodePolicy::service();
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root input");
    let parameter = parameter_id("auxiliary");
    let parameter_bytes = parameter_bytes(&parameter);
    assert_cost(&ctx, &SketchAxis::Horizontal, TAG_BYTES);
    assert_cost(&ctx, &SketchAxis::Vertical, TAG_BYTES);
    for distance in [
        SketchPatternDistance::Spacing {
            parameter: parameter.clone(),
        },
        SketchPatternDistance::Span {
            parameter: parameter.clone(),
        },
    ] {
        assert_cost(&ctx, &distance, TAG_BYTES + parameter_bytes);
    }
    let first = SketchLocus::Entity(entity_id("aux-first"));
    let second = SketchLocus::Start(entity_id("aux-second"));
    for measurement in [
        SketchDistanceMeasurement::Distance {
            first: first.clone(),
            second: second.clone(),
        },
        SketchDistanceMeasurement::Horizontal {
            first: first.clone(),
            second: second.clone(),
        },
        SketchDistanceMeasurement::Vertical {
            first: first.clone(),
            second: second.clone(),
        },
    ] {
        assert_cost(&ctx, &measurement, measured_distance_bytes(&measurement));
    }
    for (alignment, index_bytes) in [
        (SketchInternalAlignment::EllipseMajorDiameter, 0),
        (SketchInternalAlignment::EllipseMinorDiameter, 0),
        (SketchInternalAlignment::EllipseFocus1, 0),
        (SketchInternalAlignment::EllipseFocus2, 0),
        (SketchInternalAlignment::HyperbolaMajor, 0),
        (SketchInternalAlignment::HyperbolaMinor, 0),
        (SketchInternalAlignment::HyperbolaFocus, 0),
        (SketchInternalAlignment::ParabolaFocus, 0),
        (SketchInternalAlignment::BsplineControlPoint(5), 4),
        (SketchInternalAlignment::BsplineKnotPoint(7), 4),
        (SketchInternalAlignment::ParabolaFocalAxis, 0),
    ] {
        assert_cost(&ctx, &alignment, TAG_BYTES + index_bytes);
    }
}

#[test]
fn constraint_wrappers_cost_every_optional_field() {
    let policy = DecodePolicy::service();
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root input");
    let kind = SketchConstraintDefinitionInput::Disabled {};
    let definition = SketchConstraintDefinition::try_from(kind).expect("admitted definition");
    assert_cost(&ctx, &definition, TAG_BYTES);

    let constraint = SketchConstraint {
        id: SketchConstraintId::mint("synthetic:test:constraint#1").expect("constraint ID"),
        sketch: SketchId::mint("synthetic:test:sketch#1").expect("sketch ID"),
        definition,
        name: Some("relation".to_owned()),
        driving: Some(true),
        active: Some(false),
        virtual_space: Some(true),
        visible: Some(false),
        orientation: Some(19),
        label_distance: Some(crate::sketches::SketchLabelValue::try_from(1.5).unwrap()),
        label_position: Some(crate::sketches::SketchLabelValue::try_from(2.5).unwrap()),
        metadata: Some("metadata".to_owned()),
        native_ref: Some("native-ref".to_owned()),
    };
    let expected = u64::try_from(constraint.id.as_str().len()).unwrap()
        + u64::try_from(constraint.sketch.as_str().len()).unwrap()
        + TAG_BYTES
        + TAG_BYTES
        + u64::try_from("relation".len()).unwrap()
        + 2 * (TAG_BYTES + u64::try_from(std::mem::size_of::<bool>()).unwrap())
        + 2 * (TAG_BYTES + u64::try_from(std::mem::size_of::<bool>()).unwrap())
        + TAG_BYTES
        + u64::try_from(std::mem::size_of::<u32>()).unwrap()
        + 2 * (TAG_BYTES + u64::try_from(std::mem::size_of::<f64>()).unwrap())
        + TAG_BYTES
        + u64::try_from("metadata".len()).unwrap()
        + TAG_BYTES
        + u64::try_from("native-ref".len()).unwrap();
    assert_cost(&ctx, &constraint, expected);

    let empty_constraint = SketchConstraint {
        id: SketchConstraintId::mint("synthetic:test:constraint#empty").expect("constraint ID"),
        sketch: SketchId::mint("synthetic:test:sketch#empty").expect("sketch ID"),
        definition: SketchConstraintDefinition::try_from(
            SketchConstraintDefinitionInput::Disabled {},
        )
        .expect("admitted definition"),
        name: None,
        driving: None,
        active: None,
        virtual_space: None,
        visible: None,
        orientation: None,
        label_distance: None,
        label_position: None,
        metadata: None,
        native_ref: None,
    };
    let empty_expected = u64::try_from(empty_constraint.id.as_str().len()).unwrap()
        + u64::try_from(empty_constraint.sketch.as_str().len()).unwrap()
        + TAG_BYTES
        + 10 * TAG_BYTES;
    assert_cost(&ctx, &empty_constraint, empty_expected);
}

#[test]
fn sketch_constraint_retain_mut_refuses_service_work_before_editing() {
    let constraint = SketchConstraint {
        id: SketchConstraintId::mint("synthetic:test:constraint#retain").expect("constraint ID"),
        sketch: SketchId::mint("synthetic:test:sketch#retain").expect("sketch ID"),
        definition: SketchConstraintDefinition::try_from(
            SketchConstraintDefinitionInput::Disabled {},
        )
        .expect("admitted definition"),
        name: None,
        driving: None,
        active: None,
        virtual_space: None,
        visible: None,
        orientation: None,
        label_distance: None,
        label_position: None,
        metadata: None,
        native_ref: None,
    };
    let mut constraints = vec![constraint.clone()];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root input");
    let mut called = false;
    let error = ctx
        .retain_mut(
            &mut constraints,
            |_| {
                called = true;
                Ok(true)
            },
            "creo sketch constraint parameter reconciliation",
        )
        .expect_err("service work limit refuses the admitted mutable scan");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::WorkUnits
            && resource.operation == "creo sketch constraint parameter reconciliation")
    );
    assert!(!called, "callback stays untouched on an admission refusal");
    assert_eq!(constraints, vec![constraint]);
}
