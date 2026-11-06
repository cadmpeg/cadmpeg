// SPDX-License-Identifier: Apache-2.0
//! Reconstruct typed native records without reading retained source bytes.

use super::{
    attributes, number, reference_marker, CadIr, DirectoryLineFont, Entity, EntityStatus,
    FiniteReal, LossNote,
};
use cadmpeg_ir::native::NativeRecord;
use serde_json::{Map, Value};
use std::collections::BTreeSet;

mod annotations;
pub(super) mod append;
mod associations;
pub(super) mod copious;
pub(super) mod drawings;
mod fem;
mod fonts;
mod levels;
mod macros;
pub(super) mod numeric;
mod patterns;
mod products;
pub(super) mod solids;
mod structure;

/// Directory sequences model curves claim as their source carriers. A
/// copious record with a claimed carrier is covered by curve synthesis
/// (emitted, or withheld with its own loss); re-emitting it would duplicate
/// the boundary or mint standalone topology on re-decode.
pub(super) fn topology_claimed(ir: &CadIr) -> BTreeSet<u32> {
    ir.model
        .curves
        .iter()
        .filter_map(|curve| curve.source_object.as_ref())
        .filter(|source| source.format == cadmpeg_ir::CodecFormat::Iges)
        .filter_map(|source| {
            source
                .object_id
                .as_str()
                .strip_prefix('D')?
                .parse::<u32>()
                .ok()
        })
        .collect()
}
/// The Directory status a re-emitted record carries. The typed record holds
/// parameters only; its source Directory entry holds the dependence the
/// re-decode keys on (an independent copy of a boundary carrier mints
/// standalone topology). Only status combinations with an exact spelling are
/// restored; anything else keeps the encoder's status.
pub(super) fn source_status(ir: &CadIr, record: &NativeRecord) -> Option<EntityStatus> {
    let source = record.field("source_entity")?;
    let source = source.as_str()?;
    let namespace = ir.native.namespace("iges")?;
    let directory = namespace
        .arenas()
        .get("entities")?
        .iter()
        .find(|row| row.id() == source)?;
    let fields = directory.fields().ok()?;
    let subordinate = fields.get("subordinate_status")?.as_i64()?;
    let use_flag = fields.get("use_flag")?.as_i64()?;
    match (subordinate, use_flag) {
        (0, 0) => Some(EntityStatus::Independent),
        (1, 0) => Some(EntityStatus::PhysicallyDependent),
        (2, 0) => Some(EntityStatus::LogicallyDependent),
        (3, 0) => Some(EntityStatus::BothDependent),
        (0, 1) => Some(EntityStatus::Annotation),
        (0, 2) => Some(EntityStatus::Definition),
        (1, 1) => Some(EntityStatus::AnnotationDependent),
        (1, 5) => Some(EntityStatus::ParameterCurve),
        _ => None,
    }
}

pub(super) const ARENAS: &[&str] = &[
    "text_fonts",
    "line_fonts",
    "text_templates",
    "flashes",
    "units_data",
    "groups",
    "associativities",
    "fem_entities",
    "annotations",
    "copious_data",
    "macro_definitions",
    "macro_instances",
    "definition_levels",
    "subfigure_definitions",
    "subfigure_instances",
    "network_definitions",
    "network_instances",
    "connect_points",
    "solid_assemblies",
    "solid_instances",
    "external_references",
    "transformations",
    "views",
    "view_visibility",
    "segmented_visibility",
    "drawings",
    "primitive_solids",
    "procedural_solids",
    "boolean_trees",
    "selected_components",
    "rectangular_arrays",
    "circular_arrays",
];

pub(super) struct NativeExport {
    pub(super) regenerated: BTreeSet<String>,
    pub(super) losses: Vec<LossNote>,
}

pub(super) fn primitive_feature_coverage(
    ir: &CadIr,
    entities: &[Entity],
) -> (cadmpeg_ir::codec::write::ArenaDisposition, Vec<LossNote>) {
    use cadmpeg_ir::codec::write::ArenaDisposition;
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};

    let mut missing = Vec::new();
    for feature in &ir.model.features {
        let name_fits = feature.name.as_ref().is_none_or(|name| {
            !name.is_empty()
                && name.len() <= 8
                && name.trim() == name
                && name
                    .bytes()
                    .all(|byte| byte.is_ascii_graphic() || byte == b' ')
        });
        let represented = matches!(
            feature.evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::Primitive { .. })
        ) && entities.iter().any(|entity| {
            matches!(entity.type_code, 150 | 152 | 154 | 156 | 158 | 160 | 168)
                && entity
                    .source_targets
                    .iter()
                    .any(|id| id == feature.id.as_str())
        }) && feature.product_definition.is_none()
            && feature.suppressed.is_none()
            && !feature.needs_recompute
            && feature.dependencies.is_empty()
            && feature.evaluation.outputs().is_empty()
            && name_fits;
        if !represented {
            missing.push(feature.id.to_string());
        }
    }
    let coverage = if missing.is_empty() {
        ArenaDisposition::Written
    } else if missing.len() == ir.model.features.len() {
        ArenaDisposition::Dropped
    } else {
        ArenaDisposition::Reported
    };
    let losses = if missing.is_empty() {
        Vec::new()
    } else {
        vec![
            super::IgesLossCode::DesignParametricRecordsOmitted.note(format!(
                "IGES cannot fully encode features: {}",
                missing.join(", ")
            )),
        ]
    };
    (coverage, losses)
}

type Targets = super::attributes::Targets;

fn directory_label(name: &str) -> Option<String> {
    (!name.is_empty()
        && name.len() <= 8
        && name.trim() == name
        && name
            .bytes()
            .all(|byte| byte.is_ascii_graphic() || byte == b' '))
    .then(|| name.to_owned())
}

struct Fields<'a> {
    values: Map<String, Value>,
    targets: &'a Targets,
    references: BTreeSet<usize>,
    /// When set, an unresolvable reference records its identity in
    /// `missing` and encodes as a null pointer instead of failing, so the
    /// append loop can tell a record blocked only on emission order from a
    /// record with invalid data.
    lenient: bool,
    missing: Vec<String>,
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
        match self.target_index(id) {
            Ok(index) => Ok(reference_marker(index)),
            Err(_) if self.lenient => {
                self.missing.push(id.to_owned());
                Ok("0".into())
            }
            Err(error) => Err(error),
        }
    }
    fn array(&self, name: &str) -> Result<Vec<Value>, String> {
        self.value(name)
            .and_then(Value::as_array)
            .cloned()
            .ok_or_else(|| format!("{name} has no typed array"))
    }
    fn target_index(&mut self, id: &str) -> Result<usize, String> {
        let [index] = self.targets.get(id).map_or(&[][..], Vec::as_slice) else {
            return Err(format!(
                "current reference {id} has no unique emitted target"
            ));
        };
        self.references.insert(*index);
        Ok(*index)
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
    if bytes.is_empty() {
        return Err(
            "an explicit empty string has no Parameter Data Hollerith representation".into(),
        );
    }
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
        transform_link: None,
        subscript: 0,
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
        "fem_entities" => fem::shape(record)?,
        "annotations" => annotations::shape(arena, record)?,
        "copious_data" => copious::shape(record)?,
        "macro_definitions" | "macro_instances" => macros::shape(arena, record)?,
        "definition_levels" => (406, 1),
        "subfigure_definitions"
        | "subfigure_instances"
        | "network_definitions"
        | "network_instances"
        | "connect_points"
        | "solid_assemblies"
        | "solid_instances"
        | "external_references"
        | "transformations" => products::shape(arena, record)?,
        "views" | "view_visibility" | "segmented_visibility" | "drawings" => {
            drawings::shape(arena, record)?
        }
        "primitive_solids"
        | "procedural_solids"
        | "boolean_trees"
        | "selected_components"
        | "rectangular_arrays"
        | "circular_arrays" => solids::shape(arena, record)?,
        _ => return Err("record has no native inverse".into()),
    })
}

#[cfg(test)]
mod tests;
