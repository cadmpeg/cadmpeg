// SPDX-License-Identifier: Apache-2.0
//! Ergonomic builders for invariant-bearing IR entities in tests.
//!
//! Production types have no public [`Default`] when an empty id would be
//! illegal. Tests may invent placeholder identities here.

use std::collections::BTreeMap;

use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureId};
use cadmpeg_ir::sketches::{
    SketchEntity, SketchEntityId, SketchGeometry, SketchId, SpatialSketchEntity,
    SpatialSketchEntityId, SpatialSketchGeometry, SpatialSketchId,
};

/// Build a [`Feature`] with a placeholder id and a native definition kind.
pub fn feature(ordinal: u64, kind: &str) -> Feature {
    Feature::new(
        FeatureId(format!("test:feature#{ordinal}")),
        ordinal,
        FeatureDefinition::Native {
            kind: kind.to_owned(),
            parameters: BTreeMap::new(),
            properties: BTreeMap::new(),
        },
    )
}

/// Build a [`SketchEntity`] with placeholder ids and the given geometry.
pub fn sketch_entity(index: u64, geometry: SketchGeometry) -> SketchEntity {
    SketchEntity::new(
        SketchEntityId(format!("test:sketch-entity#{index}")),
        SketchId("test:sketch#0".into()),
        geometry,
    )
}

/// Build a [`SpatialSketchEntity`] with placeholder ids and the given geometry.
pub fn spatial_sketch_entity(index: u64, geometry: SpatialSketchGeometry) -> SpatialSketchEntity {
    SpatialSketchEntity::new(
        SpatialSketchEntityId(format!("test:spatial-sketch-entity#{index}")),
        SpatialSketchId("test:spatial-sketch#0".into()),
        geometry,
    )
}
