// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use crate::examples::unit_cube;
use crate::geometry::SurfaceGeometry;
use crate::ids::UnknownId;
use crate::unknown::NativeUnknownRecord;

/// Replace the surface of the cube's first face with an unknown surface,
/// optionally linking a preserved record, and return the face id and its
/// surface id. Leaves every loop/coedge/edge of the face intact.
fn make_first_face_surface_unknown(ir: &mut crate::CadIr, record: Option<UnknownId>) -> String {
    let face = &ir.model.faces[0];
    let surface_id = face.surface.0.clone();
    for s in &mut ir.model.surfaces {
        if s.id.0 == surface_id {
            s.geometry = SurfaceGeometry::Unknown { record };
            break;
        }
    }
    surface_id
}

#[test]
fn unknown_surface_json_round_trips() {
    let mut ir = unit_cube();
    let rec = UnknownId("synthetic:cube:unknown#0".into());
    ir.set_native_unknowns(
        "synthetic",
        &[NativeUnknownRecord {
            id: rec.clone(),
            links: Vec::new(),
        }],
    )
    .unwrap();
    make_first_face_surface_unknown(&mut ir, Some(rec));

    let json = ir.to_canonical_json().unwrap();
    let parsed = crate::CadIr::from_json(&json).unwrap();
    assert_eq!(parsed, ir, "round-trip must preserve the unknown surface");
}

#[test]
fn ordered_pcurve_uses_round_trip_with_isoparametric_state() {
    let uses = vec![
        crate::topology::PcurveUse {
            pcurve: crate::ids::PcurveId("test:pcurve#first".into()),
            isoparametric: Some(true),
            parameter_range: None,
        },
        crate::topology::PcurveUse {
            pcurve: crate::ids::PcurveId("test:pcurve#second".into()),
            isoparametric: Some(false),
            parameter_range: Some([0.0, 1.0]),
        },
    ];
    let json = serde_json::to_string(&uses).unwrap();
    assert_eq!(
        serde_json::from_str::<Vec<crate::topology::PcurveUse>>(&json).unwrap(),
        uses
    );
}

#[test]
fn asm_inline_pcurve_metadata_keeps_the_flat_wire_shape() {
    let pcurve = crate::geometry::Pcurve {
        id: crate::ids::PcurveId("test:pcurve#inline".into()),
        geometry: crate::geometry::PcurveGeometry::Line {
            origin: crate::math::Point2::new(1.0, 2.0),
            direction: crate::math::Point2::new(3.0, 4.0),
        },
        metadata: crate::geometry::PcurveMetadata::AsmInline(crate::geometry::PcurveInlineForm {
            wrapper_reversed: false,
            native_tail_flags: [true, false, true, false],
            parameter_range: [-1.0, 2.0],
            fit_tolerance: 0.001,
        }),
    };
    let value = serde_json::to_value(&pcurve).unwrap();
    assert_eq!(
        value,
        serde_json::json!({
            "id": "test:pcurve#inline",
            "geometry": {
                "kind": "line",
                "origin": [1.0, 2.0],
                "direction": [3.0, 4.0]
            },
            "wrapper_reversed": false,
            "native_tail_flags": [true, false, true, false],
            "parameter_range": [-1.0, 2.0],
            "fit_tolerance": 0.001
        })
    );
    assert_eq!(
        serde_json::from_value::<crate::geometry::Pcurve>(value).unwrap(),
        pcurve
    );
}

#[test]
fn incomplete_asm_inline_pcurve_metadata_is_rejected() {
    let result = serde_json::from_value::<crate::geometry::Pcurve>(serde_json::json!({
        "id": "test:pcurve#incomplete",
        "geometry": {
            "kind": "line",
            "origin": [1.0, 2.0],
            "direction": [3.0, 4.0]
        },
        "wrapper_reversed": false,
        "native_tail_flags": [true, false, true, false],
        "parameter_range": [-1.0, 2.0]
    }));
    assert!(result.is_err());
}

#[test]
fn g2_full_support_keeps_the_flat_wire_shape() {
    let shape = crate::geometry::G2BlendFirstShape::Full {
        support: Some(crate::geometry::G2BlendFullSupport {
            surface: crate::ids::SurfaceId("test:surface#support".into()),
            tolerance: 0.02,
        }),
    };
    let value = serde_json::to_value(&shape).unwrap();
    assert_eq!(
        value,
        serde_json::json!({
            "kind": "full",
            "surface": "test:surface#support",
            "tolerance": 0.02
        })
    );
    assert_eq!(
        serde_json::from_value::<crate::geometry::G2BlendFirstShape>(value).unwrap(),
        shape
    );
}

#[test]
fn g2_full_support_rejects_split_wire_fields() {
    let error = serde_json::from_value::<crate::geometry::G2BlendFirstShape>(serde_json::json!({
        "kind": "full",
        "surface": "test:surface#support"
    }))
    .unwrap_err();
    assert!(error
        .to_string()
        .contains("G2 full surface and tolerance must occur together"));
}

#[derive(Debug, PartialEq, serde::Serialize, serde::Deserialize)]
struct RevisionCompoundLoftDirectionWireTest {
    #[serde(flatten, with = "super::revision_compound_loft_direction_wire")]
    direction: crate::geometry::CompoundLoftDirection,
}

#[test]
fn revision_compound_loft_direction_keeps_the_flat_wire_shape() {
    let value = RevisionCompoundLoftDirectionWireTest {
        direction: crate::geometry::CompoundLoftDirection::Curve {
            curve: crate::ids::CurveId("test:curve#direction".into()),
            selector: std::num::NonZeroI64::new(4).unwrap(),
        },
    };
    let wire = serde_json::to_value(&value).unwrap();
    assert_eq!(
        wire,
        serde_json::json!({
            "selector": 4,
            "direction_curve": "test:curve#direction"
        })
    );
    assert_eq!(
        serde_json::from_value::<RevisionCompoundLoftDirectionWireTest>(wire).unwrap(),
        value
    );
}

#[test]
fn revision_compound_loft_direction_rejects_a_mismatched_selector() {
    let error =
        serde_json::from_value::<RevisionCompoundLoftDirectionWireTest>(serde_json::json!({
            "selector": 0,
            "direction_curve": "test:curve#direction"
        }))
        .unwrap_err();
    assert!(error
        .to_string()
        .contains("compound-loft direction conflicts with its selector"));
}

#[derive(Debug, PartialEq, serde::Serialize, serde::Deserialize)]
struct VariableBlendShapeWireTest {
    #[serde(flatten, with = "super::variable_blend_radii_wire")]
    radii: crate::geometry::VariableBlendRadii,
    #[serde(with = "super::variable_blend_u_range_wire")]
    u_range: [f64; 2],
    #[serde(rename = "v_range", with = "super::variable_blend_v_range_wire")]
    v_lower: Option<f64>,
}

fn variable_blend_value(name: &str) -> crate::geometry::VariableBlendValue {
    crate::geometry::VariableBlendValue {
        name: name.into(),
        modern_flag: false,
        discriminator: 0,
        calibrated: 0,
        payload: crate::geometry::VariableBlendValuePayload::TwoEnds {
            parameters: [0.0, 1.0],
            radii: [1.0, 2.0],
        },
    }
}

#[test]
fn variable_blend_shape_keeps_the_flat_wire_fields() {
    let value = VariableBlendShapeWireTest {
        radii: crate::geometry::VariableBlendRadii::Two {
            first: variable_blend_value("first"),
            second: variable_blend_value("second"),
        },
        u_range: [-1.0, 2.0],
        v_lower: Some(-0.5),
    };
    let wire = serde_json::to_value(&value).unwrap();
    assert_eq!(wire["radius_kind"], "two_radii");
    assert_eq!(wire["first_value"]["name"], "first");
    assert_eq!(wire["second_value"]["name"], "second");
    assert_eq!(wire["u_range"], serde_json::json!([-1.0, 2.0]));
    assert_eq!(wire["v_range"], serde_json::json!([-0.5, null]));
    assert_eq!(
        serde_json::from_value::<VariableBlendShapeWireTest>(wire).unwrap(),
        value
    );
}

#[test]
fn variable_blend_shape_rejects_inconsistent_wire_fields() {
    let mut wire = serde_json::to_value(VariableBlendShapeWireTest {
        radii: crate::geometry::VariableBlendRadii::Single {
            value: variable_blend_value("first"),
        },
        u_range: [-1.0, 2.0],
        v_lower: None,
    })
    .unwrap();
    wire["second_value"] = serde_json::to_value(variable_blend_value("second")).unwrap();
    assert!(serde_json::from_value::<VariableBlendShapeWireTest>(wire).is_err());

    let mut wire = serde_json::to_value(VariableBlendShapeWireTest {
        radii: crate::geometry::VariableBlendRadii::Single {
            value: variable_blend_value("first"),
        },
        u_range: [-1.0, 2.0],
        v_lower: None,
    })
    .unwrap();
    wire["u_range"] = serde_json::json!([-1.0, null]);
    assert!(serde_json::from_value::<VariableBlendShapeWireTest>(wire).is_err());
}

fn empty_loft_subdata() -> crate::geometry::LoftSubdata {
    crate::geometry::LoftSubdata::type_211([0.0, 1.0])
}

#[test]
fn loft_subdata_derives_counts_and_rejects_inconsistent_wire_counts() {
    let table = crate::geometry::LoftSubdata::table(
        7,
        vec![
            crate::geometry::LoftSubdataRow {
                parameters: [0.0, 1.0],
                columns: vec![[2.0, 3.0]],
                extra: None,
            },
            crate::geometry::LoftSubdataRow {
                parameters: [4.0, 5.0],
                columns: vec![[6.0, 7.0]],
                extra: Some([8.0, 9.0]),
            },
        ],
    )
    .unwrap();
    let wire = serde_json::to_value(&table).unwrap();
    assert_eq!(wire["type_code"], 7);
    assert_eq!(wire["row_count"], 2);
    assert_eq!(wire["column_count"], 1);
    assert_eq!(
        serde_json::from_value::<crate::geometry::LoftSubdata>(wire.clone()).unwrap(),
        table
    );

    let mut wrong_rows = wire.clone();
    wrong_rows["row_count"] = serde_json::json!(3);
    let error = serde_json::from_value::<crate::geometry::LoftSubdata>(wrong_rows).unwrap_err();
    assert!(error.to_string().contains("row_count does not match rows"));

    let mut wrong_columns = wire;
    wrong_columns["rows"][1]["columns"] = serde_json::json!([]);
    let error = serde_json::from_value::<crate::geometry::LoftSubdata>(wrong_columns).unwrap_err();
    assert!(error
        .to_string()
        .contains("column_count does not match every row"));

    assert!(crate::geometry::LoftSubdata::table(
        9,
        vec![
            crate::geometry::LoftSubdataRow {
                parameters: [0.0, 1.0],
                columns: Vec::new(),
                extra: None,
            },
            crate::geometry::LoftSubdataRow {
                parameters: [2.0, 3.0],
                columns: vec![[4.0, 5.0]],
                extra: None,
            },
        ],
    )
    .is_none());
}

#[test]
fn loft_subdata_type_211_has_one_row_and_no_columns() {
    let table = crate::geometry::LoftSubdata::type_211([2.0, 3.0]);
    let wire = serde_json::to_value(&table).unwrap();
    assert_eq!(
        wire,
        serde_json::json!({
            "type_code": 211,
            "row_count": 1,
            "column_count": 0,
            "rows": [{ "parameters": [2.0, 3.0], "columns": [] }]
        })
    );
    assert_eq!(
        serde_json::from_value::<crate::geometry::LoftSubdata>(wire.clone()).unwrap(),
        table
    );

    let mut invalid = wire;
    invalid["column_count"] = serde_json::json!(1);
    invalid["rows"][0]["columns"] = serde_json::json!([[4.0, 5.0]]);
    let error = serde_json::from_value::<crate::geometry::LoftSubdata>(invalid).unwrap_err();
    assert!(error
        .to_string()
        .contains("type 211 forbids columns and a trailing pair"));
}

#[test]
fn loft_member_form_keeps_the_nested_wire_shape() {
    let member = crate::geometry::LoftProfileMember {
        curve: crate::geometry::LoftPathCurve {
            id: crate::ids::CurveId("test:curve#loft".into()),
            endpoints: Some([Some(0.0), Some(1.0)]),
        },
        form: crate::geometry::LoftMemberForm::Support {
            type_code: 3,
            surface: Some(crate::ids::SurfaceId("test:surface#loft".into())),
            support_bounds: [Some(-1.0), Some(1.0), None, None],
            pcurve: None,
            first_flag: true,
            asm_extension: Some(-1),
            subdata: empty_loft_subdata(),
            direction: None,
        },
    };
    let wire = serde_json::to_value(&member).unwrap();
    assert_eq!(wire["type_code"], 3);
    assert_eq!(wire["data"]["surface"], "test:surface#loft");
    assert_eq!(wire["data"]["first_flag"], true);
    assert!(wire["data"].get("secondary_pcurve").is_none());
    assert_eq!(
        serde_json::from_value::<crate::geometry::LoftProfileMember>(wire).unwrap(),
        member
    );
}

#[test]
fn loft_member_form_rejects_a_payload_that_disagrees_with_its_type() {
    let pair = crate::geometry::LoftProfileMember {
        curve: crate::geometry::LoftPathCurve {
            id: crate::ids::CurveId("test:curve#loft".into()),
            endpoints: Some([None, None]),
        },
        form: crate::geometry::LoftMemberForm::PcurvePair {
            pcurve: None,
            secondary_pcurve: None,
            asm_extension: None,
            subdata: empty_loft_subdata(),
            direction: None,
        },
    };
    let mut pair_wire = serde_json::to_value(pair).unwrap();
    pair_wire["data"]["surface"] = serde_json::json!("test:surface#conflict");
    let error =
        serde_json::from_value::<crate::geometry::LoftProfileMember>(pair_wire).unwrap_err();
    assert!(error
        .to_string()
        .contains("pcurve-pair form cannot carry a support surface"));

    let mut support_wire = serde_json::to_value(crate::geometry::LoftProfileMember {
        curve: crate::geometry::LoftPathCurve {
            id: crate::ids::CurveId("test:curve#loft".into()),
            endpoints: None,
        },
        form: crate::geometry::LoftMemberForm::Support {
            type_code: 4,
            surface: None,
            support_bounds: [None; 4],
            pcurve: None,
            first_flag: false,
            asm_extension: Some(-1),
            subdata: empty_loft_subdata(),
            direction: None,
        },
    })
    .unwrap();
    support_wire["data"]["first_flag"] = serde_json::Value::Null;
    let error =
        serde_json::from_value::<crate::geometry::LoftProfileMember>(support_wire).unwrap_err();
    assert!(error
        .to_string()
        .contains("nonzero loft type_code requires data.first_flag"));
}

#[test]
fn loft_path_rejects_endpoints_without_a_curve() {
    let path = crate::geometry::LoftPath {
        curve: Some(crate::geometry::LoftPathCurve {
            id: crate::ids::CurveId("test:curve#path".into()),
            endpoints: Some([Some(0.0), Some(1.0)]),
        }),
        auxiliaries: Vec::new(),
        flag: 4,
    };
    let wire = serde_json::to_value(&path).unwrap();
    assert_eq!(wire["curve"], "test:curve#path");
    assert_eq!(wire["endpoints"], serde_json::json!([0.0, 1.0]));
    assert_eq!(
        serde_json::from_value::<crate::geometry::LoftPath>(wire.clone()).unwrap(),
        path
    );

    let mut invalid = wire;
    invalid.as_object_mut().unwrap().remove("curve");
    let error = serde_json::from_value::<crate::geometry::LoftPath>(invalid).unwrap_err();
    assert!(error.to_string().contains("endpoints require a curve"));
}

#[test]
fn law_edge_keeps_its_flat_curve_and_endpoints_wire_shape() {
    let expression = crate::geometry::LawExpression::Edge {
        curve: crate::geometry::LoftPathCurve {
            id: crate::ids::CurveId("test:curve#law".into()),
            endpoints: Some([None, Some(2.0)]),
        },
        parameters: [-1.0, 3.0],
    };
    let wire = serde_json::to_value(&expression).unwrap();
    assert_eq!(wire["kind"], "edge");
    assert_eq!(wire["curve"], "test:curve#law");
    assert_eq!(wire["endpoints"], serde_json::json!([null, 2.0]));
    assert_eq!(
        serde_json::from_value::<crate::geometry::LawExpression>(wire).unwrap(),
        expression
    );
}

#[test]
fn law_formula_keeps_its_flat_wire_shape_and_rejects_sentinel_payloads() {
    let null = crate::geometry::LawFormula::Null;
    let null_wire = serde_json::to_value(&null).unwrap();
    assert_eq!(
        null_wire,
        serde_json::json!({ "name": "null_law", "variables": [] })
    );
    assert_eq!(
        serde_json::from_value::<crate::geometry::LawFormula>(null_wire).unwrap(),
        null
    );

    let named = crate::geometry::LawFormula::Named {
        name: crate::geometry::LawFormulaName::new("distance-law").unwrap(),
        variables: vec![crate::geometry::LawExpression::Double { value: 2.0 }],
    };
    let named_wire = serde_json::to_value(&named).unwrap();
    assert_eq!(
        named_wire,
        serde_json::json!({
            "name": "distance-law",
            "variables": [{ "kind": "double", "value": 2.0 }]
        })
    );
    assert_eq!(
        serde_json::from_value::<crate::geometry::LawFormula>(named_wire).unwrap(),
        named
    );

    assert!(crate::geometry::LawFormulaName::new("null_law").is_none());
    let error = serde_json::from_value::<crate::geometry::LawFormula>(serde_json::json!({
        "name": "null_law",
        "variables": [{ "kind": "double", "value": 2.0 }]
    }))
    .unwrap_err();
    assert!(error
        .to_string()
        .contains("null_law formula cannot carry variables"));
}

fn ranged_spring_definition() -> crate::geometry::ProceduralCurveDefinition {
    crate::geometry::ProceduralCurveDefinition::Spring {
        layout: crate::geometry::SpringLayout::ContextFirst {
            supports: [
                crate::geometry::SpringSupport::Ranges([[0.0, 1.0], [2.0, 3.0]]),
                crate::geometry::SpringSupport::Ranges([[4.0, 5.0], [6.0, 7.0]]),
            ],
            first_pcurve: crate::geometry::SpringPcurve::Range([8.0, 9.0]),
            second_pcurve: None,
            parameter_range: [-1.0, 2.0],
            discontinuities: [Vec::new(), Vec::new(), Vec::new()],
            discontinuity_flag: true,
        },
        direction: 4,
    }
}

#[test]
fn spring_layout_keeps_the_flat_conditional_range_wire_shape() {
    let definition = ranged_spring_definition();
    let wire = serde_json::to_value(&definition).unwrap();
    assert_eq!(wire["kind"], "spring");
    assert_eq!(wire["context"]["sides"][0], serde_json::json!({}));
    assert_eq!(
        wire["surface_parameter_ranges"][0],
        serde_json::json!([[0.0, 1.0], [2.0, 3.0]])
    );
    assert_eq!(
        wire["first_pcurve_parameter_range"],
        serde_json::json!([8.0, 9.0])
    );
    assert!(wire.get("cache_first").is_none());
    assert_eq!(
        serde_json::from_value::<crate::geometry::ProceduralCurveDefinition>(wire).unwrap(),
        definition
    );
}

#[test]
fn spring_layout_rejects_split_support_state() {
    let mut wire = serde_json::to_value(ranged_spring_definition()).unwrap();
    wire["context"]["sides"][0]["surface"] = serde_json::json!("test:surface#conflict");
    let error =
        serde_json::from_value::<crate::geometry::ProceduralCurveDefinition>(wire).unwrap_err();
    assert!(error
        .to_string()
        .contains("spring support side 0 requires exactly one"));
}

#[test]
fn projection_role_keeps_the_native_string_wire_shape() {
    let tail = crate::geometry::ProjectionTail::Ranged {
        flag: true,
        parameter_range: [-1.0, 2.0],
        role: crate::geometry::ProjectionRole::Surf2,
    };
    let wire = serde_json::to_value(&tail).unwrap();
    assert_eq!(wire["role"], "surf2");
    assert_eq!(
        serde_json::from_value::<crate::geometry::ProjectionTail>(wire).unwrap(),
        tail
    );
    let error = serde_json::from_value::<crate::geometry::ProjectionTail>(serde_json::json!({
        "kind": "ranged",
        "flag": true,
        "parameter_range": [-1.0, 2.0],
        "role": "other"
    }))
    .unwrap_err();
    assert!(error.to_string().contains("projection role field"));
}

#[test]
fn vector_offset_roles_keep_the_fixed_flat_wire_shape() {
    let definition = crate::geometry::ProceduralCurveDefinition::VectorOffset {
        source: crate::ids::CurveId("test:curve#source".into()),
        parameter_range: [-1.0, 2.0],
        offset: crate::math::Vector3::new(3.0, 4.0, 5.0),
        roles: crate::geometry::VectorOffsetRoles {
            source_code: 7,
            offset_code: 9,
        },
    };
    let wire = serde_json::to_value(&definition).unwrap();
    assert_eq!(wire["labels"], serde_json::json!(["source", "offset"]));
    assert_eq!(wire["codes"], serde_json::json!([7, 9]));
    assert_eq!(
        serde_json::from_value::<crate::geometry::ProceduralCurveDefinition>(wire.clone()).unwrap(),
        definition
    );

    let mut invalid = wire;
    invalid["labels"] = serde_json::json!(["offset", "source"]);
    let error =
        serde_json::from_value::<crate::geometry::ProceduralCurveDefinition>(invalid).unwrap_err();
    assert!(error
        .to_string()
        .contains("vector-offset labels must be [\"source\", \"offset\"]"));
}
