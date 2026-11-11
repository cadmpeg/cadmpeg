// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::feature::{
    edge_selection::{geometry::EdgeGeometry, historical::SelectedEdge},
    face_selection::{SelectedFace, Surface},
    tests::{test_feature, test_property},
    PmDcFeatureEnumFamily, PmDcFeaturePropertyKind,
};
use cadmpeg_ir::{
    features::{edge_treatments::ChamferGroup, FeatureEvaluation, NonEmptyMembers},
    math::Point3,
    units::UnitVector3,
};

/// Lengths of slots 2 and 3; the first is the larger one.
const SLOT_FIRST: f64 = 3.0;
const SLOT_SECOND: f64 = 2.0;
const EDGE_SOURCE: u32 = 9;
const FACE_SOURCE: u32 = 9;
const EDGE_FACES: [i64; 2] = [21, 22];

fn point(x: f64) -> cadmpeg_ir::features::FinitePoint3 {
    cadmpeg_ir::features::FinitePoint3::new(Point3::new(x, 0.0, 0.0)).expect("point")
}

fn context() -> DecodeContext<'static> {
    // The arena outlives every test context.
    let arena: &'static cadmpeg_core::decode::DecodeArena =
        Box::leak(Box::new(cadmpeg_core::decode::DecodeArena::new()));
    DecodeContext::from_root_bytes(&[], arena, &cadmpeg_core::decode::DecodePolicy::service())
        .expect("context")
        .0
}

fn token(inventory: &FeatureInventory) -> String {
    inventory.features[0]
        .identity
        .segment_token
        .as_str()
        .to_owned()
}

/// An unequal two-distance chamfer of form 2 with one edge selector and one
/// support-face selector, both joined, and the native oriented group.
fn fixture() -> (FeatureInventory, Feature) {
    let (original, mut properties) = crate::feature::chamfer::tests::fixture(2);
    let mut slots = original
        .properties
        .references()
        .iter()
        .enumerate()
        .filter_map(|(slot, reference)| Some((slot, reference.record_ordinal()?)))
        .collect::<Vec<_>>();
    for (slot, ordinal, value) in [(6, 80, true), (9, 81, false)] {
        slots.push((slot, ordinal));
        properties.push(test_property(
            ordinal,
            PmDcFeaturePropertyKind::Boolean {
                name: String::new(),
                name_value: 0,
                value,
            },
        ));
    }
    // The result-body collection of slot 11 holds one surface body.
    slots.push((11, 90));
    properties.push(test_property(
        90,
        PmDcFeaturePropertyKind::References {
            family: crate::feature::PmDcFeatureReferenceFamily::ObjectCollection,
            items: crate::feature::tests::reference_list(&[92]),
        },
    ));
    properties.push(test_property(
        91,
        PmDcFeaturePropertyKind::SurfaceBody {
            body: crate::feature::tests::reference(0),
        },
    ));
    let source = test_feature(
        original.identity.record_ordinal,
        original.properties.references().len(),
        &slots,
    );
    let label = crate::feature::fillet::tests::label(true);
    let mut feature =
        crate::feature::native_feature(&context(), &source.identity, Some(&label), source.id())
            .expect("feature");
    feature.evaluation = FeatureEvaluation::from_definition(FeatureDefinition::Operation(
        FeatureOperation::Chamfer {
            groups: NonEmptyMembers::one(ChamferGroup {
                edges: EdgeSelection::Native(source.id()),
                spec: ChamferSpec::Unresolved {
                    form: Some(ChamferForm::TwoDistances),
                },
            }),
            flip_direction: true,
        },
    ));
    let mut inventory = FeatureInventory {
        features: vec![source],
        properties,
        labels: vec![label],
        ..FeatureInventory::default()
    };
    let token = token(&inventory);
    inventory.edge_inputs.historical.insert(
        (token.clone(), EDGE_SOURCE),
        SelectedEdge {
            owner: 1,
            owners: 1,
            generators: 1,
            item: 2,
            geometry: EdgeGeometry {
                start: point(0.0),
                end: point(2.0),
                interior: point(1.0),
                tangent: UnitVector3::X_AXIS,
            },
            faces: Some(EDGE_FACES),
        },
    );
    inventory.edge_inputs.faces.insert(
        (token, FACE_SOURCE),
        SelectedFace {
            schema: 42,
            owner: 5,
            owners: vec![5, EDGE_FACES[0]],
            generators: 1,
            repeated_attribute: false,
            unshared_family: false,
            identical_owners: false,
            form: face_selection::SelectorForm::Direct,
            item: 6,
            surface: Surface::Plane,
            state: -1,
            feature_sources: Vec::new(),
        },
    );
    (inventory, feature)
}

fn run(inventory: &FeatureInventory, feature: &Feature) -> (Feature, Vec<FeatureInputTopology>) {
    let mut features = vec![feature.clone()];
    let states = project(
        &context(),
        &mut features,
        inventory,
        |token, reference| {
            inventory.properties.iter().find(|property| {
                property.identity.segment_token.as_str() == token
                    && Some(property.identity.record_ordinal) == reference.record_ordinal()
            })
        },
        |_, slot| match slot {
            2 => Length::new(SLOT_FIRST),
            3 => Length::new(SLOT_SECOND),
            _ => None,
        },
    )
    .expect("projection");
    (features.remove(0), states)
}

fn group(feature: &Feature) -> &ChamferGroup {
    let FeatureDefinition::Operation(FeatureOperation::Chamfer { groups, .. }) =
        feature.evaluation.definition()
    else {
        panic!("chamfer")
    };
    &groups.as_slice()[0]
}

fn length(value: f64) -> PositiveLength {
    PositiveLength::new(value).expect("length")
}

#[test]
fn the_first_distance_is_slot_two_on_the_support_face_and_the_second_is_slot_three() {
    let (inventory, feature) = fixture();
    let (projected, states) = run(&inventory, &feature);
    // The slot-2 length is the larger one, so a swap of the slots changes the result.
    assert_eq!(
        group(&projected).spec,
        ChamferSpec::TwoDistances {
            first: length(SLOT_FIRST),
            second: length(SLOT_SECOND),
        }
    );
    let token = token(&inventory);
    let collection = inventory.properties[0].id();
    let EdgeSelection::Historical {
        state,
        edges,
        native,
    } = &group(&projected).edges
    else {
        panic!("historical edges")
    };
    assert_eq!(native.as_str(), collection);
    assert_eq!(
        edges.as_slice(),
        [historical::edge_id(&token, EDGE_SOURCE).expect("edge")]
    );
    let [row] = states.as_slice() else {
        panic!("one state")
    };
    assert_eq!(row.id, *state);
    assert_eq!(row.input_of, projected.id);
    let face = face_selection::face_id(&token, FACE_SOURCE).expect("face");
    assert_eq!(row.faces.as_slice(), std::slice::from_ref(&face));
    assert_eq!(row.edge_geometry[0].faces.as_slice(), [face]);
    assert_eq!(row.edge_geometry[0].interior, point(1.0));
}

#[test]
fn the_support_face_may_be_either_face_of_the_edge() {
    let (mut inventory, feature) = fixture();
    let key = (token(&inventory), FACE_SOURCE);
    inventory
        .edge_inputs
        .faces
        .get_mut(&key)
        .expect("face")
        .owners = vec![5, EDGE_FACES[1]];
    let (projected, states) = run(&inventory, &feature);
    assert!(matches!(
        group(&projected).spec,
        ChamferSpec::TwoDistances { .. }
    ));
    assert_eq!(states.len(), 1);
}

#[test]
fn every_missing_proof_of_the_support_side_keeps_the_native_group() {
    type Edit = fn(&mut FeatureInventory, &mut Feature);
    let edits: [(&str, Edit); 17] = [
        ("face selector stores state 2", |inventory, _| {
            let key = (token(inventory), FACE_SOURCE);
            inventory
                .edge_inputs
                .faces
                .get_mut(&key)
                .expect("selected")
                .state = 2;
        }),
        ("both faces of the edge own the selector", |inventory, _| {
            let key = (token(inventory), FACE_SOURCE);
            inventory
                .edge_inputs
                .faces
                .get_mut(&key)
                .expect("selected")
                .owners = vec![EDGE_FACES[0], EDGE_FACES[1]];
        }),
        ("no owner uses the edge", |inventory, _| {
            let key = (token(inventory), FACE_SOURCE);
            inventory
                .edge_inputs
                .faces
                .get_mut(&key)
                .expect("selected")
                .owners = vec![5, 6];
        }),
        ("edge has no face pair", |inventory, _| {
            let key = (token(inventory), EDGE_SOURCE);
            inventory
                .edge_inputs
                .historical
                .get_mut(&key)
                .expect("selected")
                .faces = None;
        }),
        ("edge has several owners", |inventory, _| {
            let key = (token(inventory), EDGE_SOURCE);
            inventory
                .edge_inputs
                .historical
                .get_mut(&key)
                .expect("selected")
                .owners = 2;
        }),
        ("edge item differs", |inventory, _| {
            let key = (token(inventory), EDGE_SOURCE);
            inventory
                .edge_inputs
                .historical
                .get_mut(&key)
                .expect("selected")
                .item = 3;
        }),
        ("face item differs", |inventory, _| {
            let key = (token(inventory), FACE_SOURCE);
            inventory
                .edge_inputs
                .faces
                .get_mut(&key)
                .expect("selected")
                .item = 7;
        }),
        ("face selector has three generators", |inventory, _| {
            let key = (token(inventory), FACE_SOURCE);
            inventory
                .edge_inputs
                .faces
                .get_mut(&key)
                .expect("selected")
                .generators = 3;
        }),
        (
            "face selector repeats its matching attribute",
            |inventory, _| {
                let key = (token(inventory), FACE_SOURCE);
                inventory
                    .edge_inputs
                    .faces
                    .get_mut(&key)
                    .expect("selected")
                    .repeated_attribute = true;
            },
        ),
        (
            "face selector has an unshared generated-name family",
            |inventory, _| {
                let key = (token(inventory), FACE_SOURCE);
                inventory
                    .edge_inputs
                    .faces
                    .get_mut(&key)
                    .expect("selected")
                    .unshared_family = true;
            },
        ),
        ("edge selector is not joined", |inventory, _| {
            inventory.edge_inputs.historical.clear();
        }),
        ("face selector is not joined", |inventory, _| {
            inventory.edge_inputs.faces.clear();
        }),
        ("two edge selectors", |inventory, _| {
            inventory.properties[1].edit(|payload| {
                if let PmDcFeaturePropertyKind::EdgeItem {
                    index_references, ..
                } = &mut payload.kind
                {
                    *index_references = crate::pmdc::PmDcU32List::new(
                        2,
                        Some(crate::pmdc::PmDcListMetadata::U32([1, 0])),
                        vec![EDGE_SOURCE, EDGE_SOURCE + 1],
                    )
                    .expect("list");
                }
            });
        }),
        ("two face selectors", |inventory, _| {
            inventory.properties[5].edit(|payload| {
                if let PmDcFeaturePropertyKind::FaceItem { index_references } = &mut payload.kind {
                    *index_references = crate::pmdc::PmDcU32List::new(
                        2,
                        Some(crate::pmdc::PmDcListMetadata::U32([1, 0])),
                        vec![FACE_SOURCE, FACE_SOURCE + 1],
                    )
                    .expect("list");
                }
            });
        }),
        ("direction control is false", |_, feature| {
            feature.evaluation.edit(|definition, _| {
                if let FeatureDefinition::Operation(FeatureOperation::Chamfer {
                    flip_direction,
                    ..
                }) = definition
                {
                    *flip_direction = false;
                }
            });
        }),
        ("edge control is false", |inventory, _| {
            let control = inventory
                .properties
                .iter_mut()
                .find(|property| property.identity.record_ordinal == 80)
                .expect("control");
            control.edit(|payload| {
                if let PmDcFeaturePropertyKind::Boolean { value, .. } = &mut payload.kind {
                    *value = false;
                }
            });
        }),
        ("form is distance and angle", |inventory, _| {
            inventory.properties[2].edit(|payload| {
                if let PmDcFeaturePropertyKind::Enumeration { family, value, .. } =
                    &mut payload.kind
                {
                    assert_eq!(*family, PmDcFeatureEnumFamily::Chamfer);
                    *value = 1;
                }
            });
        }),
    ];
    for (label, edit) in edits {
        let (mut inventory, mut feature) = fixture();
        edit(&mut inventory, &mut feature);
        let (projected, states) = run(&inventory, &feature);
        assert!(states.is_empty(), "{label}");
        assert_eq!(group(&projected).edges, group(&feature).edges, "{label}");
        assert_eq!(
            group(&projected).spec,
            ChamferSpec::Unresolved {
                form: Some(ChamferForm::TwoDistances)
            },
            "{label}"
        );
    }
}

#[test]
fn a_chamfer_with_resolved_dimensions_is_not_rewritten() {
    let (inventory, mut feature) = fixture();
    feature.evaluation.edit(|definition, _| {
        if let FeatureDefinition::Operation(FeatureOperation::Chamfer { groups, .. }) = definition {
            for group in groups.iter_mut() {
                group.spec = ChamferSpec::TwoDistances {
                    first: length(1.0),
                    second: length(1.0),
                };
            }
        }
    });
    let (projected, states) = run(&inventory, &feature);
    assert!(states.is_empty());
    assert_eq!(group(&projected), group(&feature));
}

fn operands_of(inventory: &FeatureInventory) -> Operands<'_> {
    crate::feature::chamfer::operands(
        &inventory.features[0],
        |reference| {
            inventory.properties.iter().find(|property| {
                Some(property.identity.record_ordinal) == reference.record_ordinal()
            })
        },
        |slot| match slot {
            2 => Length::new(SLOT_FIRST),
            3 => Length::new(SLOT_SECOND),
            _ => None,
        },
        |_| None,
    )
    .expect("operands")
}

#[test]
fn the_validator_accepts_the_projection_and_rejects_each_departure() {
    let (inventory, feature) = fixture();
    let (projected, states) = run(&inventory, &feature);
    let expected = operands_of(&inventory);
    let source = &inventory.features[0];
    let flat = [group(&projected).clone()];
    assert!(crate::feature::chamfer::matches(
        &expected, source, &flat, true
    ));
    assert!(valid(&projected, source, &expected, &states));
    // The first and second distances keep their slots.
    let mut swapped = flat.clone();
    swapped[0].spec = ChamferSpec::TwoDistances {
        first: length(SLOT_SECOND),
        second: length(SLOT_FIRST),
    };
    assert!(!crate::feature::chamfer::matches(
        &expected, source, &swapped, true
    ));
    // The edges of a resolved group belong to the slot-0 collection.
    let mut native = flat.clone();
    native[0].edges = EdgeSelection::Native(source.id());
    assert!(!crate::feature::chamfer::matches(
        &expected, source, &native, true
    ));
    let mut other = flat.clone();
    other[0].edges = EdgeSelection::historical(
        historical::state_id(source).expect("state"),
        vec![historical::edge_id(&token(&inventory), EDGE_SOURCE).expect("edge")],
        inventory.properties[1].id(),
    )
    .expect("selection");
    assert!(!crate::feature::chamfer::matches(
        &expected, source, &other, true
    ));
    // The state holds the support face once and nothing else.
    let mut tampered = states.clone();
    tampered[0].faces = DistinctMembers::default();
    assert!(!valid(&projected, source, &expected, &tampered));
    let mut tampered = states.clone();
    tampered[0].edge_geometry[0].faces = DistinctMembers::default();
    assert!(!valid(&projected, source, &expected, &tampered));
    let mut tampered = states.clone();
    tampered[0].faces =
        vec![face_selection::face_id(&token(&inventory), FACE_SOURCE + 1).expect("face")]
            .try_into()
            .expect("faces");
    assert!(!valid(&projected, source, &expected, &tampered));
    assert!(!valid(&projected, source, &expected, &[]));
    // A native group needs no state.
    assert!(valid(&feature, source, &expected, &[]));
}

/// Projects the fixture through the whole feature projection, with the
/// lengths of slots 2 and 3 as parameters.
fn whole_projection(inventory: FeatureInventory) -> crate::feature::FeatureProjection {
    use crate::feature::tests::{neutral_parameter, raw_parameter};
    let raw = vec![raw_parameter(49), raw_parameter(50)];
    let neutral = [
        neutral_parameter(
            &raw[0],
            cadmpeg_ir::features::ParameterValue::Length(Length::new(SLOT_FIRST).expect("length")),
        ),
        neutral_parameter(
            &raw[1],
            cadmpeg_ir::features::ParameterValue::Length(Length::new(SLOT_SECOND).expect("length")),
        ),
    ];
    crate::feature::fillet::tests::project_inventory_fixture(inventory, raw, &neutral)
}

fn chamfer_group(projection: &crate::feature::FeatureProjection) -> &ChamferGroup {
    assert_eq!(projection.features.len(), 1);
    group(&projection.features[0])
}

#[test]
fn the_whole_projection_charges_no_orientation_note_for_a_joined_chamfer() {
    let (inventory, _) = fixture();
    let projection = whole_projection(inventory);
    assert!(matches!(
        chamfer_group(&projection).spec,
        ChamferSpec::TwoDistances { .. }
    ));
    assert_eq!(projection.input_topologies.len(), 1);
    assert!(projection.edge_treatment_losses.is_empty());
    assert_eq!(projection.unresolved_features, 0);
}

#[test]
fn the_whole_projection_charges_the_orientation_note_for_a_chamfer_that_stays_native() {
    for support in [false, true] {
        let (mut inventory, _) = fixture();
        if support {
            let key = (token(&inventory), FACE_SOURCE);
            inventory
                .edge_inputs
                .faces
                .get_mut(&key)
                .expect("selected")
                .repeated_attribute = true;
        } else {
            inventory.edge_inputs.historical.clear();
        }
        let projection = whole_projection(inventory);
        assert_eq!(
            chamfer_group(&projection).spec,
            ChamferSpec::Unresolved {
                form: Some(ChamferForm::TwoDistances)
            }
        );
        assert!(projection.input_topologies.is_empty());
        let [loss] = projection.edge_treatment_losses.as_slice() else {
            panic!("one orientation note")
        };
        assert_eq!(loss.code.local_code(), "feature.chamfer-operands-native");
        assert!(loss.message.contains("support-side ownership"));
        assert_eq!(projection.unresolved_features, 1);
    }
}
