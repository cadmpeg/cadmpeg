// SPDX-License-Identifier: Apache-2.0
//! Literal facts from stored configuration-property carriers.

use std::mem::size_of;

use cadmpeg_core::decode::{index_from_u32, u64_from_index, DecodeContext};
use cadmpeg_core::CodecError;
use cadmpeg_ir::annotations::Annotations;
use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue, SourceAttribute};
use cadmpeg_ir::ids::{AttributeId, IdentityKey};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::Exactness;

use crate::container::{ContainerScan, Section};
use crate::loss::SldprtLossCode;

const ATTRIBUTE_PREFIX: &str = "sldprt:metadata:configuration_property#";
const FMTID: &str = "D5CDD505-2E9C-101B-9397-08002B2CF9AE";
const ROOT_NAMESPACE: &str = "http://www.solidworks.com/config-properties";
const VALUE_NAMESPACE: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/docPropsVTypes";
const ATTRIBUTE_NAME: &str = "saved_configuration_property";

pub(super) fn is_attribute(attribute: &SourceAttribute) -> bool {
    attribute.id.as_str().starts_with(ATTRIBUTE_PREFIX)
}

fn decimal(text: &str) -> Option<u32> {
    (!text.is_empty()
        && (text.len() == 1 || !text.starts_with('0'))
        && text.bytes().all(|byte| byte.is_ascii_digit()))
    .then_some(())?;
    text.parse().ok()
}

fn namespace_index(section: Section<'_>) -> Option<u32> {
    let name = section.name()?;
    let index = match section {
        Section::Block(_) => name
            .strip_prefix("docProps/Config-")?
            .strip_suffix("-Properties.xml")?,
        Section::Compound(_) => name.strip_prefix("Config-")?.strip_suffix("-Properties")?,
    };
    decimal(index)
}

pub(super) fn is_owner(section: Section<'_>) -> bool {
    namespace_index(section).is_some()
}

pub(super) fn append(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan<'_>,
    out: &mut Vec<SourceAttribute>,
    annotations: &mut Annotations,
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    let count = scan.sections().filter(|owner| is_owner(*owner)).count();
    let mut owners =
        ctx.alloc_filled(count, None, "allocate configuration-property source owners")?;
    for (slot, owner) in owners
        .iter_mut()
        .zip(scan.sections().filter(|owner| is_owner(*owner)))
    {
        *slot = namespace_index(owner).map(|index| (index, owner));
    }
    owners.sort_unstable_by_key(|owner| owner.map(|(index, _)| index));
    let mut cursor = 0;
    while cursor < owners.len() {
        let Some((index, owner)) = owners[cursor] else {
            cursor += 1;
            continue;
        };
        let mut end = cursor + 1;
        while owners
            .get(end)
            .is_some_and(|other| other.is_some_and(|(other, _)| other == index))
        {
            end += 1;
        }
        if end == cursor + 1 {
            let projected = match owner {
                Section::Block(_) => project_owner(ctx, owner, index, out, annotations, losses),
                Section::Compound(_) => {
                    project_compound_owner(ctx, owner, index, out, annotations, losses)
                }
            };
            match projected {
                Ok(()) => {}
                Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
                Err(error) => losses.push(
                    SldprtLossCode::MetadataConfigurationPropertyCarrierUnsupported.note(format!(
                        "Configuration-property carrier {} retains unsupported data: {error}",
                        owner.source_stream().as_str()
                    )),
                ),
            }
        } else {
            losses.push(
                SldprtLossCode::MetadataConfigurationPropertyOwnerAmbiguous.note(format!(
                    "Configuration-property namespace {index} has multiple physical owners."
                )),
            );
        }
        cursor = end;
    }
    Ok(())
}

#[derive(Clone, Copy, Default)]
struct Property<'a> {
    pid: u32,
    name: &'a str,
    kind: &'static str,
    value: Option<&'a str>,
    supported: bool,
}

fn identifier(node: roxmltree::Node<'_, '_>) -> Result<u32, CodecError> {
    node.attribute("pid")
        .and_then(decimal)
        .ok_or_else(|| CodecError::malformed("configuration-property PID is not canonical"))
}

fn property<'a>(node: roxmltree::Node<'a, '_>) -> Result<Property<'a>, CodecError> {
    let mut row = Property {
        pid: identifier(node)?,
        name: node.attribute("name").unwrap_or_default(),
        ..Property::default()
    };
    if row.pid < 2 || node.attribute("IsEquation") != Some("False") {
        return Ok(row);
    }
    let mut values = node
        .children()
        .filter(roxmltree::Node::is_element)
        .filter(|child| child.tag_name().namespace() == Some(VALUE_NAMESPACE));
    let Some(value) = values.next() else {
        return Ok(row);
    };
    if values.next().is_some()
        || value.attributes().len() != 0
        || value.children().any(|child| !child.is_text())
        || node
            .children()
            .filter(roxmltree::Node::is_element)
            .any(|child| {
                child != value
                    && (child.tag_name().name() != "FPVals"
                        || child
                            .tag_name()
                            .namespace()
                            .is_some_and(|namespace| !namespace.is_empty()))
            })
    {
        return Ok(row);
    }
    match value.tag_name().name() {
        "lpstr" | "lpwstr" => {
            if value.children().count() > 1 {
                return Ok(row);
            }
            row.kind = if value.tag_name().name() == "lpstr" {
                "lpstr"
            } else {
                "lpwstr"
            };
            row.value = Some(value.text().unwrap_or_default());
        }
        "empty" | "null" if value.text().is_none_or(str::is_empty) => {
            row.kind = if value.tag_name().name() == "empty" {
                "empty"
            } else {
                "null"
            };
        }
        _ => return Ok(row),
    }
    row.supported = true;
    Ok(row)
}

fn decimal_digits(mut value: usize) -> usize {
    let mut digits = 1;
    while value >= 10 {
        value /= 10;
        digits += 1;
    }
    digits
}

fn project_owner(
    ctx: &DecodeContext<'_>,
    owner: Section<'_>,
    index: u32,
    out: &mut Vec<SourceAttribute>,
    annotations: &mut Annotations,
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    let text = std::str::from_utf8(owner.payload())
        .map_err(|_| CodecError::malformed("configuration-property XML is not UTF-8"))?;
    let document = ctx.parse_xml(text, "decode configuration-property XML")?;
    let root = document.root_element();
    if root.tag_name().name() != "ConfigProperties"
        || root.tag_name().namespace() != Some(ROOT_NAMESPACE)
    {
        return Err(CodecError::malformed(
            "configuration-property XML root is unsupported",
        ));
    }
    let mut sets = root.children().filter(|node| {
        node.is_element()
            && node.tag_name().name() == "propertySection"
            && node.attribute("fmtid").is_some_and(|fmtid| {
                fmtid
                    .strip_prefix('{')
                    .and_then(|value| value.strip_suffix('}'))
                    .unwrap_or(fmtid)
                    .eq_ignore_ascii_case(FMTID)
            })
    });
    let section = sets.next().ok_or_else(|| {
        CodecError::malformed("configuration-property XML has no UserDefinedProperties set")
    })?;
    if sets.next().is_some()
        || section
            .tag_name()
            .namespace()
            .is_some_and(|namespace| !namespace.is_empty())
    {
        return Err(CodecError::malformed(
            "configuration-property set is repeated or namespaced",
        ));
    }
    if section
        .children()
        .filter(roxmltree::Node::is_element)
        .any(|node| {
            node.tag_name()
                .namespace()
                .is_some_and(|namespace| !namespace.is_empty())
                || !matches!(
                    node.tag_name().name(),
                    "property" | "propertyNameDictionaryElement"
                )
        })
    {
        return Err(CodecError::malformed(
            "configuration-property set has an unsupported record",
        ));
    }
    let named = || {
        section
            .children()
            .filter(|node| node.is_element() && node.tag_name().name() == "property")
    };
    let mut rows = ctx.alloc_filled(
        named().count(),
        Property::default(),
        "allocate configuration-property rows",
    )?;
    for (slot, node) in rows.iter_mut().zip(named()) {
        if node
            .tag_name()
            .namespace()
            .is_some_and(|namespace| !namespace.is_empty())
        {
            return Err(CodecError::malformed(
                "configuration-property record is namespaced",
            ));
        }
        *slot = property(node)?;
    }
    rows.sort_unstable_by_key(|row| row.pid);
    if rows.windows(2).any(|pair| pair[0].pid == pair[1].pid) {
        return Err(CodecError::malformed(
            "configuration-property PID has multiple records",
        ));
    }
    rows.sort_unstable_by_key(|row| row.name);
    if rows
        .windows(2)
        .any(|pair| !pair[0].name.is_empty() && pair[0].name == pair[1].name)
    {
        return Err(CodecError::malformed(
            "configuration-property name has multiple records",
        ));
    }
    for alias in section.children().filter(|node| {
        node.is_element()
            && node.tag_name().name() == "propertyNameDictionaryElement"
            && node.attribute("name").is_some_and(|name| !name.is_empty())
    }) {
        let name = alias.attribute("name").unwrap_or_default();
        if let Ok(position) = rows.binary_search_by_key(&name, |row| row.name) {
            if alias
                .tag_name()
                .namespace()
                .is_some_and(|namespace| !namespace.is_empty())
                || identifier(alias)? != rows[position].pid
            {
                return Err(CodecError::malformed(
                    "configuration-property dictionary conflicts with the named PID",
                ));
            }
        }
    }
    project_rows(ctx, owner, index, rows, out, annotations, losses)
}

fn project_compound_owner(
    ctx: &DecodeContext<'_>,
    owner: Section<'_>,
    index: u32,
    out: &mut Vec<SourceAttribute>,
    annotations: &mut Annotations,
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    const COMPANION_FLAG: u32 = 0x0100_0000;
    let mut table = super::default_properties::compound_configuration_table(ctx, owner.payload())?;
    table.values.sort_unstable_by_key(|row| row.pid);
    table.aliases.sort_unstable_by_key(|row| row.pid);
    if table
        .values
        .windows(2)
        .any(|pair| pair[0].pid == pair[1].pid)
        || table
            .aliases
            .windows(2)
            .any(|pair| pair[0].pid == pair[1].pid)
    {
        return Err(CodecError::malformed(
            "configuration-property PID has multiple records or aliases",
        ));
    }
    table
        .aliases
        .sort_unstable_by(|left, right| left.name.cmp(&right.name));
    if table
        .aliases
        .windows(2)
        .any(|pair| pair[0].name == pair[1].name)
    {
        return Err(CodecError::malformed(
            "configuration-property name has multiple records",
        ));
    }
    let mut rows = ctx.alloc_filled(
        table.aliases.len(),
        Property::default(),
        "allocate compound configuration-property rows",
    )?;
    for (row, alias) in rows.iter_mut().zip(&table.aliases) {
        row.pid = alias.pid;
        row.name = &alias.name;
        if alias.pid < 2 || alias.pid >= COMPANION_FLAG {
            continue;
        }
        let Ok(position) = table
            .values
            .binary_search_by_key(&alias.pid, |value| value.pid)
        else {
            continue;
        };
        let value = &table.values[position];
        row.kind = value.source_type;
        if table
            .values
            .binary_search_by_key(&(alias.pid | COMPANION_FLAG), |value| value.pid)
            .is_ok()
        {
            continue;
        }
        match &value.value {
            Some(AttributeValue::String(text))
                if matches!(value.source_type, "lpstr" | "lpwstr") =>
            {
                row.value = Some(text);
                row.supported = true;
            }
            None if matches!(value.source_type, "empty" | "null") => row.supported = true,
            _ => {}
        }
    }
    project_rows(ctx, owner, index, rows, out, annotations, losses)
}

fn project_rows(
    ctx: &DecodeContext<'_>,
    owner: Section<'_>,
    index: u32,
    rows: Vec<Property<'_>>,
    out: &mut Vec<SourceAttribute>,
    annotations: &mut Annotations,
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    for row in rows {
        if row.name.is_empty() {
            continue;
        }
        if !row.supported {
            losses.push(
                SldprtLossCode::MetadataConfigurationPropertyValueUnsupported.note(format!(
                    "Configuration-property {} PID {} has no supported literal scalar fact.",
                    owner.source_stream().as_str(),
                    row.pid
                )),
            );
            continue;
        }
        let member = owner.name().ok_or_else(|| {
            CodecError::malformed("configuration-property carrier has no member name")
        })?;
        let count = 6 + usize::from(row.value.is_some());
        let bytes = [
            member.len(),
            FMTID.len(),
            row.name.len(),
            row.kind.len(),
            row.value.map_or(0, str::len),
            ATTRIBUTE_NAME.len(),
            ATTRIBUTE_PREFIX.len()
                + decimal_digits(owner.ordinal())
                + 1
                + decimal_digits(index_from_u32(row.pid)),
            "saved_configuration_property:".len() + row.kind.len(),
            size_of::<SourceAttribute>(),
            count * size_of::<AttributeValue>(),
        ]
        .into_iter()
        .try_fold(0usize, usize::checked_add)
        .ok_or_else(|| CodecError::malformed("configuration-property text length overflow"))?;
        ctx.retain_bytes(u64_from_index(bytes), "retain configuration-property fact")?;
        ctx.charge_entities(1, "admit configuration-property fact")?;
        let id = AttributeId::compose(
            &cadmpeg_ir::identity_namespace!("sldprt", "metadata", "configuration_property"),
            IdentityKey::from(owner.ordinal()).colon(row.pid),
        );
        let tag = format!("saved_configuration_property:{}", row.kind);
        crate::annotations::note(
            annotations,
            id.as_str(),
            owner.source_stream(),
            0,
            &tag,
            Exactness::ByteExact,
        )?;
        let mut values = Vec::with_capacity(count);
        values.extend([
            AttributeValue::String(member.to_owned()),
            AttributeValue::Integer(i64::from(index)),
            AttributeValue::String(FMTID.to_owned()),
            AttributeValue::Integer(i64::from(row.pid)),
            AttributeValue::String(row.name.to_owned()),
            AttributeValue::String(row.kind.to_owned()),
        ]);
        if let Some(value) = row.value {
            values.push(AttributeValue::String(value.to_owned()));
        }
        out.push(SourceAttribute {
            id,
            target: AttributeTarget::Document,
            name: ATTRIBUTE_NAME.into(),
            values,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests;
