use crate::design::decode::scopes::align::decode_align_envelope;
use crate::design::feature_project::{
    project_parameter_design_with_edge_identities, ProjectInputs, ProjectedParameterDesign,
};
use crate::records::feature::align::DesignAlignForm;
use crate::records::feature::direct_face::{
    DesignMoveForm, DesignMoveLeading, DesignMoveOperation, DesignMovePrefix, DesignMoveReference,
};
use crate::records::feature::scope::{DesignParameterScope, DesignScopePayloadMut};
use crate::records::identity::Located;
use crate::records::sketch_placement::SketchPlacementMatrix;
use crate::records::topology::construction::DesignConstructionOperandGroup;
use crate::records::topology::extrude_selection::DesignOperandRole;
use crate::test_support::align::{align_motion_fixture, ALIGN_MOTION_MATRIX};
use cadmpeg_ir::features::{BodySelection, FeatureDefinition, FeatureOperation};

fn fixture(
    omitted: bool,
    role: DesignOperandRole,
) -> (DesignParameterScope, DesignConstructionOperandGroup) {
    let fixture = align_motion_fixture(
        DesignAlignForm::Compact,
        [true; 2],
        [false; 4],
        0.625,
        [2, 0],
        omitted,
        false,
    );
    let mut envelope = decode_align_envelope(&fixture.bytes, &fixture.scope).unwrap();
    envelope.motion = Some(DesignMoveOperation {
        transform: if omitted {
            SketchPlacementMatrix::IDENTITY
        } else {
            ALIGN_MOTION_MATRIX.try_into().unwrap()
        },
        transform_offset: u64::try_from(fixture.matrix_offset).unwrap(),
        transform_record_index: 806,
        form: DesignMoveForm::Form4,
        form_offset: u64::try_from(fixture.form_offset).unwrap(),
        transform_reference_offset: envelope.first_reference.offset.checked_sub(1),
        prefix: Some(DesignMovePrefix {
            leading: Some(DesignMoveLeading {
                scalar: cadmpeg_ir::scalar::FiniteReal::new(0.625).unwrap(),
                references: [
                    DesignMoveReference::Local { record_index: 810 },
                    DesignMoveReference::Local { record_index: 812 },
                ],
            }),
            references: std::array::from_fn(|_| DesignMoveReference::Null),
            codes: [2, 0],
            matrix_omitted: omitted,
        }),
        ..DesignMoveOperation::default()
    });
    let mut scope = fixture.scope;
    let DesignScopePayloadMut::Align(slot) = scope.payload_mut() else {
        panic!("Align");
    };
    *slot = Some(envelope);
    let mut group = super::direct_face::group(role);
    group.id = "f3d:Design/BulkStream.dat:group#802".into();
    group.scope_record_index = 800;
    group.scope_reference_ordinal = 0;
    group.record_index = 802;
    group
        .try_set_members(vec![Located {
            value: 804,
            offset: 1026,
        }])
        .unwrap();
    (scope, group)
}

fn project(
    scope: &DesignParameterScope,
    groups: &[DesignConstructionOperandGroup],
) -> ProjectedParameterDesign {
    let scopes = std::slice::from_ref(scope);
    let timelines = crate::design::test_support::synthetic_feature_timelines(scopes);
    crate::test_support::with_decode_context(|ctx| {
        project_parameter_design_with_edge_identities(
            ctx,
            &ProjectInputs {
                scopes,
                timelines: &timelines,
                construction_groups: groups,
                ..Default::default()
            },
        )
        .unwrap()
    })
}

#[test]
fn align_dispatcher_preserves_signed_motion_and_canonical_units() {
    let (scope, group) = fixture(false, DesignOperandRole::BODIES_A);
    let projected = project(&scope, std::slice::from_ref(&group));
    assert_eq!(projected.features.len(), 1);
    let feature = &projected.features[0];
    assert_eq!(feature.source_tag.as_deref(), Some("Align"));
    assert_eq!(feature.native_ref.as_deref(), Some(scope.id.as_str()));
    let FeatureDefinition::Operation(FeatureOperation::MoveBody {
        bodies,
        translation,
        rotation: Some(rotation),
        copies,
    }) = feature.evaluation.definition()
    else {
        panic!("MoveBody");
    };
    assert_eq!(*bodies, BodySelection::Native(group.id));
    assert_eq!(
        translation.get(),
        cadmpeg_ir::math::Vector3::new(12.5, -25., 37.5)
    );
    assert_eq!(
        rotation.origin.get(),
        cadmpeg_ir::math::Point3::new(0., 0., 0.)
    );
    assert_eq!(rotation.direction.x, 0.);
    assert_eq!(rotation.direction.y, 0.);
    assert_eq!(rotation.direction.z, 1.);
    assert!((rotation.angle.get() - std::f64::consts::FRAC_PI_2).abs() <= f64::EPSILON);
    assert_eq!(*copies, 0);
    assert!(projected
        .projection_losses
        .iter()
        .any(|loss| loss.code == crate::loss::F3dLossCode::FeatureAlignMotionAssumed.kind()));
}

#[test]
fn align_omitted_matrix_projects_identity_with_both_assumptions() {
    let (scope, group) = fixture(true, DesignOperandRole::BODIES_A);
    let projected = project(&scope, &[group]);
    let FeatureDefinition::Operation(FeatureOperation::MoveBody {
        translation,
        rotation,
        copies,
        ..
    }) = projected.features[0].evaluation.definition()
    else {
        panic!("MoveBody");
    };
    assert_eq!(
        translation.get(),
        cadmpeg_ir::math::Vector3::new(0., 0., 0.)
    );
    assert!(rotation.is_none());
    assert_eq!(*copies, 0);
    for code in [
        crate::loss::F3dLossCode::FeatureAlignMotionAssumed,
        crate::loss::F3dLossCode::FeatureAlignIdentityAssumed,
    ] {
        assert!(projected
            .projection_losses
            .iter()
            .any(|loss| loss.code == code.kind()));
    }
}

#[test]
fn align_without_its_exact_nonempty_unique_body_group_projects_an_unresolved_selection() {
    let (scope, group) = fixture(false, DesignOperandRole::BODIES_A);
    for field in 0..8 {
        let mut changed = group.clone();
        match field {
            0 => changed.id = "f3d:Other/BulkStream.dat:group#802".into(),
            1 => changed.scope_record_index += 1,
            2 => changed.scope_reference_ordinal += 1,
            3 => changed.record_index += 1,
            4 => changed = fixture(false, DesignOperandRole::ROLE_0X10).1,
            5 => changed.try_set_members(Vec::new()).unwrap(),
            _ => {}
        }
        let groups = if field == 6 {
            Vec::new()
        } else if field == 7 {
            vec![changed.clone(), changed]
        } else {
            vec![changed]
        };
        let projected = project(&scope, &groups);
        let FeatureDefinition::Operation(FeatureOperation::MoveBody {
            bodies,
            translation,
            ..
        }) = projected.features[0].evaluation.definition()
        else {
            panic!("MoveBody for field {field}");
        };
        assert_eq!(*bodies, BodySelection::Unresolved, "field {field}");
        assert_eq!(
            translation.get(),
            cadmpeg_ir::math::Vector3::new(12.5, -25., 37.5),
            "field {field}"
        );
        assert!(
            projected
                .projection_losses
                .iter()
                .any(|loss| loss.code == crate::loss::F3dLossCode::AlignOperandsUnresolved.kind()),
            "field {field}"
        );
    }
    let mut malformed = scope;
    malformed.id = "malformed".into();
    let mut losses = Vec::new();
    assert!(matches!(
        super::super::project_align(&malformed, &[group], &mut losses),
        Some(FeatureDefinition::Operation(FeatureOperation::MoveBody {
            bodies: BodySelection::Unresolved,
            ..
        }))
    ));
    assert!(losses
        .iter()
        .any(|loss| loss.code == crate::loss::F3dLossCode::AlignOperandsUnresolved.kind()));
}

#[test]
fn align_dispatcher_names_reflection_and_canonical_overflow_rejections() {
    for reflection in [false, true] {
        let (mut scope, group) = fixture(false, DesignOperandRole::BODIES_A);
        let mut matrix = SketchPlacementMatrix::IDENTITY.rows();
        if reflection {
            matrix[0][0] = -1.;
        } else {
            matrix[0][3] = f64::MAX;
        }
        let DesignScopePayloadMut::Align(Some(envelope)) = scope.payload_mut() else {
            panic!("Align");
        };
        envelope.motion.as_mut().unwrap().transform = matrix.try_into().unwrap();
        let projected = project(&scope, &[group]);
        assert!(matches!(
            projected.features[0].evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::Native { .. })
        ));
        assert!(projected.projection_losses.iter().any(
            |loss| loss.code == crate::loss::F3dLossCode::FeatureAlignMotionUnprojected.kind()
        ));
        assert!(!projected
            .projection_losses
            .iter()
            .any(|loss| loss.code == crate::loss::F3dLossCode::FeatureAlignMotionAssumed.kind()));
    }
}
