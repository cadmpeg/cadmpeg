// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::features::{DesignParameter, ParameterId, SweepCircularRegion};
use crate::scalar::PositiveLength;
use crate::CadIr;

fn parameter(id: ParameterId) -> DesignParameter {
    let value = ParameterValue::Length(Length::new(12.0).unwrap());
    DesignParameter {
        id,
        owner: None,
        ordinal: 0,
        name: "diameter".into(),
        expression: value.literal_expression(),
        driving: true,
        display: None,
        value: Some(value),
        properties: Default::default(),
        pmi: None,
        native_ref: None,
    }
}

#[test]
fn diameter_keeps_its_only_control_and_derives_radius_from_the_current_parameter() {
    let mut ir = CadIr::empty();
    let id = ParameterId::mint("test:model:parameter#diameter").unwrap();
    ir.model.parameters.push(parameter(id.clone()));
    let radial = RadialDimension::<PositiveLength>::Diameter(Dimension::Parameter(id.clone()));
    let wire = serde_json::to_value(&radial).unwrap();
    assert_eq!(
        wire,
        serde_json::json!({"kind":"diameter","value":{"kind":"parameter","value":id}})
    );
    assert_eq!(radial.resolved_radius(&ir).unwrap().get(), 6.0);
    ir.model.parameters[0].value = Some(ParameterValue::Length(Length::new(20.0).unwrap()));
    assert_eq!(radial.resolved_radius(&ir).unwrap().get(), 10.0);
    assert_eq!(serde_json::to_value(&radial).unwrap(), wire);
    let ctx = cadmpeg_test_support::service_decode_context();
    let temporary = radial.clone().resolve_dimensions(&ir, &ctx).unwrap();
    assert!(
        matches!(temporary,RadialDimension::Radius(Dimension::Literal(value)) if value.get()==10.0)
    );
    assert_eq!(serde_json::to_value(&radial).unwrap(), wire);
    ctx.finish_session().unwrap();
}

#[test]
fn circular_wall_admission_compares_against_the_derived_radius() {
    let diameter = RadialDimension::Diameter(Dimension::Literal(PositiveLength::new(8.0).unwrap()));
    assert!(SweepCircularRegion::new(
        (diameter.clone()).into(),
        Some(Dimension::Literal(PositiveLength::new(4.0).unwrap()))
    )
    .is_err());
    assert!(SweepCircularRegion::new(
        (diameter).into(),
        Some(Dimension::Literal(PositiveLength::new(3.0).unwrap()))
    )
    .is_ok());
}
