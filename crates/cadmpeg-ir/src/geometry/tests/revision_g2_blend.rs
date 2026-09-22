// SPDX-License-Identifier: Apache-2.0
use crate::geometry::{
    ProceduralGeometryError, ProceduralSurfaceDefinition, RevisionCacheForm,
    RevisionG2BlendConstruction, RevisionG2BlendConstructionWire, RevisionG2RadiusValue,
    RevisionSurfaceParameterization, RollingBallRadiusSelector, RollingBallSide,
    RollingBallSupportCurve, RollingBallSupportSurface, VariableBlendSupportKind,
};
use crate::ids::{CurveId, SurfaceId};
use crate::math::Point3;
use crate::scalar::PositiveI64;

// One value per float field family the construction carries.
struct Fields {
    leading_parameter: f64,
    side_surface_range: [[Option<f64>; 2]; 2],
    side_curve_range: [Option<f64>; 2],
    side_location: [f64; 3],
    center_range: [Option<f64>; 2],
    radius: f64,
    u_range: [Option<f64>; 2],
    v_range: [Option<f64>; 2],
    shape_parameter: f64,
    shape_length: f64,
    u_interval: [Option<f64>; 2],
    discontinuity: f64,
}

const ADMITTED: Fields = Fields {
    leading_parameter: 1.0,
    side_surface_range: [[Some(0.0), None], [None, Some(4.0)]],
    side_curve_range: [None, Some(6.0)],
    side_location: [1.0, 2.0, 3.0],
    center_range: [Some(0.0), Some(7.0)],
    radius: 8.0,
    u_range: [Some(9.0), None],
    v_range: [None, Some(10.0)],
    shape_parameter: 11.0,
    shape_length: 12.0,
    u_interval: [Some(13.0), None],
    discontinuity: 14.0,
};

fn curve() -> CurveId {
    CurveId::mint("synthetic:test:curve#g2").expect("valid identity")
}

fn surface() -> SurfaceId {
    SurfaceId::mint("synthetic:test:surface#g2").expect("valid identity")
}

fn side(fields: &Fields) -> RollingBallSide {
    RollingBallSide {
        support_kind: VariableBlendSupportKind::Surface,
        surface: Some(RollingBallSupportSurface {
            surface: surface(),
            parameter_ranges: fields.side_surface_range,
        }),
        curve: Some(RollingBallSupportCurve {
            curve: curve(),
            parameter_range: fields.side_curve_range,
        }),
        pcurve: None,
        location: Point3::new(
            fields.side_location[0],
            fields.side_location[1],
            fields.side_location[2],
        ),
        secondary_pcurve: None,
        extension: None,
    }
}

// The fields are private, so the wire mirror is the only input `admit`
// accepts and the only shape the deserializer builds.
fn stored(fields: &Fields) -> RevisionG2BlendConstructionWire {
    RevisionG2BlendConstructionWire {
        revision: PositiveI64::new(1).expect("positive revision"),
        leading_parameters: [fields.leading_parameter, 2.0],
        sides: Box::new([side(fields), side(&ADMITTED)]),
        center: curve(),
        center_range: fields.center_range,
        radii: [fields.radius, 15.0],
        radius_selector: RollingBallRadiusSelector::Value {
            value: RevisionG2RadiusValue::new(3).expect("a positive selector"),
        },
        u_range: fields.u_range,
        v_range: fields.v_range,
        shape_prefix: 4,
        shape_parameter: fields.shape_parameter,
        shape_length: fields.shape_length,
        shape_tail: 5,
        cache: RevisionCacheForm::Parameterization(RevisionSurfaceParameterization {
            u_interval: fields.u_interval,
            v_interval: [None, None],
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
        tail_extensions: [6, 7, 8],
    }
}

fn wire(fields: &Fields) -> Result<RevisionG2BlendConstruction, ProceduralGeometryError> {
    RevisionG2BlendConstruction::try_from(stored(fields))
}

#[test]
fn the_revision_g2_blend_admission_writes_every_scalar_it_admits() {
    let definition = ProceduralSurfaceDefinition::RevisionG2Blend {
        construction: Box::new(
            RevisionG2BlendConstruction::admit(stored(&ADMITTED)).expect("every scalar is finite"),
        ),
    };
    let value = serde_json::to_value(&definition).expect("serialize the definition");
    let stored = &value["construction"];
    assert_eq!(stored["leading_parameters"], serde_json::json!([1.0, 2.0]));
    assert_eq!(
        stored["sides"][0]["surface"]["parameter_ranges"],
        serde_json::json!([[0.0, null], [null, 4.0]])
    );
    assert_eq!(
        stored["sides"][0]["curve"]["parameter_range"],
        serde_json::json!([null, 6.0])
    );
    assert_eq!(
        stored["sides"][0]["location"],
        serde_json::json!({"x": 1.0, "y": 2.0, "z": 3.0})
    );
    assert_eq!(stored["center_range"], serde_json::json!([0.0, 7.0]));
    assert_eq!(stored["radii"], serde_json::json!([8.0, 15.0]));
    assert_eq!(stored["u_range"], serde_json::json!([9.0, null]));
    assert_eq!(stored["v_range"], serde_json::json!([null, 10.0]));
    assert_eq!(stored["shape_parameter"], serde_json::json!(11.0));
    assert_eq!(stored["shape_length"], serde_json::json!(12.0));
    assert_eq!(
        stored["cache"]["u_interval"],
        serde_json::json!([13.0, null])
    );
    assert_eq!(stored["discontinuities"][0], serde_json::json!([14.0]));
    assert_eq!(
        serde_json::from_value::<ProceduralSurfaceDefinition>(value)
            .expect("the definition round-trips"),
        definition
    );
    assert!(wire(&ADMITTED).is_ok());
}

#[test]
fn the_revision_g2_blend_admits_the_full_positive_revision_lane() {
    let definition = ProceduralSurfaceDefinition::RevisionG2Blend {
        construction: Box::new(
            RevisionG2BlendConstruction::admit(stored(&ADMITTED))
                .expect("positive revision and finite scalars"),
        ),
    };
    let value = serde_json::to_value(definition).expect("serialize the definition");

    for revision in [0_i64, -1] {
        assert!(PositiveI64::new(revision).is_none());
        let mut invalid = value.clone();
        invalid["construction"]["revision"] = serde_json::json!(revision);
        assert!(serde_json::from_value::<ProceduralSurfaceDefinition>(invalid).is_err());
    }

    let mut maximum = stored(&ADMITTED);
    maximum.revision = PositiveI64::new(i64::MAX).expect("maximum is positive");
    assert_eq!(
        RevisionG2BlendConstruction::admit(maximum)
            .expect("maximum revision is admitted")
            .revision()
            .get(),
        i64::MAX
    );
    let mut maximum = value;
    maximum["construction"]["revision"] = serde_json::json!(i64::MAX);
    let parsed = serde_json::from_value::<ProceduralSurfaceDefinition>(maximum)
        .expect("maximum JSON revision is admitted");
    let ProceduralSurfaceDefinition::RevisionG2Blend { construction } = parsed else {
        panic!("expected revision G2 blend")
    };
    assert_eq!(construction.revision().get(), i64::MAX);
}

#[test]
fn the_revision_g2_blend_admission_refuses_a_non_finite_scalar() {
    // JSON itself states no infinity or NaN, so the wire cannot spell a
    // refused value; `TryFrom<…Wire>` is the conversion the deserializer
    // runs, and it is exercised directly here.
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        for fields in [
            Fields {
                leading_parameter: value,
                ..ADMITTED
            },
            Fields {
                side_surface_range: [[Some(0.0), None], [Some(value), None]],
                ..ADMITTED
            },
            Fields {
                side_curve_range: [Some(value), None],
                ..ADMITTED
            },
            Fields {
                side_location: [1.0, value, 3.0],
                ..ADMITTED
            },
            Fields {
                center_range: [None, Some(value)],
                ..ADMITTED
            },
            Fields {
                radius: value,
                ..ADMITTED
            },
            Fields {
                u_range: [Some(value), None],
                ..ADMITTED
            },
            Fields {
                v_range: [None, Some(value)],
                ..ADMITTED
            },
            Fields {
                shape_parameter: value,
                ..ADMITTED
            },
            Fields {
                shape_length: value,
                ..ADMITTED
            },
            Fields {
                u_interval: [Some(value), None],
                ..ADMITTED
            },
            Fields {
                discontinuity: value,
                ..ADMITTED
            },
        ] {
            assert!(RevisionG2BlendConstruction::admit(stored(&fields)).is_err());
            assert!(wire(&fields).is_err());
        }
    }
}
