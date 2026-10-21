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

#[test]
fn absent_revision_cache_round_trips_without_admitting_an_unfitted_solved_cache() {
    let absent = SurfaceCacheWire {
        cache: RevisionCacheForm::Absent {},
    };
    let wire = serde_json::to_value(&absent).expect("serialize absent cache");
    assert_eq!(wire, json!({"cache": {"kind": "absent"}}));
    assert_eq!(
        serde_json::from_value::<SurfaceCacheWire>(wire).expect("read absent cache"),
        absent
    );
    for invalid in [
        json!({"cache": {"kind": "solved_cache"}}),
        json!({"cache": {"kind": "absent", "fit_tolerance": 0.125}}),
    ] {
        assert!(serde_json::from_value::<SurfaceCacheWire>(invalid).is_err());
    }
}

#[test]
fn discarding_a_solved_revision_cache_keeps_every_intrinsic_construction_field() {
    use crate::geometry::{CacheContract, ProceduralSurfaceDefinition, RevisionSurfaceForm};
    let form = RevisionSurfaceForm {
        revision: crate::scalar::PositiveI64::new(23100).expect("positive revision"),
        support_bounds: [Some(-2.0), None, Some(4.0), None],
        reference_endpoints: [Some(0.25), Some(0.75)],
        second_endpoints: [Some(-1.0), Some(2.0)],
        flags: vec![true, false],
        cache: RevisionCacheForm::SolvedCache {
            fit_tolerance: FitTolerance::try_new(0.125).expect("positive original fit"),
        },
        discontinuities: [
            vec![0.25],
            vec![],
            vec![0.75],
            vec![-1.0],
            vec![],
            vec![2.0],
        ],
        tail_flag: true,
        trailing_flags: vec![false, true],
    };
    let mut construction = ProceduralSurfaceDefinition::Extrusion(
        crate::geometry::surface_payloads::ExtrusionSurfaceConstruction::try_new(
            crate::ids::CurveId::mint("test:model:curve#directrix").expect("curve identity"),
            Some([0.25, 0.75]),
            crate::math::Vector3::new(0.0, 0.0, 20.0),
            Some(crate::math::Point3::new(40.0, 50.0, 60.0)),
            CacheContract::from_form(Some(form)),
        )
        .expect("admit complete revision construction"),
    );
    assert!(construction.set_cache_fit_tolerance(None).is_err());
    let mut expected = serde_json::to_value(&construction).expect("original complete construction");
    expected["cache"]["form"]["cache"] = json!({"kind": "absent"});
    construction
        .clear_fitted_cache_contract()
        .expect("remove the rejected cache contract");
    assert_eq!(construction.cache_fit_tolerance(), None);
    assert_eq!(
        serde_json::to_value(&construction).expect("current construction"),
        expected
    );
    assert_eq!(
        serde_json::from_value::<ProceduralSurfaceDefinition>(expected)
            .expect("read current intrinsic fields"),
        construction
    );
    construction
        .clear_fitted_cache_contract()
        .expect("removal is idempotent");
    assert_eq!(construction.cache_fit_tolerance(), None);
}
