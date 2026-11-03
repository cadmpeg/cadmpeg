// SPDX-License-Identifier: Apache-2.0
//! XML records in the saved default-property member.

use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use cadmpeg_core::CodecError;
use cadmpeg_ir::attributes::AttributeValue;
use std::borrow::Cow;

use super::{Alias, SavedValue, Table, USER_DEFINED_FMTID};

const ROOT_NAMESPACE: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/SolidworksInformation-properties";
const VALUE_NAMESPACE: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/docPropsVTypes";

pub(super) fn table(ctx: &DecodeContext<'_>, payload: &[u8]) -> Result<Table, CodecError> {
    let text = xml_text(ctx, payload)?;
    let document = ctx.parse_xml(text.as_ref(), "decode saved default-property XML")?;
    let root = document.root_element();
    if root.tag_name().name() != "Properties" || root.tag_name().namespace() != Some(ROOT_NAMESPACE)
    {
        return Err(CodecError::malformed(
            "default-property XML root namespace is unsupported",
        ));
    }
    let mut selected = None;
    for node in root.children().filter(roxmltree::Node::is_element) {
        if node.tag_name().name() != "propertySection" {
            continue;
        }
        if node
            .tag_name()
            .namespace()
            .is_some_and(|namespace| !namespace.is_empty())
        {
            return Err(CodecError::malformed(
                "default-property section namespace is unsupported",
            ));
        }
        if !node.attribute("fmtid").is_some_and(user_defined_fmtid) {
            continue;
        }
        if selected.replace(node).is_some() {
            return Err(CodecError::malformed(
                "default-property XML repeats its property set",
            ));
        }
    }
    let section = selected.ok_or_else(|| {
        CodecError::malformed("default-property XML has no UserDefinedProperties set")
    })?;
    let nodes = || section.children().filter(roxmltree::Node::is_element);
    let values = nodes()
        .filter(|node| node.tag_name().name() == "property")
        .count();
    let aliases = nodes()
        .filter(|node| node.attribute("name").is_some_and(|name| !name.is_empty()))
        .count();
    let mut table = Table {
        values: ctx.alloc_filled(
            values,
            SavedValue {
                pid: 0,
                value: None,
                source_type: "unsupported",
            },
            "allocate saved XML values",
        )?,
        aliases: ctx.alloc_filled(
            aliases,
            Alias {
                pid: 0,
                name: String::new(),
            },
            "allocate saved XML aliases",
        )?,
    };
    let mut value_index = 0;
    let mut alias_index = 0;
    for node in nodes() {
        if node
            .tag_name()
            .namespace()
            .is_some_and(|namespace| !namespace.is_empty())
        {
            return Err(CodecError::malformed(
                "default-property record namespace is unsupported",
            ));
        }
        match node.tag_name().name() {
            "property" => {
                let pid = property_identifier(node)?;
                let mut typed = node.children().filter(roxmltree::Node::is_element);
                let first = typed.next();
                let (value, source_type) = if typed.next().is_none() {
                    if let Some(first) =
                        first.filter(|child| child.tag_name().namespace() == Some(VALUE_NAMESPACE))
                    {
                        typed_value(ctx, first)?
                    } else {
                        (None, "unsupported")
                    }
                } else {
                    (None, "unsupported")
                };
                table.values[value_index] = SavedValue {
                    pid,
                    value,
                    source_type,
                };
                value_index += 1;
                if let Some(name) = node.attribute("name").filter(|name| !name.is_empty()) {
                    ctx.retain_bytes(u64_from_index(name.len()), "retain saved XML alias")?;
                    table.aliases[alias_index] = Alias {
                        name: name.to_owned(),
                        pid,
                    };
                    alias_index += 1;
                }
            }
            "propertyNameDictionaryElement" => {
                let pid = property_identifier(node)?;
                if let Some(name) = node.attribute("name").filter(|name| !name.is_empty()) {
                    ctx.retain_bytes(u64_from_index(name.len()), "retain saved XML alias")?;
                    table.aliases[alias_index] = Alias {
                        name: name.to_owned(),
                        pid,
                    };
                    alias_index += 1;
                }
            }
            _ => {
                return Err(CodecError::malformed(
                    "default-property section has an unsupported record",
                ));
            }
        }
    }
    Ok(table)
}

fn xml_text<'a>(ctx: &DecodeContext<'_>, bytes: &'a [u8]) -> Result<Cow<'a, str>, CodecError> {
    if let Some(body) = bytes.strip_prefix(&[0xff, 0xfe]) {
        if body.len() % 2 != 0 {
            return Err(CodecError::malformed(
                "default-property XML has truncated UTF16 text",
            ));
        }
        let maximum = (body.len() / 2)
            .checked_mul(3)
            .ok_or_else(|| CodecError::malformed("default-property XML text length overflow"))?;
        ctx.retain_bytes(u64_from_index(maximum), "admit saved XML UTF16 text")?;
        return ctx
            .utf16le_text(body, body.len() / 2, false, "decode saved XML UTF16 text")
            .map(Cow::Owned);
    }
    std::str::from_utf8(bytes)
        .map(Cow::Borrowed)
        .map_err(|_| CodecError::malformed("default-property XML has no supported text encoding"))
}

fn user_defined_fmtid(text: &str) -> bool {
    text.strip_prefix('{')
        .and_then(|text| text.strip_suffix('}'))
        .unwrap_or(text)
        .eq_ignore_ascii_case(USER_DEFINED_FMTID)
}

fn property_identifier(node: roxmltree::Node<'_, '_>) -> Result<u32, CodecError> {
    node.attribute("pid")
        .and_then(|pid| pid.parse().ok())
        .ok_or_else(|| CodecError::malformed("default-property record has no valid PID"))
}

fn typed_value(
    ctx: &DecodeContext<'_>,
    node: roxmltree::Node<'_, '_>,
) -> Result<(Option<AttributeValue>, &'static str), CodecError> {
    let nil = match node.tag_name().name() {
        "empty" => Some("empty"),
        "null" => Some("null"),
        _ => None,
    };
    if let Some(kind) = nil {
        let valid = node.attributes().len() == 0
            && node.children().all(|child| {
                !child.is_element()
                    && child.text().is_none_or(|text| {
                        !child.is_text() || text.bytes().all(|byte| b" \t\r\n".contains(&byte))
                    })
            });
        return Ok((None, if valid { kind } else { "unsupported" }));
    }
    scalar(ctx, node)
        .map(|value| value.map_or((None, "unsupported"), |(value, kind)| (Some(value), kind)))
}

fn scalar(
    ctx: &DecodeContext<'_>,
    node: roxmltree::Node<'_, '_>,
) -> Result<Option<(AttributeValue, &'static str)>, CodecError> {
    if node.children().any(|child| child.is_element()) {
        return Ok(None);
    }
    let length = node
        .children()
        .filter(roxmltree::Node::is_text)
        .filter_map(|child| child.text())
        .try_fold(0usize, |size, text| size.checked_add(text.len()))
        .ok_or_else(|| CodecError::malformed("saved XML text length overflow"))?;
    ctx.retain_bytes(u64_from_index(length), "retain saved XML scalar")?;
    let value = node
        .children()
        .filter(roxmltree::Node::is_text)
        .filter_map(|child| child.text())
        .collect::<String>();
    Ok(scalar_value(node.tag_name().name(), value))
}

fn scalar_value(kind: &str, value: String) -> Option<(AttributeValue, &'static str)> {
    let text = value.trim();
    let (value, kind) = match kind {
        "lpstr" => (AttributeValue::String(value), "lpstr"),
        "lpwstr" => (AttributeValue::String(value), "lpwstr"),
        "bstr" => (AttributeValue::String(value), "bstr"),
        "i1" => (
            AttributeValue::Integer(i64::from(text.parse::<i8>().ok()?)),
            "i1",
        ),
        "i2" => (
            AttributeValue::Integer(i64::from(text.parse::<i16>().ok()?)),
            "i2",
        ),
        "i4" => (
            AttributeValue::Integer(i64::from(text.parse::<i32>().ok()?)),
            "i4",
        ),
        "i8" => (AttributeValue::Integer(text.parse().ok()?), "i8"),
        "int" => (
            AttributeValue::Integer(i64::from(text.parse::<i32>().ok()?)),
            "int",
        ),
        "ui1" => (
            AttributeValue::Integer(i64::from(text.parse::<u8>().ok()?)),
            "ui1",
        ),
        "ui2" => (
            AttributeValue::Integer(i64::from(text.parse::<u16>().ok()?)),
            "ui2",
        ),
        "ui4" => (
            AttributeValue::Integer(i64::from(text.parse::<u32>().ok()?)),
            "ui4",
        ),
        "ui8" => (
            AttributeValue::Integer(i64::try_from(text.parse::<u64>().ok()?).ok()?),
            "ui8",
        ),
        "uint" => (
            AttributeValue::Integer(i64::from(text.parse::<u32>().ok()?)),
            "uint",
        ),
        "r4" => (
            AttributeValue::float(f64::from(text.parse::<f32>().ok()?))?,
            "r4",
        ),
        "r8" => (AttributeValue::float(text.parse().ok()?)?, "r8"),
        "bool" => (
            AttributeValue::Boolean(match text {
                "true" | "1" => true,
                "false" | "0" => false,
                _ => return None,
            }),
            "bool",
        ),
        _ => return None,
    };
    Some((value, kind))
}
