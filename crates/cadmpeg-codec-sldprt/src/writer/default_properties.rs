// SPDX-License-Identifier: Apache-2.0
use std::collections::{BTreeMap, HashSet};

use cadmpeg_core::CodecError;
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

/// Every saved-property attribute the document states: with no retained
/// carrier, none of them is written.
pub(super) fn stated_properties(ir: &CadIr) -> BTreeMap<String, Held> {
    ir.model
        .attributes
        .iter()
        .filter(|attribute| is_property(attribute))
        .map(|attribute| (attribute.id.as_str().to_owned(), Held::Nothing))
        .collect()
}

pub(super) fn validate_retention(
    scan: &ContainerScan<'_>,
    records: &[crate::SourceRecord<'_>],
    annotations: &cadmpeg_ir::Annotations,
) -> Result<(), CodecError> {
    for owner in scan.sections().filter(|section| is_owner(*section)) {
        if matches!(owner, Section::Compound(_)) {
            return Err(CodecError::NotImplemented(
                "SLDPRT saved properties in compound streams require a retained compound writer"
                    .into(),
            ));
        }
        let id = owner.native_id();
        let mut retained = records.iter().filter(|record| record.id == id);
        let record = retained.next().ok_or_else(|| {
            CodecError::NotImplemented(
                "SLDPRT saved-property carrier has no retained source record".into(),
            )
        })?;
        if retained.next().is_some()
            || record.data != Some(owner.payload())
            || annotations
                .provenance
                .get(id.as_str())
                .is_none_or(|provenance| provenance.stream() != owner.source_stream().as_str())
        {
            return Err(CodecError::NotImplemented(
                "SLDPRT saved-property carrier differs from its complete source owner".into(),
            ));
        }
    }
    Ok(())
}

pub(super) fn validate_emitted(
    scan: &ContainerScan<'_>,
    sections: &[(String, Vec<u8>)],
) -> Result<(), CodecError> {
    for owner in scan.sections().filter(|section| is_owner(*section)) {
        let mut emitted = sections
            .iter()
            .filter(|(name, _)| name == owner.source_stream().as_str());
        if matches!(owner, Section::Compound(_))
            || emitted
                .next()
                .is_none_or(|(_, payload)| payload != owner.payload())
            || emitted.next().is_some()
        {
            return Err(CodecError::NotImplemented("SLDPRT saved-property carrier must be emitted unchanged under its source namespace".into()));
        }
    }
    Ok(())
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
