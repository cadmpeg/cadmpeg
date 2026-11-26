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

fn literal(value: f64, quantity: PmiQuantity) -> Dimension<PmiValue> {
    Dimension::Literal(PmiValue::new(value, quantity).expect("finite value"))
}

fn stating(
    kind: PmiDimensionKind,
    nominal: Option<Dimension<PmiValue>>,
    tolerance: Option<Tolerance>,
    decimals: Option<u8>,
) -> PmiDimension {
    let mut dimension = PmiDimension::new(kind);
    dimension.nominal = nominal;
    dimension.tolerance = tolerance;
    dimension.decimals = decimals;
    dimension
}

#[test]
fn a_dimension_tolerance_goes_with_a_length_or_angle_and_a_fit_with_a_length() {
    let other = |quantity| Some(literal(0.5, quantity));
    let mut ir = document([
        stating(
            PmiDimensionKind::Angular,
            None,
            Some(Tolerance::Deviations(deviations(-0.1, 0.1))),
            None,
        ),
        stating(
            PmiDimensionKind::Diameter,
            None,
            Some(fit(None, None)),
            None,
        ),
        stating(
            PmiDimensionKind::Other("slope".into()),
            None,
            Some(Tolerance::Deviations(deviations(-0.1, 0.1))),
            None,
        ),
    ]);
    assert_errors(&mut ir, "", 0);

    let mut ir = document([
        stating(
            PmiDimensionKind::Other("slope".into()),
            other(PmiQuantity::Ratio),
            Some(Tolerance::Deviations(deviations(-0.1, 0.1))),
            None,
        ),
        stating(PmiDimensionKind::Angular, None, Some(fit(None, None)), None),
    ]);
    let findings = pmi_errors(&mut ir);
    assert!(
        findings.iter().any(|finding| finding
            .message
            .contains("its quantity is no length or angle")),
        "{findings:#?}"
    );
    assert!(
        findings.iter().any(|finding| finding
            .message
            .contains("states a fit, but its quantity is no length")),
        "{findings:#?}"
    );
}

#[test]
fn a_dimension_fit_designation_is_capital_letters_and_a_grade() {
    let mut ir = document([stating(
        PmiDimensionKind::Diameter,
        None,
        Some(Tolerance::Fit {
            fit: LimitsAndFits {
                form_variance: "h".to_owned(),
                zone: FitZone::Shaft,
                grade: String::new(),
                source: None,
            },
            mate: None,
            deviations: None,
        }),
        None,
    )]);
    let findings = pmi_errors(&mut ir);
    assert_eq!(findings.len(), 2, "{findings:#?}");
    assert!(findings[0].message.contains("is not capital letters only"));
    assert!(findings[1].message.contains("states an empty grade"));
}

#[test]
fn basic_and_single_limit_need_a_stated_nominal() {
    let length = || Some(literal(10.0, PmiQuantity::Length));
    let mut ir = document([
        stating(
            PmiDimensionKind::Size,
            length(),
            Some(Tolerance::Basic),
            Some(2),
        ),
        stating(
            PmiDimensionKind::Size,
            length(),
            Some(Tolerance::SingleLimit(LimitSide::Minimum)),
            None,
        ),
        stating(PmiDimensionKind::Size, None, Some(Tolerance::General), None),
    ]);
    assert_errors(&mut ir, "", 0);

    let mut ir = document([
        stating(PmiDimensionKind::Size, None, Some(Tolerance::Basic), None),
        stating(
            PmiDimensionKind::Size,
            None,
            Some(Tolerance::SingleLimit(LimitSide::Maximum)),
            None,
        ),
    ]);
    assert_errors(&mut ir, "basic or single-limit tolerance and no nominal", 2);
}

#[test]
fn decimals_need_a_stated_length_or_angle_nominal() {
    let mut ir = document([
        stating(
            PmiDimensionKind::Size,
            Some(literal(10.0, PmiQuantity::Length)),
            None,
            Some(2),
        ),
        stating(
            PmiDimensionKind::Angular,
            Some(literal(0.5, PmiQuantity::Angle)),
            None,
            Some(1),
        ),
        stating(PmiDimensionKind::Size, None, None, None),
    ]);
    assert_errors(&mut ir, "", 0);

    let mut ir = document([
        stating(PmiDimensionKind::Size, None, None, Some(2)),
        stating(
            PmiDimensionKind::Other("ratio".into()),
            Some(literal(0.5, PmiQuantity::Ratio)),
            None,
            Some(2),
        ),
    ]);
    assert_errors(
        &mut ir,
        "states `decimals` and no nominal with a length or angle quantity",
        2,
    );
}

fn warnings(ir: &mut CadIr) -> Vec<Finding> {
    ir.finalize();
    validate_neutral(ir, Vec::new())
        .expect("validate the neutral model")
        .findings
        .into_iter()
        .filter(|finding| finding.check == Check::Pmi && finding.severity == Severity::Warning)
        .collect()
}

#[test]
fn a_bound_dimension_restating_its_parameters_tolerance_or_decimals_is_a_warning() {
    let mut ir = bound_document(
        [("length", ValueKind::Quantity(Quantity::LENGTH))],
        [stating(
            PmiDimensionKind::Size,
            Some(Dimension::Parameter(parameter_id("length"))),
            Some(Tolerance::Deviations(deviations(-0.1, 0.1))),
            Some(2),
        )],
    );
    ir.design.parameters[0].tolerance = Some(Tolerance::Deviations(deviations(-0.1, 0.2)));
    ir.design.parameters[0].decimals = Some(3);
    assert!(warnings(&mut ir).is_empty());

    ir.design.parameters[0].tolerance = Some(Tolerance::Deviations(deviations(-0.1, 0.1)));
    ir.design.parameters[0].decimals = Some(2);
    let found = warnings(&mut ir);
    assert_eq!(found.len(), 2, "{found:#?}");
    assert!(found[0].message.contains("(D61)"));
    assert!(found[1].message.contains("(D60)"));
}

#[test]
fn a_directed_location_has_two_targets() {
    let shape = |key: &str| {
        crate::references::InstanceTarget::new(crate::pmi::PmiTarget::Product {
            product: ProductDefinitionId::mint(format!("test:model:definition#{key}"))
                .expect("valid identity"),
        })
    };
    let mut ir = document([PmiDimension::new(PmiDimensionKind::DirectedLocation)]);
    assert_errors(&mut ir, "directed location dimension has 0 targets", 1);
    ir.model.pmi[0].targets = vec![shape("origin"), shape("target")];
    let findings = pmi_errors(&mut ir);
    assert!(
        findings
            .iter()
            .all(|finding| !finding.message.contains("directed location")),
        "{findings:#?}"
    );
}

/// Under `Negated` the tolerance compares in the dimension's sign: the
/// parameter's mirrored, its lower and upper exchanged and negated (D61).
#[test]
fn a_negated_dimension_compares_its_tolerance_with_the_parameters_mirrored() {
    let negated = |tolerance: Tolerance| {
        let mut ir = bound_document(
            [("length", ValueKind::Quantity(Quantity::LENGTH))],
            [stating(
                PmiDimensionKind::Size,
                Some(Dimension::Negated(parameter_id("length"))),
                Some(tolerance),
                None,
            )],
        );
        ir.design.parameters[0].tolerance = Some(Tolerance::Deviations(deviations(-0.1, 0.2)));
        warnings(&mut ir)
    };
    let found = negated(Tolerance::Deviations(deviations(-0.2, 0.1)));
    assert_eq!(found.len(), 1, "{found:#?}");
    assert!(found[0].message.contains("(D61)"));
    assert!(negated(Tolerance::Deviations(deviations(-0.1, 0.2))).is_empty());
}
