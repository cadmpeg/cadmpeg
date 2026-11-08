// SPDX-License-Identifier: Apache-2.0
//! Canonical edits update native copies without creating another authority.

use std::collections::BTreeSet;
use std::fs;

#[cfg(feature = "fcstd")]
use cadmpeg_ir::document::DimensionEditError;
#[cfg(any(feature = "fcstd", feature = "sldprt"))]
use cadmpeg_ir::features::{Dimension, FeatureOperation};
use cadmpeg_ir::features::{Expression, FeatureId, ParameterId, ParameterValue};
use cadmpeg_ir::report::{
    export::{ExportReport, WritePath},
    Severity,
};
use cadmpeg_ir::scalar::Length;
use cadmpeg_ir::CadIr;
use serde_json::Value;

use super::{decode, workspace, write_decode};

const EPS_DIMENSION_MM: f64 = 1.0e-9;

#[cfg(any(feature = "fcstd", feature = "sldprt"))]
struct BoundLength {
    feature: FeatureId,
    feature_name: String,
    parameter: ParameterId,
    parameter_name: String,
    path: String,
    value_mm: f64,
}

#[cfg(any(feature = "fcstd", feature = "sldprt"))]
fn extrusion_length(ir: &CadIr) -> BoundLength {
    ir.model
        .features
        .iter()
        .find_map(|feature| {
            if !matches!(
                feature.definition().operation(),
                FeatureOperation::Extrude { .. }
            ) {
                return None;
            }
            let definition = serde_json::to_value(feature.definition()).unwrap();
            feature
                .definition()
                .dimension_paths()
                .into_iter()
                .find_map(|path| {
                    let dimension: Dimension<Length> =
                        serde_json::from_value(definition.pointer(&path)?.clone()).ok()?;
                    let parameter = dimension.parameter()?;
                    let row = ir.model.parameters.iter().find(|row| {
                        row.id == *parameter && row.owner.as_ref() == Some(&feature.id)
                    })?;
                    let ParameterValue::Length(value) = row.value.as_ref()? else {
                        return None;
                    };
                    Some(BoundLength {
                        feature: feature.id.clone(),
                        feature_name: feature.name.clone().expect("named extrusion"),
                        parameter: parameter.clone(),
                        parameter_name: row.name.clone(),
                        path,
                        value_mm: value.get(),
                    })
                })
        })
        .expect("the source extrusion has a canonical bound length")
}

fn assert_invalidated(ir: &CadIr, feature: &FeatureId) {
    let mut affected = BTreeSet::from([feature.clone()]);
    loop {
        let before = affected.len();
        for feature in &ir.model.features {
            if feature
                .dependencies
                .iter()
                .any(|dependency| affected.contains(dependency))
            {
                affected.insert(feature.id.clone());
            }
        }
        if before == affected.len() {
            break;
        }
    }
    assert!(
        ir.model
            .features
            .iter()
            .filter(|feature| affected.contains(&feature.id))
            .all(|feature| feature.needs_recompute),
        "the edited feature and its dependents carry stale evaluation state"
    );
}

#[cfg(any(feature = "fcstd", feature = "sldprt"))]
fn assert_bound_length(
    ir: &CadIr,
    feature_name: &str,
    parameter_name: &str,
    expected_mm: f64,
) -> ParameterId {
    let feature = ir
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some(feature_name))
        .expect("written extrusion");
    let parameter = ir
        .model
        .parameters
        .iter()
        .find(|parameter| {
            parameter.owner.as_ref() == Some(&feature.id) && parameter.name == parameter_name
        })
        .expect("written canonical parameter");
    let definition = serde_json::to_value(feature.definition()).unwrap();
    let bindings = feature
        .definition()
        .dimension_paths()
        .into_iter()
        .filter(|path| {
            definition.pointer(path).is_some_and(|value| {
                value["kind"] == "parameter"
                    && value["value"].as_str() == Some(parameter.id.as_str())
            })
        })
        .collect::<Vec<_>>();
    assert_eq!(
        bindings.len(),
        1,
        "the operation has one binding instead of a second value copy"
    );
    let resolved = ir
        .dimension_value(&Dimension::<Length>::Parameter(parameter.id.clone()))
        .expect("resolved canonical length");
    assert!((resolved.get() - expected_mm).abs() <= EPS_DIMENSION_MM);
    assert_eq!(
        parameter
            .expression
            .literal_value_as(parameter.value.as_ref().unwrap()),
        parameter.value
    );
    parameter.id.clone()
}

fn assert_staleness_is_written_or_reported(report: &ExportReport, ir: &CadIr, feature_name: &str) {
    assert!(matches!(report.write_path(), WritePath::Synthesized { .. }));
    let written_flag = ir
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some(feature_name))
        .is_some_and(|feature| feature.needs_recompute);
    assert!(
        written_flag
            || report
                .losses
                .iter()
                .any(|loss| loss.code.local_code() == "evaluation.stale"
                    && loss.severity >= Severity::Warning),
        "a stale evaluation is written explicitly or reported at Warning severity"
    );
}

#[cfg(any(feature = "fcstd", feature = "sldprt", feature = "f3d"))]
fn native_records(ir: &CadIr, namespace: &str, arena: &str) -> Vec<Value> {
    ir.native
        .namespace(namespace)
        .expect("native namespace")
        .arenas()
        .get(arena)
        .into_iter()
        .flatten()
        .map(|record| serde_json::to_value(record).unwrap())
        .collect()
}

#[cfg(feature = "fcstd")]
#[test]
fn freecad_one_fact_updates_pad_length_expression_engine_and_touched() {
    use cadmpeg_codec_freecad::FcstdCodec;
    let bytes =
        fs::read(workspace().join("corpus/freecad_fcstd/fixtures/design_history.FCStd")).unwrap();
    let decoded = decode(&FcstdCodec, &bytes);
    let mut ir = decoded.ir().clone();
    let bound = extrusion_length(&ir);
    let expected_mm = bound.value_mm + 5.0;
    ir.set_dimension(
        &bound.feature,
        &bound.path,
        Length::new(expected_mm).unwrap(),
    )
    .unwrap();
    assert_eq!(
        ir.dimension_value(&Dimension::<Length>::Parameter(bound.parameter.clone()))
            .unwrap()
            .get(),
        expected_mm
    );
    assert_invalidated(&ir, &bound.feature);
    let (written, report) = write_decode(&FcstdCodec, &FcstdCodec, &ir, None, None);
    let after = written.ir();
    assert_bound_length(
        after,
        &bound.feature_name,
        &bound.parameter_name,
        expected_mm,
    );
    assert_staleness_is_written_or_reported(&report, after, &bound.feature_name);
    assert!(
        after
            .model
            .features
            .iter()
            .find(|feature| feature.name.as_deref() == Some(&bound.feature_name))
            .unwrap()
            .needs_recompute,
        "FreeCAD persists Touched"
    );
    let feature = after
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some(&bound.feature_name))
        .unwrap();
    let owner = feature.native_ref.as_ref().expect("Pad native counterpart");
    let properties = native_records(after, "fcstd", "properties");
    let length = properties
        .iter()
        .find(|property| property["owner"].as_str() == Some(owner) && property["name"] == "Length")
        .expect("Pad.Length");
    assert!(
        length["values"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value["attributes"]["value"]
                .as_str()
                .and_then(|value| value.parse::<f64>().ok())
                .is_some_and(|value| (value - expected_mm).abs() <= EPS_DIMENSION_MM)),
        "the native property agrees with the canonical cache"
    );
    let engines = properties
        .iter()
        .filter(|property| {
            property["owner"].as_str() == Some(owner) && property["name"] == "ExpressionEngine"
        })
        .collect::<Vec<_>>();
    assert!(
        engines
            .iter()
            .flat_map(|engine| engine["values"].as_array().into_iter().flatten())
            .all(|value| value["attributes"]["path"] != "Length"),
        "a literal binding lives in the value lane, never as a second engine copy"
    );
}

#[cfg(feature = "fcstd")]
#[test]
fn freecad_nonliteral_pad_expression_survives_in_the_expression_engine() {
    use cadmpeg_codec_freecad::FcstdCodec;
    use cadmpeg_ir::features::ExpressionToken;

    let bytes =
        fs::read(workspace().join("corpus/freecad_fcstd/fixtures/design_history.FCStd")).unwrap();
    let decoded = decode(&FcstdCodec, &bytes);
    let mut ir = decoded.ir().clone();
    let bound = extrusion_length(&ir);
    let driver = ir
        .model
        .parameters
        .iter()
        .find(|parameter| {
            parameter.id != bound.parameter
                && parameter.owner.as_ref() != Some(&bound.feature)
                && matches!(parameter.value, Some(ParameterValue::Length(_)))
        })
        .expect("independent length driver")
        .clone();
    let ParameterValue::Length(driver_value) = driver.value.unwrap() else {
        unreachable!()
    };
    let expected_mm = driver_value.get() + 1.0;
    ir.set_parameter_expression(
        &bound.parameter,
        Expression {
            tokens: vec![
                ExpressionToken::Parameter(driver.id),
                ExpressionToken::Text(" + 1 mm".into()),
            ],
        },
        Some(ParameterValue::Length(Length::new(expected_mm).unwrap())),
    )
    .unwrap();
    assert_invalidated(&ir, &bound.feature);
    let (written, report) = write_decode(&FcstdCodec, &FcstdCodec, &ir, None, None);
    let after = written.ir();
    let feature = after
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some(&bound.feature_name))
        .expect("written Pad");
    let parameter = after
        .model
        .parameters
        .iter()
        .find(|parameter| {
            parameter.owner.as_ref() == Some(&feature.id) && parameter.name == bound.parameter_name
        })
        .expect("written Pad length");
    assert_eq!(parameter.expression.dependencies().count(), 1);
    assert!(
        matches!(parameter.value, Some(ParameterValue::Length(value)) if (value.get() - expected_mm).abs() <= EPS_DIMENSION_MM)
    );
    assert_staleness_is_written_or_reported(&report, after, &bound.feature_name);
    let owner = feature.native_ref.as_ref().expect("Pad native counterpart");
    let properties = native_records(after, "fcstd", "properties");
    let engine = properties
        .iter()
        .find(|property| {
            property["owner"].as_str() == Some(owner) && property["name"] == "ExpressionEngine"
        })
        .expect("Pad ExpressionEngine");
    assert!(engine["values"].as_array().unwrap().iter().any(|value| {
        value["attributes"]["path"] == bound.parameter_name
            && value["attributes"]["expression"]
                .as_str()
                .is_some_and(|s| !s.is_empty())
    }));
}

#[cfg(feature = "fcstd")]
#[test]
fn driven_dimension_edits_preserve_reference_state_and_report_formula_drivers() {
    use cadmpeg_codec_freecad::FcstdCodec;
    use cadmpeg_ir::features::ExpressionToken;
    let bytes =
        fs::read(workspace().join("corpus/freecad_fcstd/fixtures/design_history.FCStd")).unwrap();
    let decoded = decode(&FcstdCodec, &bytes);
    for formula_driven in [false, true] {
        let mut ir = decoded.ir().clone();
        let bound = extrusion_length(&ir);
        let drivers = if formula_driven {
            let driver = ir
                .model
                .parameters
                .iter()
                .find(|parameter| {
                    parameter.id != bound.parameter
                        && matches!(parameter.value, Some(ParameterValue::Length(_)))
                })
                .expect("independent length driver")
                .id
                .clone();
            let expression = Expression {
                tokens: vec![ExpressionToken::Parameter(driver.clone())],
            };
            let cache = ir
                .model
                .parameters
                .iter()
                .find(|parameter| parameter.id == bound.parameter)
                .unwrap()
                .value
                .clone();
            ir.set_parameter_expression(&bound.parameter, expression, cache)
                .unwrap();
            vec![driver]
        } else {
            // A measured/reference parameter may have no expression drivers.
            ir.model
                .parameters
                .iter_mut()
                .find(|parameter| parameter.id == bound.parameter)
                .unwrap()
                .driving = false;
            Vec::new()
        };
        let before = ir.clone();
        assert_eq!(
            ir.set_dimension(
                &bound.feature,
                &bound.path,
                Length::new(bound.value_mm + 5.0).unwrap()
            ),
            Err(DimensionEditError::DrivenParameter {
                parameter: bound.parameter,
                drivers
            })
        );
        assert_eq!(ir, before, "a refused driven edit is atomic");
    }
}

#[cfg(feature = "f3d")]
#[test]
fn fusion_driven_extrude_depth_rejects_dimension_edit() {
    use cadmpeg_codec_f3d::F3dCodec;
    use cadmpeg_ir::document::DimensionEditError;
    use cadmpeg_ir::features::FeatureOperation;

    let bytes = fs::read(
        workspace().join("crates/cadmpeg-codec-f3d/tests/golden/fixtures/design_extrude_depth.f3d"),
    )
    .unwrap();
    let source = decode(&F3dCodec, &bytes);
    let mut ir = source.ir().clone();
    let feature = ir
        .model
        .features
        .iter()
        .find(|feature| {
            matches!(
                feature.definition().operation(),
                FeatureOperation::Extrude { .. }
            )
        })
        .unwrap();
    let feature_id = feature.id.clone();
    let definition = serde_json::to_value(feature.definition()).unwrap();
    let parameter = ir
        .model
        .parameters
        .iter()
        .find(|parameter| {
            parameter.name == "Depth" && parameter.owner.as_ref() == Some(&feature_id)
        })
        .unwrap()
        .id
        .clone();
    let path = feature
        .definition()
        .dimension_paths()
        .into_iter()
        .find(|path| {
            definition.pointer(path).is_some_and(|value| {
                value["kind"] == "parameter" && value["value"] == parameter.as_str()
            })
        })
        .unwrap();
    ir.model
        .parameters
        .iter_mut()
        .find(|row| row.id == parameter)
        .unwrap()
        .driving = false;
    let before = ir.clone();
    assert_eq!(
        ir.set_dimension(&feature_id, &path, Length::new(15.0).unwrap()),
        Err(DimensionEditError::DrivenParameter {
            parameter,
            drivers: Vec::new()
        })
    );
    assert_eq!(ir, before);
}

#[cfg(feature = "fcstd")]
#[test]
fn feature_rename_renders_dependent_expression_tokens_with_the_current_owner_name() {
    use cadmpeg_codec_freecad::FcstdCodec;
    use cadmpeg_ir::features::ExpressionToken;
    let bytes =
        fs::read(workspace().join("corpus/freecad_fcstd/fixtures/design_history.FCStd")).unwrap();
    let decoded = decode(&FcstdCodec, &bytes);
    let mut ir = decoded.ir().clone();
    let bound = extrusion_length(&ir);
    let owner_ordinal = ir
        .model
        .features
        .iter()
        .find(|feature| feature.id == bound.feature)
        .unwrap()
        .ordinal;
    let dependent_owner = ir
        .model
        .features
        .iter()
        .find(|feature| {
            feature.id != bound.feature
                && feature.ordinal > owner_ordinal
                && feature.native_ref.is_some()
        })
        .expect("a later independent native feature can own the derived parameter")
        .id
        .clone();
    let mut dependent = ir
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.id == bound.parameter)
        .unwrap()
        .clone();
    dependent.id = ParameterId::mint("cadir:design:parameter#dependent-depth").unwrap();
    dependent.name = "DependentDepth".into();
    dependent.owner = Some(dependent_owner);
    dependent.native_ref = None;
    dependent.properties.clear();
    dependent.pmi = None;
    dependent.driving = false;
    let dependent_id = dependent.id.clone();
    ir.model.parameters.push(dependent);
    ir.model.finalize();
    let expression = Expression {
        tokens: vec![
            ExpressionToken::Parameter(bound.parameter.clone()),
            ExpressionToken::Text(" + 1 mm".into()),
        ],
    };
    ir.set_parameter_expression(
        &dependent_id,
        expression,
        Some(ParameterValue::Length(
            Length::new(bound.value_mm + 1.0).unwrap(),
        )),
    )
    .unwrap();
    let current_name = "CurrentOwner";
    ir.model
        .features
        .iter_mut()
        .find(|feature| feature.id == bound.feature)
        .unwrap()
        .name = Some(current_name.into());
    let (written, _) = write_decode(&FcstdCodec, &FcstdCodec, &ir, None, None);
    let after = written.ir();
    let feature = after
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some(current_name))
        .expect("renamed owner");
    let base = after
        .model
        .parameters
        .iter()
        .find(|parameter| {
            parameter.owner.as_ref() == Some(&feature.id) && parameter.name == bound.parameter_name
        })
        .expect("current base parameter");
    let dependent = after
        .model
        .parameters
        .iter()
        .find(|parameter| {
            parameter.owner.as_ref() != Some(&feature.id) && parameter.name == "DependentDepth"
        })
        .expect("written dependent parameter");
    assert_eq!(
        dependent.expression.dependencies().collect::<Vec<_>>(),
        vec![&base.id]
    );
    let owner = after
        .model
        .features
        .iter()
        .find(|feature| Some(&feature.id) == dependent.owner.as_ref())
        .unwrap()
        .native_ref
        .as_ref()
        .expect("dependent owner native counterpart");
    let properties = native_records(after, "fcstd", "properties");
    let engine = properties
        .iter()
        .find(|property| {
            property["owner"].as_str() == Some(owner) && property["name"] == "ExpressionEngine"
        })
        .expect("current owner ExpressionEngine");
    let rendered = engine["values"]
        .as_array()
        .unwrap()
        .iter()
        .find(|value| value["attributes"]["path"] == "DependentDepth")
        .expect("dependent expression")["attributes"]["expression"]
        .as_str()
        .unwrap();
    assert!(
        rendered.contains(current_name),
        "the writer renders the current owner name: {rendered}"
    );
    assert!(
        !rendered.contains(&format!("{}.{}", bound.feature_name, bound.parameter_name)),
        "a prior owner name has no rename authority: {rendered}"
    );
}
