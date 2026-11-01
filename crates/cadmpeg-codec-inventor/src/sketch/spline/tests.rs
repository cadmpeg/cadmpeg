// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::test_support::test_fixtures::{content_at, parse as with_view};

fn word(bytes: &mut Vec<u8>, value: u32) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

fn real(bytes: &mut Vec<u8>, value: f64) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

fn empty_list(bytes: &mut Vec<u8>) {
    word(bytes, 0x3000_0002);
    word(bytes, 0);
}

fn curve_bytes(bytes: &mut Vec<u8>) {
    for value in [1, 1] {
        word(bytes, value);
    }
    real(bytes, 1e-9);
    for value in [7, 10, 8] {
        word(bytes, value);
    }
    for value in [0., 0., 1., 2., 3., 4., 4.] {
        real(bytes, value);
    }
    for value in [0, 0, 8, 5, 5, 8] {
        word(bytes, value);
    }
    for point in [[0., 0.], [2., 0.], [2., 2.], [0., 2.], [0., 0.]] {
        for value in point {
            real(bytes, value);
        }
    }
    real(bytes, 1e-12);
    for value in [1, 1] {
        word(bytes, value);
    }
    for value in [0., 4., 0.] {
        real(bytes, value);
    }
    word(bytes, 8);
}

fn fixture(version: u8) -> Vec<u8> {
    let stamps: &[i32] = match version {
        0..=22 => &[],
        23 => &[-1],
        _ => &[-1, -1],
    };
    let mut bytes = content_at(8, version, stamps);
    word(&mut bytes, 4);
    word(&mut bytes, 3);
    empty_list(&mut bytes);
    bytes.extend_from_slice(&[0; 36]);
    empty_list(&mut bytes);
    word(&mut bytes, 0);
    curve_bytes(&mut bytes);
    bytes.push(2);
    empty_list(&mut bytes);
    empty_list(&mut bytes);
    bytes.extend_from_slice(&[0; 12]);
    bytes.push(0);
    word(&mut bytes, 1);
    bytes.push(0);
    curve_bytes(&mut bytes);
    word(&mut bytes, 2);
    bytes.extend_from_slice(&[0; 32]);
    bytes
}

#[test]
fn closed_polynomial_arrays_survive_versioned_headers() {
    for version in [22, 23, 25, 29, 31] {
        let record = with_view(&fixture(version), |ctx, source| {
            parse(ctx, source, version).expect("synthetic spline fixture")
        });
        let PmDcSketchEntityKind::Spline { data } = record.kind else {
            panic!("spline");
        };
        let curve = data.closed_curve().expect("closed polynomial");
        assert_eq!(curve.degree(), 1);
        assert_eq!(curve.knots().as_slice(), &[0., 0., 1., 2., 3., 4., 4.]);
        assert_eq!(curve.control_points()[2].get(), Point2::new(2., 2.));
        assert!(curve.periodic());
        assert!(curve.weights().is_none());
        let wire = serde_json::to_value(&data).expect("serialize spline");
        assert_eq!(
            serde_json::from_value::<Box<SplineRecord>>(wire).expect("deserialize spline"),
            data
        );
    }
}

#[test]
fn every_truncated_prefix_and_trailing_byte_is_refused() {
    let mut bytes = fixture(29);
    for end in 0..bytes.len() {
        with_view(&bytes[..end], |ctx, source| {
            assert!(parse(ctx, source, 29).is_err());
        });
    }
    bytes.push(0);
    with_view(&bytes, |ctx, source| {
        assert!(parse(ctx, source, 29).is_err());
    });
}

#[test]
fn differing_arrays_and_unresolved_states_do_not_project() {
    let record = with_view(&fixture(29), |ctx, source| {
        parse(ctx, source, 29).expect("synthetic spline fixture")
    });
    let PmDcSketchEntityKind::Spline { data } = record.kind else {
        panic!("spline");
    };
    let mut changed = data.clone();
    changed.secondary.poles[1][0] = FiniteReal::ONE;
    assert!(changed.closed_curve().is_none());
    let mut changed = data.clone();
    changed.curve_state = 0;
    assert!(changed.closed_curve().is_none());
    let mut changed = data.clone();
    changed.primary.knots[0] = FiniteReal::ONE.negated();
    changed.secondary = changed.primary.clone();
    assert!(changed.closed_curve().is_none());
    let mut changed = data.clone();
    changed.primary.knots.fill(FiniteReal::ZERO);
    changed.primary.interval = [FiniteReal::ZERO; 2];
    changed.secondary = changed.primary.clone();
    assert!(changed.closed_curve().is_none());
    let mut changed = data;
    changed.primary.poles[0][0] = FiniteReal::ONE;
    changed.secondary = changed.primary.clone();
    assert!(changed.closed_curve().is_none());
}

#[test]
fn oversized_scalar_and_pole_counts_fail_before_allocation() {
    for corrupt_knots in [true, false] {
        let mut bytes = Vec::new();
        for value in [1, 1] {
            word(&mut bytes, value);
        }
        real(&mut bytes, 1e-9);
        for value in [if corrupt_knots { u32::MAX } else { 0 }, 10, 8] {
            word(&mut bytes, value);
        }
        if !corrupt_knots {
            for value in [0, 0, 8, u32::MAX, 5, 8] {
                word(&mut bytes, value);
            }
        }
        with_view(&bytes, |ctx, source| {
            assert!(curve(ctx, &mut Cursor::new(source)).is_err());
        });
    }
}

#[test]
fn closed_spline_geometry_projects_without_fit_points_as_endpoints() {
    let record = with_view(&fixture(29), |ctx, source| {
        parse(ctx, source, 29).expect("synthetic spline fixture")
    });
    let raw = crate::record_identity::Located::new(
        record,
        crate::record_identity::RecordTypeId::from_bytes(TYPE),
        cadmpeg_ir::identity_key!("segment"),
        7,
    );
    let geometry = super::super::project_geometry(&raw, &std::collections::HashMap::new())
        .expect("spline geometry");
    let SketchGeometryDefinition::Nurbs { curve } = geometry.definition() else {
        panic!("nurbs");
    };
    assert_eq!(curve.control_points()[2].get(), Point2::new(20., 20.));
    let entity = SketchEntity::new(
        super::super::entity_id(&raw).expect("synthetic spline fixture"),
        cadmpeg_ir::sketches::SketchId::mint("inventor:design:sketch#segment-2")
            .expect("synthetic spline fixture"),
        geometry,
    );
    let profiles = super::super::build_profiles(&[&entity]);
    assert_eq!(
        profiles,
        vec![vec![cadmpeg_ir::sketches::SketchEntityUse {
            entity: entity.id().clone(),
            reversed: false
        }]]
    );
}

#[test]
fn spline_point_relation_retains_ordered_native_operands() {
    use super::super::{parse_constraint, project_constraint, SketchConstraintTag};
    use crate::record_identity::{Located, RecordTypeId};
    use cadmpeg_ir::sketches::{
        SketchConstraintDefinitionInput, SketchGeometry, SketchGeometryDefinition, SketchId,
    };
    use std::collections::HashMap;
    let mut bytes = content_at(10, 29, &[]);
    for value in [u32::MAX, 0, 0x3000_0006, 0, 0x3000_0006, 0, 16, 8, 4] {
        word(&mut bytes, value);
    }
    let payload = with_view(&bytes, |_, source| {
        parse_constraint(SketchConstraintTag::SplinePoint, source, 29)
            .expect("synthetic spline fixture")
    });
    let raw = Located::new(
        payload,
        RecordTypeId::from_bytes(POINT_RELATION_TYPE),
        cadmpeg_ir::identity_key!("segment"),
        9,
    );
    assert!(point_relation_loss(&[]).is_none());
    let loss = point_relation_loss(std::slice::from_ref(&raw)).expect("spline point loss");
    assert_eq!(loss.code.namespace(), "inventor");
    assert_eq!(
        loss.code.local_code(),
        crate::loss::InventorLossCode::SketchSplinePointNative.code()
    );
    assert!(loss.message.contains("spline locus and parameter roles"));
    let sketch =
        SketchId::mint("inventor:design:sketch#segment-2").expect("synthetic spline fixture");
    let entity = |n| {
        cadmpeg_ir::sketches::SketchEntityId::mint(format!(
            "inventor:design:sketch-entity#segment-{n}"
        ))
        .expect("synthetic spline fixture")
    };
    let spline = SketchEntity::new(
        entity(7),
        sketch.clone(),
        SketchGeometry::native(cadmpeg_core::nonblank_literal!("spline")),
    );
    let point = SketchEntity::new(
        entity(3),
        sketch,
        SketchGeometry::try_from(SketchGeometryDefinition::Point {
            position: Point2::new(0., 0.),
        })
        .expect("synthetic spline fixture"),
    );
    let entities = HashMap::from([(("segment", 7), &spline), (("segment", 3), &point)]);
    with_view(&[], |ctx, _| {
        let projected = project_constraint(
            ctx,
            &raw,
            &entities,
            &HashMap::new(),
            &std::collections::HashSet::new(),
        )
        .expect("resolved relation operands")
        .expect("relation projection");
        let SketchConstraintDefinitionInput::Native {
            entities,
            operands,
            native_kind,
            ..
        } = projected.definition.kind()
        else {
            panic!("native relation");
        };
        assert_eq!(native_kind.as_str(), "spline_point");
        assert_eq!(entities, &vec![spline.id().clone(), point.id().clone()]);
        assert_eq!(operands.len(), 2);
        let missing = HashMap::from([(("segment", 7), &spline)]);
        assert!(project_constraint(
            ctx,
            &raw,
            &missing,
            &HashMap::new(),
            &std::collections::HashSet::new()
        )
        .is_none());
    });
}

#[test]
fn refused_spline_controls_emit_a_native_curve_loss() {
    let mut record = with_view(&fixture(29), |ctx, source| {
        parse(ctx, source, 29).expect("synthetic spline")
    });
    let native = |payload| {
        crate::record_identity::Located::new(
            payload,
            crate::record_identity::RecordTypeId::from_bytes(TYPE),
            cadmpeg_ir::identity_key!("generated"),
            0,
        )
    };
    assert!(crate::sketch::curve::native_losses(&[native(record.clone())]).is_empty());
    let PmDcSketchEntityKind::Spline { data } = &mut record.kind else {
        panic!("spline");
    };
    data.curve_state = 6;
    let losses = crate::sketch::curve::native_losses(&[native(record)]);
    assert_eq!(losses.len(), 1);
    assert_eq!(losses[0].code.namespace(), "inventor");
    assert_eq!(
        losses[0].code.local_code(),
        crate::loss::InventorLossCode::SketchSplineCurveNative.code()
    );
    assert!(losses[0]
        .message
        .contains("spline envelope or construction flags"));
}

mod open;
