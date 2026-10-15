// SPDX-License-Identifier: Apache-2.0
//! Direct product ownership across representation and assembly boundaries.

use super::super::{named_parameter, topology::TopologyData, ValueExt};
use crate::parse::Exchange;
use cadmpeg_core::{decode::DecodeContext, CodecError};
use cadmpeg_ir::ids::BodyId;
use std::collections::{BTreeMap, BTreeSet};

pub(super) struct RepresentationOwnership<'a, 'ctx> {
    exchange: &'a Exchange,
    topology: &'a TopologyData,
    ctx: &'a DecodeContext<'ctx>,
    owners: BTreeMap<u64, BTreeSet<u64>>,
    children: BTreeMap<u64, BTreeSet<u64>>,
}

impl<'a, 'ctx> RepresentationOwnership<'a, 'ctx> {
    pub(super) fn build(
        exchange: &'a Exchange,
        topology: &'a TopologyData,
        shapes: &BTreeMap<u64, u64>,
        definitions: &BTreeMap<u64, u64>,
        ctx: &'a DecodeContext<'ctx>,
    ) -> RepresentationOwnership<'a, 'ctx> {
        let mut owners = BTreeMap::<u64, BTreeSet<u64>>::new();
        for (definition, representations) in super::definition_representations(exchange, shapes) {
            if !definitions.contains_key(&definition) {
                continue;
            }
            for representation in representations {
                owners.entry(representation).or_default().insert(definition);
            }
        }
        let mut children = BTreeMap::<u64, BTreeSet<u64>>::new();
        for (_, record) in exchange.entities("NEXT_ASSEMBLY_USAGE_OCCURRENCE") {
            let parent = named_parameter(record, "NEXT_ASSEMBLY_USAGE_OCCURRENCE", 3)
                .and_then(ValueExt::reference);
            let child = named_parameter(record, "NEXT_ASSEMBLY_USAGE_OCCURRENCE", 4)
                .and_then(ValueExt::reference);
            if let (Some(parent), Some(child)) = (parent, child) {
                children.entry(parent).or_default().insert(child);
            }
        }
        Self {
            exchange,
            topology,
            ctx,
            owners,
            children,
        }
    }

    pub(super) fn bodies(
        &self,
        definition: u64,
        representation: u64,
    ) -> Result<Vec<BodyId>, CodecError> {
        let ctx = self.ctx;
        let mut descendants = BTreeSet::new();
        let mut pending = ctx.alloc_filled(1, definition, "step ownership pending")?;
        while let Some(parent) = pending.pop() {
            if !descendants.insert(parent) {
                continue;
            }
            for &child in self.children.get(&parent).into_iter().flatten() {
                pending.push(child);
            }
        }
        descendants.remove(&definition);
        let mut visited = BTreeSet::new();
        let mut bodies = BTreeSet::new();
        pending.push(representation);
        while let Some(current) = pending.pop() {
            if !visited.insert(current) {
                continue;
            }
            if let Some(direct) = self.topology.body_by_root.get(&current) {
                for body in direct {
                    if !bodies.contains(body) {
                        bodies.insert(body.clone());
                    }
                }
            }
            if let Some(items) = self
                .exchange
                .records()
                .get(&current)
                .and_then(super::super::topology::representation_item_values)
            {
                for item in items.iter().filter_map(ValueExt::reference) {
                    if self.topology.body_by_root.contains_key(&item) {
                        pending.push(item);
                    } else if let Some(mapped) =
                        self.exchange.records().get(&item).and_then(|record| {
                            super::super::topology::mapped_representation(record, self.exchange)
                        })
                    {
                        // A mapped child with an assembly usage is placed by that occurrence.
                        let placed_by_child = self.owners.get(&mapped).is_some_and(|owners| {
                            owners.iter().any(|owner| descendants.contains(owner))
                        });
                        if !placed_by_child {
                            pending.push(mapped);
                        }
                    }
                }
            }
            for &related in self
                .topology
                .shape_representation_relationships
                .get(&current)
                .into_iter()
                .flatten()
            {
                // Links within one definition may lead from a placement-only shape
                // to a geometric representation. A different definition owns its own bodies.
                let foreign = self
                    .owners
                    .get(&related)
                    .is_some_and(|owners| !owners.contains(&definition));
                if !foreign {
                    pending.push(related);
                }
            }
        }
        let mut output = Vec::new();
        output.extend(bodies);
        Ok(output)
    }
}
