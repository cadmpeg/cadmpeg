// SPDX-License-Identifier: Apache-2.0
//! Each surface-curve family carries its own tail flags type, so the
//! parametric-only `second_flag` has no wire form on the other three.

use crate::geometry::{
    CacheFirstCurveParameterization, IntcurveSupportContext, IntcurveSupportSide,
    ParametricSurfaceCurveFlags, RevisionCacheForm, SurfaceCurveCacheFirst, SurfaceCurveFamily,
    SurfaceCurveTail,
};

fn support_context() -> IntcurveSupportContext {
    IntcurveSupportContext::try_new(
        [
            IntcurveSupportSide {
                surface: None,
                pcurve: None,
            },
            IntcurveSupportSide {
                surface: None,
                pcurve: None,
            },
        ],
        [0.0, 1.0],
        [Vec::new(), Vec::new(), Vec::new()],
    )
    .expect("finite ordered support context")
}

fn tail() -> SurfaceCurveTail {
    SurfaceCurveTail {
        extension: 7,
        revision: 23100,
        cache: RevisionCacheForm::Parameterization(CacheFirstCurveParameterization {
            interval: [Some(0.0), Some(1.0)],
            closed_form: 0,
        }),
        support_bounds: [[None; 4]; 2],
        solved_range: [Some(-1.0), Some(2.0)],
    }
}

#[test]
fn a_blend_surface_curve_tail_refuses_the_parametric_second_flag() {
    let blend = SurfaceCurveFamily::Blend {
        context: support_context(),
        tail: Some(SurfaceCurveCacheFirst {
            form: tail(),
            flags: true,
        }),
    };
    let wire = serde_json::to_value(&blend).expect("serializes");
    assert_eq!(wire["family"], "blend");
    assert_eq!(wire["tail"]["flags"], serde_json::json!(true));
    assert_eq!(wire["tail"]["form"]["extension"], serde_json::json!(7));
    assert_eq!(
        serde_json::from_value::<SurfaceCurveFamily>(wire.clone()).expect("round trip"),
        blend
    );

    for pointer in ["/tail", "/tail/form", "/tail/flags"] {
        let mut mutated = wire.clone();
        let node = mutated.pointer_mut(pointer).expect("pointer");
        let object = match node.as_object_mut() {
            Some(object) => object,
            None => {
                // `flags` is a bare bool on a blend tail, so the key cannot be
                // written beside it at all.
                *node = serde_json::json!({"flag": true, "second_flag": false});
                let error = serde_json::from_value::<SurfaceCurveFamily>(mutated)
                    .err()
                    .expect("parametric flags are not a blend tail")
                    .to_string();
                assert!(error.contains("bool"), "{pointer}: {error}");
                continue;
            }
        };
        object.insert("second_flag".to_string(), serde_json::json!(false));
        let error = serde_json::from_value::<SurfaceCurveFamily>(mutated)
            .err()
            .expect("second_flag is parametric-only")
            .to_string();
        assert!(error.contains("second_flag"), "{pointer}: {error}");
    }

    let parametric = SurfaceCurveFamily::Parametric {
        context: support_context(),
        tail: Some(SurfaceCurveCacheFirst {
            form: tail(),
            flags: ParametricSurfaceCurveFlags {
                flag: true,
                second_flag: Some(false),
            },
        }),
    };
    let wire = serde_json::to_value(&parametric).expect("serializes");
    assert_eq!(wire["family"], "parametric");
    assert_eq!(wire["tail"]["flags"]["flag"], serde_json::json!(true));
    assert_eq!(wire["tail"]["flags"]["second_flag"], serde_json::json!(false));
    assert_eq!(
        serde_json::from_value::<SurfaceCurveFamily>(wire).expect("round trip"),
        parametric
    );
}
