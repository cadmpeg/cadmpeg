// SPDX-License-Identifier: Apache-2.0
use crate::geometry::{RevisionSurfaceParameterization, VariableBlendCache};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::num::NonZeroI64;

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct CacheWire {
    cache: VariableBlendCache,
}

#[test]
fn the_variable_blend_cache_is_one_tagged_object_carrying_its_own_tolerance() {
    for prefix in [-3, 1, 11] {
        let cache = VariableBlendCache::Current {
            shape_prefix: NonZeroI64::new(prefix).unwrap(),
            fit_tolerance: crate::geometry::FitTolerance::try_new(0.125).unwrap(),
        };
        let wire = serde_json::to_value(CacheWire {
            cache: cache.clone(),
        })
        .unwrap();
        assert_eq!(
            wire,
            json!({"cache": {
                "kind": "current",
                "shape_prefix": prefix,
                "fit_tolerance": 0.125
            }})
        );
        assert_eq!(
            serde_json::from_value::<CacheWire>(wire).unwrap().cache,
            cache
        );
    }

    let stale = CacheWire {
        cache: VariableBlendCache::Stale {},
    };
    let wire = serde_json::to_value(&stale).unwrap();
    assert_eq!(wire, json!({"cache": {"kind": "stale"}}));
    assert_eq!(serde_json::from_value::<CacheWire>(wire).unwrap(), stale);

    for prefix in [-1, 0, 11] {
        let cache = CacheWire {
            cache: VariableBlendCache::Parameterization {
                shape_prefix: prefix,
                parameterization: RevisionSurfaceParameterization::default(),
            },
        };
        let wire = serde_json::to_value(&cache).unwrap();
        assert_eq!(wire["cache"]["kind"], "parameterization");
        assert_eq!(wire["cache"]["shape_prefix"], prefix);
        assert_eq!(serde_json::from_value::<CacheWire>(wire).unwrap(), cache);
    }
}

#[test]
fn a_variable_blend_cache_carries_no_key_of_another_form() {
    for wire in [
        json!({"cache": {"kind": "stale", "fit_tolerance": 0.125}}),
        json!({"cache": {"kind": "current", "shape_prefix": 1}}),
        json!({"cache": {"kind": "current", "shape_prefix": 0, "fit_tolerance": 0.125}}),
        json!({"cache": {"kind": "parameterization", "shape_prefix": 1}}),
        json!({"cache": {
            "kind": "stale",
            "parameterization": RevisionSurfaceParameterization::default()
        }}),
        json!({"cache": {
            "kind": "parameterization",
            "shape_prefix": 0,
            "parameterization": RevisionSurfaceParameterization::default(),
            "fit_tolerance": 0.125
        }}),
    ] {
        assert!(
            serde_json::from_value::<CacheWire>(wire.clone()).is_err(),
            "{wire}"
        );
    }

    let bogus = json!({"cache": {"kind": "stale", "zz_bogus": 1}});
    let error = serde_json::from_value::<CacheWire>(bogus)
        .unwrap_err()
        .to_string();
    assert!(error.contains("zz_bogus"), "{error}");
}
