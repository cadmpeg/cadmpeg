// SPDX-License-Identifier: Apache-2.0
//! Registration admission for construction tracking wrappers and carriers.
use crate::metastream::MetaStream;
use crate::records::entity_header::SegmentTypeData;
use crate::records::references::DesignClassTag;
use crate::records::topology::construction::DesignConstructionTrackingPath;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Wrapper,
    Carrier,
}

pub(super) struct Registry {
    meta: Rc<MetaStream>,
    kinds: HashMap<usize, Kind>,
    entities: HashSet<(usize, u64)>,
}

fn registration(kind: &SegmentTypeData) -> Option<Kind> {
    if kind.module != "EntityTracking" {
        return None;
    }
    let base = kind.base_type_guid.value()?.as_str();
    if kind.version == 0
        && kind
            .type_guid
            .as_str()
            .eq_ignore_ascii_case("335CC7BD-BBE5-40AA-9E8C-5EC99D427A3A")
        && base.eq_ignore_ascii_case("5C069ACB-169F-4555-9792-815B4671DDA0")
    {
        Some(Kind::Wrapper)
    } else if kind.version == 2
        && kind
            .type_guid
            .as_str()
            .eq_ignore_ascii_case("2DEE34BE-FA70-4D54-979C-164D86FDA786")
        && base.eq_ignore_ascii_case("70F5E801-1E8E-436E-8A09-36487F8A2859")
    {
        Some(Kind::Carrier)
    } else {
        None
    }
}

impl Registry {
    pub(super) fn new(meta: Rc<MetaStream>) -> Self {
        let mut kinds = HashMap::new();
        let mut entities = HashSet::new();
        for (ordinal, row) in meta.types.iter().enumerate() {
            let Some(kind) = registration(row) else {
                continue;
            };
            kinds.insert(ordinal, kind);
            entities.extend(row.entities.values().map(|entity| (ordinal, *entity)));
        }
        Self {
            meta,
            kinds,
            entities,
        }
    }

    fn header(&self, class: &DesignClassTag, entity: u32, offset: u64, kind: Kind) -> bool {
        let Some(ordinal) = class.dynamic_ordinal() else {
            return false;
        };
        self.kinds.get(&ordinal) == Some(&kind)
            && self.entities.contains(&(ordinal, u64::from(entity)))
            && self
                .meta
                .records
                .binary_search_by_key(&offset, |record| record.bulk_offset)
                .is_ok_and(|index| self.meta.records[index].entity_id == u64::from(entity))
    }

    pub(super) fn supports(&self, path: &DesignConstructionTrackingPath) -> bool {
        self.header(
            &path.wrapper_class_tag,
            path.wrapper_record_index(),
            path.wrapper_byte_offset(),
            Kind::Wrapper,
        ) && self.header(
            &path.carrier_class_tag,
            path.carrier_record_index(),
            path.carrier_byte_offset(),
            Kind::Carrier,
        )
    }
}

#[cfg(test)]
mod tests;
