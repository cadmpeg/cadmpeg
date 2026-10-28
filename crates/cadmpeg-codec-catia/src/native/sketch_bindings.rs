// SPDX-License-Identifier: Apache-2.0
//! Source membership and ordered local point-value incidences.

use super::entity_record::CatiaEntityRecord;
use super::paired_records::PairedRecords;
use super::{CatiaDesignObject, CatiaObjectGraph, CatiaObjectRecord};
use crate::entity_table::NumericPairSlot;
use crate::object_graph::{HeadToken, ListItem, PayloadField};
use crate::unique_index::UniqueIndex;
use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use cadmpeg_core::CodecError;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

const RETAIN_BINDING: &str = "retain CATIA sketch source bindings";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CatiaSketchMemberTarget {
    pub(crate) entity: String,
    pub(crate) object: String,
    pub(crate) group: String,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_sketch_child"
    )]
    pub(crate) child: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CatiaSketchMember {
    pub(crate) ordinal: u64,
    pub(crate) payload_offset: u64,
    pub(crate) entity_id: u32,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_sketch_target"
    )]
    pub(crate) target: Option<CatiaSketchMemberTarget>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CatiaSketchMembership {
    pub(crate) source_entity: String,
    pub(crate) source_object: String,
    pub(crate) list_payload_offset: u64,
    pub(crate) declared_count: u32,
    pub(crate) items: Vec<CatiaSketchMember>,
    pub(crate) groups: Vec<String>,
    pub(crate) children: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CatiaLocalPointSlot {
    pub(crate) bits: u64,
    pub(crate) payload_offset: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CatiaLocalPointPair {
    pub(crate) point_entity: String,
    pub(crate) point_object: String,
    pub(crate) coordinate_entity: String,
    pub(crate) coordinate_object: String,
    pub(crate) slots: [CatiaLocalPointSlot; 2],
}

struct Groups<'a> {
    ids: UniqueIndex<&'a str, &'a CatiaDesignObject>,
    owners: UniqueIndex<(&'a str, u32), &'a CatiaDesignObject>,
    fields: HashMap<(&'a str, u32), Vec<&'a CatiaObjectRecord>>,
}

impl<'a> Groups<'a> {
    fn new(groups: &'a [CatiaDesignObject], objects: &[&'a CatiaObjectRecord]) -> Self {
        let mut fields: HashMap<(&str, u32), Vec<&CatiaObjectRecord>> = HashMap::new();
        for object in objects {
            if let Some(owner) = object.owner_entity_id() {
                fields
                    .entry((&object.parent, owner))
                    .or_default()
                    .push(object);
            }
        }
        Self {
            ids: UniqueIndex::collect(groups.iter().map(|g| (g.id.as_str(), g))),
            owners: UniqueIndex::collect(
                groups
                    .iter()
                    .map(|g| ((g.parent.as_str(), g.owner_entity_id), g)),
            ),
            fields,
        }
    }

    fn owner(&self, graph: &str, identity: u32) -> Option<&'a CatiaDesignObject> {
        let group = *self.owners.get(&(graph, identity))?;
        self.ids.get(group.id.as_str())?;
        let fields = self.fields.get(&(graph, identity))?;
        (fields.len() == group.fields.len()
            && fields.iter().zip(&group.fields).all(|(record, id)| {
                record.id == *id && record.design_object.as_deref() == Some(group.id.as_str())
            }))
        .then_some(group)
    }

    fn containing(&self, object: &CatiaObjectRecord) -> Option<&'a CatiaDesignObject> {
        let group = self.owner(&object.parent, object.owner_entity_id()?)?;
        (object.design_object.as_deref() == Some(group.id.as_str())
            && group.fields.contains(&object.id))
        .then_some(group)
    }
}

fn named_separator(record: &CatiaObjectRecord, name: &str) -> bool {
    let roles = crate::object_graph::head_roles(record.lead, &record.head);
    let owner = match roles.owner {
        Some(crate::object_graph::HeadOwner::Entity(owner)) => Some(owner),
        _ => None,
    };
    matches!(record.head.first(), Some(HeadToken::Lead(lead)) if *lead == record.lead)
        && matches!(record.head.get(1), Some(HeadToken::Separator))
        && owner.is_some()
        && owner == record.owner_entity_id()
        && roles.class_ref == record.class_ref()
        && record.class_name() == Some(name)
        && record.class_entry().is_some()
}

fn retain<T>(ctx: &DecodeContext<'_>) -> Result<(), CodecError> {
    ctx.retain_bytes(u64_from_index(std::mem::size_of::<T>()), RETAIN_BINDING)?;
    Ok(())
}

fn text(ctx: &DecodeContext<'_>, value: &str) -> Result<String, CodecError> {
    ctx.retain_bytes(u64_from_index(value.len()), RETAIN_BINDING)?;
    Ok(value.to_owned())
}

fn summary(ctx: &DecodeContext<'_>, values: &mut Vec<String>, id: &str) -> Result<(), CodecError> {
    if !values.iter().any(|value| value == id) {
        retain::<String>(ctx)?;
        values.push(text(ctx, id)?);
    }
    Ok(())
}

fn member_target(
    ctx: &DecodeContext<'_>,
    graph: &str,
    identity: u32,
    pairs: &PairedRecords<'_, '_>,
    groups: &Groups<'_>,
) -> Result<Option<CatiaSketchMemberTarget>, CodecError> {
    let Some((entity, object)) = pairs.pair(graph, identity) else {
        return Ok(None);
    };
    let Some(group) = groups.containing(object) else {
        return Ok(None);
    };
    let child = groups
        .owner(graph, identity)
        .filter(|child| child.owner_record.as_deref() == Some(object.id.as_str()));
    retain::<CatiaSketchMemberTarget>(ctx)?;
    Ok(Some(CatiaSketchMemberTarget {
        entity: text(ctx, &entity.id)?,
        object: text(ctx, &object.id)?,
        group: text(ctx, &group.id)?,
        child: child.map(|child| text(ctx, &child.id)).transpose()?,
    }))
}

fn membership(
    ctx: &DecodeContext<'_>,
    group: &CatiaDesignObject,
    pairs: &PairedRecords<'_, '_>,
    groups: &Groups<'_>,
) -> Result<Option<Box<CatiaSketchMembership>>, CodecError> {
    let Some((entity, object)) = pairs.pair(&group.parent, group.owner_entity_id) else {
        return Ok(None);
    };
    if group.owner_record.as_deref() != Some(object.id.as_str())
        || !named_separator(object, "Sketch")
    {
        return Ok(None);
    }
    let Some(class) = &group.owner_class else {
        return Ok(None);
    };
    if class.name != "Sketch" || Some(class.entry.as_str()) != object.class_entry() {
        return Ok(None);
    }
    let Some(PayloadField::List {
        declared_count,
        items,
        offset: 0,
    }) = object.payload.fields.first()
    else {
        return Ok(None);
    };
    if usize::try_from(*declared_count).ok() != Some(items.len())
        || items
            .iter()
            .any(|item| !matches!(item, ListItem::Reference { .. }))
    {
        return Ok(None);
    }
    retain::<CatiaSketchMembership>(ctx)?;
    let mut result = CatiaSketchMembership {
        source_entity: text(ctx, &entity.id)?,
        source_object: text(ctx, &object.id)?,
        list_payload_offset: 0,
        declared_count: *declared_count,
        items: Vec::new(),
        groups: Vec::new(),
        children: Vec::new(),
    };
    for (ordinal, item) in items.iter().enumerate() {
        let ListItem::Reference { value, offset } = item else {
            return Ok(None);
        };
        retain::<CatiaSketchMember>(ctx)?;
        let target = member_target(ctx, &group.parent, *value, pairs, groups)?;
        if let Some(target) = &target {
            summary(ctx, &mut result.groups, &target.group)?;
            if let Some(child) = &target.child {
                summary(ctx, &mut result.children, child)?;
            }
        }
        result.items.push(CatiaSketchMember {
            ordinal: u64_from_index(ordinal),
            payload_offset: u64_from_index(*offset),
            entity_id: *value,
            target,
        });
    }
    Ok(Some(Box::new(result)))
}

fn finite_slot(slot: &NumericPairSlot) -> Option<CatiaLocalPointSlot> {
    match slot {
        NumericPairSlot::Binary64 { bits, offset } if f64::from_bits(*bits).is_finite() => {
            Some(CatiaLocalPointSlot {
                bits: *bits,
                payload_offset: u64_from_index(*offset),
            })
        }
        NumericPairSlot::Binary64 { .. } | NumericPairSlot::ControlE8 { .. } => None,
    }
}

fn point_pairs(
    ctx: &DecodeContext<'_>,
    group: &CatiaDesignObject,
    pairs: &PairedRecords<'_, '_>,
    groups: &Groups<'_>,
) -> Result<Vec<CatiaLocalPointPair>, CodecError> {
    let mut result = Vec::new();
    let Some((entity, owner)) = pairs.pair(&group.parent, group.owner_entity_id) else {
        return Ok(result);
    };
    if group.owner_record.as_deref() != Some(owner.id.as_str()) {
        return Ok(result);
    }
    let Some(pair) = entity.numeric_pair() else {
        return Ok(result);
    };
    let (Some(first), Some(second)) = (finite_slot(&pair.slots[0]), finite_slot(&pair.slots[1]))
    else {
        return Ok(result);
    };
    for object in groups
        .fields
        .get(&(group.parent.as_str(), group.owner_entity_id))
        .into_iter()
        .flatten()
    {
        if !named_separator(object, "2DPoint") {
            continue;
        }
        let Some((point, selected)) = object
            .entity_id()
            .and_then(|id| pairs.pair(&group.parent, id))
        else {
            continue;
        };
        if selected.id != object.id {
            continue;
        }
        retain::<CatiaLocalPointPair>(ctx)?;
        result.push(CatiaLocalPointPair {
            point_entity: text(ctx, &point.id)?,
            point_object: text(ctx, &object.id)?,
            coordinate_entity: text(ctx, &entity.id)?,
            coordinate_object: text(ctx, &owner.id)?,
            slots: [first.clone(), second.clone()],
        });
    }
    Ok(result)
}

pub(super) fn bind(
    ctx: &DecodeContext<'_>,
    graphs: &[CatiaObjectGraph],
    entities: &[CatiaEntityRecord],
    designs: &mut [CatiaDesignObject],
) -> Result<(), CodecError> {
    for group in designs.iter_mut() {
        group.sketch_membership = None;
        group.local_point_pairs.clear();
    }
    if !graphs
        .iter()
        .flat_map(|g| &g.records)
        .any(|r| named_separator(r, "Sketch") || named_separator(r, "2DPoint"))
    {
        return Ok(());
    }
    let objects = graphs.iter().flat_map(|g| &g.records).collect::<Vec<_>>();
    let pairs = PairedRecords::new(entities, &objects);
    let groups = Groups::new(designs, &objects);
    let derived = designs
        .iter()
        .map(|group| {
            if groups.owner(&group.parent, group.owner_entity_id).is_none() {
                return Ok((None, Vec::new()));
            }
            Ok((
                membership(ctx, group, &pairs, &groups)?,
                point_pairs(ctx, group, &pairs, &groups)?,
            ))
        })
        .collect::<Result<Vec<_>, CodecError>>()?;
    for (group, (membership, points)) in designs.iter_mut().zip(derived) {
        group.sketch_membership = membership;
        group.local_point_pairs = points;
    }
    Ok(())
}

#[cfg(test)]
mod tests;

cadmpeg_core::named_optional_field!(deserialize_sketch_child, String, "child");
cadmpeg_core::named_optional_field!(deserialize_sketch_target, CatiaSketchMemberTarget, "target");
