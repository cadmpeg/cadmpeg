// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]
#![allow(unused_imports)]

use std::collections::{BTreeMap, BTreeSet};
use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, CodecBackend, Confidence, DecodeOptions};
use cadmpeg_ir::sketches::{SketchConstraintDefinition, SketchEntityId};
use cadmpeg_ir::Exactness;

use crate::container::{self, role, Layout};
use crate::surface::TorusRadius2Encoding;
use crate::test_support::*;
use crate::CreoCodec;

const EPS_HELIX_FEATURE: f64 = f64::EPSILON;

#[test]
fn decode_preserves_counted_curve_expression_programs() {
    let payload = b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x89\x4c\
        \xe0\x02local_sys\0\xf9\x04\x03\x18\xe4\x0f\xe4\x18\xe5\x0f\x18\xe6\
        \xe0\x0aexpression\0\xf8\x04r=5\0w=1\0theta=w*t*360\0z=71*t\0"
        .to_vec();
    let data = build_prt("c", &[("DEPDB_DATA", payload)]);
    let scan = container::scan_bytes(data.clone());
    assert_eq!(scan.curves.expressions.len(), 1);
    assert_eq!(scan.curves.expressions[0].entity_id, 0x094c);
    assert_eq!(scan.curves.expressions[0].lines.len(), 4);
    let local_system = scan.curves.expressions[0]
        .local_system
        .as_ref()
        .expect("curve local system");
    assert_eq!((local_system.dimensions, local_system.count), (4, 3));
    assert_eq!(
        local_system.body,
        [0x18, 0xe4, 0x0f, 0xe4, 0x18, 0xe5, 0x0f, 0x18, 0xe6]
    );

    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode");
    let records = &result.ir().native.namespace("creo").unwrap().arenas["curve_expressions"];
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].fields()["entity_id"], 0x094c);
    assert_eq!(records[0].fields()["lines"][2]["text"], "theta=w*t*360");
    assert_eq!(
        records[0].fields()["assignments"][2]["target"]["name"],
        "theta"
    );
    assert_eq!(
        records[0].fields()["assignments"][2]["dependencies"][0],
        "w"
    );
    assert_eq!(records[0].fields()["assignments"][0]["value"], 5.0);
    assert_eq!(records[0].fields()["local_system"]["dimensions"], 4);
    assert_eq!(result.ir().model.features.len(), 1);
    let cadmpeg_ir::features::FeatureDefinition::Helix {
        axis_origin,
        axis_direction,
        radius,
        pitch,
        revolutions,
        start_angle,
        clockwise,
        ..
    } = &result.ir().model.features[0].definition
    else {
        panic!("complete curve-equation frame transfers a neutral helix");
    };
    assert!(axis_origin.x.abs() <= EPS_HELIX_FEATURE);
    assert!(axis_origin.y.abs() <= EPS_HELIX_FEATURE);
    assert!(axis_origin.z.abs() <= EPS_HELIX_FEATURE);
    assert!(axis_direction.x.abs() <= EPS_HELIX_FEATURE);
    assert!(axis_direction.y.abs() <= EPS_HELIX_FEATURE);
    assert!((axis_direction.z + 1.0).abs() <= EPS_HELIX_FEATURE);
    assert!((radius.0 - 5.0).abs() <= EPS_HELIX_FEATURE);
    assert!((pitch.0 - 71.0).abs() <= EPS_HELIX_FEATURE);
    assert!((*revolutions - 1.0).abs() <= EPS_HELIX_FEATURE);
    assert!(start_angle.0.abs() <= EPS_HELIX_FEATURE);
    assert!(!clockwise);
    assert_eq!(
        result
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_NATIVE_AXIS_HELIX_FEATURE_COUNT),
        0
    );
    assert_eq!(
        result.report().coverage_count(
            crate::coverage::TRANSFERRED_INCOMPLETE_OTHER_CONSTRUCTION_FEATURE_COUNT
        ),
        0
    );
    assert!(!result
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains("native-axis helix=1")));
    assert_eq!(result.ir().model.parameters.len(), 4);
    assert_eq!(result.ir().model.parameters[0].name, "r");
    assert_eq!(
        result.ir().model.parameters[0].value,
        Some(cadmpeg_ir::features::ParameterValue::Real(5.0))
    );
    assert_eq!(result.ir().model.parameters[2].name, "theta");
    assert_eq!(
        result.ir().model.parameters[2].dependencies,
        [result.ir().model.parameters[1].id.clone()]
    );
    assert_eq!(
        result.ir().model.parameters[2].properties["independent_variables"],
        "t"
    );
    assert!(!result.ir().model.parameters[2]
        .properties
        .contains_key("external_dependencies"));
    assert_eq!(
        result.ir().model.features[0].source_content,
        result
            .ir()
            .model
            .parameters
            .iter()
            .map(
                |parameter| cadmpeg_ir::features::FeatureSourceContent::Parameter(
                    parameter.id.clone()
                )
            )
            .collect::<Vec<_>>()
    );
    assert_annotation(
        &result.source_fidelity().annotations,
        records[0].id(),
        "creo:DEPDB_DATA",
        scan.curves.expressions[0].expression_offset as u64,
        "curve_expression_program",
        Exactness::ByteExact,
    );
}

#[test]
fn decode_preserves_curve_expression_source_section() {
    let payload = b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x07\
        \xe0\x0aexpression\0\xf8\x01value=5\0"
        .to_vec();
    let data = build_prt("c", &[("FeatDefs", payload)]);
    let scan = container::scan_bytes(data.clone());
    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode");
    let records = &result.ir().native.namespace("creo").unwrap().arenas["curve_expressions"];

    assert_eq!(records.len(), 1);
    assert_annotation(
        &result.source_fidelity().annotations,
        records[0].id(),
        "creo:FeatDefs",
        scan.curves.expressions[0].expression_offset as u64,
        "curve_expression_program",
        Exactness::ByteExact,
    );
}

#[test]
fn decode_binds_unique_forward_curve_expression_dependencies() {
    let payload = b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x07\
        \xe0\x0aexpression\0\xf8\x04r=A\0a=5\0theta=T*360\0z=1\0"
        .to_vec();
    let data = build_prt("c", &[("DEPDB_DATA", payload)]);

    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode");
    let [r, a, theta, _] = result.ir().model.parameters.as_slice() else {
        panic!("four curve-expression parameters");
    };

    assert_eq!(r.name, "r");
    assert_eq!(r.ordinal, 1);
    assert_eq!(r.value, None);
    assert_eq!(r.dependencies, std::slice::from_ref(&a.id));
    assert_eq!(a.ordinal, 0);
    assert!(!r.properties.contains_key("external_dependencies"));
    assert_eq!(theta.properties["independent_variables"], "T");
    assert_eq!(
        result.ir().model.features[0].source_content,
        result
            .ir()
            .model
            .parameters
            .iter()
            .map(
                |parameter| cadmpeg_ir::features::FeatureSourceContent::Parameter(
                    parameter.id.clone()
                )
            )
            .collect::<Vec<_>>()
    );
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone());
    assert!(validation.is_ok(), "{validation:#?}");
}

#[test]
fn decode_retains_complete_scoped_curve_expression_dependencies() {
    let payload = b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x07\
        \xe0\x0aexpression\0\xf8\x01value=d1:2+PARAM:FID_20+PI\0"
        .to_vec();
    let data = build_prt("c", &[("DEPDB_DATA", payload)]);

    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode");
    let [parameter] = result.ir().model.parameters.as_slice() else {
        panic!("one curve-expression parameter");
    };

    assert_eq!(
        parameter.properties["external_dependencies"],
        "d1:2,PARAM:FID_20"
    );
    assert!(!parameter.properties.contains_key("ambiguous_dependencies"));
    let coverage = result.report();
    assert_eq!(
        coverage.coverage_count(crate::coverage::DECODED_ACTIVE_CURVE_EXPRESSION_ASSIGNMENT_COUNT),
        1
    );
    assert_eq!(
        coverage.coverage_count(crate::coverage::TRANSFERRED_CURVE_EXPRESSION_PARAMETER_COUNT),
        1
    );
    assert_eq!(
        coverage
            .coverage_count(crate::coverage::EVALUATED_ACTIVE_CURVE_EXPRESSION_ASSIGNMENT_COUNT),
        0
    );
}

#[test]
fn decode_retains_simultaneous_curve_expression_blocks() {
    let payload = b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x07\
        \xe0\x0aexpression\0\xf8\x09area=100\0base=10\0SOLVE\0width=height+1\0\
        offset=base+1\0width*height=area\0FOR width, height\0\
        present=exists('width')\0result=area+1\0"
        .to_vec();
    let data = build_prt("c", &[("DEPDB_DATA", payload)]);

    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode");
    assert_eq!(
        result
            .ir()
            .model
            .parameters
            .iter()
            .map(|parameter| parameter.name.as_str())
            .collect::<Vec<_>>(),
        ["area", "base", "offset", "present", "result"]
    );
    assert_eq!(
        result
            .ir()
            .model
            .parameters
            .iter()
            .map(|parameter| parameter.value.as_ref())
            .collect::<Vec<_>>(),
        [
            Some(&cadmpeg_ir::features::ParameterValue::Real(100.0)),
            Some(&cadmpeg_ir::features::ParameterValue::Real(10.0)),
            Some(&cadmpeg_ir::features::ParameterValue::Real(11.0)),
            Some(&cadmpeg_ir::features::ParameterValue::Real(1.0)),
            Some(&cadmpeg_ir::features::ParameterValue::Real(101.0)),
        ]
    );

    let native = &result.ir().native.namespace("creo").unwrap().arenas["curve_expressions"][0];
    assert_eq!(native.fields()["solve_blocks"][0]["variables"][0], "width");
    assert_eq!(native.fields()["solve_blocks"][0]["variables"][1], "height");
    assert_eq!(
        native.fields()["solve_blocks"][0]["equations"][0]["left"],
        "width"
    );
    assert_eq!(
        native.fields()["solve_blocks"][0]["equations"][0]["right"],
        "height+1"
    );
    assert_eq!(
        native.fields()["solve_blocks"][0]["equations"][1]["dependencies"][2],
        "area"
    );
    assert_eq!(
        native.fields()["solve_blocks"][0]["assignments"][0]["target"]["name"],
        "offset"
    );
    assert_eq!(
        native.fields()["solve_blocks"][0]["assignments"][0]["dependencies"][0],
        "base"
    );
    assert_eq!(
        native.fields()["solve_blocks"][0]["assignments"][0]["value"],
        11.0
    );
    assert_eq!(
        result
            .report()
            .coverage_count(crate::coverage::DECODED_ACTIVE_CURVE_EXPRESSION_ASSIGNMENT_COUNT),
        5
    );
    assert_eq!(
        result
            .report()
            .coverage_count(crate::coverage::EVALUATED_ACTIVE_CURVE_EXPRESSION_ASSIGNMENT_COUNT),
        5
    );
    assert_eq!(
        result
            .report()
            .coverage_count(crate::coverage::DECODED_ACTIVE_CURVE_EXPRESSION_SOLVE_BLOCK_COUNT),
        1
    );
    assert_eq!(
        result.report().coverage_count(
            crate::coverage::DECODED_ACTIVE_CURVE_EXPRESSION_SIMULTANEOUS_EQUATION_COUNT
        ),
        2
    );
    assert_eq!(
        result.report().coverage_count(
            crate::coverage::DECODED_ACTIVE_CURVE_EXPRESSION_SOLVE_ASSIGNMENT_COUNT
        ),
        1
    );
    assert_eq!(
        result
            .report()
            .coverage_count(crate::coverage::DECODED_ACTIVE_CURVE_EXPRESSION_SOLVE_VARIABLE_COUNT),
        2
    );
    assert_eq!(
        result.report().coverage_count(
            crate::coverage::UNRESOLVED_ACTIVE_CURVE_EXPRESSION_SOLVE_CONTROL_COUNT
        ),
        0
    );
    assert_eq!(
        result
            .report()
            .coverage_count(crate::coverage::EVALUATED_ACTIVE_CURVE_EXPRESSION_SOLVE_BLOCK_COUNT),
        0
    );
    assert_eq!(
        result.report().coverage_count(
            crate::coverage::EVALUATED_ACTIVE_CURVE_EXPRESSION_SOLVE_VARIABLE_COUNT
        ),
        0
    );
    assert!(native.fields()["solve_blocks"][0]["solutions"][0].is_null());
    assert!(native.fields()["solve_blocks"][0]["solutions"][1].is_null());
}

#[test]
fn decode_evaluates_affine_simultaneous_curve_expression_blocks() {
    let payload = b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x07\
        \xe0\x0aexpression\0\xf8\x08x=0\0y=0\0sum=10\0SOLVE\0\
        x+y=sum\0x-y=2\0FOR x,y\0product=x*y\0"
        .to_vec();
    let data = build_prt("c", &[("DEPDB_DATA", payload)]);

    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode affine solve block");

    let values = result
        .ir()
        .model
        .parameters
        .iter()
        .map(|parameter| (parameter.name.as_str(), parameter.value.as_ref()))
        .collect::<BTreeMap<_, _>>();
    assert_eq!(
        values["x"],
        Some(&cadmpeg_ir::features::ParameterValue::Real(6.0))
    );
    assert_eq!(
        values["y"],
        Some(&cadmpeg_ir::features::ParameterValue::Real(4.0))
    );
    assert_eq!(
        values["sum"],
        Some(&cadmpeg_ir::features::ParameterValue::Real(10.0))
    );
    assert_eq!(
        values["product"],
        Some(&cadmpeg_ir::features::ParameterValue::Real(24.0))
    );

    let native = &result.ir().native.namespace("creo").unwrap().arenas["curve_expressions"][0];
    assert_eq!(native.fields()["solve_blocks"][0]["solutions"][0], 6.0);
    assert_eq!(native.fields()["solve_blocks"][0]["solutions"][1], 4.0);
    assert_eq!(
        result
            .report()
            .coverage_count(crate::coverage::EVALUATED_ACTIVE_CURVE_EXPRESSION_SOLVE_BLOCK_COUNT),
        1
    );
    assert_eq!(
        result.report().coverage_count(
            crate::coverage::EVALUATED_ACTIVE_CURVE_EXPRESSION_SOLVE_VARIABLE_COUNT
        ),
        2
    );
    assert!(!result.report().losses.iter().any(|loss| {
        loss.message
            .contains("ordered equations and unknowns without solved values")
    }));
}

#[test]
fn decode_evaluates_dimensioned_affine_simultaneous_curve_expression_blocks() {
    let payload = b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x07\
        \xe0\x0aexpression\0\xf8\x06x=0[mm]\0y=0[mm]\0SOLVE\0\
        x+y=10[mm]\0x-y=2[mm]\0FOR x,y\0"
        .to_vec();
    let data = build_prt("c", &[("DEPDB_DATA", payload)]);

    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode dimensioned affine solve block");

    let values = result
        .ir()
        .model
        .parameters
        .iter()
        .map(|parameter| (parameter.name.as_str(), parameter.value.as_ref()))
        .collect::<BTreeMap<_, _>>();
    assert_eq!(
        values["x"],
        Some(&cadmpeg_ir::features::ParameterValue::Length(
            cadmpeg_ir::features::Length(6.0)
        ))
    );
    assert_eq!(
        values["y"],
        Some(&cadmpeg_ir::features::ParameterValue::Length(
            cadmpeg_ir::features::Length(4.0)
        ))
    );

    let native = &result.ir().native.namespace("creo").unwrap().arenas["curve_expressions"][0];
    assert_eq!(native.fields()["solve_blocks"][0]["solutions"][0], 6.0);
    assert_eq!(native.fields()["solve_blocks"][0]["solutions"][1], 4.0);
    assert_eq!(
        result.report().coverage_count(
            crate::coverage::EVALUATED_ACTIVE_CURVE_EXPRESSION_SOLVE_VARIABLE_COUNT
        ),
        2
    );
}

#[test]
fn decode_evaluates_dimensioned_relation_string_conversion() {
    let payload = b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x07\
        \xe0\x0aexpression\0\xf8\x03length_text=rtos(1[in],1)\0\
        angle_text=rtos(0.5[rad],3)\0force_text=rtos(2[N],0)\0"
        .to_vec();
    let data = build_prt("c", &[("DEPDB_DATA", payload)]);

    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode");
    let values = result
        .ir()
        .model
        .parameters
        .iter()
        .map(|parameter| (parameter.name.as_str(), parameter.value.as_ref()))
        .collect::<std::collections::BTreeMap<_, _>>();

    assert_eq!(
        values["length_text"],
        Some(&cadmpeg_ir::features::ParameterValue::String(
            "25.4".to_owned()
        ))
    );
    assert_eq!(
        values["angle_text"],
        Some(&cadmpeg_ir::features::ParameterValue::String(
            "28.648".to_owned()
        ))
    );
    assert_eq!(
        values["force_text"],
        Some(&cadmpeg_ir::features::ParameterValue::String(
            "2000".to_owned()
        ))
    );
    assert_eq!(
        result
            .report()
            .coverage_count(crate::coverage::EVALUATED_ACTIVE_CURVE_EXPRESSION_ASSIGNMENT_COUNT),
        3
    );
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone());
    assert!(validation.is_ok(), "{validation:#?}");
}

#[test]
fn decode_retains_scoped_model_name_call_as_model_context() {
    let payload = b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x07\
        \xe0\x0aexpression\0\xf8\x01component_name=rel_model_name:27()\0"
        .to_vec();
    let data = build_prt("c", &[("DEPDB_DATA", payload)]);

    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode");
    let [parameter] = result.ir().model.parameters.as_slice() else {
        panic!("one curve-expression parameter");
    };

    assert_eq!(parameter.name, "component_name");
    assert_eq!(parameter.expression, "rel_model_name:27()");
    assert_eq!(parameter.value, None);
    assert!(parameter.dependencies.is_empty());
    assert!(!parameter.properties.contains_key("external_dependencies"));
    let native = &result.ir().native.namespace("creo").unwrap().arenas["curve_expressions"][0];
    assert!(native.fields()["assignments"][0]["dependencies"]
        .as_array()
        .is_some_and(Vec::is_empty));
}

#[test]
fn decode_retains_scoped_assignment_targets_without_emitting_local_parameters() {
    let payload = b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x07\
        \xe0\x0aexpression\0\xf8\x04\
        d7:0=3\0\
        width:fid_25:cid_12=5\0\
        copy=d7:0*2\0\
        present=exists('width:fid_25:cid_12')\0"
        .to_vec();
    let data = build_prt("c", &[("DEPDB_DATA", payload)]);

    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode");
    let [copy, present] = result.ir().model.parameters.as_slice() else {
        panic!("two local curve-expression parameters");
    };

    assert_eq!(copy.name, "copy");
    assert_eq!(
        copy.value,
        Some(cadmpeg_ir::features::ParameterValue::Real(6.0))
    );
    assert_eq!(present.name, "present");
    assert_eq!(
        present.value,
        Some(cadmpeg_ir::features::ParameterValue::Real(1.0))
    );
    let native = &result.ir().native.namespace("creo").unwrap().arenas["curve_expressions"][0];
    assert_eq!(
        native.fields()["assignments"][0]["target"]["kind"],
        "scoped_symbol"
    );
    assert_eq!(native.fields()["assignments"][0]["target"]["name"], "d7:0");
    assert_eq!(
        native.fields()["assignments"][1]["target"]["name"],
        "width:fid_25:cid_12"
    );
    assert_eq!(
        result.report().coverage_count(
            crate::coverage::DECODED_ACTIVE_CURVE_EXPRESSION_SCOPED_SYMBOL_ASSIGNMENT_COUNT
        ),
        2
    );
    assert_eq!(
        result
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_CURVE_EXPRESSION_PARAMETER_COUNT),
        2
    );
}

#[test]
fn decode_retains_system_symbol_targets_without_emitting_user_parameters() {
    let payload = b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x07\
        \xe0\x0aexpression\0\xf8\x02d42=5\0result=d42+1\0"
        .to_vec();
    let data = build_prt("c", &[("DEPDB_DATA", payload)]);

    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode");
    let [parameter] = result.ir().model.parameters.as_slice() else {
        panic!("one local curve-expression parameter");
    };

    assert_eq!(parameter.name, "result");
    assert_eq!(
        parameter.value,
        Some(cadmpeg_ir::features::ParameterValue::Real(6.0))
    );
    assert_eq!(parameter.properties["external_dependencies"], "d42");
    let native = &result.ir().native.namespace("creo").unwrap().arenas["curve_expressions"][0];
    assert_eq!(
        native.fields()["assignments"][0]["target"]["kind"],
        "system_symbol"
    );
    assert_eq!(native.fields()["assignments"][0]["target"]["name"], "d42");
    assert_eq!(
        native.fields()["assignments"][0]["target"]["family"],
        "dimension"
    );
    assert_eq!(
        result.report().coverage_count(
            crate::coverage::DECODED_ACTIVE_CURVE_EXPRESSION_SYSTEM_SYMBOL_ASSIGNMENT_COUNT
        ),
        1
    );
    assert_eq!(
        result
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_CURVE_EXPRESSION_PARAMETER_COUNT),
        1
    );
}

#[test]
fn decode_retains_registered_function_write_targets_without_emitting_parameters() {
    let payload = b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x07\
        \xe0\x0aexpression\0\xf8\x02\
        store_value(component,row,column)=driver\0\
        result=1\0"
        .to_vec();
    let data = build_prt("c", &[("DEPDB_DATA", payload)]);

    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode");
    let [parameter] = result.ir().model.parameters.as_slice() else {
        panic!("one local curve-expression parameter");
    };

    assert_eq!(parameter.name, "result");
    let native = &result.ir().native.namespace("creo").unwrap().arenas["curve_expressions"][0];
    assert_eq!(
        native.fields()["assignments"][0]["target"]["kind"],
        "function_write"
    );
    assert_eq!(
        native.fields()["assignments"][0]["target"]["name"],
        "store_value"
    );
    let fields = native.fields();
    let arguments = fields["assignments"][0]["target"]["arguments"]
        .as_array()
        .expect("function arguments");
    assert_eq!(arguments.len(), 3);
    assert_eq!(arguments[0], "component");
    assert_eq!(arguments[1], "row");
    assert_eq!(arguments[2], "column");
    let dependencies = fields["assignments"][0]["dependencies"]
        .as_array()
        .expect("function dependencies");
    assert_eq!(dependencies.len(), 4);
    assert_eq!(dependencies[0], "component");
    assert_eq!(dependencies[1], "row");
    assert_eq!(dependencies[2], "column");
    assert_eq!(dependencies[3], "driver");
    assert_eq!(
        result.report().coverage_count(
            crate::coverage::DECODED_ACTIVE_CURVE_EXPRESSION_FUNCTION_WRITE_ASSIGNMENT_COUNT
        ),
        1
    );
    assert_eq!(
        result
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_CURVE_EXPRESSION_PARAMETER_COUNT),
        1
    );
}

#[test]
fn decode_retains_table_cell_assignments_without_emitting_scalar_parameters() {
    let payload = b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x07\
        \xe0\x0aexpression\0\xf8\x03\
        value(samples,row_index,column_index)=driver*2\0\
        VALUE(series,2)=5\0\
        after=value(samples,row_index,column_index)\0"
        .to_vec();
    let data = build_prt("c", &[("DEPDB_DATA", payload)]);

    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode");
    let [parameter] = result.ir().model.parameters.as_slice() else {
        panic!("one scalar curve-expression parameter");
    };
    assert_eq!(parameter.name, "after");
    assert_eq!(
        parameter.properties["external_dependencies"],
        "samples,row_index,column_index"
    );
    let native = &result.ir().native.namespace("creo").unwrap().arenas["curve_expressions"][0];
    let first = &native.fields()["assignments"][0];
    assert_eq!(first["target"]["kind"], "table_cell");
    assert_eq!(first["target"]["parameter"], "samples");
    assert_eq!(first["target"]["row"], "row_index");
    assert_eq!(first["target"]["column"], "column_index");
    assert_eq!(first["dependencies"][0], "samples");
    assert_eq!(first["dependencies"][1], "row_index");
    assert_eq!(first["dependencies"][2], "column_index");
    assert_eq!(first["dependencies"][3], "driver");
    let second = &native.fields()["assignments"][1];
    assert_eq!(second["target"]["kind"], "table_cell");
    assert_eq!(second["target"]["parameter"], "series");
    assert_eq!(second["target"]["row"], "2");
    assert!(second["target"]["column"].is_null());
    assert_eq!(
        result.ir().model.features[0].source_content,
        [cadmpeg_ir::features::FeatureSourceContent::Parameter(
            parameter.id.clone()
        )]
    );
    assert_eq!(
        result
            .report()
            .coverage_count(crate::coverage::DECODED_ACTIVE_CURVE_EXPRESSION_ASSIGNMENT_COUNT),
        3
    );
    assert_eq!(
        result
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_CURVE_EXPRESSION_PARAMETER_COUNT),
        1
    );
    assert_eq!(
        result.report().coverage_count(
            crate::coverage::DECODED_ACTIVE_CURVE_EXPRESSION_TABLE_CELL_ASSIGNMENT_COUNT
        ),
        2
    );
}

#[test]
fn decode_binds_curve_expression_dependencies_to_unique_dimensions() {
    let featdefs = b"feat_defs_917\0\xe0\x01feat_id\0\x28\xe0\x00gsec2d_ptr\0\
        dimtab_ptr\0\xf8\x01\xf7\x81\x02\xfb\xe2\
        \xe0\x01type\0\x0a\xe0\x01value\0\xe4\
        \xe0\x01direct\0\x01\xe0\x01aux_value\0\x0f\
        \xe0\x01ext_id\0\x2a\xe0\x00relat_ptr\0"
        .to_vec();
    let expressions = b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x07\
        \xe0\x0aexpression\0\xf8\x01result=d42+1[deg]\0"
        .to_vec();
    let data = build_prt(
        "c",
        &[
            ("FeatDefs", featdefs),
            ("MdlStatus", b"Extrude id 40\0".to_vec()),
            ("DEPDB_DATA", expressions),
        ],
    );

    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode");
    let dimension = result
        .ir()
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.name == "d42")
        .expect("dimension parameter");
    let relation = result
        .ir()
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.name == "result")
        .expect("relation parameter");

    assert_eq!(relation.dependencies, std::slice::from_ref(&dimension.id));
    assert!(!relation.properties.contains_key("external_dependencies"));
    assert_eq!(
        relation.value,
        Some(cadmpeg_ir::features::ParameterValue::Angle(
            cadmpeg_ir::features::Angle(1.0 + 1.0f64.to_radians())
        ))
    );
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone());
    assert!(validation.is_ok(), "{validation:#?}");
}

#[test]
fn decode_retains_prohibited_curve_expression_strings_without_values() {
    let payload = b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x07\
        \xe0\x0aexpression\0\xf8\x06material='steel'\0label=material+'-'+itos(2)\0\
        length=string_length(label)\0match=label=='steel-2'\0formatted=rtos(123.456,2)\0\
        kind=rel_model_type()\0"
        .to_vec();
    let data = build_prt("c", &[("DEPDB_DATA", payload)]);

    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode");
    let parameters = &result.ir().model.parameters;

    assert!(parameters.iter().all(|parameter| parameter.value.is_none()));
    let native = &result
        .ir()
        .native
        .namespace("creo")
        .expect("Creo native data")
        .arenas["curve_expressions"][0];
    assert_eq!(native.fields()["prohibited_constructs"][0], "itos");
    let coverage = result.report();
    assert_eq!(
        coverage.coverage_count(crate::coverage::PROHIBITED_ACTIVE_CURVE_EXPRESSION_RECORD_COUNT),
        1
    );
    assert_eq!(
        coverage.coverage_count(crate::coverage::PROHIBITED_ACTIVE_CURVE_EXPRESSION_KIND_COUNT),
        1
    );
    assert!(result.report().losses.iter().any(|loss| {
        loss.code.category() == cadmpeg_ir::LossCategory::DesignIntent
            && loss.severity == cadmpeg_ir::Severity::Warning
            && loss.message.contains(
                "1 active curve-equation record(s) containing prohibited datum-curve constructs \
                 were not evaluated",
            )
    }));
    assert!(result.report().losses.iter().any(|loss| {
        loss.code.category() == cadmpeg_ir::LossCategory::DesignIntent
            && loss.severity == cadmpeg_ir::Severity::Warning
            && loss.message.contains(
                "1 prohibited datum-curve construct(s) across active curve-equation records were \
                 not evaluated",
            )
    }));
    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains(
            "1 active curve-equation record(s) containing prohibited datum-curve constructs"
        )));
    assert_eq!(
        native.fields()["assignments"][4]["expression"],
        "rtos(123.456,2)"
    );
    assert!(native.fields()["assignments"][5]["value"].is_null());
    assert_eq!(parameters[4].expression, "rtos(123.456,2)");
    assert_eq!(parameters[5].expression, "rel_model_type()");
    assert_eq!(parameters[5].value, None);
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone());
    assert!(validation.is_ok(), "{validation:#?}");
}

#[test]
fn decode_evaluates_relation_model_name_from_unique_counted_header() {
    let payload = b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x07\
        \xe0\x0aexpression\0\xf8\x01name=rel_model_name()\0"
        .to_vec();
    let mut data = build_prt("c", &[("DEPDB_DATA", payload)]);
    let header_end = data
        .windows(b"#-END_OF_UGC_HEADER\n".len())
        .position(|window| window == b"#-END_OF_UGC_HEADER\n")
        .expect("header end");
    data.splice(
        header_end..header_end,
        b"#- CMNM 00bwidget.prt \n".iter().copied(),
    );

    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode");
    let [parameter] = result.ir().model.parameters.as_slice() else {
        panic!("one curve-expression parameter")
    };
    assert_eq!(
        parameter.value,
        Some(cadmpeg_ir::features::ParameterValue::String(
            "widget".to_owned()
        ))
    );
}

#[test]
fn decode_transfers_new_relation_parameter_unit_declarations() {
    let payload = b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x07\
        \xe0\x0aexpression\0\xf8\x05span[inch]=2\0copy=span+25.4[mm]\0\
        stress[N/mm^2]=2\0angle=atan2(span,25.4[mm])\0freezing[C]=0\0"
        .to_vec();
    let data = build_prt("c", &[("DEPDB_DATA", payload)]);

    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode");
    let parameters = &result.ir().model.parameters;
    assert_eq!(parameters.len(), 5);
    assert_eq!(parameters[0].name, "span");
    assert_eq!(parameters[0].properties["declared_unit"], "inch");
    assert_eq!(
        parameters[0].value,
        Some(cadmpeg_ir::features::ParameterValue::Length(
            cadmpeg_ir::features::Length(50.8)
        ))
    );
    let Some(cadmpeg_ir::features::ParameterValue::Length(copy)) = &parameters[1].value else {
        panic!("dimensioned copy");
    };
    assert!((copy.0 - 76.2).abs() < 1e-12);
    let native = &result.ir().native.namespace("creo").unwrap().arenas["curve_expressions"][0];
    assert_eq!(native.fields()["assignments"][0]["target"]["name"], "span");
    assert_eq!(
        native.fields()["assignments"][0]["target"]["declared_unit"],
        "inch"
    );
    assert_eq!(parameters[2].properties["declared_unit"], "N/mm^2");
    assert_eq!(
        parameters[2].properties["evaluated_canonical_value"],
        "2000"
    );
    assert_eq!(
        parameters[2].properties["evaluated_dimension"],
        "length:-1,mass:1,time:-2,angle:0,temperature:0"
    );
    assert_eq!(parameters[2].value, None);
    assert_eq!(native.fields()["assignments"][2]["value"]["value"], 2_000.0);
    assert_eq!(
        native.fields()["assignments"][2]["value"]["length_power"],
        -1
    );
    let Some(cadmpeg_ir::features::ParameterValue::Angle(angle)) = &parameters[3].value else {
        panic!("angle parameter");
    };
    assert!((angle.0 - 2.0f64.atan()).abs() < 1e-12);
    assert_eq!(parameters[4].properties["declared_unit"], "C");
    assert_eq!(
        parameters[4].properties["evaluated_dimension"],
        "length:0,mass:0,time:0,angle:0,temperature:1"
    );
    assert_eq!(
        parameters[4].properties["evaluated_canonical_value"],
        "273.15"
    );
    assert_eq!(parameters[4].value, None);
}

#[test]
fn decode_transfers_curve_expression_conditional_activation() {
    let payload = b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x07\
        \xe0\x0aexpression\0\xf8\x07a=YES\0IF a\0value=5\0ELSE\0value=9\0ENDIF\0z=value\0"
        .to_vec();
    let data = build_prt("c", &[("DEPDB_DATA", payload)]);

    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode");
    let parameters = &result.ir().model.parameters;
    assert_eq!(parameters.len(), 4);
    assert_eq!(parameters[0].properties["activation"], "active");
    assert_eq!(parameters[1].properties["activation"], "active");
    assert_eq!(parameters[2].properties["activation"], "inactive");
    assert_eq!(parameters[3].properties["activation"], "active");
    assert_eq!(parameters[3].value, None);
    assert_eq!(parameters[3].dependencies, [parameters[1].id.clone()]);
    assert!(!parameters[3]
        .properties
        .contains_key("ambiguous_dependencies"));
    let curve_expression_fields = result
        .ir()
        .native
        .namespace("creo")
        .expect("Creo native data")
        .arenas["curve_expressions"][0]
        .fields();
    let native_assignments = curve_expression_fields["assignments"]
        .as_array()
        .expect("assignments");
    assert_eq!(native_assignments[2]["activation"], "inactive");
    let prohibited = curve_expression_fields["prohibited_constructs"]
        .as_array()
        .expect("prohibited constructs");
    assert_eq!(prohibited.len(), 3);
    assert_eq!(prohibited[0], "else");
    assert_eq!(prohibited[1], "endif");
    assert_eq!(prohibited[2], "if");
    let coverage = result.report();
    assert_eq!(
        coverage.coverage_count(crate::coverage::ACTIVE_CURVE_EXPRESSION_ASSIGNMENT_COUNT),
        3
    );
    assert_eq!(
        coverage.coverage_count(crate::coverage::INACTIVE_CURVE_EXPRESSION_ASSIGNMENT_COUNT),
        1
    );
    assert_eq!(
        coverage.coverage_count(crate::coverage::CONDITIONAL_CURVE_EXPRESSION_ASSIGNMENT_COUNT),
        0
    );
}

#[test]
fn decode_resolves_positive_local_exists_before_declaration() {
    let payload = b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x07\
        \xe0\x0aexpression\0\xf8\x06IF exists('later')\0value=5\0ELSE\0\
        value=9\0ENDIF\0later=1\0"
        .to_vec();
    let data = build_prt("c", &[("DEPDB_DATA", payload)]);

    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode");
    let parameters = &result.ir().model.parameters;
    assert_eq!(parameters.len(), 3);
    assert_eq!(parameters[0].properties["activation"], "active");
    assert_eq!(parameters[0].value, None);
    assert_eq!(parameters[1].properties["activation"], "inactive");
    assert_eq!(parameters[1].value, None);
    assert_eq!(parameters[2].value, None);
}

#[test]
fn decode_retains_cyclic_curve_expression_dependencies_without_invalid_edges() {
    let payload = b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x07\
        \xe0\x0aexpression\0\xf8\x04r=a\0a=r\0theta=t*360\0z=1\0"
        .to_vec();
    let data = build_prt("c", &[("DEPDB_DATA", payload)]);

    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode");
    let [r, a, _, _] = result.ir().model.parameters.as_slice() else {
        panic!("four curve-expression parameters");
    };

    assert!(r.dependencies.is_empty());
    assert_eq!(r.properties["cyclic_dependencies"], "a");
    assert!(a.dependencies.is_empty());
    assert_eq!(a.properties["cyclic_dependencies"], "r");
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone());
    assert!(validation.is_ok(), "{validation:#?}");
}

#[test]
fn decode_transfers_reassigned_curve_expression_names_without_identity_collisions() {
    let payload = b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x07\
        \xe0\x0aexpression\0\xf8\x04r=1\0R=2\0theta=t*360\0z=r\0"
        .to_vec();
    let data = build_prt("c", &[("DEPDB_DATA", payload)]);

    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode");

    assert_eq!(
        result
            .ir()
            .model
            .parameters
            .iter()
            .map(|parameter| (parameter.name.as_str(), parameter.ordinal))
            .collect::<Vec<_>>(),
        [("r#1", 0), ("R#2", 1), ("theta", 2), ("z", 3)]
    );
    assert_eq!(
        result.ir().model.parameters[0].properties["source_name"],
        "r"
    );
    assert_eq!(
        result.ir().model.parameters[0].properties["source_assignment_ordinal"],
        "0"
    );
    assert_eq!(
        result.ir().model.parameters[1].properties["source_name"],
        "R"
    );
    assert_eq!(
        result.ir().model.parameters[3].properties["ambiguous_dependencies"],
        "r"
    );
    assert!(result.ir().model.parameters[3].dependencies.is_empty());
    assert!(!result.ir().model.parameters[3]
        .properties
        .contains_key("external_dependencies"));
    assert_eq!(
        result.ir().model.features[0].source_text.as_deref(),
        Some("r=1\nR=2\ntheta=t*360\nz=r")
    );
    assert_eq!(
        result
            .ir()
            .native
            .namespace("creo")
            .expect("Creo native data")
            .arenas["curve_expressions"][0]
            .fields()["assignments"]
            .as_array()
            .expect("assignments")
            .len(),
        4
    );
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone());
    assert!(validation.is_ok(), "{validation:#?}");
}

#[test]
fn decode_places_helix_from_complete_curve_expression_frame() {
    let payload = b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x07\
        \xe0\x02local_sys\0\xf9\x04\x03\xe4\x0f\x0f\x0f\x0f\x0f\x18\xe5\x0f\x0f\x0f\
        \xe0\x0aexpression\0\xf8\x03r=5\0theta=0-t*360\0z=-2+10*t\0"
        .to_vec();
    let data = build_prt("c", &[("DEPDB_DATA", payload)]);

    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode");
    assert_eq!(result.ir().model.procedural_curves.len(), 1);
    let cadmpeg_ir::geometry::ProceduralCurveDefinition::Helix {
        angle_range,
        center,
        major,
        minor,
        pitch,
        apex_factor,
        axis,
    } = &result.ir().model.procedural_curves[0].definition
    else {
        panic!("placed helix");
    };
    assert_eq!(*angle_range, [0.0, std::f64::consts::TAU]);
    assert_eq!(*center, cadmpeg_ir::math::Point3::new(0.0, 0.0, -2.0));
    assert_eq!(*major, cadmpeg_ir::math::Vector3::new(5.0, 0.0, 0.0));
    assert_eq!(*minor, cadmpeg_ir::math::Vector3::new(0.0, -5.0, 0.0));
    assert_eq!(*pitch, cadmpeg_ir::math::Vector3::new(0.0, 0.0, 10.0));
    assert_eq!(*apex_factor, 0.0);
    assert_eq!(*axis, cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0));
}

#[test]
fn decode_places_helix_from_rank_two_curve_expression_frame() {
    let payload = b"\xe0\x00entity(crv_fr_eqn)\0\xe3\xe0\x01id\0\x07\
        \xe0\x02local_sys\0\xf9\x04\x03\x18\xe4\x0f\xe4\x18\xe5\x0f\x18\xe6\
        \xe0\x0aexpression\0\xf8\x03r=5\0theta=t*360\0z=10*t\0"
        .to_vec();
    let data = build_prt("c", &[("DEPDB_DATA", payload)]);

    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode");
    let cadmpeg_ir::geometry::ProceduralCurveDefinition::Helix {
        center,
        major,
        minor,
        pitch,
        axis,
        ..
    } = &result.ir().model.procedural_curves[0].definition
    else {
        panic!("placed helix");
    };
    assert_eq!(*center, cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0));
    assert_eq!(*major, cadmpeg_ir::math::Vector3::new(0.0, 5.0, 0.0));
    assert_eq!(*minor, cadmpeg_ir::math::Vector3::new(5.0, 0.0, 0.0));
    assert_eq!(*pitch, cadmpeg_ir::math::Vector3::new(0.0, 0.0, -10.0));
    assert_eq!(*axis, cadmpeg_ir::math::Vector3::new(0.0, 0.0, -1.0));
}
