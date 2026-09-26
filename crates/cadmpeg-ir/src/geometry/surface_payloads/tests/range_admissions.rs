// SPDX-License-Identifier: Apache-2.0
use super::assert_positive_revision_lane;
use crate::geometry::ProceduralSurfaceDefinition;

#[test]
fn the_blend_admissions_hold_ordered_ranges_and_refuse_non_finite_scalars() {
    use super::super::{
        BlendSurfacePayload, BlendSurfacePayloadWire, VariableBlendSurfacePayload,
        VariableBlendSurfacePayloadWire,
    };
    use crate::geometry::{
        BlendCrossSection, BlendRadiusLaw, CacheContract, RevisionCacheForm,
        RevisionSurfaceParameterization, RollingBallConstruction, RollingBallRadiusSelector,
        RollingBallSide, RollingBallSupportCurve, RollingBallSupportSurface, VariableBlendCache,
        VariableBlendConstruction, VariableBlendConvexity, VariableBlendRadii,
        VariableBlendRenderMode, VariableBlendSupportKind, VariableBlendSurfaceSubtype,
        VariableBlendValue, VariableBlendValuePayload,
    };
    use crate::ids::{CurveId, SurfaceId};
    use crate::math::Point3;

    // One value per float field family the two admissions reach and the
    // predecessor left unrefused.
    #[derive(Clone, Copy)]
    struct Fields {
        side_surface_range: [[Option<f64>; 2]; 2],
        side_curve_range: [Option<f64>; 2],
        slice_range: [Option<f64>; 2],
        u_interval: [Option<f64>; 2],
        discontinuity: f64,
    }
    let admitted = Fields {
        side_surface_range: [[Some(0.0), None], [None, Some(1.0)]],
        side_curve_range: [None, Some(2.0)],
        slice_range: [Some(3.0), None],
        u_interval: [Some(4.0), None],
        discontinuity: 5.0,
    };
    let curve = || CurveId::mint("synthetic:test:curve#blend").unwrap();
    let surface = || SurfaceId::mint("synthetic:test:surface#blend").unwrap();
    let side = |fields: Fields| RollingBallSide {
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
        location: Point3::new(0.0, 0.0, 0.0),
        secondary_pcurve: None,
        extension: None,
    };
    let parameterization = |fields: Fields| RevisionSurfaceParameterization {
        u_interval: fields.u_interval,
        v_interval: [None, None],
        u_closure: 0,
        v_closure: 0,
        u_singularity: 0,
        v_singularity: 0,
    };
    let discontinuities = |fields: Fields| {
        [
            vec![fields.discontinuity],
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
        ]
    };

    let variable = |fields: Fields| {
        Box::new(VariableBlendConstruction {
            subtype: VariableBlendSurfaceSubtype::VariableBlend,
            revision: crate::scalar::PositiveI64::new(1).expect("positive revision"),
            sides: [side(fields), side(admitted)],
            slice: curve(),
            slice_range: fields.slice_range,
            offsets: [0.0, 0.0],
            radii: VariableBlendRadii::Single {
                value: VariableBlendValue {
                    modern_flag: false,
                    calibrated: 0,
                    payload: VariableBlendValuePayload::TwoEnds {
                        discriminator: 0,
                        parameters: [0.0, 1.0],
                        radii: [1.0, 1.0],
                    },
                },
            },
            cross_section: None,
            u_range: [0.0, 1.0],
            v_lower: None,
            shape_parameter: 0.0,
            shape_length: 0.0,
            shape_tail: 0,
            cache: VariableBlendCache::Parameterization {
                shape_prefix: 0,
                parameterization: parameterization(fields),
            },
            discontinuities: discontinuities(fields),
            tail_flag: false,
            tail_extensions: [0; 3],
            secondary_curve: None,
            convexity: VariableBlendConvexity::Convex,
            render_mode: VariableBlendRenderMode::RollingBallEnvelope,
            post_range: [None, None],
            post_curve: None,
            post_pcurve: None,
        })
    };
    let variable_new = |fields: Fields| VariableBlendSurfacePayload::try_new(variable(fields));
    let variable_wire = |fields: Fields| {
        VariableBlendSurfacePayload::try_from(VariableBlendSurfacePayloadWire {
            construction: variable(fields),
        })
    };

    let rolling = |fields: Fields| {
        CacheContract::from_form(Some(Box::new(RollingBallConstruction {
            revision: crate::scalar::PositiveI64::new(1).expect("positive revision"),
            sides: [side(fields), side(admitted)],
            slice: curve(),
            slice_range: fields.slice_range,
            offsets: [0.0, 0.0],
            radius_selector: RollingBallRadiusSelector::None {},
            u_range: [None, None],
            v_range: [None, None],
            shape_prefix: 0,
            parameters: [0.0, 0.0],
            tail: 0,
            cache: RevisionCacheForm::Parameterization(parameterization(fields)),
            discontinuities: discontinuities(fields),
            tail_flag: false,
            third: None,
            tail_extensions: [0; 3],
        })))
    };
    let blend_new = |fields: Fields| {
        BlendSurfacePayload::try_new(
            [None, None],
            None,
            BlendRadiusLaw::constant(1.0).unwrap(),
            BlendCrossSection::Circular,
            rolling(fields),
        )
    };
    let blend_wire = |fields: Fields| {
        BlendSurfacePayload::try_from(BlendSurfacePayloadWire {
            supports: [None, None],
            spine: None,
            radius: BlendRadiusLaw::constant(1.0).unwrap(),
            cross_section: BlendCrossSection::Circular,
            cache: rolling(fields),
        })
    };

    let admitted_variable = variable_new(admitted).unwrap();
    assert_eq!(
        admitted_variable
            .slice_range()
            .endpoints()
            .map(|value| value.map(crate::scalar::FiniteReal::get)),
        [Some(3.0), None]
    );
    let definition = ProceduralSurfaceDefinition::VariableBlend(admitted_variable);
    let wire = serde_json::to_value(&definition).unwrap();
    assert_positive_revision_lane(&wire, "/construction/revision");
    let first_side = &wire["construction"]["sides"][0];
    assert_eq!(
        first_side["surface"]["parameter_ranges"],
        serde_json::json!([[0.0, null], [null, 1.0]])
    );
    assert_eq!(
        first_side["curve"]["parameter_range"],
        serde_json::json!([null, 2.0])
    );
    assert_eq!(
        wire["construction"]["slice_range"],
        serde_json::json!([3.0, null])
    );
    assert_eq!(
        wire["construction"]["cache"]["parameterization"]["u_interval"],
        serde_json::json!([4.0, null])
    );
    assert_eq!(
        wire["construction"]["discontinuities"][0],
        serde_json::json!([5.0])
    );
    assert_eq!(
        serde_json::from_value::<ProceduralSurfaceDefinition>(wire).unwrap(),
        definition
    );
    assert!(variable_wire(admitted).is_ok());
    let reversed_slice = Fields {
        slice_range: [Some(4.0), Some(3.0)],
        ..admitted
    };
    assert!(variable_new(reversed_slice).is_err());
    assert!(variable_wire(reversed_slice).is_err());
    assert!(blend_new(admitted).is_ok());
    assert!(blend_wire(admitted).is_ok());
    let blend = ProceduralSurfaceDefinition::Blend(blend_new(admitted).unwrap());
    assert_positive_revision_lane(
        &serde_json::to_value(&blend).unwrap(),
        "/cache/form/revision",
    );

    let blend_with_native_ranges = |u_range, v_range| {
        let CacheContract::Revision { mut form } = rolling(admitted) else {
            panic!("rolling construction is revision-gated")
        };
        form.u_range = u_range;
        form.v_range = v_range;
        BlendSurfacePayload::try_new(
            [None, None],
            None,
            BlendRadiusLaw::constant(1.0).unwrap(),
            BlendCrossSection::Circular,
            CacheContract::Revision { form },
        )
    };
    let ordered = blend_with_native_ranges([Some(0.0), Some(1.0)], [None, Some(2.0)]).unwrap();
    let ranges = ordered.native_ranges().unwrap();
    assert_eq!(
        ranges[0]
            .endpoints()
            .map(|value| value.map(crate::scalar::FiniteReal::get)),
        [Some(0.0), Some(1.0)]
    );
    assert_eq!(
        ranges[1]
            .endpoints()
            .map(|value| value.map(crate::scalar::FiniteReal::get)),
        [None, Some(2.0)]
    );
    assert_eq!(
        serde_json::from_value::<BlendSurfacePayload>(serde_json::to_value(&ordered).unwrap())
            .unwrap(),
        ordered
    );
    assert!(blend_with_native_ranges([Some(2.0), Some(1.0)], [None, None]).is_err());
    assert!(blend_with_native_ranges([None, None], [Some(2.0), Some(1.0)]).is_err());

    // JSON itself states no infinity or NaN, so the wire cannot spell a
    // refused value; `TryFrom<…Wire>` is the conversion the deserializer
    // runs, and it is exercised directly here.
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        for fields in [
            Fields {
                side_surface_range: [[Some(value), None], [None, Some(1.0)]],
                ..admitted
            },
            Fields {
                side_curve_range: [None, Some(value)],
                ..admitted
            },
            Fields {
                slice_range: [Some(value), None],
                ..admitted
            },
            Fields {
                u_interval: [Some(value), None],
                ..admitted
            },
            Fields {
                discontinuity: value,
                ..admitted
            },
        ] {
            assert!(variable_new(fields).is_err());
            assert!(variable_wire(fields).is_err());
            assert!(blend_new(fields).is_err());
            assert!(blend_wire(fields).is_err());
        }
    }
}
