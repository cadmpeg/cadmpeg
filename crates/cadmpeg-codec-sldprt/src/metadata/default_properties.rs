// SPDX-License-Identifier: Apache-2.0
//! Saved scalars from the document default-property namespace.

mod oleps;
mod xml;

use cadmpeg_core::decode::{index_from_u32, u64_from_index, DecodeContext};
use cadmpeg_core::CodecError;
use cadmpeg_ir::annotations::Annotations;
use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue, SourceAttribute};
use cadmpeg_ir::ids::{AttributeId, IdentityKey};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::Exactness;
use std::mem::size_of;

use crate::container::{ContainerScan, Section};
use crate::loss::SldprtLossCode;

pub(crate) const ATTRIBUTE_PREFIX: &str = "sldprt:metadata:default_property#";
const USER_DEFINED_FMTID: &str = "D5CDD505-2E9C-101B-9397-08002B2CF9AE";

pub(crate) fn is_attribute(attribute: &SourceAttribute) -> bool {
    attribute.id.as_str().starts_with(ATTRIBUTE_PREFIX)
}

pub(crate) fn is_owner(section: Section<'_>) -> bool {
    matches!(
        (section, section.name()),
        (
            Section::Block(_),
            Some("docProps/ISolidWorksInformation.xml")
        ) | (Section::Compound(_), Some("ISolidWorksInformation"))
    )
}

#[derive(Clone)]
struct SavedValue {
    pid: u32,
    value: Option<AttributeValue>,
    source_type: &'static str,
}

#[derive(Clone)]
struct Alias {
    name: String,
    pid: u32,
}

#[derive(Default)]
struct Table {
    values: Vec<SavedValue>,
    aliases: Vec<Alias>,
}

pub(super) fn append(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan<'_>,
    out: &mut Vec<SourceAttribute>,
    annotations: &mut Annotations,
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    let mut owners = scan.sections().filter(|section| is_owner(*section));
    let Some(owner) = owners.next() else {
        return Ok(());
    };
    if owners.next().is_some() {
        losses.push(
            SldprtLossCode::MetadataDefaultOwnerAmbiguous.note(
                "The document default-property namespace has multiple physical source owners.",
            ),
        );
        return Ok(());
    }
    let parsed = match owner {
        Section::Block(_) => xml::table(ctx, owner.payload()),
        Section::Compound(_) => oleps::table(ctx, owner.payload()),
    };
    let table = match parsed {
        Ok(table) => table,
        Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
        Err(error) => {
            losses.push(SldprtLossCode::MetadataDefaultCarrierUnsupported.note(format!(
                "Default-property carrier {} retains unsupported or malformed property data: {error}",
                owner.source_stream().as_str()
            )));
            return Ok(());
        }
    };
    project(ctx, owner, table, out, annotations, losses)
}

fn project(
    ctx: &DecodeContext<'_>,
    owner: Section<'_>,
    mut table: Table,
    out: &mut Vec<SourceAttribute>,
    annotations: &mut Annotations,
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    table.values.sort_by_key(|value| value.pid);
    table
        .aliases
        .sort_by(|left, right| left.name.cmp(&right.name).then(left.pid.cmp(&right.pid)));
    let mut unusable = ctx.alloc_filled(
        table.aliases.len(),
        0u32,
        "allocate unusable saved property identities",
    )?;
    let mut unusable_count = 0;
    let mut cursor = 0;
    while cursor < table.aliases.len() {
        let alias = &table.aliases[cursor];
        let mut end = cursor + 1;
        while end < table.aliases.len() && table.aliases[end].name == alias.name {
            end += 1;
        }
        if table.aliases[cursor..end]
            .iter()
            .any(|other| other.pid != alias.pid)
        {
            losses.push(SldprtLossCode::MetadataDefaultAliasAmbiguous.note(format!(
                "Default-property alias {:?} names multiple saved property identifiers.",
                alias.name
            )));
            cursor = end;
            continue;
        }
        let start = table.values.partition_point(|value| value.pid < alias.pid);
        let stop = table.values.partition_point(|value| value.pid <= alias.pid);
        if stop - start > 1 {
            losses.push(SldprtLossCode::MetadataDefaultAliasAmbiguous.note(format!(
                "Default-property alias {:?} names a repeated saved value identifier {}.",
                alias.name, alias.pid
            )));
        } else if alias.pid <= 1 || start == stop {
            unusable[unusable_count] = alias.pid;
            unusable_count += 1;
        } else if table.values[start].value.is_some()
            || matches!(table.values[start].source_type, "empty" | "null")
        {
            let value = table.values[start].value.as_ref();
            let encoded_length = alias_length(&alias.name)?;
            let id_length = ATTRIBUTE_PREFIX
                .len()
                .checked_add(decimal_digits(owner.ordinal()))
                .and_then(|size| size.checked_add(decimal_digits(index_from_u32(alias.pid))))
                .and_then(|size| size.checked_add(2))
                .and_then(|size| size.checked_add(encoded_length))
                .ok_or_else(|| {
                    CodecError::malformed("default-property identity length overflow")
                })?;
            let tag_length =
                "saved_default_property:".len() + table.values[start].source_type.len();
            let value_bytes = match value {
                Some(AttributeValue::String(text) | AttributeValue::Reference(text)) => text.len(),
                Some(AttributeValue::Vector(values)) => {
                    values.len().checked_mul(size_of::<f64>()).ok_or_else(|| {
                        CodecError::malformed("saved property vector length overflow")
                    })?
                }
                _ => 0,
            };
            let retained = id_length
                .checked_add(alias.name.len())
                .and_then(|size| size.checked_add(value_bytes))
                .and_then(|size| size.checked_add(tag_length))
                .and_then(|size| {
                    size.checked_add(
                        size_of::<SourceAttribute>()
                            + value.map_or(0, |_| size_of::<AttributeValue>()),
                    )
                })
                .ok_or_else(|| CodecError::malformed("default-property text length overflow"))?;
            ctx.retain_bytes(u64_from_index(retained), "retain saved default property")?;
            ctx.charge_entities(1, "admit saved default property")?;
            let encoded = encoded_alias(ctx, &alias.name, encoded_length)?;
            let tag = format!("saved_default_property:{}", table.values[start].source_type);
            let id = AttributeId::compose(
                &cadmpeg_ir::identity_namespace!("sldprt", "metadata", "default_property"),
                IdentityKey::from(owner.ordinal())
                    .colon(alias.pid)
                    .colon(IdentityKey::try_new(encoded).map_err(CodecError::malformed)?),
            );
            crate::annotations::note(
                annotations,
                id.as_str(),
                owner.source_stream(),
                0,
                &tag,
                Exactness::ByteExact,
            )?;
            out.push(SourceAttribute {
                id,
                target: AttributeTarget::Document,
                name: alias.name.clone(),
                values: value.cloned().into_iter().collect(),
            });
        } else {
            unusable[unusable_count] = alias.pid;
            unusable_count += 1;
        }
        cursor = end;
    }
    unusable.truncate(unusable_count);
    unusable.sort_unstable();
    unusable.dedup();
    for pid in unusable {
        losses.push(
            SldprtLossCode::MetadataDefaultValueUnsupported.note(format!(
                "Saved default property {pid} in {} has no represented usable scalar value.",
                owner.source_stream().as_str()
            )),
        );
    }
    Ok(())
}

fn decimal_digits(mut value: usize) -> usize {
    let mut digits = 1;
    while value >= 10 {
        value /= 10;
        digits += 1;
    }
    digits
}

fn alias_length(name: &str) -> Result<usize, CodecError> {
    name.bytes().try_fold(0usize, |count, byte| {
        count
            .checked_add(if byte.is_ascii_alphanumeric() || b"._-/".contains(&byte) {
                1
            } else {
                3
            })
            .ok_or_else(|| CodecError::malformed("default-property alias length overflow"))
    })
}

fn encoded_alias(ctx: &DecodeContext<'_>, name: &str, length: usize) -> Result<String, CodecError> {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut bytes = ctx.alloc_filled(length, 0u8, "allocate default-property identity")?;
    let mut cursor = 0;
    for byte in name.bytes() {
        if byte.is_ascii_alphanumeric() || b"._-/".contains(&byte) {
            bytes[cursor] = byte;
            cursor += 1;
        } else {
            bytes[cursor..cursor + 3].copy_from_slice(&[
                b'%',
                HEX[usize::from(byte >> 4)],
                HEX[usize::from(byte & 15)],
            ]);
            cursor += 3;
        }
    }
    String::from_utf8(bytes)
        .map_err(|_| CodecError::malformed("default-property identity encoding"))
}

#[cfg(test)]
mod tests;
