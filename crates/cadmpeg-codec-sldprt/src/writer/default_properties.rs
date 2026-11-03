// SPDX-License-Identifier: Apache-2.0
use std::collections::HashSet;

use cadmpeg_core::CodecError;
use cadmpeg_ir::attributes::{AttributeValue, SourceAttribute};
use cadmpeg_ir::document::CadIr;

use crate::container::{ContainerScan, Section};

use crate::metadata::{
    is_saved_property_attribute as is_property, is_saved_property_owner as is_owner,
};

const UNIT_PREFIX: &str = "sldprt:metadata:source_linear_unit_code#";

pub(super) fn requires_guard(ir: &CadIr, scan: Option<&ContainerScan<'_>>) -> bool {
    ir.model.attributes.iter().any(is_property)
        || scan.is_some_and(|scan| scan.sections().any(is_owner))
}

pub(super) fn validate(
    ir: &CadIr,
    source: &CadIr,
    scan: &ContainerScan<'_>,
) -> Result<(), CodecError> {
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
    let mut original = source
        .model
        .attributes
        .iter()
        .filter(owned)
        .collect::<Vec<_>>();
    let mut current = ir.model.attributes.iter().filter(owned).collect::<Vec<_>>();
    original.sort_by(|left, right| left.id.cmp(&right.id));
    current.sort_by(|left, right| left.id.cmp(&right.id));
    let unique = |rows: &[&SourceAttribute]| rows.windows(2).all(|pair| pair[0].id != pair[1].id);
    if !unique(&original)
        || !unique(&current)
        || original.len() != current.len()
        || !original
            .iter()
            .zip(&current)
            .all(|(left, right)| same_attribute(left, right))
    {
        return Err(CodecError::NotImplemented(
            "SLDPRT saved-property edits require a retained property serializer".into(),
        ));
    }
    Ok(())
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
                (AttributeValue::Float(left), AttributeValue::Float(right)) => {
                    left.get().to_bits() == right.get().to_bits()
                }
                (AttributeValue::Vector(left), AttributeValue::Vector(right)) => {
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
