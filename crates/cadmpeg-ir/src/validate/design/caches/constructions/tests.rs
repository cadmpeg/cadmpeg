// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use crate::design::configuration::ParameterOverride;
use crate::design::construction::{
    kinds, AnyConstruction, Construction, ConstructionInput, ConstructionKind, InputReference,
    InputRole, PlaneValue, Relation, ScalarDimension, Subject,
};
use crate::design::dimension::Dimension;
use crate::design::feature::FeatureOperation;
use crate::design::ops::datum::{Datum, DatumElement};
use crate::design::parameter::ParameterValue;
use crate::design::selection::{
    Anchor, Members, PickTarget, Picked, Picks, PrincipalEntity, Selection,
};
use crate::design::units::{Quantity, ValueKind};
use crate::document::CadIr;
use crate::features::geometry::FinitePoint3;
use crate::math::{Point3, Vector3};
use crate::report::Severity;
use crate::scalar::{FiniteReal, Length, PositiveLength};
use crate::units::UnitVector3;
use crate::validate::design::fixture::{design_findings, document, id, parameter};

/// The linear tolerance in force in these tests, exact in binary64.
const LINEAR: f64 = 1.0 / 1024.0;

const MISSES: &str = "misses its";

fn construction<K: ConstructionKind>(
    inputs: Vec<ConstructionInput>,
    value: Option<K::Value>,
) -> Construction<K> {
    Construction {
        inputs,
        after: Vec::new(),
        solution: None,
        mode: None,
        resolved: value,
        resolved_in: BTreeMap::new(),
    }
}

fn input(
    subject: Subject,
    relation: Relation,
    reference: Option<InputReference>,
    value: Option<ScalarDimension>,
) -> ConstructionInput {
    ConstructionInput {
        role: Some(InputRole { subject, relation }),
        reference,
        value,
        reversed: None,
    }
}

fn principal(entity: PrincipalEntity) -> InputReference {
    InputReference::Picks(Selection {
        at: Anchor::Input,
        picks: Picks::Named(
            Members::new(vec![Picked {
                target: PickTarget::Principal {
                    entity,
                    container: None,
                },
                context: None,
                native: None,
                domain: None,
                witness: None,
                resolved: None,
            }])
            .unwrap(),
        ),
        native: None,
    })
}

fn plane_at(z: f64, normal: [f64; 3]) -> PlaneValue {
    PlaneValue {
        origin: FinitePoint3::new(Point3::new(0.0, 0.0, z)).unwrap(),
        normal: UnitVector3::new(Vector3::new(normal[0], normal[1], normal[2])).unwrap(),
        x_axis: UnitVector3::new(Vector3::new(1.0, 0.0, 0.0)).unwrap(),
    }
}

fn datum(element: DatumElement) -> CadIr {
    let mut ir = document();
    ir.tolerances.linear = PositiveLength::new(LINEAR).unwrap();
    ir.design.features[0].definition.operation = FeatureOperation::Datum(Box::new(Datum {
        element,
        link: None,
        native_options: BTreeMap::new(),
        source_settings: BTreeMap::new(),
    }));
    ir
}

fn offset_plane(distance: Dimension<Length>, resolved: PlaneValue) -> Construction<kinds::Plane> {
    construction(
        vec![input(
            Subject::Whole,
            Relation::Offset,
            Some(principal(PrincipalEntity::XyPlane)),
            Some(ScalarDimension::Length(distance)),
        )],
        Some(resolved),
    )
}

fn warnings(ir: &CadIr) -> Vec<String> {
    design_findings(ir)
        .into_iter()
        .filter(|finding| finding.severity == Severity::Warning && finding.message.contains(MISSES))
        .map(|finding| finding.message)
        .collect()
}

#[test]
fn an_offset_plane_lies_at_its_distance_from_its_reference() {
    let five = || Dimension::Literal(Length::new(5.0).unwrap());
    let offset = |z: f64, normal: [f64; 3]| {
        datum(DatumElement::Plane(offset_plane(
            five(),
            plane_at(z, normal),
        )))
    };
    assert!(warnings(&offset(5.0, [0.0, 0.0, 1.0])).is_empty());
    assert!(warnings(&offset(5.0 + LINEAR, [0.0, 0.0, -1.0])).is_empty());
    assert_eq!(
        warnings(&offset(5.0 + 2.0 * LINEAR, [0.0, 0.0, 1.0])).len(),
        1
    );
    let tilted = [0.0, 0.6, 0.8];
    assert_eq!(warnings(&offset(5.0, tilted)).len(), 1);
}

#[test]
fn a_point_coincides_with_its_built_reference() {
    let at = |z: f64| FinitePoint3::new(Point3::new(1.0, 2.0, z)).unwrap();
    let coincident = |z: f64| {
        let built = AnyConstruction::Point(construction(Vec::new(), Some(at(3.0))));
        datum(DatumElement::Points(vec![construction(
            vec![input(
                Subject::Whole,
                Relation::Coincident,
                Some(InputReference::Built(Box::new(built))),
                None,
            )],
            Some(at(z)),
        )]))
    };
    assert!(warnings(&coincident(3.0)).is_empty());
    assert_eq!(warnings(&coincident(4.0)).len(), 1);
}

#[test]
fn a_plane_meets_its_equation() {
    let real = |value: f64| Dimension::Literal(FiniteReal::new(value).unwrap());
    let equation = |z: f64| {
        datum(DatumElement::Plane(construction(
            vec![input(
                Subject::Whole,
                Relation::Coordinates,
                None,
                Some(ScalarDimension::PlaneEquation {
                    a: real(0.0),
                    b: real(0.0),
                    c: real(2.0),
                    d: Dimension::Literal(Length::new(10.0).unwrap()),
                }),
            )],
            Some(plane_at(z, [0.0, 0.0, 1.0])),
        )))
    };
    assert!(warnings(&equation(5.0)).is_empty());
    assert_eq!(warnings(&equation(6.0)).len(), 1);
}

#[test]
fn a_resolved_in_value_reads_its_configuration() {
    let in_large = |z: f64| {
        let mut plane = offset_plane(
            Dimension::Parameter(id("parameter", "offset")),
            plane_at(5.0, [0.0, 0.0, 1.0]),
        );
        plane
            .resolved_in
            .insert(id("configuration", "large"), plane_at(z, [0.0, 0.0, 1.0]));
        let mut ir = datum(DatumElement::Plane(plane));
        let mut offset = parameter("offset", ValueKind::Quantity(Quantity::LENGTH));
        offset.value = Some(ParameterValue::Number(FiniteReal::new(5.0).unwrap()));
        ir.design.parameters.push(offset);
        let large = ir
            .design
            .configurations
            .iter_mut()
            .find(|configuration| configuration.id.as_str().ends_with("#large"))
            .unwrap();
        large.overrides.parameters.insert(
            id("parameter", "offset"),
            ParameterOverride {
                expression: None,
                value: Some(ParameterValue::Number(FiniteReal::new(7.0).unwrap())),
            },
        );
        ir
    };
    assert!(warnings(&in_large(7.0)).is_empty());
    let found = warnings(&in_large(5.0));
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].contains("resolved_in test:design:configuration#large"));
}
