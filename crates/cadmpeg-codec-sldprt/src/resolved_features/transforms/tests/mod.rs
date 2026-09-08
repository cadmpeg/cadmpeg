//! Tests for the `transforms` module.

use super::*;
use crate::records::{SketchInputEntity, SketchInputKind};

fn marker(id: &str, coordinates_m: Option<[f64; 2]>) -> SketchInputEntity {
    {
        let marker_id: String = id.into();
        let marker_parent: String = "lane".into();
        let mut constructed_marker = crate::records::SketchInputEntity::new(
            marker_id,
            marker_parent,
            0,
            0,
            SketchInputKind::Point,
        );
        constructed_marker.feature_ref = Some("feature-native".into());
        constructed_marker.object_index = None;
        constructed_marker.local_id = None;
        constructed_marker.state_value = None;
        constructed_marker.coordinates_m = coordinates_m;
        constructed_marker.links = None;
        constructed_marker
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
