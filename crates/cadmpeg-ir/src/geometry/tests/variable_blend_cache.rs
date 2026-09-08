// SPDX-License-Identifier: Apache-2.0
use crate::geometry::{RevisionSurfaceParameterization, VariableBlendCache};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::num::NonZeroI64;

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct CacheWire {
    #[serde(flatten, with = "crate::geometry::variable_blend_cache_wire")]
    cache: VariableBlendCache,
}

#[test]
fn variable_blend_cache_preserves_native_prefix_and_flat_wire() {
    for prefix in [-3, 1, 11] {
        let cache = VariableBlendCache::Current {
            shape_prefix: NonZeroI64::new(prefix).unwrap(),
            fit_tolerance: crate::geometry::FitTolerance::try_new(0.125).unwrap(),
        };
        let mut wire = serde_json::to_value(CacheWire {
            cache: cache.clone(),
        })
        .unwrap();
        assert_eq!(wire, json!({ "shape_prefix": prefix, "tail_enum": 0 }));
        // ProceduralSurface owns the outer tolerance and injects it on read.
        wire["cache_fit_tolerance"] = json!(0.125);
        assert_eq!(
            serde_json::from_value::<CacheWire>(wire).unwrap().cache,
            cache
        );
    }
    let stale = CacheWire {
        cache: VariableBlendCache::Stale,
    };
    let wire = serde_json::to_value(&stale).unwrap();
    assert_eq!(wire, json!({ "shape_prefix": 0, "tail_enum": 0 }));
    assert_eq!(serde_json::from_value::<CacheWire>(wire).unwrap(), stale);
    for prefix in [-1, 0, 11] {
        let cache = CacheWire {
            cache: VariableBlendCache::Parameterization {
                shape_prefix: prefix,
                parameterization: RevisionSurfaceParameterization::default(),
            },
        };
        let wire = serde_json::to_value(&cache).unwrap();
        assert_eq!(wire["shape_prefix"], prefix);
        assert_eq!(wire["tail_enum"], 2);
        assert_eq!(serde_json::from_value::<CacheWire>(wire).unwrap(), cache);
    }
}

#[test]
fn variable_blend_cache_rejects_contradictory_legacy_fields() {
    for wire in [
        json!({"shape_prefix": 0, "tail_enum": 0, "cache_fit_tolerance": 0.125}),
        json!({"shape_prefix": 1, "tail_enum": 0}),
        json!({"shape_prefix": -1, "tail_enum": 0}),
        json!({"shape_prefix": 1, "tail_enum": 2}),
        json!({"shape_prefix": 0, "tail_enum": 0, "tail_parameterization": RevisionSurfaceParameterization::default()}),
        json!({"shape_prefix": 0, "tail_enum": 2, "tail_parameterization": RevisionSurfaceParameterization::default(), "cache_fit_tolerance": 0.125}),
    ] {
        assert!(serde_json::from_value::<CacheWire>(wire).is_err());
    }
}
