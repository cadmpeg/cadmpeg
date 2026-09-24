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
    SurfaceCurveTail::try_new(
        7,
        crate::scalar::PositiveI64::new(23_100).expect("positive revision"),
        RevisionCacheForm::Parameterization(CacheFirstCurveParameterization {
            interval: [Some(0.0), Some(1.0)],
            closed_form: 0,
        }),
        [[None; 4]; 2],
        [Some(-1.0), Some(2.0)],
    )
    .expect("finite surface curve tail")
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
        let Some(object) = node.as_object_mut() else {
            // `flags` is a bare bool on a blend tail, so the key cannot be
            // written beside it at all.
            *node = serde_json::json!({"flag": true, "second_flag": false});
            let error = serde_json::from_value::<SurfaceCurveFamily>(mutated)
                .unwrap_err()
                .to_string();
            assert!(error.contains("bool"), "{pointer}: {error}");
            continue;
        };
        object.insert("second_flag".to_string(), serde_json::json!(false));
        let error = serde_json::from_value::<SurfaceCurveFamily>(mutated)
            .unwrap_err()
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
    assert_eq!(
        wire["tail"]["flags"]["second_flag"],
        serde_json::json!(false)
    );
    assert_eq!(
        serde_json::from_value::<SurfaceCurveFamily>(wire).expect("round trip"),
        parametric
    );
}

fn degenerate_tail(family: usize, value: f64) -> crate::geometry::SurfaceCurveTailWire {
    let mut support_bounds = [[None; 4]; 2];
    support_bounds[0][0] = Some(0.0);
    let mut solved_range = [Some(-1.0), Some(2.0)];
    let mut interval = [Some(0.0), Some(1.0)];
    match family {
        0 => support_bounds[0][0] = Some(value),
        1 => solved_range[0] = Some(value),
        _ => interval[1] = Some(value),
    }
    crate::geometry::SurfaceCurveTailWire {
        extension: 7,
        revision: crate::scalar::PositiveI64::new(23_100).expect("positive revision"),
        cache: RevisionCacheForm::Parameterization(CacheFirstCurveParameterization {
            interval,
            closed_form: 0,
        }),
        support_bounds,
        solved_range,
    }
}

#[test]
fn the_surface_curve_tail_refuses_every_non_finite_scalar() {
    let admitted = tail();
    let wire = serde_json::to_value(&admitted).expect("serializes");
    assert_eq!(wire["extension"], serde_json::json!(7));
    assert_eq!(wire["revision"], serde_json::json!(23_100));
    assert_eq!(
        wire["support_bounds"][0],
        serde_json::json!([null, null, null, null])
    );
    assert_eq!(wire["solved_range"], serde_json::json!([-1.0, 2.0]));
    // `RevisionCacheForm` is internally tagged and its parameterization is a
    // newtype variant, so the interval is one level up from the variant name.
    assert_eq!(wire["cache"]["interval"], serde_json::json!([0.0, 1.0]));
    assert_eq!(
        serde_json::from_value::<SurfaceCurveTail>(wire).expect("round trip"),
        admitted
    );

    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        for family in 0..3 {
            let wire = degenerate_tail(family, value);
            assert!(SurfaceCurveTail::try_new(
                wire.extension,
                wire.revision,
                wire.cache.clone(),
                wire.support_bounds,
                wire.solved_range,
            )
            .is_err());
            assert!(SurfaceCurveTail::try_from(wire).is_err());
        }
    }
}

#[test]
fn the_surface_curve_tail_refuses_a_non_positive_revision() {
    let value = serde_json::to_value(tail()).expect("serialize the tail");

    for revision in [0_i64, -1] {
        assert!(crate::scalar::PositiveI64::new(revision).is_none());
        let mut invalid = value.clone();
        invalid["revision"] = serde_json::json!(revision);
        assert!(serde_json::from_value::<SurfaceCurveTail>(invalid).is_err());
    }

    let maximum = SurfaceCurveTail::try_new(
        7,
        crate::scalar::PositiveI64::new(i64::MAX).expect("maximum revision is admitted"),
        RevisionCacheForm::Parameterization(CacheFirstCurveParameterization {
            interval: [None; 2],
            closed_form: 0,
        }),
        [[None; 4]; 2],
        [None; 2],
    )
    .expect("maximum revision is admitted");
    assert_eq!(maximum.revision().get(), i64::MAX);
    let mut maximum = value;
    maximum["revision"] = serde_json::json!(i64::MAX);
    assert_eq!(
        serde_json::from_value::<SurfaceCurveTail>(maximum)
            .expect("maximum JSON revision is admitted")
            .revision()
            .get(),
        i64::MAX
    );
}

#[test]
fn an_admitted_revision_builds_the_tail_that_the_raw_revision_admits() {
    let revision = crate::scalar::PositiveI64::new(23_100).expect("a positive revision");
    let cache = || {
        RevisionCacheForm::Parameterization(CacheFirstCurveParameterization {
            interval: [Some(0.0), Some(1.0)],
            closed_form: 0,
        })
    };
    let admitted = SurfaceCurveTail::try_new(
        7,
        revision,
        cache(),
        [[None; 4]; 2],
        [Some(-1.0), Some(2.0)],
    )
    .expect("finite surface curve tail");
    let mut raw = serde_json::to_value(tail()).expect("serializes");
    raw["revision"] = serde_json::json!(23_100_i64);
    assert_eq!(
        admitted,
        serde_json::from_value::<SurfaceCurveTail>(raw).expect("the raw revision is admitted")
    );
    assert_eq!(admitted.revision(), revision);
    assert!(SurfaceCurveTail::try_new(
        7,
        revision,
        cache(),
        [[None; 4]; 2],
        [Some(f64::NAN), None]
    )
    .is_err());
}

#[test]
fn a_surface_curve_tail_holds_its_admitted_bounds() {
    use crate::geometry::RecordBounds;
    use crate::scalar::FiniteReal;

    let tail = SurfaceCurveTail::try_new(
        7,
        crate::scalar::PositiveI64::new(23_100).expect("positive revision"),
        RevisionCacheForm::Parameterization(CacheFirstCurveParameterization {
            interval: [Some(0.0), Some(1.0)],
            closed_form: 0,
        }),
        [[Some(0.5), None, None, Some(2.0)], [None; 4]],
        [Some(-1.0), None],
    )
    .expect("finite surface curve tail");
    assert_eq!(
        *tail.support_bounds(),
        [
            RecordBounds::try_new([Some(0.5), None, None, Some(2.0)]).unwrap(),
            RecordBounds::try_new([None; 4]).unwrap(),
        ]
    );
    assert_eq!(*tail.solved_range(), [FiniteReal::new(-1.0), None]);
    let wire = serde_json::to_value(&tail).expect("serialize the tail");
    assert_eq!(
        wire["support_bounds"],
        serde_json::json!([[0.5, null, null, 2.0], [null, null, null, null]])
    );
    assert_eq!(wire["solved_range"], serde_json::json!([-1.0, null]));
}
