// SPDX-License-Identifier: Apache-2.0

use super::super::*;
use crate::pmdc::{PmDcListMetadata, PmDcReference};
use crate::test_support::sketch_curves::{edit_entity, open_cubic_bytes, open_cubic_entities};

fn index(entities: &[PmDcSketchEntity]) -> HashMap<(&str, u32), Option<&PmDcSketchEntity>> {
    cadmpeg_core::decode::unique_index(entities.iter().map(|entity| {
        (
            (
                entity.identity.segment_token.as_str(),
                entity.identity.record_ordinal,
            ),
            entity,
        )
    }))
}

fn mutate_spline(entities: &mut [PmDcSketchEntity], change: impl FnOnce(&mut SplineRecord)) {
    edit_entity(&mut entities[2], |payload| {
        let PmDcSketchEntityKind::Spline { data } = &mut payload.kind else {
            panic!("synthetic spline");
        };
        change(data);
    });
}

#[test]
fn endpoint_bound_open_cubic_projects_its_exact_arrays_and_units() {
    let entities = open_cubic_entities();
    let sources = index(&entities);
    let curve = resolved_curve(&entities[2], &sources).expect("open cubic");
    assert_eq!(curve.degree(), 3);
    assert!(!curve.periodic());
    assert!(curve.weights().is_none());
    assert_eq!(curve.knots().as_slice(), &[0., 0., 0., 0., 1., 1., 1., 1.]);
    assert_eq!(curve.control_points()[1].get(), Point2::new(0.0, 2.0));
    let geometry = crate::sketch::project_geometry(&entities[2], &sources).expect("neutral curve");
    let SketchGeometryDefinition::Nurbs { curve } = geometry.definition() else {
        panic!("neutral NURBS");
    };
    assert_eq!(curve.control_points()[1].get(), Point2::new(0.0, 20.0));
    assert_eq!(
        crate::sketch::entity_endpoint_refs(&entities[2], &sources),
        vec![
            "inventor:pmdc:sketch-entity#generated-3".to_owned(),
            "inventor:pmdc:sketch-entity#generated-4".to_owned(),
        ]
    );
    assert!(crate::sketch::curve::native_losses(&entities).is_empty());
    let wire = serde_json::to_value(&entities[2]).expect("spline wire");
    assert_eq!(
        serde_json::from_value::<PmDcSketchEntity>(wire).expect("restored spline"),
        entities[2]
    );
}

#[test]
fn every_open_cubic_control_is_checked() {
    let changes: &[fn(&mut SplineRecord)] = &[
        |s| s.state = 1,
        |s| s.values[0] = 0,
        |s| s.values[8] = 1,
        |s| s.curve_state = 6,
        |s| s.secondary_present = 0,
        |s| s.secondary_prefix = 1,
        |s| s.secondary_state = 1,
        |s| s.auxiliary_values[2] = 1,
        |s| s.terminal_values[0] = 0,
        |s| s.terminal_values[8] = 1,
        |s| s.primary.form = 1,
        |s| s.primary.degree = 2,
        |s| s.primary.tolerance = FiniteReal::ZERO,
        |s| s.primary.pole_tolerance = FiniteReal::ZERO,
        |s| s.primary.values[1] = 0,
        |s| s.primary.knot_values[0] = 1,
        |s| s.primary.knot_value = 7,
        |s| s.primary.pole_value = 4,
        |s| s.primary.terminal_value = FiniteReal::ONE,
        |s| s.primary.interval[0] = FiniteReal::ONE,
        |s| s.primary.knots[0] = FiniteReal::ONE.negated(),
        |s| s.primary.knots[4] = FiniteReal::ZERO,
        |s| {
            s.primary.poles.pop();
        },
        |s| s.primary.poles[3] = s.primary.poles[0],
        |s| {
            s.point_values = PmDcU32List::new(2, Some(PmDcListMetadata::U32([2, 0])), vec![1, 0])
                .expect("point values");
        },
        |s| {
            s.point_values = PmDcU32List::new(2, Some(PmDcListMetadata::U32([2, 1])), vec![0, 1])
                .expect("point metadata");
        },
        |s| {
            s.points = PmDcReferenceList::new(
                2,
                Some(PmDcListMetadata::U32([2, 1])),
                vec![PmDcReference::from_packed(4), PmDcReference::from_packed(5)],
            )
            .expect("point metadata");
        },
        |s| {
            s.points = PmDcReferenceList::new(
                2,
                Some(PmDcListMetadata::U32([3, 0])),
                vec![
                    PmDcReference::from_packed(4),
                    PmDcReference::from_packed(5),
                    PmDcReference::from_packed(4),
                ],
            )
            .expect("point count");
        },
        |s| {
            s.auxiliary[0] = PmDcReferenceList::new(
                2,
                Some(PmDcListMetadata::U32([1, 0])),
                vec![PmDcReference::from_packed(4)],
            )
            .expect("auxiliary");
        },
    ];
    for (number, change) in changes.iter().enumerate() {
        let mut entities = open_cubic_entities();
        mutate_spline(&mut entities, |data| {
            change(data);
            data.secondary = data.primary.clone();
        });
        assert!(
            resolved_curve(&entities[2], &index(&entities)).is_none(),
            "control {number}"
        );
        assert!(crate::sketch::entity_endpoint_refs(&entities[2], &index(&entities)).is_empty());
        let losses = crate::sketch::curve::native_losses(&entities);
        assert_eq!(losses.len(), 1, "control {number}");
        assert_eq!(
            losses[0].code.local_code(),
            crate::loss::InventorLossCode::SketchSplineCurveNative.code()
        );
    }
    let mut entities = open_cubic_entities();
    mutate_spline(&mut entities, |s| s.secondary.poles[1][0] = FiniteReal::ONE);
    assert!(resolved_curve(&entities[2], &index(&entities)).is_none());
}

#[test]
fn open_cubic_requires_exact_ordered_endpoint_joins() {
    for mode in 0..13 {
        let mut entities = open_cubic_entities();
        match mode {
            0 => edit_entity(&mut entities[0], |payload| {
                payload.sketch = PmDcReference::from_packed(9);
            }),
            1 => entities[0].identity.segment_token = cadmpeg_ir::identity_key!("other"),
            2 => {
                entities[0].identity.type_id =
                    crate::record_identity::RecordTypeId::from_bytes(TYPE);
            }
            3 => {
                entities.remove(0);
            }
            4 => entities.swap(0, 1),
            5 => edit_entity(&mut entities[2], |payload| payload.entity_flags = 0),
            6 => edit_entity(&mut entities[2], |payload| payload.save_version_major = 25),
            7 => edit_entity(&mut entities[2], |payload| {
                payload.sketch = PmDcReference::from_packed(0);
            }),
            8 => edit_entity(&mut entities[0], |payload| payload.save_version_major = 25),
            9 => entities.push(entities[0].clone()),
            10 => mutate_spline(&mut entities, |data| {
                data.points = PmDcReferenceList::new(
                    2,
                    Some(PmDcListMetadata::U32([2, 0])),
                    vec![PmDcReference::from_packed(0), PmDcReference::from_packed(5)],
                )
                .expect("null endpoint");
            }),
            11 => {
                entities[2].identity.type_id =
                    crate::record_identity::RecordTypeId::from_bytes(crate::sketch::POINT_TYPE);
            }
            _ => mutate_spline(&mut entities, |data| {
                data.points = PmDcReferenceList::new(
                    2,
                    Some(PmDcListMetadata::U32([2, 0])),
                    vec![PmDcReference::from_packed(5), PmDcReference::from_packed(4)],
                )
                .expect("reversed endpoints");
            }),
        }
        if mode == 4 {
            edit_entity(&mut entities[0], |payload| {
                let PmDcSketchEntityKind::Point { position, .. } = &mut payload.kind else {
                    panic!("point");
                };
                position[0] = FiniteReal::ZERO;
            });
        }
        let spline = entities
            .iter()
            .find(|entity| matches!(entity.kind, PmDcSketchEntityKind::Spline { .. }))
            .expect("spline");
        assert!(
            resolved_curve(spline, &index(&entities)).is_none(),
            "join {mode}"
        );
        let retained =
            crate::sketch::project_geometry(spline, &index(&entities)).expect("native spline");
        assert!(matches!(
            retained.definition(),
            SketchGeometryDefinition::Native { .. }
        ));
        assert_eq!(crate::sketch::curve::native_losses(&entities).len(), 1);
    }
}

#[test]
fn open_cubic_payload_refuses_every_truncation_and_suffix() {
    let mut bytes = open_cubic_bytes();
    for end in 0..bytes.len() {
        crate::test_support::test_fixtures::parse(&bytes[..end], |ctx, source| {
            assert!(parse(ctx, source, 23).is_err());
        });
    }
    bytes.push(0);
    crate::test_support::test_fixtures::parse(&bytes, |ctx, source| {
        assert!(parse(ctx, source, 23).is_err());
    });
}

#[test]
fn open_cubic_preserves_multi_span_arrays_and_a_nonzero_domain() {
    for pole_count in [4, 5, 8] {
        let entities =
            crate::test_support::sketch_curves::open_cubic_entities_with(pole_count, 3.0);
        let curve = resolved_curve(&entities[2], &index(&entities)).expect("open multi-span cubic");
        assert_eq!(
            curve.control_points().len(),
            usize::try_from(pole_count).expect("pole count")
        );
        assert_eq!(curve.knots().as_slice().first(), Some(&3.0));
        assert_eq!(curve.knots().as_slice().last(), Some(&4.0));
        assert!(!curve.periodic());
        assert_eq!(
            curve.control_points().first().expect("first pole").get(),
            Point2::new(0.0, 0.0)
        );
        assert_eq!(
            curve.control_points().last().expect("last pole").get(),
            Point2::new(2.0, 0.0)
        );
    }
}

#[test]
fn open_cubic_endpoint_knots_have_exactly_four_repeats() {
    for at_start in [true, false] {
        let mut entities = crate::test_support::sketch_curves::open_cubic_entities_with(5, 0.0);
        mutate_spline(&mut entities, |data| {
            data.primary.knots[4] = if at_start {
                FiniteReal::ZERO
            } else {
                FiniteReal::ONE
            };
            data.secondary = data.primary.clone();
        });
        assert!(resolved_curve(&entities[2], &index(&entities)).is_none());
        let loss = crate::sketch::curve::native_losses(&entities);
        assert_eq!(loss.len(), 1);
        assert_eq!(
            loss[0].code.local_code(),
            crate::loss::InventorLossCode::SketchSplineCurveNative.code()
        );
    }
}

#[test]
fn spline_midpoint_retains_its_native_locus_and_ordered_operands() {
    let entities = open_cubic_entities();
    let sources = index(&entities);
    let sketch =
        cadmpeg_ir::sketches::SketchId::mint("inventor:design:sketch#generated-2").expect("sketch");
    let projected = entities
        .iter()
        .map(|source| {
            SketchEntity::new(
                crate::sketch::entity_id(source).expect("entity"),
                sketch.clone(),
                crate::sketch::project_geometry(source, &sources).expect("geometry"),
            )
            .with_native_ref(Some(source.id()))
        })
        .collect::<Vec<_>>();
    let curve = &projected[2];
    let point = &projected[0];
    let constraint = crate::test_support::sketch_curves::spline_midpoint();
    let definition = crate::sketch::curve::midpoint_definition(
        &constraint,
        crate::pmdc::PmDcReference::from_packed(8),
        crate::pmdc::PmDcReference::from_packed(4),
        point,
        curve,
    );
    let cadmpeg_ir::sketches::SketchConstraintDefinitionInput::Native {
        native_kind,
        operands,
        ..
    } = definition
    else {
        panic!("native midpoint");
    };
    assert_eq!(native_kind.as_str(), "midpoint");
    assert_eq!(operands.len(), 2);
    assert_eq!(
        operands[0].native_ref.as_deref(),
        Some("inventor:pmdc:sketch-entity#generated-7")
    );
    assert_eq!(
        operands[1].native_ref.as_deref(),
        Some("inventor:pmdc:sketch-entity#generated-3")
    );
    let inventory = crate::sketch::SketchInventory {
        sketches: Vec::new(),
        entities,
        transforms: Vec::new(),
        directions: Vec::new(),
        constraints: vec![constraint],
        issues: Vec::new(),
    };
    let loss = crate::sketch::curve::relation_loss(&inventory).expect("midpoint locus loss");
    assert_eq!(loss.code.namespace(), "inventor");
    assert_eq!(
        loss.code.local_code(),
        crate::loss::InventorLossCode::SketchMidpointCurveNative.code()
    );
    assert!(loss.message.contains("curve locus semantics"));
}
