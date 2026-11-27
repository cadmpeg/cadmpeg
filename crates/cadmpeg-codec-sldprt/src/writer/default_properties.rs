// SPDX-License-Identifier: Apache-2.0
use std::collections::{BTreeMap, HashSet};

use cadmpeg_core::decode::{u64_from_index, DecodeArena, DecodeContext};
use cadmpeg_core::CodecError;
use cadmpeg_ir::attributes::{AttributeValue, SourceAttribute};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::Annotations;

use super::Held;
use crate::container::{
    Block, BlockName, ContainerScan, PayloadFamily, Section, SolidWorksEnvelopeScan,
};
use crate::metadata::{
    is_saved_property_attribute as is_property, is_saved_property_owner as is_owner,
};

const UNIT_PREFIX: &str = "sldprt:metadata:source_linear_unit_code#";
const BLOCK_PREFIX: &str = "sldprt:file:block#";

/// Each saved-property attribute the write does not write as the document
/// states it, and what the output holds for it. No serializer writes a saved
/// property: `opaque_blocks` emits each carrier block the source fidelity
/// holds bytes of as stored, so the output holds the properties a decode
/// reads from those bytes. The read of the carriers charges `ctx`.
pub(super) fn unwritten(
    ir: &CadIr,
    records: &[crate::SourceRecord<'_>],
    annotations: &Annotations,
    ctx: &DecodeContext<'_>,
) -> Result<BTreeMap<String, Held>, CodecError> {
    let scan = carrier_scan(records, annotations);
    let input = scan.blocks.iter().map(|block| block.payload.len()).sum();
    let arena = DecodeArena::new();
    let stored = ctx.with_arena(&arena, |ctx| {
        ctx.charge_input(u64_from_index(input), "SLDPRT saved-property carriers")?;
        crate::metadata::saved_property_rows(ctx, &scan)
    })?;
    let carriers = scan
        .blocks
        .iter()
        .map(|block| block.offset)
        .collect::<HashSet<_>>();
    Ok(changed(&ir.model.attributes, &stored, &carriers))
}

/// The saved-property carriers among `records`: each retained block with
/// bytes whose stream a saved-property owner names, at its source offset.
fn carrier_scan(
    records: &[crate::SourceRecord<'_>],
    annotations: &Annotations,
) -> ContainerScan<'static> {
    let blocks = records
        .iter()
        .filter_map(|record| {
            let offset = record
                .id
                .as_str()
                .strip_prefix(BLOCK_PREFIX)?
                .parse()
                .ok()?;
            let data = record.data?;
            let stream = annotations.provenance.get(record.id.as_str())?.stream();
            let mut block = Block {
                offset,
                type_id: 0,
                comp_sz: 0,
                section: BlockName::Named(stream.to_owned().try_into().ok()?),
                family: PayloadFamily::Xml,
                payload: Vec::new(),
                ps_streams: Vec::new(),
                ps_probe_capped: false,
            };
            is_owner(Section::Block(&block)).then(|| {
                block.payload = data.to_vec();
                block
            })
        })
        .collect();
    ContainerScan {
        source_image: &[],
        version: 0,
        blocks,
        directory: Vec::new(),
        cache_cells: Vec::new(),
        compound_streams: Vec::new(),
        solidworks: SolidWorksEnvelopeScan::default(),
        given_up: Vec::new(),
        noncanonical: Vec::new(),
    }
}

/// Each saved-property attribute, and each unit mirror of a carrier in
/// `carriers`, that `stated` and `stored` do not hold alike, by id from
/// either side: held as stored when `stored` has the id. An id stated twice
/// on a side counts as changed.
fn changed(
    stated: &[SourceAttribute],
    stored: &[SourceAttribute],
    carriers: &HashSet<usize>,
) -> BTreeMap<String, Held> {
    let owned = |attribute: &&SourceAttribute| {
        is_property(attribute)
            || unit_mirror_owner(attribute.id.as_str())
                .is_some_and(|ordinal| carriers.contains(&ordinal))
    };
    let (stored, stated) = (rows(stored, owned), rows(stated, owned));
    stored
        .keys()
        .chain(stated.keys())
        .copied()
        .filter(|id| {
            !matches!(
                (
                    stored.get(id).map(Vec::as_slice),
                    stated.get(id).map(Vec::as_slice),
                ),
                (Some([left]), Some([right])) if same_attribute(left, right)
            )
        })
        .map(|id| {
            let held = if stored.contains_key(id) {
                Held::Source
            } else {
                Held::Nothing
            };
            (id.to_owned(), held)
        })
        .collect()
}

/// The source section ordinal a unit-mirror id names.
fn unit_mirror_owner(id: &str) -> Option<usize> {
    id.strip_prefix(UNIT_PREFIX)?
        .split_once(':')?
        .0
        .parse()
        .ok()
}

/// The attributes of `attributes` that `owned` selects, by id.
fn rows(
    attributes: &[SourceAttribute],
    owned: impl Fn(&&SourceAttribute) -> bool,
) -> BTreeMap<&str, Vec<&SourceAttribute>> {
    let mut rows = BTreeMap::<_, Vec<_>>::new();
    for attribute in attributes.iter().filter(owned) {
        rows.entry(attribute.id.as_str())
            .or_default()
            .push(attribute);
    }
    rows
}

fn same_attribute(left: &SourceAttribute, right: &SourceAttribute) -> bool {
    left.id == right.id
        && left.target == right.target
        && left.name == right.name
        && left.values.len() == right.values.len()
        && left
            .values
            .iter()
            .zip(&right.values)
            .all(|(left, right)| match (left, right) {
                (Some(AttributeValue::Float(left)), Some(AttributeValue::Float(right))) => {
                    left.get() == right.get()
                }
                (Some(AttributeValue::Vector(left)), Some(AttributeValue::Vector(right))) => {
                    left.len() == right.len()
                        && left
                            .iter()
                            .zip(right)
                            .all(|(left, right)| left.get() == right.get())
                }
                _ => left == right,
            })
}

#[cfg(test)]
mod tests;
