//! Tests for the `relation_records` module.

#[test]
fn line_angles_retain_shallow_and_near_opposite_angles() {
    const ANGLE: f64 = 1e-8;
    let first = [[0.0, 0.0], [1.0, 0.0]];
    let second = [[0.0, 0.0], [ANGLE.cos(), ANGLE.sin()]];
    assert!((super::line_line_angle(first, second).unwrap() - ANGLE).abs() <= f64::EPSILON * ANGLE);
    let opposite = [[0.0, 0.0], [-ANGLE.cos(), ANGLE.sin()]];
    assert!(
        (super::line_line_angle(first, opposite).unwrap() - (std::f64::consts::PI - ANGLE)).abs()
            <= f64::EPSILON * std::f64::consts::PI
    );
}

#[test]
fn numerical_audit_line_angles_are_finite_for_large_directions() {
    let x = [[0.0, 0.0], [1.0e200, 0.0]];
    let y = [[0.0, 0.0], [0.0, 1.0e200]];
    assert_eq!(super::line_line_angle(x, x), Some(0.0));
    assert_eq!(
        super::line_line_angle(x, y),
        Some(std::f64::consts::FRAC_PI_2)
    );
    assert_eq!(super::dynamic_line_line_angle(x, x), Some(0.0));
}

use super::{
    feature_intervals, is_solver_point_operand, relation_declaration_candidates,
    relation_instances, relation_signature, relation_uses_dynamic_operands, CircleHandleDrivers,
    UNKNOWN_FEATURE_SPAN,
};
use crate::records::operand_tag::NativeOperandTag;
use crate::records::FeatureInputOperand;
use crate::records::FeatureInputOperandKind;
use crate::records::FeatureInputRelationFamily;
use crate::records::FeatureInputScalar;
use crate::records::FeatureInputScalarRole;
use crate::records::FeatureSource;
use crate::records::{
    Feature, FeatureHistory, FeatureInputClass, FeatureInputLane, FeatureInputName,
    FeatureInputRelationBinding,
};
use std::collections::BTreeMap;

mod dynamic_relations;

fn class(offset: u64, name: &str) -> FeatureInputClass {
    FeatureInputClass {
        id: format!("class-{offset}"),
        parent: "lane".into(),
        ordinal: 0,
        offset,
        name: name.into(),
    }
}

fn scalar(offset: u64, role: FeatureInputScalarRole) -> FeatureInputScalar {
    let operands = [0_u16, 1]
        .into_iter()
        .enumerate()
        .map(|(ordinal, entity_index)| FeatureInputOperand {
            offset: offset + cadmpeg_core::decode::u64_from_index(ordinal),
            reference_ref: format!("reference-{offset}-{ordinal}"),
            kind: FeatureInputOperandKind::Native(NativeOperandTag::TAG_8152),
            entity_index,
            entity_ref: None,
        })
        .collect();
    FeatureInputScalar {
        id: format!("scalar-{offset}"),
        parent: "lane".into(),
        feature_ref: Some("sketch".into()),
        ordinal: 0,
        offset,
        object_id: 1,
        name: "dimension".into(),
        value: cadmpeg_ir::scalar::FiniteReal::new(1.0).expect("finite test scalar"),
        role,
        operands,
        archive_class: None,
        archive_object_id: None,
    }
}

#[test]
fn relation_operand_index_matches_the_linear_lookups() {
    use crate::records::{SketchInputEntity, SketchInputKind};

    let mut state = 0x2545_f491_u64;
    let mut next = |bound: u64| {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (state >> 33) % bound
    };
    let base = SketchInputEntity::new("entity", "lane", 0, 0, SketchInputKind::Point);
    let features = ["sketch-0", "sketch-1", "sketch-2", "sketch-3", "absent"];
    for _ in 0..16 {
        let mut lane = lane(Vec::new(), Vec::new());
        lane.scalars = (0..200)
            .map(|offset| {
                let mut scalar = scalar(offset, FeatureInputScalarRole::Driving);
                scalar.id = format!("scalar-{}", next(60));
                scalar.value =
                    cadmpeg_ir::scalar::FiniteReal::new(f64::from(u32::try_from(offset).unwrap()))
                        .unwrap();
                scalar
            })
            .collect();
        lane.sketch_entities = (0..300)
            .map(|_| {
                let mut entity =
                    base.with_test_position(u32::try_from(next(40)).unwrap(), next(80));
                entity.feature_ref =
                    (next(5) != 0).then(|| features[usize::try_from(next(4)).unwrap()].to_owned());
                entity
            })
            .collect();
        let index = super::RelationOperandIndex::new(&lane);
        for id in (0..70).map(|id| format!("scalar-{id}")) {
            assert_eq!(
                index.scalar_value(&id),
                lane.scalars
                    .iter()
                    .find(|scalar| scalar.id == id)
                    .map(|scalar| scalar.value)
            );
        }
        for feature in features {
            let mut linear = lane
                .sketch_entities
                .iter()
                .filter(|entity| entity.feature_ref.as_deref() == Some(feature))
                .collect::<Vec<_>>();
            linear.sort_unstable_by(|left, right| {
                (left.offset(), left.ordinal()).cmp(&(right.offset(), right.ordinal()))
            });
            let indexed = index.feature_entities(feature);
            assert_eq!(linear.len(), indexed.len());
            assert!(linear
                .iter()
                .zip(indexed)
                .all(|(linear, indexed)| std::ptr::eq(*linear, *indexed)));
        }
    }
}

/// The per-class linear search the declaration index replaces.
fn linear_declaration_candidates(
    classes: &[FeatureInputClass],
    scalars: &[FeatureInputScalar],
    intervals: &[(u64, Option<u64>, String)],
    allow_dynamic: bool,
) -> Vec<(String, String)> {
    let interval_at = |offset: u64| {
        intervals
            .iter()
            .find(|(start, end, _)| offset >= *start && end.is_none_or(|end| offset < end))
    };
    classes
        .iter()
        .filter_map(|class| {
            let family = super::relation_family(&class.name)?;
            let interval = interval_at(class.offset);
            let class_feature = interval.map(|(_, _, feature)| feature.as_str());
            let next_class = classes
                .iter()
                .filter(|candidate| {
                    candidate.offset > class.offset
                        && super::relation_family(&candidate.name).is_some()
                        && class_feature.is_some_and(|feature| {
                            interval_at(candidate.offset)
                                .is_some_and(|(_, _, owner)| owner.as_str() == feature)
                        })
                })
                .map(|candidate| candidate.offset)
                .min();
            let (feature_end, unknown_feature_limit) = match interval {
                Some((_, end, _)) => (*end, None),
                None => (None, Some(class.offset.checked_add(UNKNOWN_FEATURE_SPAN)?)),
            };
            let scope_end = [next_class, feature_end, unknown_feature_limit]
                .into_iter()
                .flatten()
                .min();
            let scalar = scalars
                .iter()
                .filter(|scalar| {
                    scalar.offset > class.offset
                        && scope_end.is_none_or(|end| scalar.offset < end)
                        && class_feature
                            .is_none_or(|feature| scalar.feature_ref.as_deref() == Some(feature))
                        && if allow_dynamic {
                            super::relation_signature_for_declaration(family, scalar)
                        } else {
                            relation_signature(family, &scalar.operands)
                        }
                })
                .min_by_key(|scalar| scalar.offset)?;
            Some((class.id.clone(), scalar.id.clone()))
        })
        .collect()
}

#[test]
fn indexed_declaration_search_matches_the_linear_search() {
    let mut state = 0x2545_f491_u64;
    let mut next = |bound: u64| {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (state >> 33) % bound
    };
    let intervals = [
        (500, Some(1_500), "a".to_string()),
        (1_500, Some(2_500), "b".to_string()),
        (2_500, None, "a".to_string()),
    ];
    let mut classes = Vec::new();
    for index in 0..300 {
        let name = if next(3) == 0 {
            "sgEntHandle"
        } else {
            "sgPntPntHorDist"
        };
        let mut declared = class(next(3_200), name);
        declared.id = format!("class-{index}");
        classes.push(declared);
    }
    let mut scalars = Vec::new();
    for index in 0..600 {
        let mut stated = scalar(next(3_200), FeatureInputScalarRole::Driving);
        stated.id = format!("scalar-{index}");
        stated.feature_ref = match next(4) {
            0 => None,
            1 => Some("b".into()),
            _ => Some("a".into()),
        };
        if next(5) == 0 {
            stated.operands.truncate(1);
        }
        scalars.push(stated);
    }
    for intervals in [&intervals[..], &[]] {
        for allow_dynamic in [false, true] {
            let indexed = super::relation_declaration_candidates_impl(
                &classes,
                &scalars,
                intervals,
                allow_dynamic,
            )
            .into_iter()
            .map(|(class, scalar, _)| (class.id.clone(), scalar.id.clone()))
            .collect::<Vec<_>>();
            assert!(!indexed.is_empty());
            assert_eq!(
                indexed,
                linear_declaration_candidates(&classes, &scalars, intervals, allow_dynamic)
            );
        }
    }
}

#[test]
fn declaration_search_is_linear_in_classes_and_scalars() {
    const RELATIONS: u64 = 1 << 14;
    let intervals = [(0, None, "sketch".to_string())];
    let classes = (0..RELATIONS)
        .map(|index| class(index * 100, "sgPntPntHorDist"))
        .collect::<Vec<_>>();
    let scalars = (0..RELATIONS)
        .map(|index| scalar(index * 100 + 50, FeatureInputScalarRole::Driving))
        .collect::<Vec<_>>();
    let candidates = relation_declaration_candidates(&classes, &scalars, &intervals);
    assert_eq!(candidates.len(), usize::try_from(RELATIONS).unwrap());
    assert!(candidates
        .iter()
        .all(|(class, scalar, _)| scalar.offset == class.offset + 50));
}

fn circle_scalar(
    offset: u64,
    name: &str,
    role: FeatureInputScalarRole,
    entity_ref: Option<&str>,
) -> FeatureInputScalar {
    FeatureInputScalar {
        id: format!("scalar-{offset}"),
        parent: "lane".into(),
        feature_ref: Some("sketch".into()),
        ordinal: 0,
        offset,
        object_id: 1,
        name: name.into(),
        value: cadmpeg_ir::scalar::FiniteReal::new(1.0).expect("finite test scalar"),
        role,
        operands: vec![FeatureInputOperand {
            offset: offset + 1,
            reference_ref: format!("reference-{offset}"),
            kind: FeatureInputOperandKind::Native(NativeOperandTag::try_from(0x1234).unwrap()),
            entity_index: 0,
            entity_ref: entity_ref.map(str::to_owned),
        }],
        archive_class: None,
        archive_object_id: None,
    }
}

fn lane(classes: Vec<FeatureInputClass>, scalars: Vec<FeatureInputScalar>) -> FeatureInputLane {
    FeatureInputLane {
        id: "lane".into(),
        configuration: None,
        native_payload: Vec::new().into(),
        classes,
        names: Vec::new(),
        scalars,
        relation_bindings: Vec::new(),
        relation_instances: Vec::new(),
        body_selections: Vec::new(),
        edge_selections: Vec::new(),
        surface_selections: Vec::new(),
        generated_surface_identities: Vec::new(),
        references: Vec::new(),
        sketch_entities: Vec::new(),
    }
}

fn sketch_history() -> Vec<FeatureHistory> {
    vec![FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![Feature {
            id: "sketch".into(),
            parent: "history".into(),
            xml_tag: "Sketch".into(),
            tree_parent: None,
            source_id: None,
            ordinal: 0,
            name: "Sketch".into(),
            kind: "Sketch".into(),
            input_class: None,
            suppressed: false,
            parameters: BTreeMap::new(),
            dimension_properties: BTreeMap::new(),
            properties: BTreeMap::new(),
            text: None,
            content: Vec::new(),
        }],
    }]
}

#[test]
fn native_relation_tags_are_scoped_to_the_declared_family() {
    let operand_pair = |kind| {
        [0_u16, 1]
            .into_iter()
            .enumerate()
            .map(|(ordinal, entity_index)| FeatureInputOperand {
                offset: cadmpeg_core::decode::u64_from_index(ordinal),
                reference_ref: format!("reference-{ordinal}"),
                kind,
                entity_index,
                entity_ref: None,
            })
            .collect::<Vec<_>>()
    };
    let point_tagged = operand_pair(FeatureInputOperandKind::Native(NativeOperandTag::TAG_80D5));
    let line_tagged = operand_pair(FeatureInputOperandKind::Native(NativeOperandTag::TAG_810F));
    let roster_point_tagged =
        operand_pair(FeatureInputOperandKind::Native(NativeOperandTag::TAG_81DD));
    let roster_line_tagged =
        operand_pair(FeatureInputOperandKind::Native(NativeOperandTag::TAG_81E7));

    assert!(relation_signature(
        FeatureInputRelationFamily::PointPointDistance,
        &point_tagged
    ));
    assert!(relation_signature(
        FeatureInputRelationFamily::PointPointHorizontalDistance,
        &point_tagged
    ));
    assert!(relation_signature(
        FeatureInputRelationFamily::PointPointVerticalDistance,
        &point_tagged
    ));
    assert!(relation_signature(
        FeatureInputRelationFamily::Angle,
        &point_tagged
    ));
    assert!(relation_signature(
        FeatureInputRelationFamily::LineLineDistance,
        &line_tagged
    ));
    assert!(relation_signature(
        FeatureInputRelationFamily::PointPointDistance,
        &roster_point_tagged
    ));
    assert!(relation_signature(
        FeatureInputRelationFamily::LineLineDistance,
        &roster_line_tagged
    ));
    assert!(relation_signature(
        FeatureInputRelationFamily::PointLineDistance,
        &[
            FeatureInputOperand {
                kind: FeatureInputOperandKind::Native(NativeOperandTag::TAG_81DD),
                ..roster_point_tagged[0].clone()
            },
            FeatureInputOperand {
                kind: FeatureInputOperandKind::Native(NativeOperandTag::TAG_81E7),
                ..roster_line_tagged[1].clone()
            },
        ]
    ));

    for tag in [0x8138, 0x80ac] {
        let point_distance_tagged =
            operand_pair(FeatureInputOperandKind::Native(tag.try_into().unwrap()));
        assert!(relation_signature(
            FeatureInputRelationFamily::PointPointDistance,
            &point_distance_tagged
        ));
        assert!(!relation_signature(
            FeatureInputRelationFamily::PointPointHorizontalDistance,
            &point_distance_tagged
        ));
        assert!(!relation_signature(
            FeatureInputRelationFamily::PointPointVerticalDistance,
            &point_distance_tagged
        ));
        assert!(!relation_signature(
            FeatureInputRelationFamily::Angle,
            &point_distance_tagged
        ));
    }

    assert!(!relation_signature(
        FeatureInputRelationFamily::PointPointDistance,
        &line_tagged
    ));
    assert!(!relation_signature(
        FeatureInputRelationFamily::LineLineDistance,
        &point_tagged
    ));
    assert!(!relation_signature(
        FeatureInputRelationFamily::PointLineDistance,
        &point_tagged
    ));

    for tag in [0x8100, 0x820f] {
        let solver_point_tagged =
            operand_pair(FeatureInputOperandKind::Native(tag.try_into().unwrap()));
        assert!(relation_signature(
            FeatureInputRelationFamily::PointPointDistance,
            &solver_point_tagged
        ));
        assert!(!relation_signature(
            FeatureInputRelationFamily::PointPointHorizontalDistance,
            &solver_point_tagged
        ));
        assert!(solver_point_tagged
            .iter()
            .all(|operand| is_solver_point_operand(operand.kind)));
    }
}

/// A class whose unknown-feature span no `u64` can name states no scope,
/// so it declares no relation. The control states the same shape at an
/// offset the span can name and declares one.
#[test]
fn a_class_whose_unknown_feature_span_is_unstatable_declares_no_relation() {
    let unstatable = class(u64::MAX - 100, "sgPntPntHorDist");
    let following = scalar(u64::MAX - 50, FeatureInputScalarRole::Driving);
    assert!(relation_declaration_candidates(
        std::slice::from_ref(&unstatable),
        std::slice::from_ref(&following),
        &[]
    )
    .is_empty());

    let stated = class(1_000, "sgPntPntHorDist");
    let within = scalar(1_050, FeatureInputScalarRole::Driving);
    assert_eq!(
        relation_declaration_candidates(
            std::slice::from_ref(&stated),
            std::slice::from_ref(&within),
            &[]
        )
        .len(),
        1
    );
}

/// The lane the decoder's own readers make of a `ResolvedFeatures`
/// stream that opens with the map count `0x20`: the relation class
/// `sgPntPntDist` declared with `moLengthParameter_c` inside its first
/// relation object, whose scalar `D1` is a padded value-only scalar, then
/// two dimensions in the token form with the relation-class token
/// `relation` and the parameter-class token `parameter`, each a display
/// scalar and a driving scalar. Every scalar belongs to `sketch` and
/// holds two point operands: the padded scalar and the first dimension
/// one pair, the second dimension another. The scalars' object
/// identifiers run from `0x40` in steps of 2, in stream order.
fn token_form_lane(relation: u16, parameter: u16) -> FeatureInputLane {
    let mut opening = 0x20u32.to_le_bytes().to_vec();
    opening.extend_from_slice(&[0x02, 0x00]);
    token_form_lane_opening(&opening, relation, parameter)
}

/// The lane of [`token_form_lane`] in a stream that opens with `opening`.
fn token_form_lane_opening(opening: &[u8], relation: u16, parameter: u16) -> FeatureInputLane {
    const NAME_CLASS: u16 = 0x72;
    let object = |position: u32| 0x40 + 2 * position;
    let mut bytes = opening.to_vec();
    let declare = |bytes: &mut Vec<u8>, class: &str| {
        bytes.extend_from_slice(&[0xff, 0xff, 0x01, 0x00]);
        bytes.extend_from_slice(&u16::try_from(class.len()).unwrap().to_le_bytes());
        bytes.extend_from_slice(class.as_bytes());
    };
    let name = |bytes: &mut Vec<u8>, name: &str| {
        bytes.extend_from_slice(&(0x8000 | NAME_CLASS).to_le_bytes());
        bytes.extend_from_slice(&[0xff, 0xfe, 0xff]);
        bytes.push(u8::try_from(name.encode_utf16().count()).unwrap());
        for unit in name.encode_utf16() {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }
    };
    let trailer = |bytes: &mut Vec<u8>, object: u32, len: usize| {
        let at = bytes.len();
        bytes.resize(at + len, 0);
        bytes[at + 3..at + 7].copy_from_slice(&object.to_le_bytes());
        at
    };
    declare(&mut bytes, "sgPntPntDist");
    bytes.extend_from_slice(&[0; 4]);
    declare(&mut bytes, "moLengthParameter_c");
    name(&mut bytes, "D1");
    bytes.extend_from_slice(super::super::VALUE_ONLY_SCALAR_HEADER);
    bytes.extend_from_slice(&[0; 4]);
    bytes.extend_from_slice(&0.0215f64.to_le_bytes());
    trailer(&mut bytes, object(0), 40);
    for (position, (dimension, metres, role)) in (1u32..).zip([
        ("D1", 0.0412f64, 1u8),
        ("D1", 0.045, 0),
        ("D2", 0.0176, 1),
        ("D2", 0.025, 0),
    ]) {
        bytes.extend_from_slice(&(0x8000 | relation).to_le_bytes());
        bytes.extend_from_slice(&[0; 4]);
        bytes.extend_from_slice(&(0x8000 | parameter).to_le_bytes());
        name(&mut bytes, dimension);
        bytes.extend_from_slice(super::super::SCALAR_HEADER);
        bytes.extend_from_slice(&metres.to_le_bytes());
        let at = trailer(&mut bytes, object(position), 35);
        bytes[at + 24..at + 29].copy_from_slice(&[0, 0, 0, 2, 0]);
        bytes[at + 29] = role;
    }
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&bytes, &arena, &policy)
        .expect("test context");
    let names = super::super::names::object_names(&ctx, &bytes, "lane").unwrap();
    let mut scalars = super::super::scalars::named_scalars(&bytes, "lane", &names).unwrap();
    let points = |entities: [u16; 2]| {
        entities
            .into_iter()
            .enumerate()
            .map(|(ordinal, entity_index)| FeatureInputOperand {
                offset: cadmpeg_core::decode::u64_from_index(ordinal),
                reference_ref: format!("reference-{entity_index}"),
                kind: FeatureInputOperandKind::D6,
                entity_index,
                entity_ref: None,
            })
            .collect::<Vec<_>>()
    };
    for (index, scalar) in scalars.iter_mut().enumerate() {
        scalar.feature_ref = Some("sketch".into());
        scalar.operands = points(if index < 3 { [1, 2] } else { [1, 3] });
    }
    let mut lane = lane(
        super::super::names::class_declarations(&bytes, "lane").unwrap(),
        scalars,
    );
    lane.names = names;
    lane.native_payload = bytes.into();
    lane
}

#[test]
fn the_first_token_form_dimension_after_a_native_declared_scalar_is_its_relation() {
    let lane = token_form_lane(0x30, 0x32);
    assert_eq!(lane.scalars.len(), 5, "{:#?}", lane.scalars);
    let class = lane
        .classes
        .iter()
        .find(|class| class.name == "sgPntPntDist")
        .unwrap()
        .id
        .clone();
    let instances = relation_instances(&sketch_history(), &lane).unwrap();
    let ids = |range: std::ops::Range<usize>| {
        lane.scalars[range]
            .iter()
            .map(|scalar| scalar.id.clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        instances
            .iter()
            .map(|relation| (
                relation.family,
                relation.class_ref.clone(),
                relation.scalar_refs().to_vec(),
                relation.parameter_scalar_ref().map(str::to_owned),
            ))
            .collect::<Vec<_>>(),
        vec![
            (
                FeatureInputRelationFamily::PointPointDistance,
                class.clone(),
                ids(0..1),
                None
            ),
            (
                FeatureInputRelationFamily::PointPointDistance,
                class,
                ids(1..3),
                Some(lane.scalars[2].id.clone())
            ),
        ]
    );

    // A parameter-class token three above the relation-class token names
    // no class declared beside the relation class.
    let lane = token_form_lane(0x30, 0x33);
    let instances = relation_instances(&sketch_history(), &lane).unwrap();
    assert_eq!(
        instances
            .iter()
            .map(|relation| relation.scalar_refs().to_vec())
            .collect::<Vec<_>>(),
        vec![vec![lane.scalars[0].id.clone()]]
    );
}

/// A relation class declared before a dimension whose record states a
/// parameter of another quantity claims none of its scalars, even where
/// their operands fit the declared family.
#[test]
fn a_declaration_claims_no_dimension_whose_tokens_name_another_relation() {
    let mut lane = token_form_lane(0x30, 0x32);
    let angle = class(lane.scalars[0].offset + 1, "sgAnglDim");
    lane.classes.push(angle.clone());
    for scalar in &mut lane.scalars[1..3] {
        for operand in &mut scalar.operands {
            operand.kind = FeatureInputOperandKind::Native(NativeOperandTag::TAG_8386);
        }
    }
    let distance = lane
        .classes
        .iter()
        .find(|class| class.name == "sgPntPntDist")
        .unwrap()
        .id
        .clone();
    let instances = relation_instances(&sketch_history(), &lane).unwrap();
    assert!(instances
        .iter()
        .all(|relation| relation.class_ref != angle.id));
    let dimension = instances
        .iter()
        .find(|relation| relation.scalar_refs().contains(&lane.scalars[2].id))
        .expect("the dimension's relation");
    assert_eq!(
        dimension.family,
        FeatureInputRelationFamily::PointPointDistance
    );
    assert_eq!(dimension.class_ref, distance);
}

/// The lane of [`token_form_lane`] with an `sgAnglDim` declaration
/// directly after the scalar at `after`, and the second dimension's
/// scalars holding two line operands, which the angle family's signature
/// fits.
fn refused_dimension_lane(after: usize) -> (FeatureInputLane, FeatureInputClass) {
    refused_dimension_lane_of(token_form_lane(0x30, 0x32), after)
}

/// [`refused_dimension_lane`] built on `lane`.
fn refused_dimension_lane_of(
    mut lane: FeatureInputLane,
    after: usize,
) -> (FeatureInputLane, FeatureInputClass) {
    let angle = class(lane.scalars[after].offset + 1, "sgAnglDim");
    lane.classes.push(angle.clone());
    for scalar in &mut lane.scalars[3..5] {
        for operand in &mut scalar.operands {
            operand.kind = FeatureInputOperandKind::Native(NativeOperandTag::TAG_8386);
        }
    }
    (lane, angle)
}

/// A dimension that an angle declaration before it does not claim, its
/// record stating a length, takes the relation class its relation-class
/// token names: the one relation class declared before that token's
/// first use.
#[test]
fn a_refused_dimension_takes_the_relation_its_token_names() {
    let (lane, angle) = refused_dimension_lane(2);
    let distance = lane
        .classes
        .iter()
        .find(|class| class.name == "sgPntPntDist")
        .unwrap()
        .id
        .clone();
    let instances = relation_instances(&sketch_history(), &lane).unwrap();
    assert!(instances
        .iter()
        .all(|relation| relation.class_ref != angle.id));
    let dimension = instances
        .iter()
        .find(|relation| relation.scalar_refs().contains(&lane.scalars[3].id))
        .expect("the refused dimension's relation");
    assert_eq!(
        (
            dimension.family,
            dimension.class_ref.clone(),
            dimension.scalar_refs().to_vec(),
            dimension.parameter_scalar_ref().map(str::to_owned),
        ),
        (
            FeatureInputRelationFamily::PointPointDistance,
            distance,
            vec![lane.scalars[3].id.clone(), lane.scalars[4].id.clone()],
            Some(lane.scalars[4].id.clone()),
        )
    );
    assert!(instances.iter().all(|relation| !relation.class_assumed));
}

/// In a compound-file lane, whose stream opens with a u16 and a class
/// declaration and does not state where the lane's own class tokens begin
/// (open item EV-05), a refused dimension takes the relation class its
/// relation-class token names when every token is read as the lane's own,
/// and its relation states that its class rests on that reading. A length
/// relation declared in place beside the first display scalar's
/// parameter-class token states that token's class.
#[test]
fn a_refused_dimension_of_a_compound_lane_takes_its_token_class_as_assumed() {
    let (mut lane, angle) =
        refused_dimension_lane_of(token_form_lane_opening(&[0x02, 0x00], 0x30, 0x32), 2);
    assert!(crate::archive::compound_opening(&lane.native_payload));
    let name = lane
        .names
        .iter()
        .find(|name| name.id == lane.scalars[1].name)
        .unwrap()
        .offset;
    // The declaration ends at prefix +2, where the relation-class token
    // would end; it is the marker, the u16 length and the name.
    let prefix = crate::layout::feature_input_scalar_class_prefix::LEN
        - crate::layout::feature_input_scalar_class_prefix::ZERO;
    let end = name - u64::try_from(prefix).unwrap();
    let in_place = "sgLLDist";
    lane.classes.push(class(
        end - u64::try_from(6 + in_place.len()).unwrap(),
        in_place,
    ));
    let distance = lane
        .classes
        .iter()
        .find(|class| class.name == "sgPntPntDist")
        .unwrap()
        .id
        .clone();
    let instances = relation_instances(&sketch_history(), &lane).unwrap();
    assert!(instances
        .iter()
        .all(|relation| relation.class_ref != angle.id));
    let dimension = instances
        .iter()
        .find(|relation| relation.scalar_refs().contains(&lane.scalars[3].id))
        .expect("the refused dimension's relation");
    assert_eq!(
        (
            dimension.class_ref.clone(),
            dimension.scalar_refs().to_vec(),
            dimension.class_assumed,
        ),
        (
            distance,
            vec![lane.scalars[3].id.clone(), lane.scalars[4].id.clone()],
            true,
        )
    );
    assert!(instances
        .iter()
        .filter(|relation| relation.class_assumed)
        .all(|relation| relation.id == dimension.id));
}

/// The relation-class token takes a declaration before the parameter-class
/// token's: where a second relation class is declared after the length
/// parameter's declaration and before the tokens' first use, the token
/// still names the first relation class.
#[test]
fn a_refused_dimension_takes_the_relation_declared_before_its_parameter() {
    let (mut lane, _) = refused_dimension_lane(2);
    lane.classes
        .push(class(lane.scalars[0].offset + 1, "sgLLDist"));
    let distance = lane
        .classes
        .iter()
        .find(|class| class.name == "sgPntPntDist")
        .unwrap()
        .id
        .clone();
    let instances = relation_instances(&sketch_history(), &lane).unwrap();
    let dimension = instances
        .iter()
        .find(|relation| relation.scalar_refs().contains(&lane.scalars[3].id))
        .expect("the refused dimension's relation");
    assert_eq!(dimension.class_ref, distance);
}

/// Where two relation classes are declared before the parameter class
/// that the refused dimension's parameter-class token names, its
/// relation-class token names neither, and the dimension forms no
/// relation.
#[test]
fn a_refused_dimension_whose_token_two_classes_precede_forms_no_relation() {
    let (mut lane, angle) = refused_dimension_lane(2);
    let parameter = lane
        .classes
        .iter()
        .find(|class| class.name == "moLengthParameter_c")
        .unwrap()
        .offset;
    lane.classes.push(class(parameter - 1, "sgLLDist"));
    let instances = relation_instances(&sketch_history(), &lane).unwrap();
    assert!(instances
        .iter()
        .all(|relation| relation.class_ref != angle.id
            && !relation.scalar_refs().contains(&lane.scalars[3].id)));
}

#[test]
fn declaration_skips_nearer_incompatible_scalar() {
    let relation_class = class(10, "sgPntPntHorDist");
    let mut native = scalar(20, FeatureInputScalarRole::Native);
    native.operands.clear();

    let driving = scalar(40, FeatureInputScalarRole::Driving);
    let lane = lane(vec![relation_class], vec![native, driving.clone()]);

    let instances = relation_instances(&sketch_history(), &lane).unwrap();
    let [relation] = instances.as_slice() else {
        panic!("one relation instance");
    };
    assert_eq!(
        relation.parameter_scalar_ref().map(str::to_owned),
        Some(driving.id.clone())
    );
    assert_eq!(relation.scalar_refs(), vec![driving.id]);
}

#[test]
fn declaration_reaches_compatible_scalar_after_auxiliary_records() {
    let relation_class = class(10, "sgPntPntHorDist");
    let driving = scalar(200, FeatureInputScalarRole::Driving);
    let mut lane = lane(vec![relation_class], vec![driving.clone()]);
    lane.names.push(FeatureInputName {
        id: "name-sketch".into(),
        parent: "lane".into(),
        ordinal: 0,
        offset: 0,
        object_id: None,
        value: "Sketch".into(),
    });

    let instances = relation_instances(&sketch_history(), &lane).unwrap();
    let [relation] = instances.as_slice() else {
        panic!("one relation instance");
    };
    assert_eq!(
        relation.parameter_scalar_ref().map(str::to_owned),
        Some(driving.id.clone())
    );
    assert_eq!(relation.scalar_refs(), vec![driving.id]);
}

#[test]
fn declaration_stops_before_the_next_relation_class() {
    let first = class(10, "sgPntPntHorDist");
    let second = class(100, "sgPntPntVertDist");
    let driving = scalar(200, FeatureInputScalarRole::Driving);
    let mut lane = lane(vec![first, second.clone()], vec![driving]);
    lane.names.push(FeatureInputName {
        id: "name-sketch".into(),
        parent: "lane".into(),
        ordinal: 0,
        offset: 0,
        object_id: None,
        value: "Sketch".into(),
    });

    let instances = relation_instances(&sketch_history(), &lane).unwrap();
    let [relation] = instances.as_slice() else {
        panic!("one relation instance");
    };
    assert_eq!(relation.class_ref, second.id);
    assert_eq!(
        relation.family,
        FeatureInputRelationFamily::PointPointVerticalDistance
    );
}

#[test]
fn display_scalar_joins_driving_scalar_after_class_declaration() {
    let vertical = class(10, "sgPntPntVertDist");
    let horizontal = class(30, "sgPntPntHorDist");
    let display = scalar(20, FeatureInputScalarRole::Display);
    let driving = scalar(40, FeatureInputScalarRole::Driving);
    let lane = lane(
        vec![vertical, horizontal.clone()],
        vec![display.clone(), driving.clone()],
    );

    let history = sketch_history();
    let instances = relation_instances(&history, &lane).unwrap();
    let [relation] = instances.as_slice() else {
        panic!("one relation instance");
    };
    assert_eq!(
        relation.family,
        FeatureInputRelationFamily::PointPointHorizontalDistance
    );
    assert_eq!(relation.class_ref, horizontal.id);
    assert_eq!(
        relation.scalar_refs(),
        vec![display.id.clone(), driving.id.clone()]
    );
    assert_eq!(
        relation.display_scalar_ref().map(str::to_owned),
        Some(display.id)
    );
    assert_eq!(
        relation.parameter_scalar_ref().map(str::to_owned),
        Some(driving.id)
    );
}

#[test]
fn display_scalars_do_not_cross_a_class_declaration() {
    let vertical = class(10, "sgPntPntVertDist");
    let horizontal = class(30, "sgPntPntHorDist");
    let first = scalar(20, FeatureInputScalarRole::Display);
    let second = scalar(40, FeatureInputScalarRole::Display);
    let lane = lane(vec![vertical, horizontal], vec![first, second]);

    let relations = relation_instances(&sketch_history(), &lane).unwrap();
    assert_eq!(relations.len(), 2);
    assert_eq!(
        relations
            .iter()
            .map(|relation| relation.family)
            .collect::<Vec<_>>(),
        vec![
            FeatureInputRelationFamily::PointPointVerticalDistance,
            FeatureInputRelationFamily::PointPointHorizontalDistance,
        ]
    );
}

#[test]
fn ambiguous_relation_declarations_leave_scalar_unbound() {
    let distance = class(10, "sgPntPntDist");
    let vertical = class(20, "sgPntPntVertDist");
    let lane = lane(
        vec![distance, vertical],
        vec![scalar(30, FeatureInputScalarRole::Driving)],
    );

    assert!(relation_instances(&sketch_history(), &lane)
        .unwrap()
        .is_empty());
}

/// Two features, so the first interval ends at the second's start. A class
/// inside the first interval reaches a scalar past its own
/// unknown-feature span but inside the interval: the scope the class takes
/// is the interval's end, not the span.
#[test]
fn a_class_inside_an_interval_takes_the_interval_end_not_the_unknown_span() {
    let mut history = sketch_history();
    history[0].features[0].id = "first".into();
    history[0].features[0].name = "First".into();
    let mut second = history[0].features[0].clone();
    second.id = "second".into();
    second.name = "Second".into();
    second.ordinal = 1;
    history[0].features.push(second);

    let mut relation_scalar = scalar(150, FeatureInputScalarRole::Driving);
    relation_scalar.feature_ref = Some("first".into());
    let mut lane = lane(
        vec![class(10, "sgPntPntHorDist")],
        vec![relation_scalar.clone()],
    );
    lane.names = vec![
        FeatureInputName {
            id: "name-first".into(),
            parent: "lane".into(),
            ordinal: 0,
            offset: 0,
            object_id: None,
            value: "First".into(),
        },
        FeatureInputName {
            id: "name-second".into(),
            parent: "lane".into(),
            ordinal: 1,
            offset: 200,
            object_id: None,
            value: "Second".into(),
        },
    ];

    let intervals = feature_intervals(&history, &lane);
    assert_eq!(
        intervals,
        vec![
            (0, Some(200), "first".to_owned()),
            (200, None, "second".to_owned()),
        ]
    );
    // 150 is past 10 + UNKNOWN_FEATURE_SPAN and inside the interval.
    assert!(relation_scalar.offset > 10 + UNKNOWN_FEATURE_SPAN);
    assert_eq!(
        relation_declaration_candidates(&lane.classes, &lane.scalars, &intervals).len(),
        1
    );
}

/// The last feature interval is open, so a class inside it is bounded by
/// nothing and reaches a scalar at the highest offset a `u64` states.
#[test]
fn a_class_in_the_open_last_interval_reaches_the_highest_scalar_offset() {
    let history = sketch_history();
    let mut relation_scalar = scalar(10, FeatureInputScalarRole::Driving);
    relation_scalar.offset = u64::MAX;
    let mut lane = lane(
        vec![class(u64::MAX - 1, "sgPntPntHorDist")],
        vec![relation_scalar],
    );
    lane.names = vec![FeatureInputName {
        id: "name-sketch".into(),
        parent: "lane".into(),
        ordinal: 0,
        offset: 0,
        object_id: None,
        value: "Sketch".into(),
    }];

    let intervals = feature_intervals(&history, &lane);
    assert_eq!(
        relation_declaration_candidates(&lane.classes, &lane.scalars, &intervals).len(),
        1,
        "the open last interval bounds no scalar offset"
    );
    assert_eq!(intervals, vec![(0, None, "sketch".to_owned())]);
}

#[test]
fn relation_declarations_do_not_cross_feature_intervals() {
    let mut history = sketch_history();
    history[0].features[0].id = "first".into();
    history[0].features[0].name = "First".into();
    let mut second = history[0].features[0].clone();
    second.id = "second".into();
    second.name = "Second".into();
    second.ordinal = 1;
    history[0].features.push(second);

    let mut relation_scalar = scalar(120, FeatureInputScalarRole::Driving);
    relation_scalar.feature_ref = Some("second".into());
    let mut lane = lane(vec![class(10, "sgPntPntDist")], vec![relation_scalar]);
    lane.names = vec![
        FeatureInputName {
            id: "name-first".into(),
            parent: "lane".into(),
            ordinal: 0,
            offset: 0,
            object_id: None,
            value: "First".into(),
        },
        FeatureInputName {
            id: "name-second".into(),
            parent: "lane".into(),
            ordinal: 1,
            offset: 100,
            object_id: None,
            value: "Second".into(),
        },
    ];

    assert!(relation_instances(&history, &lane).unwrap().is_empty());
}

#[test]
fn metadata_records_do_not_split_relation_feature_intervals() {
    let mut history = sketch_history();
    let mut metadata = history[0].features[0].clone();
    metadata.id = "attribute-definition".into();
    metadata.xml_tag = "Feature".into();
    metadata.source_id = Some(FeatureSource::Reserved);
    metadata.ordinal = 1;
    metadata.name = "Attribute-Definition".into();
    metadata.kind = "Attribute-Definition".into();
    metadata.input_class = None;
    history[0].features.push(metadata);

    let mut relation_scalar = scalar(120, FeatureInputScalarRole::Driving);
    relation_scalar.feature_ref = Some("sketch".into());
    let mut lane = lane(vec![class(110, "sgPntPntDist")], vec![relation_scalar]);
    lane.names = vec![
        FeatureInputName {
            id: "name-sketch".into(),
            parent: "lane".into(),
            ordinal: 0,
            offset: 0,
            object_id: None,
            value: "Sketch".into(),
        },
        FeatureInputName {
            id: "name-attribute".into(),
            parent: "lane".into(),
            ordinal: 1,
            offset: 100,
            object_id: None,
            value: "Attribute-Definition".into(),
        },
    ];

    let relations = relation_instances(&history, &lane).unwrap();
    let [relation] = relations.as_slice() else {
        panic!("metadata must not terminate the sketch interval");
    };
    assert_eq!(relation.feature_ref, "sketch");
}

#[test]
fn repeated_driving_scalar_starts_another_anchored_relation() {
    let lane = lane(
        vec![class(10, "sgPntPntDist")],
        vec![
            scalar(20, FeatureInputScalarRole::Display),
            scalar(30, FeatureInputScalarRole::Driving),
            scalar(40, FeatureInputScalarRole::Driving),
        ],
    );

    let relations = relation_instances(&sketch_history(), &lane).unwrap();
    assert_eq!(relations.len(), 2);
    assert_eq!(
        relations
            .iter()
            .map(|relation| relation.scalar_refs().len())
            .collect::<Vec<_>>(),
        vec![2, 1]
    );
    assert!(relations.iter().all(|relation| {
        relation.family == FeatureInputRelationFamily::PointPointDistance
            && relation.class_ref == "class-10"
    }));
}

#[test]
fn unclaimed_relation_binding_scalar_becomes_an_instance() {
    let dynamic = dynamic_scalar(
        20,
        FeatureInputOperandKind::Native(NativeOperandTag::try_from(0x812a).unwrap()),
        &[1, 2],
        2.0,
    );
    let mut bound = dynamic_scalar(30, FeatureInputOperandKind::E1, &[3, 4], 3.0);
    bound.role = FeatureInputScalarRole::Display;
    let mut lane = lane(vec![class(10, "sgLLDist")], vec![dynamic, bound.clone()]);
    lane.relation_bindings = vec![FeatureInputRelationBinding {
        id: "binding-10".into(),
        parent: lane.id.clone(),
        ordinal: 0,
        offset: 10,
        class_ref: "class-10".into(),
        family: FeatureInputRelationFamily::LineLineDistance,
        scalar_ref: bound.id,
        feature_ref: Some("sketch".into()),
    }];
    lane.relation_bindings.push(FeatureInputRelationBinding {
        id: "binding-11".into(),
        parent: lane.id.clone(),
        ordinal: 1,
        offset: 11,
        class_ref: "class-11".into(),
        family: FeatureInputRelationFamily::LineLineDistance,
        scalar_ref: "scalar-30".into(),
        feature_ref: Some("sketch".into()),
    });

    let instances = relation_instances(&sketch_history(), &lane).unwrap();
    assert_eq!(instances.len(), 2);
    assert_eq!(instances[1].offset, 30);
    assert_eq!(
        instances[1].family,
        FeatureInputRelationFamily::LineLineDistance
    );
    assert_eq!(instances[1].scalar_refs(), vec!["scalar-30"]);
    assert_eq!(instances[1].display_scalar_ref(), Some("scalar-30"));
}

#[test]
fn circle_dimension_binds_driver_inside_declared_entity_handle() {
    let first = circle_scalar(20, "name-first", FeatureInputScalarRole::Display, None);
    let nested_display = circle_scalar(40, "name-nested", FeatureInputScalarRole::Display, None);
    let nested_driver = circle_scalar(50, "name-nested", FeatureInputScalarRole::Driving, None);
    let mut lane = lane(
        vec![class(10, "sgCircleDim"), class(31, "sgEntHandle")],
        vec![first.clone(), nested_display.clone(), nested_driver.clone()],
    );
    lane.names = vec![
        FeatureInputName {
            id: "name-first".into(),
            parent: "lane".into(),
            ordinal: 0,
            offset: 20,
            object_id: None,
            value: "D1".into(),
        },
        FeatureInputName {
            id: "name-nested".into(),
            parent: "lane".into(),
            ordinal: 1,
            offset: 40,
            object_id: None,
            value: "D4".into(),
        },
    ];

    let instances = relation_instances(&sketch_history(), &lane).unwrap();
    let [relation] = instances.as_slice() else {
        panic!("one circle relation");
    };
    assert_eq!(relation.scalar_refs(), vec![first.id]);
    assert_eq!(relation.display_scalar_ref(), Some("scalar-20"));
    assert!(relation.parameter_scalar_ref().is_none());
    assert_eq!(
        CircleHandleDrivers::new(&lane)
            .driver(relation)
            .map(|scalar| scalar.id.as_str()),
        Some("scalar-50")
    );
}

#[test]
fn circle_dimension_keeps_ambiguous_handle_drivers_unbound() {
    let first = circle_scalar(20, "name-first", FeatureInputScalarRole::Display, None);
    let d4_display = circle_scalar(40, "name-d4", FeatureInputScalarRole::Display, None);
    let d4_driver = circle_scalar(50, "name-d4", FeatureInputScalarRole::Driving, None);
    let d5_display = circle_scalar(60, "name-d5", FeatureInputScalarRole::Display, None);
    let d5_driver = circle_scalar(70, "name-d5", FeatureInputScalarRole::Driving, None);
    let mut lane = lane(
        vec![class(10, "sgCircleDim"), class(31, "sgEntHandle")],
        vec![first, d4_display, d4_driver, d5_display, d5_driver],
    );
    lane.names = [("name-first", "D1"), ("name-d4", "D4"), ("name-d5", "D5")]
        .into_iter()
        .enumerate()
        .map(|(ordinal, (id, value))| FeatureInputName {
            id: id.into(),
            parent: "lane".into(),
            ordinal: u32::try_from(ordinal).expect("test index fits u32"),
            offset: 20 + cadmpeg_core::decode::u64_from_index(ordinal) * 10,
            object_id: None,
            value: value.into(),
        })
        .collect();

    let instances = relation_instances(&sketch_history(), &lane).unwrap();
    let [relation] = instances.as_slice() else {
        panic!("one circle relation");
    };
    assert_eq!(relation.scalar_refs().len(), 1);
    assert!(relation.parameter_scalar_ref().is_none());
    assert!(CircleHandleDrivers::new(&lane).driver(relation).is_none());
}

fn linear_handle_driver<'a>(
    relation: &crate::records::FeatureInputRelationInstance,
    lane: &'a FeatureInputLane,
) -> Option<&'a FeatureInputScalar> {
    use super::{relation_family, same_scalar_operands};
    if relation.family != FeatureInputRelationFamily::CircleDiameter
        || relation.parameter_scalar_ref().is_some()
        || relation.scalar_refs().len() != 1
    {
        return None;
    }
    let mut scalars = lane.scalars.iter().collect::<Vec<_>>();
    scalars.sort_unstable_by_key(|left| left.offset);
    let mut names = std::collections::HashMap::new();
    for name in &lane.names {
        names.insert(name.id.as_str(), name.value.as_str());
    }
    let first = relation
        .scalar_refs()
        .first()
        .and_then(|id| scalars.iter().find(|scalar| scalar.id == *id))
        .copied()
        .filter(|scalar| scalar.role == FeatureInputScalarRole::Display)?;
    let next_relation_offset = lane
        .classes
        .iter()
        .filter(|class| class.offset > first.offset && relation_family(&class.name).is_some())
        .map(|class| class.offset)
        .min();
    let mut candidates = scalars.windows(2).filter_map(|pair| {
        let display = pair[0];
        let driving = pair[1];
        let declared_handle = lane.classes.iter().any(|class| {
            class.name == "sgEntHandle"
                && class.offset > first.offset
                && class.offset < display.offset
        });
        let same_feature =
            display.feature_ref == first.feature_ref && driving.feature_ref == first.feature_ref;
        let same_name = matches!(
            (
                names.get(display.name.as_str()),
                names.get(driving.name.as_str())
            ),
            (Some(left), Some(right)) if left == right
        );
        let in_relation = first.offset < display.offset
            && next_relation_offset.is_none_or(|limit| driving.offset < limit);
        (declared_handle
            && same_feature
            && same_scalar_operands(display, first)
            && same_scalar_operands(driving, first)
            && display.role == FeatureInputScalarRole::Display
            && driving.role == FeatureInputScalarRole::Driving
            && same_name
            && in_relation)
            .then_some(driving)
    });
    let first = candidates.next();
    first.filter(|_| candidates.next().is_none())
}

#[test]
fn indexed_circle_handle_drivers_match_the_linear_search() {
    use crate::records::relation_scalars::RelationScalars;
    use crate::records::FeatureInputRelationInstance;

    let mut state = 0x2545_f491_u64;
    let mut next = |bound: u64| {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (state >> 33) % bound
    };
    let mut found = 0;
    for _ in 0..40 {
        let classes = (0..8)
            .map(|_| {
                let name = ["sgEntHandle", "sgEntHandle", "sgCircleDim", "moSketch_c"]
                    [usize::try_from(next(4)).unwrap()];
                class(next(300), name)
            })
            .collect();
        let scalars = (0..80)
            .map(|_| {
                let role = match next(6) {
                    0 => FeatureInputScalarRole::Native,
                    1..=3 => FeatureInputScalarRole::Display,
                    _ => FeatureInputScalarRole::Driving,
                };
                let mut scalar = circle_scalar(
                    next(300),
                    &format!("name-{}", next(4)),
                    role,
                    (next(6) == 0).then_some("entity"),
                );
                scalar.id = format!("scalar-{}", next(70));
                if next(8) == 0 {
                    scalar.feature_ref = Some("other".into());
                }
                scalar.operands[0].entity_index = u16::from(next(6) == 0);
                if next(12) == 0 {
                    scalar.operands.clear();
                }
                scalar
            })
            .collect();
        let mut lane = lane(classes, scalars);
        lane.names = (0..5)
            .map(|ordinal| FeatureInputName {
                id: format!("name-{}", ordinal % 4),
                parent: "lane".into(),
                ordinal,
                offset: u64::from(ordinal),
                object_id: None,
                value: format!("D{}", next(2)),
            })
            .collect();
        let drivers = CircleHandleDrivers::new(&lane);
        for ordinal in 0..60 {
            let mut refs = vec![format!("scalar-{}", next(75))];
            if next(10) == 0 {
                refs.push("scalar-extra".into());
            }
            let parameter = (next(10) == 0).then(|| refs[0].clone());
            let relation = FeatureInputRelationInstance {
                id: format!("relation-{ordinal}"),
                parent: "lane".into(),
                ordinal,
                offset: 0,
                family: if next(10) == 0 {
                    FeatureInputRelationFamily::LineLineDistance
                } else {
                    FeatureInputRelationFamily::CircleDiameter
                },
                class_ref: "class".into(),
                feature_ref: "sketch".into(),
                scalars: RelationScalars::from_refs(refs, parameter, None).unwrap(),
                operands: Vec::new(),
                class_assumed: false,
            };
            let linear = linear_handle_driver(&relation, &lane);
            assert_eq!(
                drivers.driver(&relation).map(std::ptr::from_ref),
                linear.map(std::ptr::from_ref)
            );
            found += usize::from(linear.is_some());
        }
    }
    assert!(found > 0);
}

#[test]
fn circle_display_aggregate_matches_the_linear_repeat_check() {
    use super::{CircleDisplays, RelationGroup};
    use std::collections::HashMap;

    fn linear(
        group: &RelationGroup<'_>,
        scalar: &FeatureInputScalar,
        names: &HashMap<&str, &str>,
    ) -> bool {
        let scalar_name = names.get(scalar.name.as_str()).copied();
        scalar.role == FeatureInputScalarRole::Display
            && scalar.operands.len() == 1
            && group.scalars.iter().all(|(_, candidate)| {
                candidate.role == FeatureInputScalarRole::Display
                    && candidate.operands.len() == 1
                    && candidate.operands[0].kind == scalar.operands[0].kind
                    && candidate.operands[0].entity_index != scalar.operands[0].entity_index
                    && scalar_name.is_some_and(|name| {
                        names.get(candidate.name.as_str()).copied() == Some(name)
                    })
            })
    }

    let mut state = 0x2545_f491_u64;
    let mut next = |bound: u64| {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (state >> 33) % bound
    };
    let names = HashMap::from([("name-0", "D1"), ("name-1", "D1"), ("name-2", "D2")]);
    let scalars = (0..3_000)
        .map(|offset| {
            let role = match next(8) {
                0 => FeatureInputScalarRole::Driving,
                1 => FeatureInputScalarRole::Native,
                _ => FeatureInputScalarRole::Display,
            };
            let name = if next(4) == 0 {
                format!("name-{}", next(4))
            } else {
                "name-0".to_string()
            };
            let mut scalar = circle_scalar(offset, &name, role, None);
            scalar.operands[0].entity_index = u16::try_from(next(10)).unwrap();
            if next(10) == 0 {
                scalar.operands[0].kind = FeatureInputOperandKind::E1;
            }
            match next(20) {
                0 => scalar.operands.clear(),
                1 => scalar.operands.push(scalar.operands[0].clone()),
                _ => {}
            }
            scalar
        })
        .collect::<Vec<_>>();
    let name_of = |scalar: &FeatureInputScalar| names.get(scalar.name.as_str()).copied();
    let mut repeats = 0;
    let mut rest = scalars.iter().enumerate();
    while let Some((index, first)) = rest.next() {
        let mut group = RelationGroup {
            feature_ref: "sketch",
            family: FeatureInputRelationFamily::CircleDiameter,
            class_ref: "class",
            class_assumed: false,
            operands: &first.operands,
            scalars: vec![(index, first)],
            circle_displays: CircleDisplays::of(first, name_of(first)),
        };
        for (index, scalar) in rest.by_ref().take(usize::try_from(next(12)).unwrap()) {
            let repeated = linear(&group, scalar, &names);
            assert_eq!(
                group
                    .circle_displays
                    .as_ref()
                    .is_some_and(|displays| displays.repeated_by(scalar, name_of(scalar))),
                repeated
            );
            repeats += usize::from(repeated && group.scalars.len() > 1);
            if repeated || next(4) == 0 {
                group.push(index, scalar, name_of(scalar));
            }
        }
    }
    assert!(repeats > 0);
}

#[test]
fn circle_dimension_groups_repeated_display_scalars_by_name_and_entity() {
    let mut first = circle_scalar(20, "name-d1", FeatureInputScalarRole::Display, None);
    first.operands[0].entity_index = 0;
    let mut second = circle_scalar(30, "name-d1", FeatureInputScalarRole::Display, None);
    second.operands[0].entity_index = 1;
    let mut third = circle_scalar(40, "name-d1", FeatureInputScalarRole::Display, None);
    third.operands[0].entity_index = 2;
    let mut different_name = circle_scalar(50, "name-d2", FeatureInputScalarRole::Display, None);
    different_name.operands[0].entity_index = 3;
    let mut lane = lane(
        vec![class(10, "sgCircleDim")],
        vec![first.clone(), second, third, different_name],
    );
    lane.names = vec![
        FeatureInputName {
            id: "name-d1".into(),
            parent: "lane".into(),
            ordinal: 0,
            offset: 20,
            object_id: None,
            value: "D1".into(),
        },
        FeatureInputName {
            id: "name-d2".into(),
            parent: "lane".into(),
            ordinal: 1,
            offset: 50,
            object_id: None,
            value: "D2".into(),
        },
    ];

    let instances = relation_instances(&sketch_history(), &lane).unwrap();
    let [relation] = instances.as_slice() else {
        panic!("one repeated circle relation");
    };
    assert_eq!(
        relation.scalar_refs(),
        vec!["scalar-20", "scalar-30", "scalar-40"]
    );
    assert!(relation.parameter_scalar_ref().is_none());
    assert!(relation.display_scalar_ref().is_none());
    assert_eq!(relation.operands, first.operands);
}

fn dynamic_scalar(
    offset: u64,
    kind: FeatureInputOperandKind,
    indices: &[u16],
    value: f64,
) -> FeatureInputScalar {
    let mut scalar = scalar(offset, FeatureInputScalarRole::Driving);
    scalar.value = cadmpeg_ir::scalar::FiniteReal::new(value).expect("finite test scalar");

    scalar.operands = indices
        .iter()
        .enumerate()
        .map(|(ordinal, entity_index)| FeatureInputOperand {
            offset: offset + cadmpeg_core::decode::u64_from_index(ordinal),
            reference_ref: format!("reference-{offset}-{ordinal}"),
            kind,
            entity_index: *entity_index,
            entity_ref: None,
        })
        .collect();
    scalar
}
