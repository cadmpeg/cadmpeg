// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use cadmpeg_core::text::NonBlankString;

use crate::design::dimension::Dimension;
use crate::design::parameter::{
    DesignParameter, Deviations, FitDeviations, LimitSide, ParameterDefinition, Tolerance,
};
use crate::design::units::{Quantity, ValueKind};
use crate::document::CadIr;
use crate::features::ParameterId;
use crate::ids::{PmiId, ProductDefinitionId};
use crate::pmi::{
    FitZone, LimitsAndFits, PmiAnnotation, PmiDefinition, PmiDimension, PmiDimensionKind,
    PmiQuantity, PmiValue,
};
use crate::report::check::{Check, Finding};
use crate::report::Severity;
use crate::scalar::FiniteReal;
use crate::validate::validate_neutral;

fn parameter_id(key: &str) -> ParameterId {
    ParameterId::mint(format!("test:design:parameter#{key}")).expect("valid identity")
}

fn design_parameter(key: &str, kind: ValueKind) -> DesignParameter {
    DesignParameter {
        id: parameter_id(key),
        scope: ProductDefinitionId::mint("test:model:definition#part").expect("valid identity"),
        owner: None,
        name: Some(NonBlankString::new(key).expect("nonblank name")),
        kind,
        unit: None,
        definition: ParameterDefinition::Driven,
        value: None,
        display: None,
        tolerance: None,
        decimals: None,
        comment: None,
        native_options: BTreeMap::new(),
        source_settings: BTreeMap::new(),
        native_ref: None,
    }
}

fn model_parameter(key: &str) -> crate::features::parameters::DesignParameter {
    crate::features::parameters::DesignParameter {
        product_definition: None,
        id: parameter_id(key),
        owner: None,
        ordinal: 0,
        name: key.into(),
        expression: "1mm".into(),
        display: None,
        value: None,
        dependencies: crate::features::selections::DistinctMembers::default(),
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: None,
    }
}

fn document(dimensions: impl IntoIterator<Item = PmiDimension>) -> CadIr {
    let mut ir = CadIr::empty();
    for (index, dimension) in dimensions.into_iter().enumerate() {
        ir.model.pmi.push(PmiAnnotation::new(
            PmiId::mint(format!("test:model:pmi#{index}")).expect("valid identity"),
            Vec::new(),
            PmiDefinition::Dimension(dimension),
        ));
    }
    ir
}

fn pmi_errors(ir: &mut CadIr) -> Vec<Finding> {
    ir.finalize();
    validate_neutral(ir, Vec::new())
        .expect("validate the neutral model")
        .findings
        .into_iter()
        .filter(|finding| finding.check == Check::Pmi && finding.severity == Severity::Error)
        .collect()
}

#[track_caller]
fn assert_errors(ir: &mut CadIr, fragment: &str, count: usize) {
    let findings = pmi_errors(ir);
    assert_eq!(
        findings
            .iter()
            .filter(|finding| finding.message.contains(fragment))
            .count(),
        count,
        "{fragment:?} in {findings:#?}"
    );
    assert_eq!(findings.len(), count, "{findings:#?}");
}

fn real(value: f64) -> FiniteReal {
    FiniteReal::new(value).expect("finite value")
}

fn deviations(lower: f64, upper: f64) -> Deviations {
    Deviations {
        lower: real(lower),
        upper: real(upper),
    }
}

fn designation(form_variance: &str, zone: FitZone) -> LimitsAndFits {
    LimitsAndFits {
        form_variance: form_variance.to_owned(),
        zone,
        grade: "7".to_owned(),
        source: None,
    }
}

fn diameter(tolerance: Tolerance) -> PmiDimension {
    let mut dimension = PmiDimension::new(PmiDimensionKind::Diameter);
    dimension.nominal = Some(Dimension::Literal(
        PmiValue::new(10.0, PmiQuantity::Length).expect("finite length"),
    ));
    dimension.tolerance = Some(tolerance);
    dimension
}

fn fit(mate: Option<FitZone>, stored: Option<FitDeviations>) -> Tolerance {
    Tolerance::Fit {
        fit: designation("H", FitZone::Hole),
        mate: mate.map(|zone| designation("G", zone)),
        deviations: stored,
    }
}

#[test]
fn a_dimension_deviations_lower_lies_below_its_upper() {
    let mut ir = document([
        diameter(Tolerance::Deviations(deviations(-0.1, 0.2))),
        diameter(Tolerance::Deviations(deviations(0.0, 0.015))),
        diameter(fit(
            None,
            Some(FitDeviations::Unchecked(deviations(0.0, 0.015))),
        )),
        diameter(Tolerance::Basic),
        diameter(Tolerance::SingleLimit(LimitSide::Maximum)),
    ]);
    assert_errors(&mut ir, "", 0);

    let mut ir = document([
        diameter(Tolerance::Deviations(deviations(0.2, -0.1))),
        diameter(Tolerance::Deviations(deviations(0.05, 0.05))),
        diameter(fit(None, Some(FitDeviations::Cell(deviations(0.015, 0.0))))),
        diameter(fit(None, Some(FitDeviations::Own(deviations(0.0, 0.0))))),
        diameter(fit(
            None,
            Some(FitDeviations::Unchecked(deviations(0.0, -0.01))),
        )),
    ]);
    assert_errors(&mut ir, "is not below its upper deviation", 5);
}

#[test]
fn a_dimension_fit_and_its_mate_state_the_other_zone() {
    let mut ir = document([
        diameter(fit(Some(FitZone::Shaft), None)),
        diameter(fit(None, None)),
    ]);
    assert_errors(&mut ir, "", 0);

    let mut ir = document([
        diameter(fit(Some(FitZone::Hole), None)),
        diameter(Tolerance::Fit {
            fit: designation("H", FitZone::Shaft),
            mate: Some(designation("G", FitZone::Shaft)),
            deviations: None,
        }),
    ]);
    assert_errors(&mut ir, "fit and its mate state the same zone", 2);
}

fn bound(kind: PmiDimensionKind, nominal: Dimension<PmiValue>) -> PmiDimension {
    let mut dimension = PmiDimension::new(kind);
    dimension.nominal = Some(nominal);
    dimension
}

fn bound_document(
    parameters: impl IntoIterator<Item = (&'static str, ValueKind)>,
    dimensions: impl IntoIterator<Item = PmiDimension>,
) -> CadIr {
    let mut ir = document(dimensions);
    ir.design.parameters.extend(
        parameters
            .into_iter()
            .map(|(key, kind)| design_parameter(key, kind)),
    );
    ir
}

#[test]
fn a_bound_nominal_takes_its_parameter_kind_as_its_quantity() {
    let length = || Dimension::Parameter(parameter_id("length"));
    let angle = || Dimension::Parameter(parameter_id("angle"));
    let ratio = || Dimension::Parameter(parameter_id("ratio"));
    let parameters = [
        ("angle", ValueKind::Quantity(Quantity::ANGLE)),
        ("length", ValueKind::Quantity(Quantity::LENGTH)),
        ("ratio", ValueKind::Quantity(Quantity::REAL)),
    ];
    let mut ir = bound_document(
        parameters.clone(),
        [
            bound(PmiDimensionKind::Size, length()),
            bound(
                PmiDimensionKind::Radius,
                Dimension::Negated(parameter_id("length")),
            ),
            bound(PmiDimensionKind::Angular, angle()),
            bound(PmiDimensionKind::Other("slope".into()), ratio()),
            bound(PmiDimensionKind::Other("taper".into()), angle()),
            bound(PmiDimensionKind::Other("offset".into()), length()),
        ],
    );
    assert_errors(&mut ir, "", 0);

    let mut ir = bound_document(
        parameters,
        [
            bound(PmiDimensionKind::Angular, length()),
            bound(PmiDimensionKind::Size, angle()),
            bound(
                PmiDimensionKind::CurveLength,
                Dimension::Negated(parameter_id("ratio")),
            ),
        ],
    );
    assert_errors(&mut ir, "whose kind disagrees with the dimension's kind", 3);
}

#[test]
fn a_bound_nominal_binds_a_length_angle_or_dimensionless_parameter() {
    let other = |key: &str| {
        bound(
            PmiDimensionKind::Other("count".into()),
            Dimension::Parameter(parameter_id(key)),
        )
    };
    let mut ir = bound_document(
        [
            ("area", ValueKind::Quantity(Quantity::AREA)),
            ("flag", ValueKind::Boolean),
            ("label", ValueKind::Text),
            ("quantity", ValueKind::Integer),
        ],
        [
            other("area"),
            other("flag"),
            other("label"),
            other("quantity"),
        ],
    );
    assert_errors(
        &mut ir,
        "whose kind is no length, angle, or dimensionless quantity",
        4,
    );

    let mut ir = bound_document(
        [(
            "native",
            ValueKind::Native(NonBlankString::new("test:kind").expect("nonblank kind")),
        )],
        [bound(
            PmiDimensionKind::Size,
            Dimension::Parameter(parameter_id("native")),
        )],
    );
    assert_errors(&mut ir, "", 0);
}

#[test]
fn a_bound_nominal_names_a_parameter_of_the_document() {
    let mut ir = document([bound(
        PmiDimensionKind::Size,
        Dimension::Parameter(parameter_id("missing")),
    )]);
    assert_errors(&mut ir, "which is no parameter of the document", 1);

    let mut ir = document([bound(
        PmiDimensionKind::Size,
        Dimension::Parameter(parameter_id("model")),
    )]);
    ir.model.parameters.push(model_parameter("model"));
    assert_errors(&mut ir, "", 0);
}
