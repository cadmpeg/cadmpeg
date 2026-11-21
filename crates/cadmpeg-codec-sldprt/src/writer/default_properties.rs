// SPDX-License-Identifier: Apache-2.0
use std::collections::{BTreeMap, HashSet};

use cadmpeg_ir::attributes::{AttributeValue, SourceAttribute};
use cadmpeg_ir::document::CadIr;

use super::Held;
use crate::container::{ContainerScan, Section};

use crate::metadata::{
    is_saved_property_attribute as is_property, is_saved_property_owner as is_owner,
};

const UNIT_PREFIX: &str = "sldprt:metadata:source_linear_unit_code#";

pub(super) fn requires_guard(ir: &CadIr, scan: Option<&ContainerScan<'_>>) -> bool {
    ir.model.attributes.iter().any(is_property)
        || scan.is_some_and(|scan| scan.sections().any(is_owner))
}

/// Each saved-property attribute the document adds, removes or changes
/// against its `source` decode, unit mirrors in a default carrier included,
/// by id from either side. An id stated twice on a side counts as changed.
pub(super) fn edited_properties(
    ir: &CadIr,
    source: &CadIr,
    scan: &ContainerScan<'_>,
) -> BTreeMap<String, Held> {
    let owners = scan
        .sections()
        .filter(|section| crate::metadata::default_properties::is_owner(*section))
        .map(Section::ordinal)
        .collect::<HashSet<_>>();
    let owned = |attribute: &&SourceAttribute| {
        if is_property(attribute) {
            return true;
        }
        let Some(key) = attribute.id.as_str().strip_prefix(UNIT_PREFIX) else {
            return false;
        };
        key.split_once(':')
            .and_then(|(ordinal, _)| ordinal.parse::<usize>().ok())
            .is_some_and(|ordinal| owners.contains(&ordinal))
    };
    let (original, current) = (rows(source, owned), rows(ir, owned));
    original
        .keys()
        .chain(current.keys())
        .copied()
        .filter(|id| {
            !matches!(
                (
                    original.get(id).map(Vec::as_slice),
                    current.get(id).map(Vec::as_slice),
                ),
                (Some([left]), Some([right])) if same_attribute(left, right)
            )
        })
        .map(|id| {
            let held = if original.contains_key(id) {
                Held::Source
            } else {
                Held::Nothing
            };
            (id.to_owned(), held)
        })
        .collect()
}

/// The attributes of `document` that `owned` selects, by id.
fn rows(
    document: &CadIr,
    owned: impl Fn(&&SourceAttribute) -> bool,
) -> BTreeMap<&str, Vec<&SourceAttribute>> {
    let mut rows = BTreeMap::<_, Vec<_>>::new();
    for attribute in document.model.attributes.iter().filter(owned) {
        rows.entry(attribute.id.as_str())
            .or_default()
            .push(attribute);
    }
    rows
}

/// Each saved-property attribute the write does not write, and what the
/// output holds for it. A property of a carrier emitted as the source image
/// stores it is charged when `source` shows it edited. A property of a
/// carrier emitted from other retained bytes is charged as kept. A property
/// of a carrier the output does not hold is charged with no record.
pub(super) fn unwritten(
    ir: &CadIr,
    source: Option<&CadIr>,
    scan: Option<&ContainerScan<'_>>,
    records: &[crate::SourceRecord<'_>],
    annotations: &cadmpeg_ir::Annotations,
) -> BTreeMap<String, Held> {
    let carriers = scan.map_or_else(BTreeMap::new, |scan| carriers(scan, records, annotations));
    let carrier = |id: &str| {
        owner_ordinal(id)
            .and_then(|ordinal| carriers.get(&ordinal).copied())
            .unwrap_or(Carrier::Absent)
    };
    let mut unwritten = match (source, scan) {
        (Some(source), Some(scan)) => edited_properties(ir, source, scan)
            .into_iter()
            .filter(|(id, _)| carrier(id) == Carrier::AsStored)
            .collect(),
        _ => BTreeMap::new(),
    };
    for attribute in ir
        .model
        .attributes
        .iter()
        .filter(|attribute| is_property(attribute))
    {
        let held = match carrier(attribute.id.as_str()) {
            Carrier::AsStored if source.is_some() => continue,
            Carrier::AsStored | Carrier::Retained => Held::Source,
            Carrier::Absent => Held::Nothing,
        };
        unwritten.insert(attribute.id.as_str().to_owned(), held);
    }
    unwritten
}

/// The source section ordinal a saved-property or unit-mirror id names.
fn owner_ordinal(id: &str) -> Option<usize> {
    let (_, key) = id.split_once('#')?;
    key.split_once(':')?.0.parse().ok()
}

/// What the write emits for one saved-property owner.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Carrier {
    /// One retained native record with the owner's bytes under its stream:
    /// emitted as the source image stores it.
    AsStored,
    /// Retained native bytes unlike the owner's, or under another stream:
    /// emitted as the record stores them.
    Retained,
    /// A compound-file owner, or no retained record with bytes: no carrier.
    Absent,
}

/// Each saved-property owner of the source, by section ordinal.
fn carriers(
    scan: &ContainerScan<'_>,
    records: &[crate::SourceRecord<'_>],
    annotations: &cadmpeg_ir::Annotations,
) -> BTreeMap<usize, Carrier> {
    scan.sections()
        .filter(|section| is_owner(*section))
        .map(|owner| {
            let id = owner.native_id();
            let mut retained = records
                .iter()
                .filter(|record| record.id == id && record.data.is_some());
            let stream = annotations.provenance.get(id.as_str());
            let carrier = match (owner, retained.next(), retained.next(), stream) {
                (Section::Compound(_), ..) | (_, None, ..) | (.., None) => Carrier::Absent,
                (_, Some(record), None, Some(stream))
                    if record.data == Some(owner.payload())
                        && stream.stream() == owner.source_stream().as_str() =>
                {
                    Carrier::AsStored
                }
                _ => Carrier::Retained,
            };
            (owner.ordinal(), carrier)
        })
        .collect()
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
                    left.get().to_bits() == right.get().to_bits()
                }
                (Some(AttributeValue::Vector(left)), Some(AttributeValue::Vector(right))) => {
                    left.len() == right.len()
                        && left
                            .iter()
                            .zip(right)
                            .all(|(left, right)| left.get().to_bits() == right.get().to_bits())
                }
                _ => left == right,
            })
}

#[cfg(test)]
mod tests;
