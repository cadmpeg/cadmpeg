// SPDX-License-Identifier: Apache-2.0
//! Bind source-owned operation quantities without copying parameter caches.

use std::collections::BTreeMap;

use cadmpeg_core::CodecError;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::{
    features::{FeatureId, ParameterValue},
    CadIr,
};

use crate::loss::CreoLossCode;

/// A single source-owned quantity and a single operation slot of that quantity
/// form an unambiguous link. Multiple candidates require additional source
/// field semantics; equality of evaluated numbers never establishes a link.
pub(in crate::decode) fn bind_owned_dimensions(
    ir: &mut CadIr,
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    let mut owners = BTreeMap::<FeatureId, Vec<_>>::new();
    for parameter in &ir.model.parameters {
        if let Some(owner) = &parameter.owner {
            if parameter.native_ref.is_some() {
                owners
                    .entry(owner.clone())
                    .or_default()
                    .push(parameter.clone());
            }
        }
    }
    for feature in &mut ir.model.features {
        let Some(parameters) = owners.get(&feature.id) else {
            continue;
        };
        let mut definition = feature.evaluation.definition().clone();
        let value = serde_json::to_value(&definition)
            .map_err(|error| CodecError::malformed(error.to_string()))?;
        let paths = definition.dimension_paths();
        for quantity in [Quantity::Length, Quantity::Angle, Quantity::Integer] {
            let parameters = parameters
                .iter()
                .filter(|parameter| {
                    parameter
                        .value
                        .as_ref()
                        .is_some_and(|value| quantity.accepts(value))
                })
                .collect::<Vec<_>>();
            let slots = paths
                .iter()
                .filter(|path| quantity.slot(path))
                .filter(|path| {
                    value
                        .pointer(path)
                        .and_then(|value| value.get("kind"))
                        .and_then(serde_json::Value::as_str)
                        == Some("literal")
                })
                .collect::<Vec<_>>();
            if let ([parameter], [path]) = (parameters.as_slice(), slots.as_slice()) {
                let old = value
                    .pointer(path)
                    .and_then(|value| value.get("value"))
                    .and_then(serde_json::Value::as_f64);
                let cache = parameter.value.as_ref().and_then(number);
                if old.zip(cache).is_some_and(|(old, cache)| {
                    (old - cache).abs() > 64.0 * f64::EPSILON * old.abs().max(cache.abs()).max(1.0)
                }) {
                    feature.needs_recompute = true;
                    losses.push(CreoLossCode::EvaluationStale.note(format!(
                        "Feature {} source evaluation at {} disagrees with dimension-table parameter {}; the parameter binding is canonical and the solved geometry requires recomputation.",
                        feature.id, path, parameter.id
                    )));
                }
                definition
                    .bind_dimension(path, parameter.id.clone())
                    .map_err(CodecError::malformed)?;
            } else if !parameters.is_empty() && !slots.is_empty() {
                for parameter in parameters {
                    losses.push(CreoLossCode::NeutralRecordUnprojected.note(format!(
                        "Parameter {} belongs to feature {}, but its operation slot is not identified by the decoded dimension-table fields; no binding was inferred from matching numbers.",
                        parameter.id, feature.id
                    )));
                }
            }
        }
        feature.evaluation.set_definition(definition);
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum Quantity {
    Length,
    Angle,
    Integer,
}
impl Quantity {
    fn accepts(self, value: &ParameterValue) -> bool {
        matches!(
            (self, value),
            (Self::Length, ParameterValue::Length(_))
                | (Self::Angle, ParameterValue::Angle(_))
                | (Self::Integer, ParameterValue::Integer(_))
        )
    }
    fn slot(self, path: &str) -> bool {
        let field = path.rsplit('/').next().unwrap_or("");
        match self {
            Self::Length => matches!(
                field,
                "length"
                    | "radius"
                    | "diameter"
                    | "distance"
                    | "height"
                    | "width"
                    | "depth"
                    | "offset"
                    | "thickness"
                    | "pitch"
            ),
            Self::Angle => matches!(field, "angle" | "draft" | "half_angle" | "taper_angle"),
            Self::Integer => matches!(field, "count" | "occurrences"),
        }
    }
}
fn number(value: &ParameterValue) -> Option<f64> {
    match value {
        ParameterValue::Length(value) => Some(value.get()),
        ParameterValue::Angle(value) => Some(value.get()),
        ParameterValue::Integer(value) => cadmpeg_core::convert::f64_from_i64(*value),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadmpeg_ir::features::{
        DesignParameter, Dimension, Feature, FeatureContent, FeatureDefinition, FeatureEvaluation,
        FeatureOperation, ParameterId,
    };
    use cadmpeg_ir::scalar::{Length, PositiveLength};

    fn document(operation: FeatureOperation) -> CadIr {
        let mut ir = CadIr::empty();
        let feature = FeatureId::mint("creo:test:feature#1").expect("fixture value is valid");
        ir.model.features.push(Feature {
            product_definition: None,
            id: feature.clone(),
            ordinal: 0,
            name: None,
            suppressed: None,
            needs_recompute: false,
            dependencies: cadmpeg_ir::features::DistinctMembers::default(),
            source_properties: BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: FeatureContent::default(),
            native_ref: None,
            evaluation: FeatureEvaluation::from_definition(FeatureDefinition::Operation(operation)),
        });
        let value = ParameterValue::Length(Length::new(3.0).expect("fixture value is valid"));
        ir.model.parameters.push(DesignParameter {
            product_definition: None,
            id: ParameterId::mint("creo:test:parameter#1").expect("fixture value is valid"),
            owner: Some(feature),
            ordinal: 0,
            name: "d1".into(),
            expression: value.literal_expression(),
            value: Some(value),
            driving: true,
            display: None,
            properties: BTreeMap::new(),
            pmi: None,
            native_ref: Some("creo:featdefs:sketch#1".into()),
        });
        ir
    }

    #[test]
    fn unique_source_owned_quantity_binds_without_a_value_copy() {
        let mut ir = document(FeatureOperation::DatumOffsetPlane {
            reference: None,
            distance: Length::new(3.0).expect("fixture value is valid").into(),
        });
        let mut losses = Vec::new();
        bind_owned_dimensions(&mut ir, &mut losses).expect("fixture value is valid");
        assert!(
            matches!(ir.model.features[0].definition(), FeatureDefinition::Operation(FeatureOperation::DatumOffsetPlane { distance: Dimension::Parameter(id), .. }) if id == &ir.model.parameters[0].id)
        );
        assert!(!ir.model.features[0].needs_recompute);
        assert!(losses.is_empty());
    }

    #[test]
    fn station_disagreement_is_stale_and_parameter_is_canonical() {
        let mut ir = document(FeatureOperation::DatumOffsetPlane {
            reference: None,
            distance: Length::new(2.0).expect("fixture value is valid").into(),
        });
        let mut losses = Vec::new();
        bind_owned_dimensions(&mut ir, &mut losses).expect("fixture value is valid");
        assert!(ir.model.features[0].needs_recompute);
        assert!(losses
            .iter()
            .any(|loss| loss.code == CreoLossCode::EvaluationStale.kind()));
        assert!(matches!(
            ir.model.features[0].definition(),
            FeatureDefinition::Operation(FeatureOperation::DatumOffsetPlane {
                distance: Dimension::Parameter(_),
                ..
            })
        ));
    }

    #[test]
    fn equal_numbers_do_not_disambiguate_source_slots() {
        let mut ir = document(FeatureOperation::PlanarPatch {
            length: PositiveLength::new(3.0)
                .expect("fixture value is valid")
                .into(),
            width: PositiveLength::new(3.0)
                .expect("fixture value is valid")
                .into(),
        });
        let mut losses = Vec::new();
        bind_owned_dimensions(&mut ir, &mut losses).expect("fixture value is valid");
        assert!(matches!(
            ir.model.features[0].definition(),
            FeatureDefinition::Operation(FeatureOperation::PlanarPatch {
                length: Dimension::Literal(_),
                width: Dimension::Literal(_)
            })
        ));
        assert!(losses.iter().any(|loss| loss.code
            == CreoLossCode::NeutralRecordUnprojected.kind()
            && loss.message.contains(ir.model.parameters[0].id.as_str())));
    }
}
