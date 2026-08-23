// SPDX-License-Identifier: Apache-2.0
//! Final-state body selectors carried by Parasolid deltas entities.

use super::{bodies, scan_stream_entities, BodyRecord};

/// Reconstruct the final body records carried by explicit deltas body roots.
///
/// The body root is the native membership relation. Its transitive references
/// may name unchanged partition entities and new delta entities, so selection
/// runs over the combined entity table. A body with no matching deltas root is
/// not a final selector.
pub(crate) fn scan_final_body_selectors(
    streams: &[(&[u8], &str, bool)],
) -> Option<Vec<BodyRecord>> {
    let (entities, delta_body_attrs) = scan_stream_entities(streams);
    if delta_body_attrs.is_empty() {
        return None;
    }
    Some(
        bodies(&entities)
            .0
            .into_iter()
            .filter(|body| delta_body_attrs.contains(&body.attr))
            .collect(),
    )
}
