use super::*;
use crate::design::{
    decode::parameters::{parse_design_parameter_record, parse_parameter_owner},
    test_support::{parameter_owner_frame, parameter_record},
};
use crate::records::{
    feature::{
        scope::{DesignFeatureKind, DesignScopePayloadMut},
        sheet_metal::contour::{
            DesignContourFlangeOperation, DesignContourRule, DesignContourTravel,
        },
    },
    identity::Located,
};
use cadmpeg_ir::features::geometry::FeatureDirection3;
use cadmpeg_ir::math::Vector3;
use cadmpeg_ir::scalar::{FiniteReal, PositiveReal};

fn scope() -> DesignParameterScope {
    let mut scope = DesignParameterScope::empty(
        "f3d:test/Design1/BulkStream.dat:scope#0",
        DesignFeatureKind::ContourFlange,
        12,
    );
    crate::design::test_support::name_scope_references(&mut scope, [44, 30, 40]);
    scope
        .try_edit(|draft| crate::design::test_support::lengthen_scope_frame(draft, 512))
        .unwrap();
    scope
}

fn prefix() -> DesignContourFlangeOperation {
    DesignContourFlangeOperation {
        operation: Located {
            value: 4,
            offset: 26,
        },
        settings: Some(Located {
            value: 30,
            offset: 90,
        }),
        carrier: Some(Located {
            value: 40,
            offset: 120,
        }),
        profile_control: Some(Located {
            value: 2,
            offset: 115,
        }),
        inline_thickness: Some(Located {
            value: PositiveReal::new(0.125).unwrap(),
            offset: 131,
        }),
        travel: Some(DesignContourTravel {
            owner_role_assumed: false,
            owner: Located {
                value: 44,
                offset: 63,
            },
            direction: Located {
                value: FeatureDirection3::new(Vector3::new(0.0, -1.0, 0.0)).unwrap(),
                offset: 38,
            },
        }),
        rule: None,
    }
}

fn construction(definition: FeatureDefinition) -> SheetMetalContourFlangeConstruction {
    let FeatureDefinition::Operation(FeatureOperation::SheetMetalContourFlange { construction }) =
        definition
    else {
        panic!("contour flange");
    };
    construction
}

#[test]
fn contour_retains_signed_travel_and_independent_rule_values() {
    let mut scope = scope();
    let mut value = prefix();
    value.rule = Some(DesignContourRule {
        record_index: 60,
        reference_assumed: true,
        first_value: Located {
            value: FiniteReal::new(0.2).unwrap(),
            offset: 900,
        },
        thickness: Located {
            value: PositiveReal::new(0.125).unwrap(),
            offset: 950,
        },
        material_scalar: Located {
            value: FiniteReal::new(0.44).unwrap(),
            offset: 920,
        },
    });
    if let DesignScopePayloadMut::ContourFlange(slot) = scope.payload_mut() {
        *slot = Some(value);
    }
    let parameter = parse_design_parameter_record(&parameter_record(
        Some(44),
        "-15 mm",
        "AlongDistance",
        Some("mm"),
        "d1",
        -1.5,
    ))
    .unwrap();
    let radius = parse_design_parameter_record(&parameter_record(
        Some(50),
        "0 mm",
        "BendRadius",
        Some("mm"),
        "d2",
        0.0,
    ))
    .unwrap();
    let mut losses = Vec::new();
    let result = construction(project(
        &scope,
        &[(0, &parameter)],
        &ProjectInputs::default(),
        &mut losses,
    ));
    assert_eq!(result.thickness.unwrap().get(), 1.25);
    assert_eq!(result.inside_radius.unwrap().get(), 2.0);
    assert_eq!(result.neutral_factor.unwrap().get(), 0.44);
    assert!(
        matches!(result.extent, Some(ExtrudeExtent::OneSided { side: ExtrudeSide { termination: LinearTermination::Blind { length }, .. } }) if length.get() == -15.0)
    );
    assert!(losses
        .iter()
        .any(|loss| loss.code == F3dLossCode::ContourSingleDistanceOneSidedAssumed.kind()));
    assert!(losses
        .iter()
        .any(|loss| loss.code == F3dLossCode::ContourSettingsRuleReferenceAssumed.kind()));
    let overridden = construction(project(
        &scope,
        &[(0, &parameter), (1, &radius)],
        &ProjectInputs::default(),
        &mut Vec::new(),
    ));
    assert_eq!(overridden.inside_radius.unwrap().get(), 0.0);
}

#[test]
fn contour_missing_definition_uses_its_cached_vector_owner() {
    let mut scope = scope();
    if let DesignScopePayloadMut::ContourFlange(slot) = scope.payload_mut() {
        *slot = Some(prefix());
    }
    let mut bytes = parameter_owner_frame();
    bytes[40..48].copy_from_slice(&(-1.5_f64).to_le_bytes());
    let owners = [parse_parameter_owner(&bytes)
        .unwrap()
        .into_record("test/Design1/BulkStream.dat", 0)
        .unwrap()];
    let inputs = ProjectInputs {
        owners: &owners,
        ..Default::default()
    };
    let mut losses = Vec::new();
    let result = construction(project(&scope, &[], &inputs, &mut losses));
    assert_eq!(result.thickness.unwrap().get(), 1.25);
    assert!(
        matches!(result.extent, Some(ExtrudeExtent::OneSided { side: ExtrudeSide { termination: LinearTermination::Blind { length }, .. } }) if length.get() == -15.0)
    );
    assert!(losses
        .iter()
        .any(|loss| loss.code == F3dLossCode::ContourCachedOwnerDistanceAssumed.kind()));
    assert!(losses
        .iter()
        .any(|loss| loss.code == F3dLossCode::ContourInlineThicknessAssumed.kind()));
}

#[test]
fn absent_contour_inputs_still_project_a_neutral_partial_construction() {
    let mut losses = Vec::new();
    let scope = scope();
    let result = construction(project(&scope, &[], &ProjectInputs::default(), &mut losses));
    assert_eq!(
        result.profile,
        Some(PlanarProfileRef::Native(scope.id.clone()))
    );
    assert_eq!(result.side, Some(SheetMetalContourThicknessSide::Left));
    assert!(result.extent.is_none());
    assert!(result.thickness.is_none());
    assert!(losses
        .iter()
        .any(|loss| loss.code == F3dLossCode::ContourEnvelopeUnresolved.kind()));
    assert!(losses
        .iter()
        .any(|loss| loss.code == F3dLossCode::ContourDistanceUnresolved.kind()));
}

#[test]
fn contour_missing_source_links_preserves_direction_and_signed_distance() {
    let mut scope = scope();
    let mut value = prefix();
    value.carrier = None;
    value.settings = None;
    value.profile_control = None;
    value.inline_thickness = None;
    value.travel.as_mut().unwrap().owner_role_assumed = true;
    if let DesignScopePayloadMut::ContourFlange(slot) = scope.payload_mut() {
        *slot = Some(value);
    }
    let parameter = parse_design_parameter_record(&parameter_record(
        Some(44),
        "-15 mm",
        "AlongDistance",
        Some("mm"),
        "d1",
        -1.5,
    ))
    .unwrap();
    let mut losses = Vec::new();
    let result = construction(project(
        &scope,
        &[(0, &parameter)],
        &ProjectInputs::default(),
        &mut losses,
    ));
    assert!(result.direction.is_some());
    assert!(result.extent.is_some());
    assert!(result.thickness.is_none());
    assert!(losses
        .iter()
        .any(|loss| loss.code == F3dLossCode::ContourVectorOwnerDistanceRoleAssumed.kind()));
}

#[test]
fn contour_native_optional_fields_refuse_nulls_and_bad_locations() {
    let value = serde_json::to_value(prefix()).unwrap();
    for key in [
        "settings",
        "carrier",
        "profile_control",
        "inline_thickness",
        "travel",
        "rule",
    ] {
        let mut invalid = value.clone();
        invalid[key] = serde_json::Value::Null;
        let error = serde_json::from_value::<DesignContourFlangeOperation>(invalid)
            .unwrap_err()
            .to_string();
        assert!(error.contains(key), "{error}");
    }
    let scope = scope();
    let mut value = prefix();
    assert!(value.valid_for_scope(&scope));
    value.operation.offset = 27;
    assert!(!value.valid_for_scope(&scope));
}

#[test]
fn contour_named_length_parameters_keep_database_values_without_display_units() {
    let distance = parse_design_parameter_record(&parameter_record(
        Some(44),
        "-15 mm",
        "AlongDistance",
        None,
        "d1",
        -1.5,
    ))
    .unwrap();
    let radius = parse_design_parameter_record(&parameter_record(
        Some(50),
        "2 mm",
        "BendRadius",
        None,
        "d2",
        0.2,
    ))
    .unwrap();
    let mut losses = Vec::new();
    let result = construction(project(
        &scope(),
        &[(0, &distance), (1, &radius)],
        &ProjectInputs::default(),
        &mut losses,
    ));
    assert!(result.extent.is_some());
    assert_eq!(result.inside_radius.unwrap().get(), 2.0);
    assert!(losses
        .iter()
        .any(|loss| loss.code == F3dLossCode::ContourDistanceDatabaseUnitAssumed.kind()));
    assert!(losses
        .iter()
        .any(|loss| loss.code == F3dLossCode::ContourRadiusDatabaseUnitAssumed.kind()));
}
