// SPDX-License-Identifier: Apache-2.0
//! Join live Body-container membership to component definitions.

use std::collections::BTreeMap;

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

use crate::records::body_ownership::{
    membership, terminal_membership, BodyOwnership, CONTAINER_BASE_GUID, CONTAINER_GUID,
};
use crate::records::entity_header::SegmentTypeData;

pub(crate) enum OwnerResolution {
    Admitted(BodyOwnership),
    Unadmitted,
    Conflicting,
}

impl OwnerResolution {
    pub(crate) fn admitted(&self) -> Option<&BodyOwnership> {
        match self {
            Self::Admitted(owner) => Some(owner),
            Self::Unadmitted | Self::Conflicting => None,
        }
    }

    fn merge(&mut self, incoming: Self, body: u64) {
        let Self::Admitted(owner) = incoming else {
            return;
        };
        match self {
            Self::Unadmitted => *self = Self::Admitted(owner),
            Self::Admitted(previous)
                if previous.component_for(body) != owner.component_for(body) =>
            {
                *self = Self::Conflicting;
            }
            Self::Admitted(_) | Self::Conflicting => {}
        }
    }
}

pub(crate) fn ownership_type(data: &SegmentTypeData) -> bool {
    [
        CONTAINER_GUID,
        crate::records::body_ownership::COMPONENT_GUID,
        crate::design::presentation::BODY_PRESENTATION_TYPE_GUID,
    ]
    .iter()
    .any(|guid| data.type_guid.as_str().eq_ignore_ascii_case(guid))
}

pub(crate) fn supported_container(data: &SegmentTypeData) -> bool {
    data.module == "Body"
        && data.version == 4
        && data.type_guid.as_str().eq_ignore_ascii_case(CONTAINER_GUID)
        && data
            .base_type_guid
            .value()
            .is_some_and(|base| base.as_str().eq_ignore_ascii_case(CONTAINER_BASE_GUID))
}

pub(crate) fn valid_owner<'a>(
    owner: &BodyOwnership,
    body: u64,
    lookup: impl Fn(u64) -> Option<&'a SegmentTypeData>,
) -> bool {
    let Some(component) = owner.component_for(body) else {
        return false;
    };
    lookup(owner.container_record_index).is_some_and(supported_container)
        && lookup(component).is_some_and(|data| {
            data.type_guid
                .as_str()
                .eq_ignore_ascii_case(crate::records::body_ownership::COMPONENT_GUID)
        })
        && lookup(body).is_some_and(|data| {
            data.type_guid
                .as_str()
                .eq_ignore_ascii_case(crate::design::presentation::BODY_PRESENTATION_TYPE_GUID)
        })
}

pub(crate) fn owners(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    meta: &crate::metastream::MetaStream,
) -> Result<BTreeMap<u64, OwnerResolution>, CodecError> {
    let mut result: BTreeMap<u64, OwnerResolution> = BTreeMap::new();
    let mut registered: BTreeMap<u64, Option<&SegmentTypeData>> = BTreeMap::new();
    for data in meta.types.iter().filter(|data| ownership_type(data)) {
        for entity in data.entities.values() {
            if !registered.contains_key(entity) {
                ctx.retain_bytes(
                    cadmpeg_core::decode::u64_from_index(std::mem::size_of::<(
                        u64,
                        Option<&SegmentTypeData>,
                    )>()),
                    "Fusion component ownership type index",
                )?;
            }
            registered
                .entry(*entity)
                .and_modify(|entry| *entry = None)
                .or_insert(Some(data));
        }
    }
    let lookup = |entity| registered.get(&entity).copied().flatten();
    for frame in super::meta::design_primary_frames(bytes, meta)? {
        if !supported_container(frame.design_type) {
            continue;
        }
        let record = &bytes[frame.start..frame.end];
        let Some(start) = terminal_membership(record) else {
            continue;
        };
        let tail = &record[start..];
        let Some(join) = membership(tail) else {
            continue;
        };
        for (ordinal, body) in join.bodies().enumerate() {
            let Some(owner) = join.proof(
                frame.entity_id,
                cadmpeg_core::decode::u64_from_index(frame.start + start),
                ordinal,
            ) else {
                continue;
            };
            let incoming = if valid_owner(&owner, body, lookup)
                && owner.component_for(body) == Some(join.component)
            {
                OwnerResolution::Admitted(owner)
            } else {
                OwnerResolution::Unadmitted
            };
            ctx.retain_bytes(
                cadmpeg_core::decode::u64_from_index(std::mem::size_of::<OwnerResolution>()),
                "Fusion component body membership",
            )?;
            match result.entry(body) {
                std::collections::btree_map::Entry::Vacant(entry) => {
                    entry.insert(incoming);
                }
                std::collections::btree_map::Entry::Occupied(mut entry) => {
                    entry.get_mut().merge(incoming, body);
                }
            }
        }
    }
    Ok(result)
}
