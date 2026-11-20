// SPDX-License-Identifier: Apache-2.0

use crate::test_support::edit;
use serde_json::json;
use std::num::NonZeroU32;

use super::{
    DatumReference, DatumReferences, DimensionModifier, FitStandard, FitZone,
    GeometricToleranceKind, LimitsAndFits, PmiAnnotation, PmiDefinition, PmiDimension,
    PmiDimensionKind, PmiMagnitude, PmiQuantity, PmiTarget, PmiValue,
};
use crate::design::dimension::Dimension;
use crate::design::parameter::{Deviations, FitDeviations, LimitSide, Tolerance};
use crate::document::CadIr;
use crate::features::ParameterId;
use crate::ids::PmiId;
use crate::report::check::Check;
use crate::scalar::FiniteReal;
use crate::validate::validate_neutral;

fn length(value: f64) -> PmiValue {
    PmiValue::new(value, PmiQuantity::Length).expect("finite length")
}

fn real(value: f64) -> FiniteReal {
    FiniteReal::new(value).expect("finite value")
}

fn annotation(id: &str, definition: PmiDefinition) -> PmiAnnotation {
    PmiAnnotation::new(
        PmiId::mint(format!("test:model:pmi#{id}")).expect("valid identity"),
        Vec::new(),
        definition,
    )
}

fn pmi_findings(ir: &CadIr) -> usize {
    validate_neutral(ir, Vec::new())
        .expect("validate the neutral model")
        .findings
        .iter()
        .filter(|finding| finding.check == Check::Pmi)
        .count()
}

#[test]
fn datum_system_references_resolve_with_precedence() {
    let mut ir = CadIr::empty();
    let mut datum = annotation(
        "datum-a",
        PmiDefinition::Datum {
            identification: "A".into(),
        },
    );
    datum.targets = vec![PmiTarget::ShapeAspect {
        source_id: cadmpeg_core::text::NonBlankString::new("#10")
            .expect("nonempty source identity"),
    }
    .into()];
    let datum_id = datum.id.clone();
    ir.model.pmi.push(datum);
    ir.model.pmi.push(annotation(
        "system",
        PmiDefinition::DatumSystem {
            references: vec![DatumReference {
                datum: datum_id,
                precedence: NonZeroU32::MIN,
                common_group: None,
                modifiers: Vec::new(),
            }]
            .try_into()
            .expect("valid datum compartments"),
        },
    ));
    ir.finalize();
    assert_eq!(pmi_findings(&ir), 0);
}

#[test]
fn a_unit_dimension_kind_states_its_kind_alone() {
    for (kind, name) in [
        (PmiDimensionKind::Size, "size"),
        (PmiDimensionKind::DirectedLocation, "directed_location"),
        (PmiDimensionKind::CurveLength, "curve_length"),
    ] {
        let wire = json!({"kind": name});
        assert_eq!(serde_json::to_value(&kind).expect("serialize"), wire);
        assert_eq!(
            serde_json::from_value::<PmiDimensionKind>(wire).expect("the bare kind reads"),
            kind
        );
        for value in [serde_json::Value::Null, json!({})] {
            let error = serde_json::from_value::<PmiDimensionKind>(json!({
                "kind": name, "value": value
            }))
            .expect_err("a stated value is a second spelling of the unit kind")
            .to_string();
            assert!(error.contains("does not state value"), "{value}: {error}");
        }
    }
}

#[test]
fn dimension_modifiers_and_fit_standards_refuse_a_value_on_a_unit_arm() {
    for name in ["reference", "statistical", "controlled_radius"] {
        assert!(serde_json::from_value::<DimensionModifier>(json!({"kind": name})).is_ok());
        assert!(
            serde_json::from_value::<DimensionModifier>(json!({"kind": name, "value": null}))
                .is_err()
        );
    }
    for name in ["iso286", "ansi_b41"] {
        assert!(serde_json::from_value::<FitStandard>(json!({"kind": name})).is_ok());
        assert!(
            serde_json::from_value::<FitStandard>(json!({"kind": name, "value": null})).is_err()
        );
    }
    assert!(
        serde_json::from_value::<FitStandard>(json!({"kind": "native", "value": " "})).is_err()
    );
}

#[test]
fn datum_reference_wire_rejects_zero_precedence() {
    let error = serde_json::from_value::<DatumReference>(json!({
        "datum": "test:model:pmi#datum-a",
        "precedence": 0,
        "modifiers": []
    }))
    .expect_err("zero is not a precedence")
    .to_string();
    assert!(error.contains("nonzero"), "{error}");
    let reference = serde_json::from_value::<DatumReference>(json!({
        "datum": "test:model:pmi#0",
        "precedence": 1,
        "modifiers": []
    }))
    .expect("one is a precedence");
    assert_eq!(reference.precedence.get(), 1);
}

#[test]
fn a_dimension_nominal_is_a_literal_or_a_parameter_binding() {
    let mut dimension = PmiDimension::new(PmiDimensionKind::Size);
    dimension.nominal = Some(Dimension::Literal(length(12.0)));
    dimension.tolerance = Some(Tolerance::Deviations(Deviations {
        lower: real(-0.1),
        upper: real(0.2),
    }));
    dimension.decimals = Some(2);
    dimension.modifiers = vec![DimensionModifier::Statistical];
    let definition = PmiDefinition::Dimension(dimension);
    let wire = json!({
        "kind": "dimension",
        "dimension": {"kind": "size"},
        "nominal": {"kind": "literal", "value": {"value": 12.0, "quantity": "length"}},
        "tolerance": {"kind": "deviations", "value": {"lower": -0.1, "upper": 0.2}},
        "decimals": 2,
        "modifiers": [{"kind": "statistical"}]
    });
    assert_eq!(serde_json::to_value(&definition).expect("serialize"), wire);
    assert_eq!(
        serde_json::from_value::<PmiDefinition>(wire).expect("read"),
        definition
    );

    let bound = json!({
        "kind": "dimension",
        "dimension": {"kind": "angular"},
        "nominal": {"kind": "parameter", "value": "test:design:parameter#angle"},
        "modifiers": []
    });
    let read = serde_json::from_value::<PmiDefinition>(bound.clone()).expect("a bound nominal");
    let PmiDefinition::Dimension(dimension) = &read else {
        panic!("a dimension");
    };
    assert_eq!(
        dimension.nominal,
        Some(Dimension::Parameter(
            ParameterId::mint("test:design:parameter#angle").expect("valid identity")
        ))
    );
    assert_eq!(dimension.literal_nominal(), None);
    assert_eq!(dimension.quantity(), Some(PmiQuantity::Angle));
    assert_eq!(serde_json::to_value(&read).expect("serialize"), bound);

    for (field, value) in [
        ("nominal", serde_json::Value::Null),
        ("tolerance", serde_json::Value::Null),
        ("decimals", serde_json::Value::Null),
        ("decimals", json!(256)),
    ] {
        let mut rejected = bound.clone();
        rejected[field] = value;
        let error = serde_json::from_value::<PmiDefinition>(rejected)
            .expect_err("an optional field states a value or is omitted")
            .to_string();
        assert!(error.contains(field), "{field}: {error}");
    }
    for deleted in ["lower_deviation", "limits_and_fits"] {
        let mut rejected = bound.clone();
        rejected[deleted] = json!({"value": -0.1, "quantity": "length"});
        assert!(serde_json::from_value::<PmiDefinition>(rejected).is_err());
    }
}

#[test]
fn a_dimension_tolerance_is_the_design_layer_tolerance() {
    let fit = LimitsAndFits {
        form_variance: "H".into(),
        zone: FitZone::Hole,
        grade: "7".into(),
        source: Some(FitStandard::Iso286),
    };
    let mut dimension = PmiDimension::new(PmiDimensionKind::Diameter);
    dimension.nominal = Some(Dimension::Literal(length(10.0)));
    dimension.tolerance = Some(Tolerance::Fit {
        fit,
        mate: Some(LimitsAndFits {
            form_variance: "G".into(),
            zone: FitZone::Shaft,
            grade: "6".into(),
            source: None,
        }),
        deviations: Some(FitDeviations::Unchecked(Deviations {
            lower: real(0.0),
            upper: real(0.015),
        })),
    });
    let wire = serde_json::to_value(&dimension).expect("serialize");
    assert_eq!(wire["tolerance"]["kind"], "fit");
    assert_eq!(wire["tolerance"]["value"]["fit"]["zone"], "hole");
    assert_eq!(
        wire["tolerance"]["value"]["fit"]["source"],
        json!({"kind": "iso286"})
    );
    assert!(wire["tolerance"]["value"]["mate"].get("source").is_none());
    assert_eq!(
        serde_json::from_value::<PmiDimension>(wire.clone()).expect("read"),
        dimension
    );
    let mut rejected = wire;
    rejected["tolerance"]["value"]["fit"]["zone_variance"] = json!("");
    assert!(serde_json::from_value::<PmiDimension>(rejected).is_err());

    dimension.tolerance = Some(Tolerance::SingleLimit(LimitSide::Maximum));
    let wire = serde_json::to_value(&dimension).expect("serialize");
    assert_eq!(
        wire["tolerance"],
        json!({"kind": "single_limit", "value": "maximum"})
    );
    dimension.tolerance = Some(Tolerance::Basic);
    let wire = serde_json::to_value(&dimension).expect("serialize");
    assert_eq!(wire["tolerance"], json!({"kind": "basic"}));
}

#[test]
fn a_literal_nominal_whose_quantity_disagrees_with_its_kind_is_a_finding() {
    let angle = PmiValue::new(1.0, PmiQuantity::Angle).expect("finite angle");
    let mut size = PmiDimension::new(PmiDimensionKind::Size);
    size.nominal = Some(Dimension::Literal(angle));
    assert!(size.nominal_disagrees_with_kind());
    let mut other = PmiDimension::new(PmiDimensionKind::Other("ratio".into()));
    other.nominal = Some(Dimension::Literal(
        PmiValue::new(0.5, PmiQuantity::Ratio).expect("finite ratio"),
    ));
    assert!(!other.nominal_disagrees_with_kind());
    assert_eq!(other.quantity(), Some(PmiQuantity::Ratio));

    let mut ir = CadIr::empty();
    ir.model
        .pmi
        .push(annotation("size", PmiDefinition::Dimension(size)));
    ir.model
        .pmi
        .push(annotation("other", PmiDefinition::Dimension(other)));
    ir.finalize();
    assert_eq!(pmi_findings(&ir), 1);
}

#[test]
fn repeated_dimension_modifiers_are_a_finding() {
    let mut dimension = PmiDimension::new(PmiDimensionKind::SphericalRadius);
    dimension.modifiers = vec![DimensionModifier::Reference, DimensionModifier::Reference];
    let mut ir = CadIr::empty();
    ir.model
        .pmi
        .push(annotation("radius", PmiDefinition::Dimension(dimension)));
    ir.finalize();
    assert_eq!(pmi_findings(&ir), 1);
}

#[test]
fn an_annotation_states_its_native_record_and_native_maps() {
    let mut value = annotation(
        "datum",
        PmiDefinition::Datum {
            identification: "A".into(),
        },
    );
    value.native_ref = Some("native:pmi#1".into());
    value.source_settings.insert(
        cadmpeg_core::text::NonBlankString::new("sldprt:val_precision").expect("a key"),
        crate::design::options::NativeOption::Integer(
            crate::design::dimension::Dimension::Literal(3),
        ),
    );
    let wire = serde_json::to_value(&value).expect("serialize");
    assert_eq!(wire["native_ref"], "native:pmi#1");
    assert_eq!(wire["native_options"], json!({}));
    assert_eq!(
        wire["source_settings"]["sldprt:val_precision"],
        json!({"kind": "integer", "value": {"kind": "literal", "value": 3}})
    );
    assert_eq!(
        serde_json::from_value::<PmiAnnotation>(wire.clone()).expect("read"),
        value
    );
    for field in ["native_options", "source_settings"] {
        let mut rejected = wire.clone();
        rejected.as_object_mut().expect("an object").remove(field);
        assert!(serde_json::from_value::<PmiAnnotation>(rejected).is_err());
    }
}

#[test]
fn curve_target_resolves_against_the_curve_arena() {
    let mut ir = crate::examples::unit_cube().expect("valid unit cube fixture");
    let curve = ir.geometry.curves[0].id.clone();
    let mut dimension = PmiDimension::new(PmiDimensionKind::Size);
    dimension.nominal = Some(Dimension::Literal(length(1.0)));
    let mut value = annotation("curve-target", PmiDefinition::Dimension(dimension));
    value.targets = vec![PmiTarget::Curve { curve }.into()];
    ir.model.pmi.push(value);
    ir.finalize();
    assert_eq!(pmi_findings(&ir), 0);
}

#[test]
fn unresolved_semantic_reference_is_invalid() {
    let mut ir = CadIr::empty();
    ir.model.pmi.push(annotation(
        "graphic",
        PmiDefinition::Presentation {
            text: None,
            placement: None,
            semantics: vec![PmiId::mint("test:model:pmi#missing").expect("valid identity")],
        },
    ));
    assert!(pmi_findings(&ir) > 0);
}

#[test]
fn datum_references_are_type_checked() {
    let mut ir = CadIr::empty();
    let mut dimension = PmiDimension::new(PmiDimensionKind::Size);
    dimension.nominal = Some(Dimension::Literal(length(1.0)));
    let dimension = annotation("dimension", PmiDefinition::Dimension(dimension));
    let dimension_id = dimension.id.clone();
    ir.model.pmi.push(dimension);
    ir.model.pmi.push(annotation(
        "system",
        PmiDefinition::DatumSystem {
            references: vec![DatumReference {
                datum: dimension_id.clone(),
                precedence: NonZeroU32::MIN,
                common_group: None,
                modifiers: Vec::new(),
            }]
            .try_into()
            .expect("valid datum compartments"),
        },
    ));
    ir.model.pmi.push(annotation(
        "tolerance",
        PmiDefinition::GeometricTolerance {
            tolerance: GeometricToleranceKind::Position,
            magnitude: PmiMagnitude::new(length(0.1)).expect("nonnegative magnitude"),
            defined_unit: None,
            defined_area_unit: None,
            defined_area_second_unit: None,
            datum_system: Some(dimension_id),
            modifiers: Vec::new(),
            displacement: None,
            maximum_upper: None,
            zones: Vec::new(),
        },
    ));
    assert!(pmi_findings(&ir) >= 2);
}

#[test]
fn a_geometric_tolerance_states_its_zone_fields_in_place() {
    let wire = json!({
        "kind": "geometric_tolerance",
        "tolerance": {"kind": "position"},
        "magnitude": {"value": 0.1, "quantity": "length"},
        "modifiers": ["maximum_material_requirement"],
        "displacement": -0.05,
        "maximum_upper": 0.3,
        "zones": [{"form": {"kind": "cylindrical_or_circular"}, "projections": []}]
    });
    let definition = serde_json::from_value::<PmiDefinition>(wire.clone()).expect("read");
    assert_eq!(serde_json::to_value(&definition).expect("serialize"), wire);
    for (field, value) in [
        ("maximum_upper", json!(0.0)),
        ("maximum_upper", serde_json::Value::Null),
        ("displacement", serde_json::Value::Null),
    ] {
        let mut rejected = wire.clone();
        rejected[field] = value;
        assert!(serde_json::from_value::<PmiDefinition>(rejected).is_err());
    }
    let mut rejected = wire;
    rejected["constraints"] = json!({"zones": []});
    assert!(serde_json::from_value::<PmiDefinition>(rejected).is_err());
}

#[test]
fn pmi_magnitude_admission_keeps_signed_dimensions_and_zero_angles() {
    let negative = length(-1.0);
    assert_eq!(
        PmiValue::from_parts(real(-1.0), PmiQuantity::Length),
        negative
    );
    assert!(PmiMagnitude::new(negative).is_none());
    assert!(PmiValue::new(f64::INFINITY, PmiQuantity::Length).is_none());
    let zero = PmiMagnitude::new(PmiValue::new(0.0, PmiQuantity::Angle).expect("zero angle"))
        .expect("zero magnitude");
    assert_eq!(
        PmiMagnitude::from_parts(crate::scalar::NonNegativeReal::ZERO, PmiQuantity::Angle),
        zero
    );
    let wire = serde_json::to_string(&zero).expect("serialize");
    assert_eq!(wire, r#"{"value":0.0,"quantity":"angle"}"#);
    assert_eq!(
        serde_json::from_str::<PmiMagnitude>(&wire).expect("deserialize"),
        zero
    );
    let error = serde_json::from_str::<PmiMagnitude>(r#"{"value":-1.0,"quantity":"length"}"#)
        .expect_err("negative magnitude");
    assert!(error.to_string().contains("magnitude"));
}

#[test]
fn source_identity_admission_preserves_nonempty_wire() {
    let wire = json!({"kind": "shape_aspect", "source_id": "#42"});
    let value: PmiTarget = serde_json::from_value(wire.clone()).expect("nonempty source_id");
    assert_eq!(serde_json::to_value(value).expect("serialize"), wire);
    let error =
        serde_json::from_value::<PmiTarget>(json!({"kind": "shape_aspect", "source_id": ""}))
            .expect_err("empty source_id");
    assert!(error.to_string().contains("source_id"));
}

#[test]
fn datum_compartments_are_checked_at_construction_wire_and_replacement() {
    let reference = |id: &str, precedence, common_group| DatumReference {
        datum: PmiId::mint(format!("test:model:pmi#{id}")).expect("valid identity"),
        precedence: NonZeroU32::new(precedence).expect("positive precedence"),
        common_group,
        modifiers: Vec::new(),
    };
    let valid = vec![
        reference("c", 2, None),
        reference("a", 1, Some(0)),
        reference("b", 1, Some(0)),
    ];
    let mut admitted = DatumReferences::try_from(valid.clone()).expect("valid compartments");
    assert_eq!(admitted.as_slice(), valid);
    let wire = json!({"kind": "datum_system", "references": valid});
    let definition: PmiDefinition = serde_json::from_value(wire.clone()).expect("valid wire");
    assert_eq!(serde_json::to_value(definition).expect("serialize"), wire);
    assert!(DatumReferences::try_from(Vec::new())
        .expect("empty system")
        .as_slice()
        .is_empty());
    for invalid in [
        vec![reference("a", 1, Some(7))],
        vec![reference("a", 1, None), reference("b", 1, None)],
        vec![reference("a", 1, Some(7)), reference("b", 1, None)],
        vec![reference("a", 1, Some(7)), reference("b", 1, Some(8))],
        vec![
            reference("a", 1, Some(7)),
            reference("b", 1, Some(7)),
            reference("c", 2, Some(7)),
            reference("d", 2, Some(7)),
        ],
    ] {
        assert!(DatumReferences::try_from(invalid.clone()).is_err());
        let error = serde_json::from_value::<PmiDefinition>(
            json!({"kind": "datum_system", "references": invalid}),
        )
        .expect_err("invalid compartments");
        assert!(error.to_string().contains("references"));
        assert!({
            let replacement = invalid;
            edit::replace(&mut admitted, |_| DatumReferences::try_from(replacement))
        }
        .is_err());
        assert_eq!(admitted.as_slice(), valid);
    }
    let replacement = vec![reference("z", 3, None)];
    {
        let replacement = replacement.clone();
        edit::replace(&mut admitted, |_| DatumReferences::try_from(replacement))
    }
    .expect("valid replacement");
    assert_eq!(admitted.as_slice(), replacement);
}
