// SPDX-License-Identifier: Apache-2.0
//! Reconstruct typed native records without reading retained source bytes.

use super::{
    attributes, number, reference_marker, CadIr, DirectoryLineFont, Entity, EntityStatus,
    FiniteReal, IgesLossCode, LossNote,
};
use cadmpeg_ir::native::NativeRecord;
use serde_json::{Map, Value};
use std::collections::BTreeSet;

mod append;
mod fonts;
pub(super) use append::append;
mod structure;

pub(super) const ARENAS: &[&str] = &[
    "text_fonts",
    "line_fonts",
    "text_templates",
    "flashes",
    "units_data",
    "groups",
    "associativities",
];

pub(super) struct NativeExport {
    pub(super) regenerated: BTreeSet<String>,
    pub(super) losses: Vec<LossNote>,
}

type Targets = super::attributes::Targets;

struct Fields<'a> {
    values: Map<String, Value>,
    targets: &'a Targets,
    references: BTreeSet<usize>,
}
impl Fields<'_> {
    fn value(&self, name: &str) -> Option<&Value> {
        self.values.get(name).filter(|value| !value.is_null())
    }
    fn integer(&self, name: &str) -> Result<String, String> {
        integer(self.value(name))
    }
    fn real(&self, name: &str) -> Result<String, String> {
        real(self.value(name))
    }
    fn text(&self, name: &str) -> Result<String, String> {
        text(self.value(name))
    }
    fn boolean(&self, name: &str) -> Result<String, String> {
        boolean(self.value(name))
    }
    fn pointer(&mut self, name: &str) -> Result<String, String> {
        let value = self.value(name).cloned();
        self.reference(value.as_ref())
    }
    fn reference(&mut self, value: Option<&Value>) -> Result<String, String> {
        let Some(value) = value.filter(|value| !value.is_null()) else {
            return Ok("0".into());
        };
        let id = value.as_str().ok_or("reference is not a typed identity")?;
        let [index] = self.targets.get(id).map_or(&[][..], Vec::as_slice) else {
            return Err(format!(
                "current reference {id} has no unique emitted target"
            ));
        };
        self.references.insert(*index);
        Ok(reference_marker(*index))
    }
    fn array(&self, name: &str) -> Result<Vec<Value>, String> {
        self.value(name)
            .and_then(Value::as_array)
            .cloned()
            .ok_or_else(|| format!("{name} has no typed array"))
    }
}

fn integer(value: Option<&Value>) -> Result<String, String> {
    match value.filter(|value| !value.is_null()) {
        None => Ok(String::new()),
        Some(value) => value
            .as_i64()
            .map(|value| value.to_string())
            .ok_or("field is not an integer".into()),
    }
}
fn real(value: Option<&Value>) -> Result<String, String> {
    match value.filter(|value| !value.is_null()) {
        None => Ok(String::new()),
        Some(value) => value
            .as_f64()
            .and_then(FiniteReal::new)
            .map(number)
            .ok_or("field is not a finite real".into()),
    }
}
fn boolean(value: Option<&Value>) -> Result<String, String> {
    match value.filter(|value| !value.is_null()) {
        None => Ok(String::new()),
        Some(value) => value
            .as_bool()
            .map(|value| i64::from(value).to_string())
            .ok_or("field is not a boolean".into()),
    }
}
fn text(value: Option<&Value>) -> Result<String, String> {
    let Some(value) = value.filter(|value| !value.is_null()) else {
        return Ok(String::new());
    };
    let bytes = crate::text::field(value)?;
    if !bytes
        .iter()
        .all(|byte| byte.is_ascii_graphic() || *byte == b' ')
    {
        return Err("text has no printable Fixed ASCII representation".into());
    }
    let value = std::str::from_utf8(&bytes).map_err(|error| error.to_string())?;
    Ok(format!("{}H{value}", bytes.len()))
}

fn coordinates(value: Option<&Value>, width: usize) -> Result<Vec<String>, String> {
    let values = value
        .and_then(Value::as_array)
        .filter(|values| values.len() == width)
        .ok_or("coordinate lane has no declared width")?;
    values.iter().map(|value| real(Some(value))).collect()
}
fn count(fields: &Map<String, Value>, name: &str, length: usize) -> Result<String, String> {
    if fields
        .get(name)
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        != Some(length)
    {
        return Err(format!(
            "{name} disagrees with the current typed record count"
        ));
    }
    Ok(length.to_string())
}
fn entity(type_code: u32, form: i64, parameters: Vec<String>) -> Entity {
    let mut parameter_body = String::new();
    for (index, value) in parameters.into_iter().enumerate() {
        if index != 0 {
            parameter_body.push(',');
        }
        parameter_body.push_str(&value);
    }
    parameter_body.push(';');
    Entity {
        structure: None,
        property_links: Vec::new(),
        association_links: Vec::new(),
        line_font: None,
        source_targets: Vec::new(),
        type_code,
        form,
        label: String::new(),
        status: EntityStatus::Independent,
        parameter_body: parameter_body.into_bytes(),
        transform: None,
    }
}
fn shape(arena: &str, record: &NativeRecord) -> Result<(u32, i64), String> {
    let form = || -> Result<i64, String> {
        record
            .field("form")
            .as_ref()
            .and_then(Value::as_i64)
            .ok_or("record has no typed form".into())
    };
    Ok(match arena {
        "text_fonts" => (310, 0),
        "text_templates" => (312, form()?),
        "flashes" => (125, form()?),
        "units_data" => (316, 0),
        "line_fonts" => (
            304,
            match record.field("kind").as_ref().and_then(Value::as_str) {
                Some("template") => 1,
                Some("visible_blank_pattern") => 2,
                _ => return Err("line font has no typed kind".into()),
            },
        ),
        "groups" => (
            402,
            match (
                record.field("ordered").as_ref().and_then(Value::as_bool),
                record
                    .field("back_pointers_required")
                    .as_ref()
                    .and_then(Value::as_bool),
            ) {
                (Some(false), Some(true)) => 1,
                (Some(false), Some(false)) => 7,
                (Some(true), Some(true)) => 14,
                (Some(true), Some(false)) => 15,
                _ => return Err("group has no typed ordering/back-pointer state".into()),
            },
        ),
        "associativities" => structure::shape(record)?,
        _ => return Err("record has no native inverse".into()),
    })
}

#[cfg(test)]
mod tests;
