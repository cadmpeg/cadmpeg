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
    crate::geometry::LoftSubdata {
        type_code: 211,
        row_count: 1,
        column_count: 0,
        rows: vec![crate::geometry::LoftSubdataRow {
            parameters: [0.0, 1.0],
            columns: Vec::new(),
            extra: None,
        }],
    }
}

#[test]
fn loft_member_form_keeps_the_nested_wire_shape() {
    let member = crate::geometry::LoftProfileMember {
        curve: crate::ids::CurveId("test:curve#loft".into()),
        endpoints: Some([Some(0.0), Some(1.0)]),
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
        curve: crate::ids::CurveId("test:curve#loft".into()),
        endpoints: Some([None, None]),
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
        curve: crate::ids::CurveId("test:curve#loft".into()),
        endpoints: None,
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
