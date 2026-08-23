use std::collections::{HashMap, HashSet};

use super::EntityRecord;

/// Return bridge attributes carried by a complete compact canonical-face set.
pub(super) fn bridge_refs(
    by_attr: &HashMap<u16, &EntityRecord>,
    face_attrs: &HashSet<u16>,
) -> Option<Vec<u16>> {
    if face_attrs.iter().any(|attr| {
        by_attr
            .get(attr)
            .is_none_or(|face| face.disc != 0x0014 || face.flo() != 2)
    }) {
        return None;
    }
    let bridge_refs = face_attrs
        .iter()
        .filter_map(|attr| {
            by_attr
                .get(attr)
                .and_then(|face| face.refs.first().copied())
                .filter(|reference| *reference > 1)
        })
        .collect::<Vec<_>>();
    if bridge_refs.len() != face_attrs.len()
        || bridge_refs.iter().collect::<HashSet<_>>().len() != bridge_refs.len()
    {
        return None;
    }
    Some(bridge_refs)
}
