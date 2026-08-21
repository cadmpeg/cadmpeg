//! Tests for the `transforms` module.

use super::*;
use crate::records::{SketchInputEntity, SketchInputKind};

fn marker(id: &str, coordinates_m: Option<[f64; 2]>) -> SketchInputEntity {
    SketchInputEntity {
        id: id.into(),
        parent: "lane".into(),
        feature_ref: Some("feature-native".into()),
        ordinal: 0,
        offset: 0,
        object_index: None,
        local_id: None,
        kind: SketchInputKind::Point,
        state_value: None,
        coordinates_m,
        links: Vec::new(),
        link_selector: None,
    }
}

mod frames;
mod join;
mod patterns;
mod profile;
mod relation_geometry;
mod relation_links;
mod relation_operands;
mod selection;
