//! Sketch dimensions of a compound-file lane whose relation class comes only
//! from their relation-class token (open item EV-05).

use super::{
    class, linear, refused_dimension_lane_of, relation_instances, sketch_history,
    token_form_lane_opening, FeatureInputLane, FeatureInputScalarRole,
};
use std::collections::BTreeSet;

/// The lane of [`refused_dimension_lane`] in a compound-file stream, which
/// opens with a u16 and a class declaration and does not state where the
/// lane's own class tokens begin (open item EV-05). A length relation
/// declared in place beside the first display scalar's parameter-class
/// token states that token's class. The refused dimension's display scalar
/// stores `display` metres, and its sketch holds four points, the fourth at
/// `reach` metres along the x axis from the second, which its operands name.
fn compound_refused_dimension_lane(display: f64, reach: f64) -> (FeatureInputLane, String) {
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
    lane.scalars[3].value = cadmpeg_ir::scalar::FiniteReal::new(display).unwrap();
    lane.sketch_entities = [[1.0, 1.0], [0.0, 0.0], [5.0, 5.0], [reach, 0.0]]
        .into_iter()
        .zip(0u32..)
        .map(|(coordinates, ordinal)| {
            let mut point = crate::records::SketchInputEntity::new(
                format!("point-{ordinal}"),
                "lane",
                ordinal,
                10 * u64::from(ordinal + 1),
                crate::records::SketchInputKind::Point,
            );
            point.feature_ref = Some("sketch".into());
            point.coordinates_m = cadmpeg_ir::units::FiniteVector::new(coordinates);
            point
        })
        .collect();
    (lane, angle.id)
}

/// The display value of the refused dimension in
/// [`compound_refused_dimension_lane`] where it differs from the driving value.
const DISPLAY_METRES: f64 = 0.0176;

/// The driving value of the refused dimension in
/// [`compound_refused_dimension_lane`].
const DRIVING_METRES: f64 = 0.025;

/// Whether a relation of `instances` holds `scalar`.
fn holds(instances: &[crate::records::FeatureInputRelationInstance], scalar: &str) -> bool {
    instances
        .iter()
        .any(|relation| relation.scalar_refs().iter().any(|id| id == scalar))
}

/// The code of each note of `notes` and the identities it names in
/// backticks.
fn named_notes(
    notes: &[cadmpeg_ir::report::loss::LossNote],
) -> Vec<(cadmpeg_ir::report::loss::LossKind, BTreeSet<&str>)> {
    notes
        .iter()
        .map(|note| {
            (
                note.code.clone(),
                note.message.split('`').skip(1).step_by(2).collect(),
            )
        })
        .collect()
}

/// The `sketch.dimension-class-unknown` note that names `ids`.
fn unknown_class<'a>(ids: &[&'a str]) -> (cadmpeg_ir::report::loss::LossKind, BTreeSet<&'a str>) {
    (
        crate::loss::SldprtLossCode::SketchDimensionClassUnknown.kind(),
        ids.iter().copied().collect(),
    )
}

/// The `sketch.dimension-repeated` note that names the dropped scalars
/// `ids` and the kept relation `relation`.
fn repeated<'a>(
    ids: &[&'a str],
    relation: &'a str,
) -> (cadmpeg_ir::report::loss::LossKind, BTreeSet<&'a str>) {
    (
        crate::loss::SldprtLossCode::SketchDimensionRepeated.kind(),
        ids.iter().copied().chain([relation]).collect(),
    )
}

/// In a compound-file lane, a refused dimension takes the relation class
/// its relation-class token names where the sketch points that its relation
/// binds measure the relation's driving value, also when its display scalar
/// stores another value.
#[test]
fn a_refused_dimension_of_a_compound_lane_forms_where_its_sketch_points_measure_it() {
    let (lane, angle) = compound_refused_dimension_lane(DISPLAY_METRES, DRIVING_METRES);
    let distance = lane
        .classes
        .iter()
        .find(|class| class.name == "sgPntPntDist")
        .unwrap()
        .id
        .clone();
    let relations = relation_instances(&sketch_history(), &lane, linear()).unwrap();
    let instances = relations.instances;
    assert!(instances.iter().all(|relation| relation.class_ref != angle));
    let dimension = instances
        .iter()
        .find(|relation| relation.scalar_refs().contains(&lane.scalars[3].id))
        .expect("the refused dimension's relation");
    assert_eq!(
        (
            dimension.class_ref.clone(),
            dimension.scalar_refs().to_vec(),
            dimension
                .operands
                .iter()
                .map(|operand| operand.entity_ref.clone())
                .collect::<Vec<_>>(),
        ),
        (
            distance,
            vec![lane.scalars[3].id.clone(), lane.scalars[4].id.clone()],
            vec![Some("point-1".to_owned()), Some("point-3".to_owned())],
        )
    );
    assert!(relations.notes.is_empty());
}

/// In a compound-file lane, the relation of a refused dimension takes its
/// value from the driving scalar: the dimension forms where its sketch
/// points measure the driving value, and stays native where they measure
/// the display value.
#[test]
fn a_refused_dimension_of_a_compound_lane_is_measured_by_its_driving_value() {
    for (reach, forms) in [(DRIVING_METRES, true), (DISPLAY_METRES, false)] {
        let (lane, _) = compound_refused_dimension_lane(DISPLAY_METRES, reach);
        let instances = relation_instances(&sketch_history(), &lane, linear())
            .unwrap()
            .instances;
        assert_eq!(holds(&instances, &lane.scalars[3].id), forms, "{reach}");
        assert!(instances
            .iter()
            .enumerate()
            .all(|(ordinal, relation)| relation.ordinal == u32::try_from(ordinal).unwrap()));
    }
}

/// In a compound-file lane, a refused dimension forms where its sketch
/// points measure its value within the part's linear tolerance. Beyond it
/// the dimension stays native, and one loss note names its scalars and
/// states that their class is unknown.
#[test]
fn a_refused_dimension_of_a_compound_lane_is_measured_within_the_linear_tolerance() {
    const TOLERANCE_MM: f64 = 1.0e-3;
    const TOLERANCE_M: f64 = TOLERANCE_MM / 1000.0;
    let tolerance = cadmpeg_ir::scalar::PositiveLength::new(TOLERANCE_MM).unwrap();
    let (lane, _) =
        compound_refused_dimension_lane(DISPLAY_METRES, DRIVING_METRES + 0.5 * TOLERANCE_M);
    let relations = relation_instances(&sketch_history(), &lane, tolerance).unwrap();
    assert!(holds(&relations.instances, &lane.scalars[3].id));
    assert!(relations.notes.is_empty());
    let relations = relation_instances(&sketch_history(), &lane, linear()).unwrap();
    assert!(!holds(&relations.instances, &lane.scalars[3].id));

    let (lane, _) =
        compound_refused_dimension_lane(DISPLAY_METRES, DRIVING_METRES + 2.0 * TOLERANCE_M);
    let relations = relation_instances(&sketch_history(), &lane, tolerance).unwrap();
    assert!(!holds(&relations.instances, &lane.scalars[3].id));
    assert_eq!(
        named_notes(&relations.notes),
        vec![unknown_class(&[&lane.scalars[3].id, &lane.scalars[4].id])]
    );
}

/// In a compound-file lane, a refused dimension whose sketch points measure
/// its value forms also where a driving scalar after it, with the same
/// operands, takes its class and measures no value of those points. The
/// loss note names that driving scalar alone.
#[test]
fn a_measured_refused_dimension_forms_beside_a_following_driving_value_that_does_not() {
    let (mut lane, _) = compound_refused_dimension_lane(DISPLAY_METRES, DRIVING_METRES);
    let mut following = lane.scalars[4].clone();
    following.id = format!("{}-following", following.id);
    following.offset += 1;
    following.value = cadmpeg_ir::scalar::FiniteReal::new(0.04).unwrap();
    lane.scalars.push(following);
    let relations = relation_instances(&sketch_history(), &lane, linear()).unwrap();
    assert!(holds(&relations.instances, &lane.scalars[3].id));
    assert!(holds(&relations.instances, &lane.scalars[4].id));
    assert!(!holds(&relations.instances, &lane.scalars[5].id));
    assert_eq!(
        named_notes(&relations.notes),
        vec![unknown_class(&[&lane.scalars[5].id])]
    );
}

/// In a compound-file lane, a driving scalar after a measured refused
/// dimension, with the same operands and value, takes that dimension's
/// class and measures the same two sketch points. The lane keeps the first
/// relation in lane order, forms no second relation on those points, and
/// one `sketch.dimension-repeated` note names the driving scalar and the
/// kept relation.
#[test]
fn a_following_driving_value_that_repeats_a_measured_refused_dimension_forms_no_second_relation() {
    let (mut lane, _) = compound_refused_dimension_lane(DISPLAY_METRES, DRIVING_METRES);
    let mut following = lane.scalars[4].clone();
    following.id = format!("{}-following", following.id);
    following.offset += 1;
    lane.scalars.push(following);
    let relations = relation_instances(&sketch_history(), &lane, linear()).unwrap();
    let on_points = relations
        .instances
        .iter()
        .filter(|relation| {
            let mut points = relation
                .operands
                .iter()
                .map(|operand| operand.entity_ref.as_deref())
                .collect::<Vec<_>>();
            points.sort_unstable();
            points == [Some("point-1"), Some("point-3")]
        })
        .collect::<Vec<_>>();
    let [kept] = on_points.as_slice() else {
        panic!("one relation on the two points: {on_points:#?}");
    };
    assert_eq!(
        kept.scalar_refs(),
        [lane.scalars[3].id.clone(), lane.scalars[4].id.clone()]
    );
    assert!(!holds(&relations.instances, &lane.scalars[5].id));
    assert_eq!(
        named_notes(&relations.notes),
        vec![repeated(&[&lane.scalars[5].id], &kept.id)]
    );
}

/// In a compound-file lane, a refused dimension whose relation measures the
/// two sketch points of a relation of the same family that the lane
/// declares, in either order, forms no second relation on them, and one
/// `sketch.dimension-repeated` note names its scalars and the declared
/// relation.
#[test]
fn a_refused_dimension_on_the_points_of_a_declared_relation_forms_no_second_relation() {
    for indices in [[1, 3], [3, 1]] {
        let (mut lane, _) = compound_refused_dimension_lane(DISPLAY_METRES, DRIVING_METRES);
        let declared = &mut lane.scalars[0];
        declared.role = FeatureInputScalarRole::Display;
        declared.value = cadmpeg_ir::scalar::FiniteReal::new(DRIVING_METRES).unwrap();
        for (operand, index) in declared.operands.iter_mut().zip(indices) {
            operand.entity_index = index;
        }
        let relations = relation_instances(&sketch_history(), &lane, linear()).unwrap();
        let declared = relations
            .instances
            .iter()
            .find(|relation| relation.scalar_refs().contains(&lane.scalars[0].id))
            .expect("the declared relation");
        let mut points = declared
            .operands
            .iter()
            .map(|operand| operand.entity_ref.clone())
            .collect::<Vec<_>>();
        points.sort();
        assert_eq!(
            points,
            vec![Some("point-1".to_owned()), Some("point-3".to_owned())],
            "{indices:?}"
        );
        assert!(
            !holds(&relations.instances, &lane.scalars[3].id)
                && !holds(&relations.instances, &lane.scalars[4].id),
            "{indices:?}"
        );
        assert_eq!(
            named_notes(&relations.notes),
            vec![repeated(
                &[&lane.scalars[3].id, &lane.scalars[4].id],
                &declared.id
            )],
            "{indices:?}"
        );
    }
}

/// In a compound-file lane, a refused dimension whose relation-class token
/// names a class outside the point-distance families stays native, and the
/// loss note names its display and driving scalars. The two relation
/// classes of [`compound_refused_dimension_lane`] trade names, and each
/// declaration keeps its end.
#[test]
fn a_refused_dimension_of_a_compound_lane_whose_token_names_another_family_stays_native() {
    let (mut lane, _) = compound_refused_dimension_lane(DISPLAY_METRES, DRIVING_METRES);
    for class in &mut lane.classes {
        let name = match class.name.as_str() {
            "sgPntPntDist" => "sgLLDist",
            "sgLLDist" => "sgPntPntDist",
            _ => continue,
        };
        let end = class.offset + u64::try_from(class.name.len()).unwrap();
        class.offset = end - u64::try_from(name.len()).unwrap();
        class.name = name.to_owned();
    }
    let tokens = crate::resolved_features::parameter_classes::token_relation_classes(
        &lane,
        &std::collections::HashSet::from(["sketch"]),
    );
    assert_eq!(
        tokens
            .get(lane.scalars[3].id.as_str())
            .map(|class| class.name.as_str()),
        Some("sgLLDist")
    );
    let relations = relation_instances(&sketch_history(), &lane, linear()).unwrap();
    assert!(
        !holds(&relations.instances, &lane.scalars[3].id)
            && !holds(&relations.instances, &lane.scalars[4].id)
    );
    assert_eq!(
        named_notes(&relations.notes),
        vec![unknown_class(&[&lane.scalars[3].id, &lane.scalars[4].id])]
    );
}

/// In a compound-file lane, a refused dimension that stores only a display
/// scalar forms where its sketch points measure the display value.
#[test]
fn a_display_only_refused_dimension_of_a_compound_lane_forms_where_its_points_measure_it() {
    let (mut lane, _) = compound_refused_dimension_lane(DISPLAY_METRES, DISPLAY_METRES);
    lane.scalars.remove(4);
    let relations = relation_instances(&sketch_history(), &lane, linear()).unwrap();
    let dimension = relations
        .instances
        .iter()
        .find(|relation| relation.scalar_refs().contains(&lane.scalars[3].id))
        .expect("the display-only dimension's relation");
    assert_eq!(
        (
            dimension.scalar_refs().to_vec(),
            dimension.parameter_scalar_ref(),
            dimension
                .operands
                .iter()
                .map(|operand| operand.entity_ref.clone())
                .collect::<Vec<_>>(),
        ),
        (
            vec![lane.scalars[3].id.clone()],
            None,
            vec![Some("point-1".to_owned()), Some("point-3".to_owned())],
        )
    );
    assert!(relations.notes.is_empty());
}

/// In a compound-file lane, a driving scalar with no operands that carries
/// the name of a refused dimension's display scalar gives that dimension's
/// relation its value. Where the sketch points measure that value the
/// relation forms and holds the driving scalar; elsewhere the loss note
/// names the display scalar and the driving scalar.
#[test]
fn a_detached_driving_value_of_a_refused_dimension_is_held_or_named_in_the_loss_note() {
    for (reach, forms) in [(DRIVING_METRES, true), (0.03, false)] {
        let (mut lane, _) = compound_refused_dimension_lane(DISPLAY_METRES, reach);
        lane.scalars[4].operands.clear();
        let driver = lane.scalars[4].id.clone();
        let relations = relation_instances(&sketch_history(), &lane, linear()).unwrap();
        let dimension = relations
            .instances
            .iter()
            .find(|relation| relation.scalar_refs().contains(&lane.scalars[3].id));
        assert_eq!(
            dimension.and_then(|relation| relation.parameter_scalar_ref()),
            forms.then_some(driver.as_str()),
            "{reach}"
        );
        if forms {
            assert!(relations.notes.is_empty());
        } else {
            assert!(!holds(&relations.instances, &driver));
            assert_eq!(
                named_notes(&relations.notes),
                vec![unknown_class(&[&lane.scalars[3].id, &driver])]
            );
        }
    }
}

/// In a compound-file lane, a refused dimension whose sketch points measure
/// its value leaves the relations of the lane's declarations as they are
/// without it: a driving scalar with no operands that carries the display
/// name of both stays with the declared relation. The scalar that
/// `sgPntPntDist` claims is a display scalar `D1` here.
#[test]
fn a_measured_refused_dimension_of_a_compound_lane_leaves_the_declared_relations_unchanged() {
    let (mut lane, _) = compound_refused_dimension_lane(DISPLAY_METRES, DISPLAY_METRES);
    lane.scalars.remove(4);
    lane.scalars[0].role = FeatureInputScalarRole::Display;
    lane.scalars[2].operands.clear();
    lane.scalars[3].name = lane.scalars[1].name.clone();
    let driver = lane.scalars[2].id.clone();
    let refused = lane.scalars[3].id.clone();
    let mut declared = lane.clone();
    declared.scalars.remove(3);
    let expected = relation_instances(&sketch_history(), &declared, linear())
        .unwrap()
        .instances;
    assert!(expected
        .iter()
        .any(|relation| relation.parameter_scalar_ref() == Some(driver.as_str())));

    let instances = relation_instances(&sketch_history(), &lane, linear())
        .unwrap()
        .instances;
    assert!(holds(&instances, &refused));
    assert_eq!(
        instances
            .into_iter()
            .filter(|relation| !relation.scalar_refs().contains(&refused))
            .collect::<Vec<_>>(),
        expected
    );
}
