// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use crate::examples::unit_cube;
use crate::geometry::{
    CircleCurve, CirclePcurve, ConeSurface, CurveGeometry, CylinderSurface, EllipseCurve,
    EllipsePcurve, HarmonicPcurve, LinePcurve, OffsetPcurve, PcurveGeometry, PlaneSurface,
    SphereSurface, SphericalGreatCirclePcurve, SurfaceGeometry, TorusSurface, TrimmedPcurve,
};
use crate::ids::UnknownId;
use crate::math::{Point2, Point3, Vector3};
use crate::unknown::NativeUnknownRecord;

/// Replace the surface of the cube's first face with an unknown surface,
/// optionally linking a preserved record, and return the face id and its
/// surface id. Leaves every loop/coedge/edge of the face intact.
fn make_first_face_surface_unknown(ir: &mut crate::CadIr, record: Option<UnknownId>) -> String {
    let face = &ir.model.faces[0];
    let surface_id = face.surface.as_str().to_owned();
    for s in &mut ir.model.surfaces {
        if s.id.as_str() == surface_id {
            s.geometry = SurfaceGeometry::Unknown { record };
            break;
        }
    }
    surface_id
}

#[test]
fn unknown_surface_json_round_trips() {
    let mut ir = unit_cube();
    let rec = UnknownId::mint("synthetic:cube:unknown#0").expect("valid identity");
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
            pcurve: crate::ids::PcurveId::mint("test:model:pcurve#first").expect("valid identity"),
            isoparametric: Some(true),
            parameter_range: None,
        },
        crate::topology::PcurveUse {
            pcurve: crate::ids::PcurveId::mint("test:model:pcurve#second").expect("valid identity"),
            isoparametric: Some(false),
            parameter_range: Some(
                crate::geometry::DirectedParameterRange::new([0.0, 1.0]).unwrap(),
            ),
        },
    ];
    let json = serde_json::to_string(&uses).unwrap();
    assert_eq!(
        serde_json::from_str::<Vec<crate::topology::PcurveUse>>(&json).unwrap(),
        uses
    );
}

#[test]
fn pcurve_lift_rejects_non_finite_model_poles() {
    let curve = crate::geometry::PcurveNurbs::new(
        1,
        vec![0.0, 0.0, 1.0, 1.0],
        vec![
            crate::math::Point2::new(0.0, 0.0),
            crate::math::Point2::new(1.0, 1.0),
        ],
        None,
        false,
    )
    .unwrap();
    assert!(curve
        .lift(|point| crate::math::Point3::new(point.u, point.v, f64::NAN))
        .is_err());
}

#[test]
fn support_side_rejects_orphan_legacy_parameter_range() {
    let wire = serde_json::json!({
        "surface": null,
        "pcurve": null,
        "pcurve_parameter_range": [0.0, 1.0],
    });
    assert!(serde_json::from_value::<crate::geometry::IntcurveSupportSide>(wire).is_err());
}

#[test]
fn asm_inline_pcurve_metadata_lives_under_its_own_nested_key() {
    let pcurve = crate::geometry::Pcurve {
        id: crate::ids::PcurveId::mint("test:model:pcurve#inline").expect("valid identity"),
        geometry: crate::geometry::PcurveGeometry::Line(
            crate::geometry::LinePcurve::try_new(
                crate::math::Point2::new(1.0, 2.0),
                crate::math::Point2::new(3.0, 4.0),
            )
            .unwrap(),
        ),
        metadata: crate::geometry::PcurveMetadata::AsmInline {
            form: crate::geometry::PcurveInlineForm::try_new(
                false,
                [true, false, true, false],
                [-1.0, 2.0],
                0.001,
            )
            .unwrap(),
        },
    };
    let value = serde_json::to_value(&pcurve).unwrap();
    assert_eq!(
        value,
        serde_json::json!({
            "id": "test:model:pcurve#inline",
            "geometry": {
                "kind": "line",
                "origin": {"u": 1.0, "v": 2.0},
                "direction": {"u": 3.0, "v": 4.0}
            },
            "metadata": {
                "source": "asm_inline",
                "form": {
                    "wrapper_reversed": false,
                    "native_tail_flags": [true, false, true, false],
                    "parameter_range": [-1.0, 2.0],
                    "fit_tolerance": 0.001
                }
            }
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
        "id": "test:model:pcurve#incomplete",
        "geometry": {
            "kind": "line",
            "origin": {"u": 1.0, "v": 2.0},
            "direction": {"u": 3.0, "v": 4.0}
        },
        "wrapper_reversed": false,
        "native_tail_flags": [true, false, true, false],
        "parameter_range": [-1.0, 2.0]
    }));
    assert!(result.is_err());
}

#[test]
fn the_g2_full_support_is_one_nested_key_or_absent() {
    let shape = crate::geometry::G2BlendFirstShape::Full {
        support: Some(crate::geometry::G2BlendFullSupport {
            surface: crate::ids::SurfaceId::mint("test:model:surface#support")
                .expect("valid identity"),
            tolerance: crate::geometry::FitTolerance::try_new(0.02).unwrap(),
        }),
    };
    let value = serde_json::to_value(&shape).unwrap();
    assert_eq!(
        value,
        serde_json::json!({
            "kind": "full",
            "support": {
                "surface": "test:model:surface#support",
                "tolerance": 0.02
            }
        })
    );
    assert_eq!(
        serde_json::from_value::<crate::geometry::G2BlendFirstShape>(value.clone()).unwrap(),
        shape
    );

    let absent = serde_json::from_value::<crate::geometry::G2BlendFirstShape>(
        serde_json::json!({"kind": "full"}),
    )
    .unwrap();
    assert_eq!(
        absent,
        crate::geometry::G2BlendFirstShape::Full { support: None }
    );

    let mut half = value.clone();
    half["support"]
        .as_object_mut()
        .expect("a support object")
        .remove("tolerance");
    let error = serde_json::from_value::<crate::geometry::G2BlendFirstShape>(half)
        .unwrap_err()
        .to_string();
    assert!(error.contains("tolerance"), "{error}");

    let mut bogus = value;
    bogus["support"]["zz_bogus"] = serde_json::json!(1);
    let error = serde_json::from_value::<crate::geometry::G2BlendFirstShape>(bogus)
        .unwrap_err()
        .to_string();
    assert!(error.contains("zz_bogus"), "{error}");
}

#[derive(Debug, PartialEq, serde::Serialize, serde::Deserialize)]
struct RevisionCompoundLoftDirectionWireTest {
    direction: crate::geometry::CompoundLoftDirection,
}

#[derive(Debug, PartialEq, serde::Serialize, serde::Deserialize)]
struct VariableBlendShapeWireTest {
    radii: crate::geometry::VariableBlendRadii,
    u_range: [f64; 2],
    #[serde(default, skip_serializing_if = "Option::is_none")]
    v_lower: Option<f64>,
}

fn variable_blend_value(discriminator: i64) -> crate::geometry::VariableBlendValue {
    crate::geometry::VariableBlendValue {
        modern_flag: false,
        calibrated: 0,
        payload: crate::geometry::VariableBlendValuePayload::TwoEnds {
            discriminator,
            parameters: [0.0, 1.0],
            radii: [1.0, 2.0],
        },
    }
}

#[test]
fn the_variable_blend_radii_are_one_nested_tagged_object() {
    let value = VariableBlendShapeWireTest {
        radii: crate::geometry::VariableBlendRadii::Two {
            first: variable_blend_value(0),
            second: variable_blend_value(1),
        },
        u_range: [-1.0, 2.0],
        v_lower: Some(-0.5),
    };
    let wire = serde_json::to_value(&value).unwrap();
    assert_eq!(wire["radii"]["kind"], "two");
    assert_eq!(wire["radii"]["first"]["name"], "two_ends");
    assert_eq!(wire["radii"]["first"]["discriminator"], 0);
    assert_eq!(wire["radii"]["second"]["name"], "two_ends");
    assert_eq!(wire["radii"]["second"]["discriminator"], 1);
    assert_eq!(wire["u_range"], serde_json::json!([-1.0, 2.0]));
    assert_eq!(wire["v_lower"], -0.5);
    assert_eq!(
        serde_json::from_value::<VariableBlendShapeWireTest>(wire).unwrap(),
        value
    );
}

#[test]
fn a_single_variable_blend_radius_has_no_second_law_key() {
    let single = VariableBlendShapeWireTest {
        radii: crate::geometry::VariableBlendRadii::Single {
            value: variable_blend_value(0),
        },
        u_range: [-1.0, 2.0],
        v_lower: None,
    };
    let base = serde_json::to_value(&single).unwrap();
    assert_eq!(base["radii"]["kind"], "single");
    assert!(base["radii"].get("second").is_none());

    let mut second_beside_single = base.clone();
    second_beside_single["radii"]["second"] =
        serde_json::to_value(variable_blend_value(1)).unwrap();
    let error = serde_json::from_value::<VariableBlendShapeWireTest>(second_beside_single)
        .unwrap_err()
        .to_string();
    assert!(error.contains("second"), "{error}");

    let mut bogus = base.clone();
    bogus["radii"]["zz_bogus"] = serde_json::json!(1);
    let error = serde_json::from_value::<VariableBlendShapeWireTest>(bogus)
        .unwrap_err()
        .to_string();
    assert!(error.contains("zz_bogus"), "{error}");

    assert!(base.get("v_lower").is_none());
    let mut wire = base;
    wire["u_range"] = serde_json::json!([-1.0, null]);
    assert!(serde_json::from_value::<VariableBlendShapeWireTest>(wire).is_err());
}

#[test]
fn an_absent_blend_tangent_is_spelled_null_and_no_magic_value() {
    use crate::geometry::VariableBlendInterpolationPoint;
    use crate::math::{Point3, Vector3};

    let point = VariableBlendInterpolationPoint {
        parameter: 0.25,
        radius: 2.0,
        tangents: [None, Some(1.0e37)],
        location: Point3::new(0.0, 0.0, 0.0),
        normal: Vector3::new(0.0, 0.0, 1.0),
    };
    let wire = serde_json::to_value(&point).unwrap();
    assert_eq!(wire["tangents"], serde_json::json!([null, 1.0e37]));
    assert_eq!(
        serde_json::from_value::<VariableBlendInterpolationPoint>(wire).unwrap(),
        point
    );
}

fn empty_loft_subdata() -> crate::geometry::LoftSubdata {
    crate::geometry::LoftSubdata::type_211([1, 0], [0.0, 1.0])
}

#[test]
fn loft_subdata_derives_counts_and_refuses_a_ragged_table() {
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
    assert_eq!(table.type_code(), 7);
    assert_eq!(table.row_count(), 2);
    assert_eq!(table.column_count(), 1);

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
fn loft_subdata_type_211_preserves_headers_independent_of_payload_size() {
    let table = crate::geometry::LoftSubdata::type_211([4, 0], [2.0, 3.0]);
    assert_eq!(table.row_count(), 4);
    assert_eq!(table.column_count(), 0);
    assert_eq!(
        serde_json::from_value::<crate::geometry::LoftSubdata>(
            serde_json::to_value(&table).unwrap()
        )
        .unwrap(),
        table
    );
}

#[test]
fn a_loft_member_form_states_its_kind_and_carries_only_its_own_keys() {
    let support = crate::geometry::LoftProfileMember {
        curve: crate::geometry::LoftPathCurve {
            id: crate::ids::CurveId::mint("test:model:curve#loft").expect("valid identity"),
            endpoints: Some([Some(0.0), Some(1.0)]),
        },
        form: crate::geometry::LoftMemberForm::Support {
            type_code: 3,
            surface: Some(
                crate::ids::SurfaceId::mint("test:model:surface#loft").expect("valid identity"),
            ),
            support_bounds: [Some(-1.0), Some(1.0), None, None],
            pcurve: None,
            first_flag: true,
            asm_extension: Some(-1),
            subdata: empty_loft_subdata(),
            direction: None,
        },
    };
    let wire = serde_json::to_value(&support).unwrap();
    assert!(wire.get("type_code").is_none());
    assert_eq!(wire["form"]["kind"], "support");
    assert_eq!(wire["form"]["type_code"], 3);
    assert_eq!(wire["form"]["surface"], "test:model:surface#loft");
    assert_eq!(wire["form"]["first_flag"], true);
    assert!(wire["form"].get("secondary_pcurve").is_none());
    assert_eq!(
        serde_json::from_value::<crate::geometry::LoftProfileMember>(wire.clone()).unwrap(),
        support
    );

    let pair = crate::geometry::LoftProfileMember {
        curve: crate::geometry::LoftPathCurve {
            id: crate::ids::CurveId::mint("test:model:curve#loft").expect("valid identity"),
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
    let mut pair_wire = serde_json::to_value(&pair).unwrap();
    assert_eq!(pair_wire["form"]["kind"], "pcurve_pair");
    assert!(pair_wire["form"].get("type_code").is_none());
    assert!(pair_wire["form"].get("support_bounds").is_none());
    assert_eq!(
        serde_json::from_value::<crate::geometry::LoftProfileMember>(pair_wire.clone()).unwrap(),
        pair
    );

    pair_wire["form"]["surface"] = serde_json::json!("test:model:surface#conflict");
    let error = serde_json::from_value::<crate::geometry::LoftProfileMember>(pair_wire)
        .unwrap_err()
        .to_string();
    assert!(error.contains("surface"), "{error}");

    let mut support_wire = wire.clone();
    support_wire["form"]["secondary_pcurve"] = serde_json::json!({
        "kind": "line",
        "origin": {"u": 0.0, "v": 0.0},
        "direction": {"u": 1.0, "v": 0.0},
    });
    let error = serde_json::from_value::<crate::geometry::LoftProfileMember>(support_wire)
        .unwrap_err()
        .to_string();
    assert!(error.contains("secondary_pcurve"), "{error}");

    let mut without_flag = wire;
    without_flag["form"]
        .as_object_mut()
        .unwrap()
        .remove("first_flag");
    let error = serde_json::from_value::<crate::geometry::LoftProfileMember>(without_flag)
        .unwrap_err()
        .to_string();
    assert!(error.contains("first_flag"), "{error}");
}

#[test]
fn law_edge_keeps_its_flat_curve_and_endpoints_wire_shape() {
    let expression = crate::geometry::LawExpression::Edge {
        curve: crate::geometry::LoftPathCurve {
            id: crate::ids::CurveId::mint("test:model:curve#law").expect("valid identity"),
            endpoints: Some([None, Some(2.0)]),
        },
        parameters: [-1.0, 3.0],
    };
    let wire = serde_json::to_value(&expression).unwrap();
    assert_eq!(wire["kind"], "edge");
    assert_eq!(wire["curve"], "test:model:curve#law");
    assert_eq!(wire["endpoints"], serde_json::json!([null, 2.0]));
    assert_eq!(
        serde_json::from_value::<crate::geometry::LawExpression>(wire).unwrap(),
        expression
    );
}

#[test]
fn a_law_formula_names_its_variant_with_a_tag() {
    let null = crate::geometry::LawFormula::Null {};
    let null_wire = serde_json::to_value(&null).unwrap();
    assert_eq!(null_wire, serde_json::json!({ "kind": "null" }));
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
            "kind": "named",
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
        "kind": "null",
        "variables": []
    }))
    .unwrap_err()
    .to_string();
    assert!(error.contains("variables"), "{error}");

    let error = serde_json::from_value::<crate::geometry::LawFormula>(serde_json::json!({
        "kind": "named",
        "name": "null_law",
        "variables": []
    }))
    .unwrap_err()
    .to_string();
    assert!(error.contains("null_law"), "{error}");
}

fn ranged_spring_definition() -> crate::geometry::ProceduralCurveDefinition {
    crate::geometry::ProceduralCurveDefinition::Spring(
        crate::geometry::curve_payloads::SpringCurvePayload::try_new(
            crate::geometry::SpringLayout::ContextFirst {
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
            4,
        )
        .unwrap(),
    )
}

#[test]
fn the_spring_layout_is_one_nested_tagged_object() {
    let definition = ranged_spring_definition();
    let wire = serde_json::to_value(&definition).unwrap();
    assert_eq!(wire["kind"], "spring");
    assert!(wire.get("context").is_none());
    assert!(wire.get("surface_parameter_ranges").is_none());
    assert_eq!(wire["layout"]["kind"], "context_first");
    assert_eq!(
        wire["layout"]["supports"][0],
        serde_json::json!({"kind": "ranges", "value": [[0.0, 1.0], [2.0, 3.0]]})
    );
    assert_eq!(
        wire["layout"]["first_pcurve"],
        serde_json::json!({"kind": "range", "value": [8.0, 9.0]})
    );
    assert!(wire["layout"].get("form").is_none());
    assert_eq!(
        serde_json::from_value::<crate::geometry::ProceduralCurveDefinition>(wire).unwrap(),
        definition
    );
}

#[test]
fn a_spring_support_side_states_one_carrier_and_no_other_key() {
    let mut split = serde_json::to_value(ranged_spring_definition()).unwrap();
    split["layout"]["supports"][0]["kind"] = serde_json::json!("surface");
    let error =
        serde_json::from_value::<crate::geometry::ProceduralCurveDefinition>(split).unwrap_err();
    assert!(error.to_string().contains("string"), "{error}");

    let mut cache_keys = serde_json::to_value(ranged_spring_definition()).unwrap();
    cache_keys["layout"]["form"] = serde_json::json!({});
    let error = serde_json::from_value::<crate::geometry::ProceduralCurveDefinition>(cache_keys)
        .unwrap_err();
    assert!(error.to_string().contains("form"), "{error}");

    let mut bogus = serde_json::to_value(ranged_spring_definition()).unwrap();
    bogus["layout"]["zz_bogus"] = serde_json::json!(1);
    let error =
        serde_json::from_value::<crate::geometry::ProceduralCurveDefinition>(bogus).unwrap_err();
    assert!(error.to_string().contains("zz_bogus"), "{error}");
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
fn vector_offset_roles_are_two_named_keys_of_one_nested_object() {
    let definition = crate::geometry::ProceduralCurveDefinition::VectorOffset(
        crate::geometry::curve_payloads::VectorOffsetCurveConstruction::try_new(
            crate::ids::CurveId::mint("test:model:curve#source").expect("valid identity"),
            [-1.0, 2.0],
            crate::math::Vector3::new(3.0, 4.0, 5.0),
            crate::geometry::VectorOffsetRoles {
                source: 7,
                offset: 9,
            },
        )
        .unwrap(),
    );
    let wire = serde_json::to_value(&definition).unwrap();
    assert_eq!(wire["roles"], serde_json::json!({"source": 7, "offset": 9}));
    assert!(wire.get("labels").is_none());
    assert!(wire.get("codes").is_none());
    assert_eq!(
        serde_json::from_value::<crate::geometry::ProceduralCurveDefinition>(wire.clone()).unwrap(),
        definition
    );

    let mut labelled = wire.clone();
    labelled["roles"]["labels"] = serde_json::json!(["source", "offset"]);
    let error = serde_json::from_value::<crate::geometry::ProceduralCurveDefinition>(labelled)
        .unwrap_err()
        .to_string();
    assert!(error.contains("labels"), "{error}");

    let mut stray = wire;
    stray["roles"]["zz_bogus"] = serde_json::json!(1);
    let error = serde_json::from_value::<crate::geometry::ProceduralCurveDefinition>(stray)
        .unwrap_err()
        .to_string();
    assert!(error.contains("zz_bogus"), "{error}");
}

#[test]
fn procedural_carrier_serialization_preserves_checked_solved_cache() {
    use crate::geometry::{CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry};
    let curve = CurveGeometry::Procedural {
        construction: "test:model:procedural_curve#0"
            .try_into()
            .expect("valid identity"),
        cache: Some(
            SolvedCurveGeometry::new(CurveGeometry::Degenerate(
                crate::geometry::DegenerateCurve::try_new(crate::math::Point3::new(1.0, 2.0, 3.0))
                    .unwrap(),
            ))
            .unwrap(),
        ),
    };
    let surface = SurfaceGeometry::Procedural {
        construction: "test:model:procedural_surface#0"
            .try_into()
            .expect("valid identity"),
        cache: Some(SolvedSurfaceGeometry::new(SurfaceGeometry::Unknown { record: None }).unwrap()),
    };
    let curve_wire = serde_json::to_value(&curve).unwrap();
    let surface_wire = serde_json::to_value(&surface).unwrap();
    assert_eq!(
        serde_json::from_value::<CurveGeometry>(curve_wire.clone()).unwrap(),
        curve
    );
    assert_eq!(
        serde_json::from_value::<SurfaceGeometry>(surface_wire.clone()).unwrap(),
        surface
    );
    assert!(serde_json::from_value::<SolvedCurveGeometry>(curve_wire).is_err());
    assert!(serde_json::from_value::<SolvedSurfaceGeometry>(surface_wire).is_err());
}

#[test]
fn solved_caches_reject_procedural_carriers_below_transform_chains() {
    use crate::geometry::{CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry};
    let mut curve = CurveGeometry::Procedural {
        construction: "test:model:procedural_curve#0"
            .try_into()
            .expect("valid identity"),
        cache: None,
    };
    let mut surface = SurfaceGeometry::Procedural {
        construction: "test:model:procedural_surface#0"
            .try_into()
            .expect("valid identity"),
        cache: None,
    };
    for _ in 0..3 {
        curve = CurveGeometry::Transformed {
            basis: Box::new(curve),
            transform: crate::transform::Transform::default(),
        };
        surface = SurfaceGeometry::Transformed {
            basis: Box::new(surface),
            transform: crate::transform::Transform::default(),
        };
        assert_eq!(SolvedCurveGeometry::new(curve.clone()), Err(curve.clone()));
        assert_eq!(
            SolvedSurfaceGeometry::new(surface.clone()),
            Err(surface.clone())
        );
        assert!(serde_json::from_value::<SolvedCurveGeometry>(
            serde_json::to_value(&curve).unwrap()
        )
        .is_err());
        assert!(serde_json::from_value::<SolvedSurfaceGeometry>(
            serde_json::to_value(&surface).unwrap()
        )
        .is_err());
    }
    let curve = CurveGeometry::Transformed {
        basis: Box::new(CurveGeometry::Unknown { record: None }),
        transform: crate::transform::Transform::default(),
    };
    let surface = SurfaceGeometry::Transformed {
        basis: Box::new(SurfaceGeometry::Unknown { record: None }),
        transform: crate::transform::Transform::default(),
    };
    assert_eq!(
        SolvedCurveGeometry::new(curve.clone())
            .unwrap()
            .as_geometry(),
        &curve
    );
    assert_eq!(
        SolvedSurfaceGeometry::new(surface.clone())
            .unwrap()
            .as_geometry(),
        &surface
    );
}

mod compound_components;
mod compound_loft;
mod fit_tolerance;
mod nurbs_invariants;
mod pcurve_metadata;

mod tspline_subtransform;

mod vertex_blend_twists;

mod revision_cache_form;

mod variable_blend_cache;

mod revision_compound_loft_tail;

mod loft_path;

mod loft_subdata;

mod surface_curve_family;

mod rolling_ball_jet;

mod rolling_ball_side;

mod variable_blend_secondary_curve;

mod variable_blend_value;

#[test]
fn bspline_surface_numeric_admission_and_transactional_edit() {
    use crate::geometry::BsplineSurface;
    use crate::math::Point3;
    let points = vec![vec![Point3::new(0.0, 0.0, 0.0); 2]; 2];
    let knots = vec![0.0, 0.0, 1.0, 1.0];
    assert!(BsplineSurface::new(
        1,
        1,
        vec![0.0, 1.0, 0.0, 1.0],
        knots.clone(),
        points.clone()
    )
    .is_err());
    let mut surface = BsplineSurface::new(1, 1, knots.clone(), knots, points).unwrap();
    let original = surface.clone();
    assert!(surface
        .edit_control_points(|point| point.x = f64::NAN)
        .is_err());
    assert_eq!(surface, original);
    let mut wire = serde_json::to_value(&surface).unwrap();
    wire["u_knots"] = serde_json::json!([0.0, 1.0, 0.0, 1.0]);
    assert!(serde_json::from_value::<BsplineSurface>(wire).is_err());
    surface.edit_control_points(|point| point.z = 2.0).unwrap();
    assert!(surface
        .control_points()
        .iter()
        .flatten()
        .all(|point| point.z == 2.0));
}

#[test]
fn analytic_circle_numeric_admission_is_shared_by_constructor_and_serde() {
    let center = Point3::new(0.0, 0.0, 0.0);
    let axis = Vector3::new(0.0, 0.0, 1.0);
    let reference = Vector3::new(1.0, 0.0, 0.0);
    for radius in [-1.0, 0.0, f64::NAN, f64::INFINITY] {
        assert!(CircleCurve::try_new(center, axis, reference, radius).is_err());
    }
    assert!(CircleCurve::try_new(Point3::new(f64::NAN, 0.0, 0.0), axis, reference, 1.0).is_err());
    assert!(CircleCurve::try_new(center, Vector3::new(0.0, 0.0, 2.0), reference, 1.0).is_err());
    assert!(CircleCurve::try_new(center, axis, axis, 1.0).is_err());
    let wire = serde_json::json!({
        "kind": "circle",
        "center": {"x": 0.0, "y": 0.0, "z": 0.0},
        "axis": {"x": 0.0, "y": 0.0, "z": 1.0},
        "ref_direction": {"x": 1.0, "y": 0.0, "z": 0.0},
        "radius": 1.0
    });
    let curve: CurveGeometry = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(serde_json::to_value(curve).unwrap(), wire);
    let mut invalid = wire.clone();
    invalid["radius"] = serde_json::json!(-1.0);
    assert!(serde_json::from_value::<CurveGeometry>(invalid)
        .unwrap_err()
        .to_string()
        .contains("radius"));
    let mut invalid = wire;
    invalid["ref_direction"] = serde_json::json!({"x": 0.0, "y": 0.0, "z": 1.0});
    assert!(serde_json::from_value::<CurveGeometry>(invalid)
        .unwrap_err()
        .to_string()
        .contains("frame"));
}

#[test]
fn analytic_surface_admission_preserves_signed_and_zero_radius_contracts() {
    let center = Point3::new(0.0, 0.0, 0.0);
    let axis = Vector3::new(0.0, 0.0, 1.0);
    let reference = Vector3::new(1.0, 0.0, 0.0);
    let tiny = 1e-200;
    for radius in [tiny, -tiny] {
        let sphere = SurfaceGeometry::Sphere(
            SphereSurface::try_new(center, axis, reference, radius).unwrap(),
        );
        let wire = serde_json::to_value(&sphere).unwrap();
        assert_eq!(
            serde_json::from_value::<SurfaceGeometry>(wire).unwrap(),
            sphere
        );
    }
    assert!(SphereSurface::try_new(center, axis, reference, 0.0).is_err());
    assert!(TorusSurface::try_new(center, axis, reference, tiny, -tiny).is_ok());
    assert!(TorusSurface::try_new(center, axis, reference, -tiny, tiny).is_err());
    assert!(ConeSurface::try_new(center, axis, reference, 0.0, 1.0, -0.5).is_ok());
    assert!(ConeSurface::try_new(center, axis, reference, -tiny, 1.0, 0.5).is_err());
    assert!(CylinderSurface::try_new(center, axis, reference, 0.0).is_err());
    assert!(PlaneSurface::try_new(center, Vector3::new(0.0, 0.0, 0.0), reference).is_err());
}

#[test]
fn analytic_pcurve_admission_preserves_nonunit_axes_and_unordered_radii() {
    let origin = Point2::new(0.0, 0.0);
    let x = Point2::new(2.0, 0.0);
    let y = Point2::new(1.0, 3.0);
    assert!(CirclePcurve::try_new(origin, x, y, 1.0).is_ok());
    assert!(EllipsePcurve::try_new(origin, x, y, 1.0, 2.0).is_ok());
    assert!(EllipseCurve::try_new(
        Point3::new(0.0, 0.0, 0.0),
        Vector3::new(0.0, 0.0, 1.0),
        Vector3::new(1.0, 0.0, 0.0),
        1.0,
        2.0,
    )
    .is_err());
    assert!(LinePcurve::try_new(origin, origin).is_err());
    assert!(HarmonicPcurve::try_new(origin, origin, x).is_ok());
    assert!(HarmonicPcurve::try_new(origin, origin, origin).is_err());
    assert!(SphericalGreatCirclePcurve::try_new(0.0, 1e-200, 0.0, 0.0).is_ok());
    assert!(SphericalGreatCirclePcurve::try_new(0.0, 0.0, 0.0, 0.0).is_err());
    let line = PcurveGeometry::Line(LinePcurve::try_new(origin, x).unwrap());
    assert!(TrimmedPcurve::try_new([2.0, 1.0], true, Box::new(line.clone())).is_err());
    assert!(TrimmedPcurve::try_new([1.0, 1.0], false, Box::new(line.clone())).is_ok());
    assert!(OffsetPcurve::try_new(f64::INFINITY, Box::new(line.clone())).is_err());
    let offset = PcurveGeometry::Offset(OffsetPcurve::try_new(-2.0, Box::new(line)).unwrap());
    let mut wire = serde_json::to_value(offset).unwrap();
    wire["basis"]["direction"] = serde_json::json!({"u": 0.0, "v": 0.0});
    assert!(serde_json::from_value::<PcurveGeometry>(wire).is_err());
}

#[test]
fn pcurve_coordinate_scaling_keeps_the_original_when_a_nested_result_overflows() {
    let mut geometry = PcurveGeometry::Offset(
        OffsetPcurve::try_new(1e300, Box::new(PcurveGeometry::Line(LinePcurve::U_AXIS))).unwrap(),
    );
    let original = geometry.clone();
    assert!(geometry.try_scale_coordinates([1e300, 1e300]).is_err());
    assert_eq!(geometry, original);
}

#[test]
fn sampled_carriers_admit_finite_numeric_payloads_and_preserve_failed_edits() {
    use super::{PolygonalSurface, PolylineCurve};
    let points = vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)];
    assert!(PolylineCurve::new(points.clone(), Some(vec![1.0, 1.0]), 0.0).is_err());
    assert!(PolylineCurve::new(points.clone(), Some(vec![0.0, f64::INFINITY]), 0.0).is_err());
    assert!(PolylineCurve::new(points.clone(), None, -1.0).is_err());
    let mut polyline = PolylineCurve::new(points, Some(vec![2.0, 1.0]), 0.0).unwrap();
    let original = polyline.clone();
    assert!(polyline
        .edit_points(|points| points[0].x = f64::NAN)
        .is_err());
    assert_eq!(polyline, original);
    assert!(polyline
        .edit_parameters(|parameters| parameters.unwrap()[1] = 2.0)
        .is_err());
    assert_eq!(polyline, original);
    assert!(polyline.set_chordal_deflection(f64::INFINITY).is_err());
    assert_eq!(polyline, original);
    let mut wire = serde_json::to_value(&polyline).unwrap();
    assert_eq!(
        serde_json::from_value::<PolylineCurve>(wire.clone()).unwrap(),
        polyline
    );
    wire["parameters"] = serde_json::json!([1.0, 1.0]);
    assert!(serde_json::from_value::<PolylineCurve>(wire).is_err());
    let mut surface = PolygonalSurface::new(
        vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(1.0, 0.0, 0.0),
            Point3::new(0.0, 1.0, 0.0),
        ],
        vec![[0, 1, 2]],
        0.0,
    )
    .unwrap();
    let original = surface.clone();
    assert!(surface
        .edit_vertices(|vertices| vertices[0].z = f64::INFINITY)
        .is_err());
    assert_eq!(surface, original);
    assert!(surface.set_chordal_deflection(-1.0).is_err());
    assert_eq!(surface, original);
    let mut wire = serde_json::to_value(&surface).unwrap();
    wire["chordal_deflection"] = serde_json::json!(-1.0);
    assert!(serde_json::from_value::<PolygonalSurface>(wire).is_err());
}

#[test]
fn support_context_admission_preserves_mapping_and_numeric_invariants() {
    use super::{
        DirectedParameterRange, IntcurveSupportContext, IntcurveSupportSide, LinePcurve,
        PcurveGeometry, SupportPcurve,
    };
    let sides = [
        IntcurveSupportSide {
            surface: None,
            pcurve: Some(SupportPcurve::new(
                PcurveGeometry::Line(
                    LinePcurve::try_new(
                        crate::math::Point2::new(0.0, 0.0),
                        crate::math::Point2::new(1.0, 0.0),
                    )
                    .unwrap(),
                ),
                Some(DirectedParameterRange::new([5.0, 2.0]).unwrap()),
            )),
        },
        IntcurveSupportSide {
            surface: None,
            pcurve: None,
        },
    ];
    let empty = || std::array::from_fn(|_| Vec::new());
    let mut context = IntcurveSupportContext::try_new(sides.clone(), [0.0, 1.0], empty()).unwrap();
    let original = context.clone();
    assert!(IntcurveSupportContext::try_new(sides.clone(), [1.0, 1.0], empty()).is_err());
    assert!(IntcurveSupportContext::try_new(sides.clone(), [1.0, 0.0], empty()).is_err());
    assert!(IntcurveSupportContext::try_new(sides.clone(), [0.0, f64::INFINITY], empty()).is_err());
    assert!(
        IntcurveSupportContext::try_new(sides, [0.0, 1.0], [vec![f64::NAN], vec![], vec![]])
            .is_err()
    );
    assert!(context.edit(|_, range, _| *range = [1.0, 1.0]).is_err());
    assert_eq!(context, original);
    assert!(context
        .edit(|_, _, discontinuities| discontinuities[1].push(f64::INFINITY))
        .is_err());
    assert_eq!(context, original);
    let mut wire = serde_json::to_value(&context).unwrap();
    assert_eq!(
        serde_json::from_value::<IntcurveSupportContext>(wire.clone()).unwrap(),
        context
    );
    wire["parameter_range"] = serde_json::json!([1.0, 1.0]);
    assert!(serde_json::from_value::<IntcurveSupportContext>(wire).is_err());
    context
        .edit(|sides, range, _| {
            sides[0].pcurve.as_mut().unwrap().parameter_range = None;
            *range = [1.0, 1.0];
        })
        .unwrap();
    let unchanged = context.clone();
    assert!(context
        .edit(|sides, _, _| {
            sides[0].pcurve.as_mut().unwrap().parameter_range =
                Some(DirectedParameterRange::new([5.0, 2.0]).unwrap());
        })
        .is_err());
    assert_eq!(context, unchanged);
}

#[test]
fn composite_curve_requires_a_segment_on_construction_and_serde() {
    use super::{
        CompositeCurveSegment, CompositeCurveSegments, CompositeCurveTransition, CurveGeometry,
    };
    assert!(CompositeCurveSegments::try_from(Vec::new()).is_err());
    let segment = CompositeCurveSegment {
        curve: crate::ids::CurveId::mint("synthetic:test:curve#child").unwrap(),
        same_sense: true,
        transition: CompositeCurveTransition::Continuous,
    };
    let mut segments = CompositeCurveSegments::try_from(vec![segment]).unwrap();
    segments[0].same_sense = false;
    let curve = CurveGeometry::Composite {
        segments,
        self_intersect: None,
    };
    let wire = serde_json::json!({
        "kind": "composite",
        "segments": [{"curve": "synthetic:test:curve#child", "same_sense": false, "transition": "continuous"}],
        "self_intersect": null,
    });
    assert_eq!(serde_json::to_value(&curve).unwrap(), wire);
    assert_eq!(
        serde_json::from_value::<CurveGeometry>(wire.clone()).unwrap(),
        curve
    );
    let mut empty = wire;
    empty["segments"] = serde_json::json!([]);
    assert!(serde_json::from_value::<CurveGeometry>(empty).is_err());
}

mod offset_coordinate;

mod tolerant_intersection;

mod loft_scale_prefix;

mod helix_payloads;

mod procedural_surface_payloads;

mod procedural_curve_payloads;

mod revolution_payloads;

#[test]
fn line_pcurve_direction_uses_the_shared_nonzero_vector_contract() {
    let origin = Point2::new(0.0, 0.0);
    let below = f64::EPSILON.sqrt() / 2.0;
    let above = f64::EPSILON.sqrt() * 2.0;
    for u in [0.0, 1e-300, below] {
        assert!(LinePcurve::try_new(origin, Point2::new(u, 0.0)).is_err());
        let wire =
            serde_json::json!({"origin": {"u": 0.0, "v": 0.0}, "direction": {"u": u, "v": 0.0}});
        assert!(serde_json::from_value::<LinePcurve>(wire).is_err());
    }
    let wire =
        serde_json::json!({"origin": {"u": 0.0, "v": 0.0}, "direction": {"u": above, "v": 0.0}});
    let line = LinePcurve::try_new(origin, Point2::new(above, 0.0)).unwrap();
    assert_eq!(serde_json::to_value(line).unwrap(), wire);
    assert_eq!(serde_json::from_value::<LinePcurve>(wire).unwrap(), line);
}

#[test]
fn the_nested_construction_enums_reject_an_unknown_key_by_name() {
    let cases: [(&str, serde_json::Value); 4] = [
        (
            "G2BlendFirstShape",
            serde_json::json!({"kind": "full", "zz_bogus": 1}),
        ),
        (
            "DeformableSurfaceData",
            serde_json::json!({"kind": "full", "zz_bogus": 1}),
        ),
        (
            "LawSurfaceTail",
            serde_json::json!({"kind": "full", "zz_bogus": 1}),
        ),
        (
            "ProjectionTail",
            serde_json::json!({"kind": "ranged", "zz_bogus": 1}),
        ),
    ];
    for (name, wire) in cases {
        let error = match name {
            "G2BlendFirstShape" => {
                serde_json::from_value::<crate::geometry::G2BlendFirstShape>(wire).unwrap_err()
            }
            "DeformableSurfaceData" => {
                serde_json::from_value::<crate::geometry::DeformableSurfaceData>(wire).unwrap_err()
            }
            "LawSurfaceTail" => {
                serde_json::from_value::<crate::geometry::LawSurfaceTail>(wire).unwrap_err()
            }
            _ => serde_json::from_value::<crate::geometry::ProjectionTail>(wire).unwrap_err(),
        }
        .to_string();
        assert!(error.contains("zz_bogus"), "{name}: {error}");
    }
}

#[test]
fn a_law_surface_full_tail_is_an_empty_struct_variant() {
    let tail = crate::geometry::LawSurfaceTail::Full {};
    let wire = serde_json::to_value(tail.clone()).unwrap();
    assert_eq!(wire, serde_json::json!({"kind": "full"}));
    assert_eq!(
        serde_json::from_value::<crate::geometry::LawSurfaceTail>(wire).unwrap(),
        tail
    );
}

/// One-pcurve document built on the unit cube, used to drive the pcurve
/// carriers over the same route a checked-in document takes.
fn document_with_pcurve(geometry: &serde_json::Value) -> serde_json::Value {
    let mut document = serde_json::to_value(unit_cube()).unwrap();
    document["model"]["pcurves"] = serde_json::json!([{
        "id": "synthetic:cube:pcurve#0",
        "geometry": geometry.clone(),
    }]);
    document
}

#[test]
fn every_pcurve_carrier_refuses_an_unknown_key_on_the_document_route() {
    let line = serde_json::json!({
        "kind": "line",
        "origin": {"u": 0.0, "v": 0.0},
        "direction": {"u": 1.0, "v": 0.0},
    });
    let cases: [(&str, serde_json::Value); 11] = [
        ("line", line.clone()),
        (
            "polar_harmonic",
            serde_json::json!({
                "kind": "polar_harmonic",
                "radial_center": {"u": 0.0, "v": 0.0},
                "radial_cos": {"u": 1.0, "v": 0.0},
                "radial_sin": {"u": 0.0, "v": 1.0},
                "axial_origin": 0.0,
                "axial_cos": 1.0,
                "axial_sin": 0.0,
            }),
        ),
        (
            "spherical_great_circle",
            serde_json::json!({
                "kind": "spherical_great_circle",
                "azimuth_origin": 0.0,
                "azimuth_rate": 1.0,
                "plane_phase": 0.0,
                "plane_slope": 1.0,
            }),
        ),
        (
            "circle",
            serde_json::json!({
                "kind": "circle",
                "center": {"u": 0.0, "v": 0.0},
                "x_axis": {"u": 1.0, "v": 0.0},
                "y_axis": {"u": 0.0, "v": 1.0},
                "radius": 1.0,
            }),
        ),
        (
            "ellipse",
            serde_json::json!({
                "kind": "ellipse",
                "center": {"u": 0.0, "v": 0.0},
                "x_axis": {"u": 1.0, "v": 0.0},
                "y_axis": {"u": 0.0, "v": 1.0},
                "major_radius": 2.0,
                "minor_radius": 1.0,
            }),
        ),
        (
            "harmonic",
            serde_json::json!({
                "kind": "harmonic",
                "center": {"u": 0.0, "v": 0.0},
                "cosine": {"u": 1.0, "v": 0.0},
                "sine": {"u": 0.0, "v": 1.0},
            }),
        ),
        (
            "parabola",
            serde_json::json!({
                "kind": "parabola",
                "vertex": {"u": 0.0, "v": 0.0},
                "x_axis": {"u": 1.0, "v": 0.0},
                "y_axis": {"u": 0.0, "v": 1.0},
                "focal_distance": 1.0,
            }),
        ),
        (
            "hyperbola",
            serde_json::json!({
                "kind": "hyperbola",
                "center": {"u": 0.0, "v": 0.0},
                "x_axis": {"u": 1.0, "v": 0.0},
                "y_axis": {"u": 0.0, "v": 1.0},
                "major_radius": 2.0,
                "minor_radius": 1.0,
            }),
        ),
        (
            "hyperbolic",
            serde_json::json!({
                "kind": "hyperbolic",
                "center": {"u": 0.0, "v": 0.0},
                "cosine": {"u": 1.0, "v": 0.0},
                "sine": {"u": 0.0, "v": 1.0},
            }),
        ),
        (
            "trimmed",
            serde_json::json!({
                "kind": "trimmed",
                "parameter_range": [0.0, 1.0],
                "same_sense": true,
                "basis": line.clone(),
            }),
        ),
        (
            "offset",
            serde_json::json!({
                "kind": "offset",
                "distance": 1.0,
                "basis": line,
            }),
        ),
    ];
    for (kind, geometry) in cases {
        let document = document_with_pcurve(&geometry);
        serde_json::from_value::<crate::CadIr>(document)
            .unwrap_or_else(|error| panic!("{kind} is a legal carrier: {error}"));

        let mut stray = geometry.as_object().unwrap().clone();
        stray.insert("zz_bogus".into(), serde_json::json!(1));
        let document = document_with_pcurve(&serde_json::Value::Object(stray));
        let error = serde_json::from_value::<crate::CadIr>(document)
            .unwrap_err()
            .to_string();
        assert!(error.contains("zz_bogus"), "{kind}: {error}");
    }
}

#[test]
fn every_payload_free_law_surface_tail_refuses_an_unknown_key() {
    for kind in ["full", "historical", "optimal"] {
        let wire = serde_json::json!({"kind": kind, "zz_bogus": 1});
        let error = serde_json::from_value::<crate::geometry::LawSurfaceTail>(wire)
            .unwrap_err()
            .to_string();
        assert!(error.contains("zz_bogus"), "{kind}: {error}");
    }
    for tail in [
        crate::geometry::LawSurfaceTail::Historical {},
        crate::geometry::LawSurfaceTail::Optimal {},
    ] {
        let wire = serde_json::to_value(tail.clone()).unwrap();
        assert_eq!(
            serde_json::from_value::<crate::geometry::LawSurfaceTail>(wire).unwrap(),
            tail
        );
    }
    assert_eq!(
        serde_json::to_value(crate::geometry::LawSurfaceTail::Historical {}).unwrap(),
        serde_json::json!({"kind": "historical"})
    );
    assert_eq!(
        serde_json::to_value(crate::geometry::LawSurfaceTail::Optimal {}).unwrap(),
        serde_json::json!({"kind": "optimal"})
    );
}

#[test]
fn the_skin_inner_count_lives_only_on_the_compact_layout_that_owns_it() {
    use crate::geometry::{SkinSurfaceLayout, SkinSurfaceProfile};

    let compact = SkinSurfaceLayout::Compact {
        inner_count: 3,
        curve: "test:model:curve#0".try_into().expect("valid identity"),
        subdata: crate::geometry::LoftSubdata::type_211([1, 1], [0.0, 1.0]),
        first_tail: 1,
        secondary_curve: "test:model:curve#1".try_into().expect("valid identity"),
        second_tail: 2,
    };
    let wire = serde_json::to_value(&compact).unwrap();
    assert_eq!(wire["inner_count"], serde_json::json!(3));
    assert_eq!(
        serde_json::from_value::<SkinSurfaceLayout>(wire).unwrap(),
        compact
    );

    let profiles = SkinSurfaceLayout::Profiles {
        profiles: Vec::<SkinSurfaceProfile>::new(),
        path: "test:model:curve#2".try_into().expect("valid identity"),
        tail: [0, 0],
    };
    let mut wire = serde_json::to_value(&profiles).unwrap();
    assert!(wire.get("inner_count").is_none());
    assert_eq!(
        serde_json::from_value::<SkinSurfaceLayout>(wire.clone()).unwrap(),
        profiles
    );

    wire["inner_count"] = serde_json::json!(0);
    let error = serde_json::from_value::<SkinSurfaceLayout>(wire)
        .unwrap_err()
        .to_string();
    assert!(error.contains("inner_count"), "{error}");
}

#[test]
fn the_shared_math_carriers_reject_an_unknown_key_by_name() {
    let vector = serde_json::json!({"x": 1.0, "y": 2.0, "z": 3.0, "zz_bogus": 1});
    let error = serde_json::from_value::<crate::math::Vector3>(vector)
        .unwrap_err()
        .to_string();
    assert!(error.contains("zz_bogus"), "{error}");

    let point = serde_json::json!({"u": 1.0, "v": 2.0, "zz_bogus": 1});
    let error = serde_json::from_value::<crate::math::Point2>(point)
        .unwrap_err()
        .to_string();
    assert!(error.contains("zz_bogus"), "{error}");
}

#[test]
fn a_law_expression_states_its_kind_and_carries_only_its_own_keys() {
    let null = crate::geometry::LawExpression::Null {};
    assert_eq!(
        serde_json::to_value(&null).unwrap(),
        serde_json::json!({"kind": "null"})
    );
    assert_eq!(
        serde_json::from_value::<crate::geometry::LawExpression>(
            serde_json::json!({"kind": "null"})
        )
        .unwrap(),
        null
    );
    for wire in [
        serde_json::json!({"kind": "null", "zz_bogus": 1}),
        serde_json::json!({"kind": "text", "value": "x", "zz_bogus": 1}),
    ] {
        let error = serde_json::from_value::<crate::geometry::LawExpression>(wire)
            .unwrap_err()
            .to_string();
        assert!(error.contains("zz_bogus"), "{error}");
    }

    let formula = serde_json::json!({
        "kind": "named",
        "name": "law",
        "variables": [],
        "zz_bogus": 1,
    });
    let error = serde_json::from_value::<crate::geometry::LawFormula>(formula)
        .unwrap_err()
        .to_string();
    assert!(error.contains("zz_bogus"), "{error}");
}
