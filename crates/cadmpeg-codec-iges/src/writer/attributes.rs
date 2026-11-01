// SPDX-License-Identifier: Apache-2.0
//! Regenerate properties and attribute tables from their current neutral values.

use super::{reference_marker, Entity, EntityStatus, IgesLossCode};
use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue, SourceAttribute};
use cadmpeg_ir::native::NativeRecord;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::{CadIr, SourceObjectAssociation};
use std::collections::{BTreeMap, BTreeSet};

pub(super) type Targets = BTreeMap<String, Vec<usize>>;
type LinkedAttributes<'a> = BTreeMap<&'a str, Vec<&'a SourceAttribute>>;

pub(super) struct AttributeExport {
    pub(super) written: BTreeSet<String>,
    pub(super) regenerated: BTreeSet<String>,
    pub(super) preserved: BTreeSet<String>,
    pub(super) losses: Vec<LossNote>,
}

impl AttributeExport {
    fn emitted(&mut self, id: &str, unchanged: bool) {
        if unchanged {
            self.written.insert(id.to_owned());
        } else {
            self.regenerated.insert(id.to_owned());
        }
    }
}

fn native_encoding_matches(
    record: &NativeRecord,
    family: fields::Family,
    current: &Entity,
    encode: impl Fn(&fields::Fields<'_>) -> Result<Entity, String>,
    owners_match: impl Fn(&fields::Fields<'_>) -> bool,
) -> bool {
    let Ok(groups) = fields::groups(record, &LinkedAttributes::new(), family) else {
        return false;
    };
    let [native] = groups.as_slice() else {
        return false;
    };
    owners_match(native)
        && encode(native).is_ok_and(|native| {
            native.type_code == current.type_code
                && native.form == current.form
                && native.structure == current.structure
                && native.parameter_body == current.parameter_body
        })
}

fn owners(
    ir: &CadIr,
    fields: &fields::Fields<'_>,
    targets: &Targets,
) -> Option<Vec<usize>> {
    if fields.has_native_values() {
        let mut owners = Vec::new();
        if let Some(native) = fields.native_value("owners") {
            for owner in native.as_array()? {
                let indices = targets.get(owner.as_str()?)?.as_slice();
                if indices.is_empty() {
                    return None;
                }
                owners.extend_from_slice(indices);
            }
        }
        Some(owners)
    } else if fields.is_self_owned() {
        Some(Vec::new())
    } else {
        fields
            .owner
            .as_ref()
            .and_then(|target| target_id(ir, target))
            .and_then(|id| targets.get(id))
            .cloned()
            .filter(|owners| !owners.is_empty())
    }
}

fn target_id<'a>(ir: &CadIr, target: &'a AttributeTarget) -> Option<&'a str> {
    match target {
        AttributeTarget::Document => None,
        AttributeTarget::Body(id) => Some(id.as_str()),
        AttributeTarget::Face(id) => Some(id.as_str()),
        AttributeTarget::Shell(id) => Some(id.as_str()),
        AttributeTarget::Loop(id) => Some(id.as_str()),
        AttributeTarget::Coedge(id) => Some(id.as_str()),
        AttributeTarget::Edge(id) => Some(id.as_str()),
        AttributeTarget::Vertex(id) => Some(id.as_str()),
        AttributeTarget::Region(id) => Some(id.as_str()),
        AttributeTarget::Surface(id) => Some(id.as_str()),
        AttributeTarget::Curve(id) => Some(id.as_str()),
        AttributeTarget::Point(id) => Some(id.as_str()),
        AttributeTarget::Feature(id) => Some(id.as_str()),
        AttributeTarget::Sketch(id) => Some(id.as_str()),
        AttributeTarget::Parameter(id) => Some(id.as_str()),
        AttributeTarget::ProductDefinition(id) => Some(id.as_str()),
        AttributeTarget::Occurrence(id) => Some(id.as_str()),
        AttributeTarget::Tessellation(id) => Some(id),
        AttributeTarget::Pmi(id) => Some(id.as_str()),
        AttributeTarget::SemanticAnnotation(id) => Some(id.as_str()),
        AttributeTarget::Drawing(id) => Some(id.as_str()),
        AttributeTarget::ViewPresentation(id) => Some(id.as_str()),
        AttributeTarget::Record { arena, id } => {
            ir.model.contains_record(*arena, id).then_some(id.as_str())
        }
    }
}

fn source_alias(source: Option<&SourceObjectAssociation>) -> Option<String> {
    let source = source?;
    if source.format != cadmpeg_ir::CodecFormat::Iges {
        return None;
    }
    let sequence = source
        .object_id
        .as_str()
        .strip_prefix('D')?
        .parse::<u32>()
        .ok()?;
    Some(format!("iges:entity:directory#{sequence}"))
}

pub(super) fn targets(ir: &CadIr, entities: &[Entity]) -> Targets {
    let aliases: BTreeMap<_, _> = ir
        .model
        .points
        .iter()
        .filter_map(|point| {
            Some((
                point.id.as_str(),
                source_alias(point.source_object.as_ref())?,
            ))
        })
        .chain(ir.model.curves.iter().filter_map(|curve| {
            Some((
                curve.id.as_str(),
                source_alias(curve.source_object.as_ref())?,
            ))
        }))
        .chain(ir.model.surfaces.iter().filter_map(|surface| {
            Some((
                surface.id.as_str(),
                source_alias(surface.source_object.as_ref())?,
            ))
        }))
        .collect();
    let mut result = BTreeMap::<String, Vec<usize>>::new();
    for (index, entity) in entities.iter().enumerate() {
        for target in &entity.source_targets {
            result.entry(target.clone()).or_default().push(index);
            if let Some(alias) = aliases.get(target.as_str()) {
                result.entry(alias.clone()).or_default().push(index);
            }
        }
    }
    for indices in result.values_mut() {
        indices.sort_unstable();
        indices.dedup();
    }
    result
}

fn string_field(record: &NativeRecord, field: &str) -> Option<String> {
    record.field(field)?.as_str().map(str::to_owned)
}

fn hollerith(value: &str) -> Result<String, String> {
    if !value
        .bytes()
        .all(|byte| byte.is_ascii_graphic() || byte == b' ')
    {
        return Err("text cannot be encoded as printable Fixed ASCII".into());
    }
    Ok(format!("{}H{value}", value.len()))
}

fn typed_value(value: &AttributeValue, targets: &Targets) -> Result<(i64, String), String> {
    Ok(match value {
        AttributeValue::Integer(value) => (1, value.to_string()),
        AttributeValue::Float(value) => (2, super::number(*value)),
        AttributeValue::String(value) => (3, hollerith(value)?),
        AttributeValue::Boolean(value) => (6, i64::from(*value).to_string()),
        AttributeValue::Reference(value) => {
            let indices = targets
                .get(value)
                .ok_or_else(|| format!("attribute reference {value} has no emitted target"))?;
            let [index] = indices.as_slice() else {
                return Err(format!(
                    "attribute reference {value} has no unique emitted target"
                ));
            };
            (4, reference_marker(*index))
        }
        AttributeValue::Vector(_) => {
            return Err("a numeric vector has no exact Generic Data scalar type".into());
        }
        AttributeValue::Map(_)
        | AttributeValue::Expression(_)
        | AttributeValue::SubReference { .. }
        | AttributeValue::ExternalReference { .. }
        | AttributeValue::EmptyReference
        | AttributeValue::Json(_) => {
            return Err(
                "a structured attribute value has no exact Generic Data scalar type".into(),
            );
        }
    })
}

fn entity(type_code: u32, form: i64, parameters: String) -> Entity {
    Entity {
        structure: None,
        property_links: Vec::new(),
        association_links: Vec::new(),
        line_font: None,
        source_targets: Vec::new(),
        type_code,
        form,
        label: String::new(),
        status: EntityStatus::PhysicallyDependent,
        parameter_body: parameters.into_bytes(),
        transform: None,
    }
}

fn append_owned(
    entities: &mut Vec<Entity>,
    mut entity: Entity,
    owners: &[usize],
    version: crate::IgesVersion,
) -> Result<usize, String> {
    if !version.admits(&entity) {
        return Err(format!(
            "the selected dialect does not hold Type {} Form {}",
            entity.type_code, entity.form
        ));
    }
    if entity.type_code == 406 {
        let is_dimension =
            |code| matches!(code, 202 | 204 | 206 | 208 | 210 | 216 | 218 | 220 | 222);
        let allowed = match entity.form {
            22 | 32 => {
                !owners.is_empty()
                    && owners.iter().all(|owner| {
                        entities
                            .get(*owner)
                            .is_some_and(|entity| entity.type_code == 404)
                    })
            }
            23 => {
                !owners.is_empty()
                    && owners.iter().all(|owner| {
                        entities.get(*owner).is_some_and(|entity| {
                            entity.type_code == 402 && matches!(entity.form, 1 | 7 | 14 | 15)
                        })
                    })
            }
            26 => {
                !owners.is_empty()
                    && owners.iter().all(|owner| {
                        entities
                            .get(*owner)
                            .is_some_and(|entity| matches!(entity.type_code, 116 | 132))
                    })
            }
            27 => owners.len() == 1,
            28..=30 => {
                !owners.is_empty()
                    && owners.iter().all(|owner| {
                        entities
                            .get(*owner)
                            .is_some_and(|entity| is_dimension(entity.type_code))
                    })
            }
            31 => {
                owners.len() == 1
                    && entities
                        .get(owners[0])
                        .is_some_and(|entity| is_dimension(entity.type_code))
            }
            33 => {
                owners.len() == 1
                    && entities
                        .get(owners[0])
                        .is_some_and(|entity| entity.type_code == 404)
            }
            34 => {
                owners.len() == 1
                    && entities
                        .get(owners[0])
                        .is_some_and(|entity| entity.type_code == 212)
            }
            35 => {
                owners.len() == 1
                    && entities
                        .get(owners[0])
                        .is_some_and(|entity| matches!(entity.type_code, 212 | 312))
            }
            _ => true,
        };
        if !allowed {
            return Err(format!(
                "Type 406 Form {} has no admissible current owner",
                entity.form
            ));
        }
        if owners.is_empty() {
            entity.status = EntityStatus::Independent;
        }
    }
    let index = entities.len();
    for owner in owners {
        let entity = entities
            .get_mut(*owner)
            .ok_or("attribute owner is not emitted")?;
        entity.property_links.push(index);
    }
    entities.push(entity);
    Ok(index)
}

fn omitted(losses: &mut Vec<LossNote>, id: &str, reason: impl std::fmt::Display) {
    losses.push(
        IgesLossCode::WriterAttributeOmitted
            .note(format!("IGES attribute {id} is omitted: {reason}")),
    );
}

struct MissingPositions<'a>(&'a [Option<AttributeValue>]);

impl std::fmt::Display for MissingPositions<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("[")?;
        let mut separator = "";
        for (position, value) in self.0.iter().enumerate() {
            if value.is_none() {
                write!(f, "{separator}{position}")?;
                separator = ", ";
            }
        }
        f.write_str("]")
    }
}

fn scalar_attribute_target(attribute: &SourceAttribute, ir: &CadIr) -> bool {
    let Some(native_ref) = attribute.native_ref.as_deref() else {
        return false;
    };
    let Some(namespace) = ir.native.namespace("iges") else {
        return false;
    };
    namespace.arenas().iter().any(|(arena, records)| {
        records.iter().any(|record| {
            record.id() == native_ref
                && match arena.as_str() {
                    "properties" => {
                        record.field("form").and_then(|value| value.as_i64()) != Some(27)
                    }
                    "attribute_table_definitions" | "attribute_table_instances" => true,
                    _ => false,
                }
        })
    })
}

pub(super) fn append(
    ir: &CadIr,
    entities: &mut Vec<Entity>,
    version: crate::IgesVersion,
) -> AttributeExport {
    let mut targets = targets(ir, entities);
    let mut result = AttributeExport {
        written: BTreeSet::new(),
        regenerated: BTreeSet::new(),
        preserved: BTreeSet::new(),
        losses: Vec::new(),
    };
    for attribute in &ir.model.attributes {
        if attribute.values.iter().any(Option::is_none) {
            let positions = MissingPositions(&attribute.values);
            if scalar_attribute_target(attribute, ir) {
                omitted(
                    &mut result.losses,
                    attribute.id.as_str(),
                    format_args!("its scalar target cannot hold None positions {positions}"),
                );
            } else {
                omitted(
                    &mut result.losses,
                    attribute.id.as_str(),
                    format_args!("None positions {positions} are dropped from its ordered values"),
                );
            }
        }
    }
    labels::apply(ir, entities, &targets, &mut result);
    let namespace = ir.native.namespace("iges");
    let mut attributes = LinkedAttributes::new();
    for attribute in &ir.model.attributes {
        if let Some(native_ref) = attribute.native_ref.as_deref() {
            attributes.entry(native_ref).or_default().push(attribute);
        }
    }
    if let Some(namespace) = namespace {
        tables::append(
            namespace,
            &attributes,
            entities,
            &mut targets,
            version,
            &mut result,
        );
        for record in namespace.arenas().get("properties").into_iter().flatten() {
            match fields::groups(record, &attributes, fields::Family::Property) {
                Ok(groups) => {
                    let mut complete = true;
                    let mut unchanged = groups.len() == 1;
                    for fields in groups {
                        let Some(owners) = owners(ir, &fields, &targets) else {
                            omitted(
                                &mut result.losses,
                                record.id(),
                                "its current owner has no emitted entity",
                            );
                            complete = false;
                            continue;
                        };
                        let encoded =
                            properties::encode(record, &fields, &targets).and_then(|entity| {
                                unchanged &= native_encoding_matches(
                                    record,
                                    fields::Family::Property,
                                    &entity,
                                    |native| properties::encode(record, native, &targets),
                                    |native| {
                                        self::owners(ir, native, &targets).as_ref()
                                            == Some(&owners)
                                    },
                                );
                                append_owned(entities, entity, &owners, version)
                            });
                        match encoded {
                            Ok(index) => {
                                result.preserved.extend(fields.attribute_ids());
                                targets
                                    .entry(record.id().to_owned())
                                    .or_default()
                                    .push(index);
                                if let Some(source) = string_field(record, "source_entity") {
                                    targets.entry(source).or_default().push(index);
                                }
                            }
                            Err(reason) => {
                                omitted(&mut result.losses, record.id(), reason);
                                complete = false;
                            }
                        }
                    }
                    if complete {
                        result.emitted(record.id(), unchanged);
                    }
                }
                Err(reason) => omitted(&mut result.losses, record.id(), reason),
            }
        }
        for record in namespace
            .arenas()
            .get("product_properties")
            .into_iter()
            .flatten()
        {
            if let Some(source) = string_field(record, "source_entity") {
                if let Some(property) = namespace
                    .arenas()
                    .get("properties")
                    .into_iter()
                    .flatten()
                    .find(|property| {
                        string_field(property, "source_entity").as_ref() == Some(&source)
                            && (result.written.contains(property.id())
                                || result.regenerated.contains(property.id()))
                    })
                {
                    result.emitted(record.id(), result.written.contains(property.id()));
                }
            }
        }
    }
    for attribute in &ir.model.attributes {
        if result.preserved.contains(attribute.id.as_str())
            || (attribute.values.iter().any(Option::is_none)
                && scalar_attribute_target(attribute, ir))
        {
            continue;
        }
        let owners = target_id(ir, &attribute.target)
            .and_then(|id| targets.get(id))
            .cloned()
            .unwrap_or_default();
        if owners.is_empty() {
            omitted(
                &mut result.losses,
                attribute.id.as_str(),
                "its current owner has no emitted entity",
            );
            continue;
        }
        let values = attribute
            .values
            .iter()
            .flatten()
            .map(|value| typed_value(value, &targets))
            .collect::<Result<Vec<_>, _>>();
        let emitted = values.and_then(|mut values| {
            if attribute.name.trim().is_empty() {
                return Err("Generic Data needs a nonempty name".into());
            }
            if values.is_empty() {
                if attribute.values.iter().any(Option::is_none) {
                    return Err("no value remains after dropping unreadable positions".into());
                }
                values.push((0, String::new()));
            }
            let mut parameters = vec![
                (2 + 2 * values.len()).to_string(),
                hollerith(&attribute.name)?,
                values.len().to_string(),
            ];
            for (kind, value) in values {
                parameters.extend([kind.to_string(), value]);
            }
            append_owned(
                entities,
                entity(406, 27, format!("{};", parameters.join(","))),
                &owners,
                version,
            )
        });
        match emitted {
            Ok(_) => {
                result.preserved.insert(attribute.id.to_string());
            }
            Err(reason) => omitted(&mut result.losses, attribute.id.as_str(), reason),
        }
    }
    result
}

mod fields;
mod labels;
mod properties;
mod tables;

#[cfg(test)]
mod tests;
