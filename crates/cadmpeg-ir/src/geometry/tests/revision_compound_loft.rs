// SPDX-License-Identifier: Apache-2.0
use crate::geometry::{
    CompoundLoftDirection, LoftMemberForm, LoftPath, LoftPathCurve, LoftProfileMember,
    LoftSectionEntry, LoftSubdata, LoftSubdataRow, ProceduralGeometryError,
    ProceduralSurfaceDefinition, RevisionCacheForm, RevisionCompoundLoftConstruction,
    RevisionCompoundLoftConstructionWire, RevisionCompoundLoftTail,
    RevisionSurfaceParameterization,
};
use crate::ids::CurveId;
use crate::math::Vector3;

// One value per float field family the construction carries.
struct Fields {
    extra: Option<[f64; 2]>,
    support_bounds: [Option<f64>; 4],
    profile_endpoints: Option<[Option<f64>; 2]>,
    path_endpoints: Option<[Option<f64>; 2]>,
    direction: [f64; 3],
    entry_parameter: f64,
    interval: [f64; 2],
    u_interval: [Option<f64>; 2],
    v_interval: [Option<f64>; 2],
    discontinuity: f64,
}

const ADMITTED: Fields = Fields {
    extra: Some([0.0, 1.0]),
    support_bounds: [Some(0.0), Some(1.0), None, Some(2.0)],
    profile_endpoints: Some([Some(0.0), None]),
    path_endpoints: Some([None, Some(1.0)]),
    direction: [0.0, 0.0, 1.0],
    entry_parameter: 4.0,
    interval: [5.0, 6.0],
    u_interval: [Some(0.0), Some(1.0)],
    v_interval: [None, Some(2.0)],
    discontinuity: 3.0,
};

fn curve() -> CurveId {
    CurveId::mint("synthetic:test:curve#cloft").expect("valid identity")
}

// `extra` is stored by the table form only; type 211 has no row to carry it.
fn profile(fields: &Fields) -> Vec<LoftProfileMember> {
    vec![LoftProfileMember {
        profile: LoftPathCurve {
            id: curve(),
            endpoints: fields.profile_endpoints,
        },
        form: LoftMemberForm::Support {
            type_code: 1,
            surface: None,
            support_bounds: fields.support_bounds,
            pcurve: None,
            first_flag: false,
            asm_extension: None,
            subdata: LoftSubdata::table(
                3,
                vec![LoftSubdataRow {
                    parameters: [0.0, 1.0],
                    columns: Vec::new(),
                    extra: fields.extra,
                }],
            )
            .expect("one row shares its own column width"),
            direction: None,
        },
    }]
}

fn path(fields: &Fields) -> LoftPath {
    LoftPath {
        path: Some(LoftPathCurve {
            id: curve(),
            endpoints: fields.path_endpoints,
        }),
        auxiliaries: Vec::new(),
        flag: 0,
    }
}

// The fields are private, so the wire mirror is the only input `admit`
// accepts and the only shape the deserializer builds.
fn stored(fields: &Fields) -> RevisionCompoundLoftConstructionWire {
    RevisionCompoundLoftConstructionWire {
        revision: 1,
        cache: RevisionCacheForm::Parameterization(RevisionSurfaceParameterization {
            u_interval: fields.u_interval,
            v_interval: fields.v_interval,
            u_closure: 0,
            v_closure: 0,
            u_singularity: 0,
            v_singularity: 0,
        }),
        discontinuities: [
            vec![fields.discontinuity],
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
        ],
        tail_flag: false,
        base_profile: profile(fields),
        base_path: path(fields),
        entries: vec![LoftSectionEntry {
            parameter: fields.entry_parameter,
            profile: profile(fields),
            path: path(fields),
        }],
        flags: [false; 2],
        kind_flags: [false; 2],
        direction: CompoundLoftDirection::Vector {
            value: Vector3::new(
                fields.direction[0],
                fields.direction[1],
                fields.direction[2],
            ),
        },
        tail: RevisionCompoundLoftTail::Curve {
            interval: fields.interval,
            curve: curve(),
        },
    }
}

fn wire(fields: &Fields) -> Result<RevisionCompoundLoftConstruction, ProceduralGeometryError> {
    RevisionCompoundLoftConstruction::try_from(stored(fields))
}

#[test]
fn the_revision_compound_loft_admission_writes_every_scalar_it_admits() {
    let definition = ProceduralSurfaceDefinition::RevisionCompoundLoft {
        construction: Box::new(
            RevisionCompoundLoftConstruction::admit(stored(&ADMITTED))
                .expect("every scalar is finite"),
        ),
    };
    let value = serde_json::to_value(&definition).expect("serialize the definition");
    let stored = &value["construction"];
    let member = &stored["base_profile"][0];
    assert_eq!(
        member["form"]["subdata"]["rows"][0]["extra"],
        serde_json::json!([0.0, 1.0])
    );
    assert_eq!(
        member["form"]["support_bounds"],
        serde_json::json!([0.0, 1.0, null, 2.0])
    );
    assert_eq!(
        member["profile"]["endpoints"],
        serde_json::json!([0.0, null])
    );
    assert_eq!(
        stored["base_path"]["path"]["endpoints"],
        serde_json::json!([null, 1.0])
    );
    assert_eq!(stored["entries"][0]["parameter"], serde_json::json!(4.0));
    assert_eq!(
        stored["direction"],
        serde_json::json!({"kind": "vector", "value": {"x": 0.0, "y": 0.0, "z": 1.0}})
    );
    assert_eq!(stored["tail"]["interval"], serde_json::json!([5.0, 6.0]));
    assert_eq!(stored["cache"]["u_interval"], serde_json::json!([0.0, 1.0]));
    assert_eq!(
        stored["cache"]["v_interval"],
        serde_json::json!([null, 2.0])
    );
    assert_eq!(stored["discontinuities"][0], serde_json::json!([3.0]));
    assert_eq!(
        serde_json::from_value::<ProceduralSurfaceDefinition>(value)
            .expect("the definition round-trips"),
        definition
    );
    assert!(wire(&ADMITTED).is_ok());
}

#[test]
fn the_revision_compound_loft_admission_refuses_a_non_finite_scalar() {
    // JSON itself states no infinity or NaN, so the wire cannot spell a
    // refused value; `TryFrom<…Wire>` is the conversion the deserializer
    // runs, and it is exercised directly here.
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        for fields in [
            Fields {
                extra: Some([value, 1.0]),
                ..ADMITTED
            },
            Fields {
                support_bounds: [Some(0.0), None, Some(value), None],
                ..ADMITTED
            },
            Fields {
                profile_endpoints: Some([Some(value), None]),
                ..ADMITTED
            },
            Fields {
                path_endpoints: Some([None, Some(value)]),
                ..ADMITTED
            },
            Fields {
                direction: [0.0, value, 1.0],
                ..ADMITTED
            },
            Fields {
                entry_parameter: value,
                ..ADMITTED
            },
            Fields {
                interval: [5.0, value],
                ..ADMITTED
            },
            Fields {
                u_interval: [Some(value), None],
                ..ADMITTED
            },
            Fields {
                v_interval: [None, Some(value)],
                ..ADMITTED
            },
            Fields {
                discontinuity: value,
                ..ADMITTED
            },
        ] {
            assert!(RevisionCompoundLoftConstruction::admit(stored(&fields)).is_err());
            assert!(wire(&fields).is_err());
        }
    }
}
