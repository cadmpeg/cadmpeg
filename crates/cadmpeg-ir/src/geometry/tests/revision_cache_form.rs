// SPDX-License-Identifier: Apache-2.0
use crate::geometry::{
    CacheFirstCurveParameterization, FitTolerance, RevisionCacheForm,
    RevisionSurfaceParameterization,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct SurfaceCacheWire {
    cache: RevisionCacheForm,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct CurveCacheWire {
    cache: RevisionCacheForm<CacheFirstCurveParameterization>,
}

#[test]
fn the_revision_surface_cache_is_one_tagged_object_carrying_its_own_tolerance() {
    let solved = SurfaceCacheWire {
        cache: RevisionCacheForm::SolvedCache {
            fit_tolerance: FitTolerance::try_new(0.125).unwrap(),
        },
    };
    let wire = serde_json::to_value(&solved).unwrap();
    assert_eq!(
        wire,
        json!({"cache": {"kind": "solved_cache", "fit_tolerance": 0.125}})
    );
    assert_eq!(
        serde_json::from_value::<SurfaceCacheWire>(wire).unwrap(),
        solved
    );

    let parameterized = SurfaceCacheWire {
        cache: RevisionCacheForm::Parameterization(RevisionSurfaceParameterization::default()),
    };
    let wire = serde_json::to_value(&parameterized).unwrap();
    assert_eq!(wire["cache"]["kind"], "parameterization");
    assert_eq!(
        serde_json::from_value::<SurfaceCacheWire>(wire).unwrap(),
        parameterized
    );
}

#[test]
fn a_revision_cache_form_carries_no_key_of_the_other_form() {
    for wire in [
        json!({"cache": {"kind": "solved_cache"}}),
        json!({"cache": {"kind": "parameterization", "fit_tolerance": 0.125}}),
        json!({"cache": {"kind": "tail_enum"}}),
    ] {
        assert!(
            serde_json::from_value::<SurfaceCacheWire>(wire.clone()).is_err(),
            "{wire}"
        );
    }

    let bogus = json!({"cache": {"kind": "solved_cache", "fit_tolerance": 0.125, "zz_bogus": 1}});
    let error = serde_json::from_value::<SurfaceCacheWire>(bogus)
        .unwrap_err()
        .to_string();
    assert!(error.contains("zz_bogus"), "{error}");
}

#[test]
fn the_cache_first_curve_cache_takes_the_same_tagged_shape() {
    let solved = CurveCacheWire {
        cache: RevisionCacheForm::SolvedCache {
            fit_tolerance: FitTolerance::try_new(0.0625).unwrap(),
        },
    };
    let wire = serde_json::to_value(&solved).unwrap();
    assert_eq!(
        wire,
        json!({"cache": {"kind": "solved_cache", "fit_tolerance": 0.0625}})
    );
    assert_eq!(
        serde_json::from_value::<CurveCacheWire>(wire).unwrap(),
        solved
    );

    let parameterized = CurveCacheWire {
        cache: RevisionCacheForm::Parameterization(CacheFirstCurveParameterization::default()),
    };
    let wire = serde_json::to_value(&parameterized).unwrap();
    assert_eq!(wire["cache"]["kind"], "parameterization");
    assert_eq!(
        serde_json::from_value::<CurveCacheWire>(wire).unwrap(),
        parameterized
    );

    let bogus = json!({"cache": {"kind": "solved_cache", "fit_tolerance": 0.0625, "zz_bogus": 1}});
    let error = serde_json::from_value::<CurveCacheWire>(bogus)
        .unwrap_err()
        .to_string();
    assert!(error.contains("zz_bogus"), "{error}");
}
