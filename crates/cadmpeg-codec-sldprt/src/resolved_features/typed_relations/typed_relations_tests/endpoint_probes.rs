//! Endpoint probes reject unrecognized layouts and unlinked markers.

use super::super::{
    coordinate_centered_line_endpoints, extended_wide_selected_axis_endpoints,
    legacy_point_roster_line_endpoint_markers, one_based_point_roster_line_endpoint_markers,
};
use crate::records::{SketchInputEntity, SketchInputKind};

#[test]
fn rejected_endpoint_layouts_resolve_no_endpoints() {
    let entities = (0..1024)
        .map(|i| {
            SketchInputEntity::new(
                format!("synthetic:marker#{i}"),
                "lane",
                0,
                0,
                SketchInputKind::LineOrCircle,
            )
        })
        .collect::<Vec<_>>();
    let markers = entities.iter().collect::<Vec<_>>();
    let owner = &entities[0];
    assert!(extended_wide_selected_axis_endpoints(&[], owner, &markers).is_none());
    assert!(one_based_point_roster_line_endpoint_markers(&[], owner, &markers).is_none());
    assert!(legacy_point_roster_line_endpoint_markers(&[], owner, &markers).is_none());
    assert!(coordinate_centered_line_endpoints(&[], owner, &markers).is_none());
}

#[test]
fn unlinked_markers_resolve_no_line_endpoints() {
    let entities = (0..1000)
        .map(|i| {
            SketchInputEntity::new(
                format!("synthetic:marker:unlinked#{i}"),
                "lane",
                0,
                0,
                SketchInputKind::LineOrCircle,
            )
        })
        .collect::<Vec<_>>();
    let markers = entities
        .iter()
        .map(|marker| (marker.id(), marker))
        .collect();
    assert!(super::super::line_endpoint_markers(&entities[0], &markers).is_empty());
}
