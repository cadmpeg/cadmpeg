//! Current- and extended-prefix zero-tail profile-curve tests.

use super::super::super::typed_relations::marker_curve_endpoint_markers;
use super::super::super::{LEGACY_EXTENDED_SKETCH_MARKER, LEGACY_SKETCH_MARKER, SKETCH_MARKER};
use super::super::*;
use crate::layout::current_extended_zero_tail_92_profile_curve as zero_tail_92;
use crate::records::{SketchInputEntity, SketchInputKind};
use std::collections::HashMap;

#[test]
fn current_extended_zero_tail_92_profile_curve_uses_coordinate_roster() {
    let mut payload = vec![0; zero_tail_92::LEN];
    payload[zero_tail_92::HEADER..zero_tail_92::NATIVE_KIND]
        .copy_from_slice(&zero_tail_92::HEADER_VALUE);
    payload[zero_tail_92::NATIVE_KIND..zero_tail_92::NATIVE_KIND + 4]
        .copy_from_slice(&2u32.to_le_bytes());
    payload[zero_tail_92::PROFILE_LOCUS..zero_tail_92::ROLE]
        .copy_from_slice(&zero_tail_92::PROFILE_LOCUS_VALUE);
    payload[zero_tail_92::ROLE..zero_tail_92::STATE]
        .copy_from_slice(&zero_tail_92::ROLE_VALUE.to_le_bytes());
    payload[zero_tail_92::STATE..zero_tail_92::SELECTOR]
        .copy_from_slice(&zero_tail_92::STATE_VALUE.to_le_bytes());
    payload[zero_tail_92::SELECTOR..zero_tail_92::SELECTOR + 8]
        .copy_from_slice(&zero_tail_92::SELECTOR_VALUE);
    payload[zero_tail_92::STATE_SCALAR..zero_tail_92::ZERO_ENDPOINT_PREFIX]
        .copy_from_slice(&zero_tail_92::STATE_SCALAR_VALUE.to_le_bytes());
    payload[zero_tail_92::ENDPOINT_FIRST..zero_tail_92::ENDPOINT_SECOND]
        .copy_from_slice(&2u16.to_le_bytes());
    payload[zero_tail_92::ENDPOINT_SECOND..zero_tail_92::ENDPOINT_SELECTOR]
        .copy_from_slice(&0u16.to_le_bytes());
    payload[zero_tail_92::ENDPOINT_SELECTOR..zero_tail_92::SIGNED_SELECTOR]
        .copy_from_slice(&zero_tail_92::ENDPOINT_SELECTOR_VALUE.to_le_bytes());
    payload[zero_tail_92::SIGNED_SELECTOR..zero_tail_92::ZERO_TAIL]
        .copy_from_slice(&zero_tail_92::SIGNED_SELECTOR_VALUE.to_le_bytes());

    let entity = |id: &str, offset, coordinates_m| SketchInputEntity {
        id: id.into(),
        parent: "lane".into(),
        feature_ref: Some("feature".into()),
        ordinal: 0,
        offset,
        object_index: None,
        local_id: None,
        kind: SketchInputKind::Point,
        state_value: Some(1.0),
        coordinates_m,
        links: Vec::new(),
        link_selector: None,
    };
    let curve = SketchInputEntity {
        id: "curve".into(),
        parent: "lane".into(),
        feature_ref: Some("feature".into()),
        ordinal: 0,
        offset: 0,
        object_index: None,
        local_id: None,
        kind: SketchInputKind::LineOrCircle,
        state_value: Some(1.0),
        coordinates_m: None,
        links: Vec::new(),
        link_selector: None,
    };
    let first = entity("first", 10, Some([0.0, 0.0]));
    let second = entity("second", 20, Some([1.0, 0.0]));
    let third = entity("third", 30, Some([2.0, 0.0]));
    let markers = [&curve, &first, &second, &third];
    let markers_by_id = markers
        .iter()
        .map(|marker| (marker.id.as_str(), *marker))
        .collect::<HashMap<_, _>>();

    for prefix in [SKETCH_MARKER, LEGACY_EXTENDED_SKETCH_MARKER] {
        payload[..prefix.len()].copy_from_slice(prefix);
        for native_kind in 0u32..=2 {
            payload[zero_tail_92::NATIVE_KIND..zero_tail_92::NATIVE_KIND + 4]
                .copy_from_slice(&native_kind.to_le_bytes());
            assert!(current_extended_zero_tail_92_profile_curve(&payload, 0));
            assert_eq!(
                marker_curve_endpoint_markers(&payload, &curve, &markers_by_id, &markers)
                    .iter()
                    .map(|marker| marker.id.as_str())
                    .collect::<Vec<_>>(),
                ["third", "first"]
            );

            payload[zero_tail_92::ENDPOINT_FIRST..zero_tail_92::ENDPOINT_SECOND]
                .copy_from_slice(&3u16.to_le_bytes());
            assert!(current_extended_zero_tail_92_profile_curve(&payload, 0));
            assert!(
                marker_curve_endpoint_markers(&payload, &curve, &markers_by_id, &markers)
                    .is_empty()
            );

            payload[zero_tail_92::ENDPOINT_FIRST..zero_tail_92::ENDPOINT_SECOND]
                .copy_from_slice(&2u16.to_le_bytes());
            payload[zero_tail_92::ZERO_TAIL + 4..zero_tail_92::ZERO_TAIL + 8]
                .copy_from_slice(&1u32.to_le_bytes());
            assert!(!current_extended_zero_tail_92_profile_curve(&payload, 0));
            payload[zero_tail_92::ZERO_TAIL + 4..zero_tail_92::ZERO_TAIL + 8].fill(0);
        }

        payload[..LEGACY_SKETCH_MARKER.len()].copy_from_slice(LEGACY_SKETCH_MARKER);
        assert!(!current_extended_zero_tail_92_profile_curve(&payload, 0));
    }
}
