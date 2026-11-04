// SPDX-License-Identifier: Apache-2.0
//! Unique paired entity and object selection.

use super::entity_record::CatiaEntityRecord;
use super::{CatiaEntityReference, CatiaObjectRecord};
use crate::unique_index::UniqueIndex;

pub(super) struct PairedRecords<'e, 'o> {
    entities: UniqueIndex<(&'e str, u32), &'e CatiaEntityRecord>,
    entity_ids: UniqueIndex<&'e str, &'e CatiaEntityRecord>,
    objects: UniqueIndex<&'o str, &'o CatiaObjectRecord>,
    object_entities: UniqueIndex<(&'o str, u32), &'o CatiaObjectRecord>,
}

impl<'e, 'o> PairedRecords<'e, 'o> {
    pub(super) fn new(
        entities: &'e [CatiaEntityRecord],
        objects: &[&'o CatiaObjectRecord],
    ) -> Self {
        Self {
            entities: UniqueIndex::collect(
                entities
                    .iter()
                    .map(|e| ((e.object_graph.as_str(), e.entity_id), e)),
            ),
            entity_ids: UniqueIndex::collect(entities.iter().map(|e| (e.id.as_str(), e))),
            objects: UniqueIndex::collect(objects.iter().map(|o| (o.id.as_str(), *o))),
            object_entities: UniqueIndex::collect(objects.iter().filter_map(|o| {
                o.entity_id()
                    .map(|identity| ((o.parent.as_str(), identity), *o))
            })),
        }
    }

    pub(super) fn pair(
        &self,
        graph: &str,
        identity: u32,
    ) -> Option<(&'e CatiaEntityRecord, &'o CatiaObjectRecord)> {
        let entity = self.entities.get(&(graph, identity)).copied()?;
        self.entity_ids.get(entity.id.as_str())?;
        let object = self.objects.get(entity.object_record.as_str()).copied()?;
        let selected = self.object_entities.get(&(graph, identity)).copied()?;
        (object.id == selected.id
            && object.parent == graph
            && object.entity_id() == Some(identity)
            && object.entity_record() == Some(entity.id.as_str()))
        .then_some((entity, object))
    }

    pub(super) fn target(
        &self,
        graph: &str,
        reference: &CatiaEntityReference,
    ) -> Option<(&'e CatiaEntityRecord, &'o CatiaObjectRecord)> {
        let pair = self.pair(graph, reference.entity_id())?;
        (reference.entity() == Some(pair.0.id.as_str())
            && reference.class_name() == pair.1.class_name())
        .then_some(pair)
    }
}
