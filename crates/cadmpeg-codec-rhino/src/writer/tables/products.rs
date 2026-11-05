// SPDX-License-Identifier: Apache-2.0
//! Local prototypes and relative block placements, authored from neutral products.

use super::{
    anonymous, anonymous_with_children, attributed_object_record, class_wrapper_with_direct,
    crc_chunk_with_direct, framed_object_record, neutral_object_uuid, records, utf16, uuid,
    wire_index, CadIr, CodecError, Value,
};
use cadmpeg_ir::products::{
    AssemblyGraph, ExternalDocument, Occurrence, OccurrenceParent, ProductDefinition,
    PrototypeReference,
};
use std::collections::BTreeSet;

const DEFINITION_CLASS: [u8; 16] = [
    0xf6, 0xbf, 0xf8, 0x26, 0x18, 0x26, 0x7f, 0x41, 0xa1, 0x58, 0x15, 0x3d, 0x64, 0xa9, 0x49, 0x89,
];
const REFERENCE_CLASS: [u8; 16] = [
    0x38, 0xb6, 0xcf, 0xf9, 0xd4, 0xb9, 0x40, 0x43, 0x87, 0xe3, 0xc5, 0x6e, 0x78, 0x65, 0xd9, 0x6a,
];

pub(in crate::writer) fn occurrence_uuid(ir: &CadIr, identity: &str) -> Option<[u8; 16]> {
    let occurrence = ir
        .model
        .occurrences
        .iter()
        .find(|row| row.id.as_str() == identity)?;
    let native = occurrence.native_ref.as_deref()?;
    let source = records(ir, "product_occurrences")
        .iter()
        .find(|record| record.id() == native)?
        .field("source_uuid")?;
    let uuid = crate::presentation::parse_uuid_text(source.as_str()?)?;
    if uuid.is_nil() && !matches!(occurrence.parent, OccurrenceParent::Root {}) {
        return None;
    }
    Some(uuid.to_wire())
}

fn product_uuid(ir: &CadIr, definition: &ProductDefinition) -> [u8; 16] {
    let native = definition.native_ref.as_deref().and_then(|id| {
        records(ir, "product_definitions")
            .iter()
            .find(|record| record.id() == id)
            .and_then(|record| record.fields().ok())
    });
    uuid(
        native.as_ref().and_then(|fields| fields.get("source_uuid")),
        definition.id.as_str(),
    )
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct ExternalDefinition<'a> {
    uuid: [u8; 16],
    path: &'a str,
}

fn external_parts(occurrence: &Occurrence) -> Option<ExternalDefinition<'_>> {
    let PrototypeReference::External {
        document: ExternalDocument::Path { path },
        object,
    } = &occurrence.prototype
    else {
        return None;
    };
    let uuid = match object {
        Some(object) => {
            let uuid = crate::presentation::parse_uuid_text(object)?;
            if uuid.is_nil() {
                return None;
            }
            uuid.to_wire()
        }
        None => neutral_object_uuid(occurrence.id.as_str()),
    };
    Some(ExternalDefinition {
        uuid,
        path: path.as_str(),
    })
}

fn external_definition<'a>(
    ir: &CadIr,
    graph: &AssemblyGraph<'_>,
    occurrence: &'a Occurrence,
) -> Option<ExternalDefinition<'a>> {
    let external = external_parts(occurrence)?;
    if ir
        .model
        .product_definitions
        .iter()
        .any(|definition| product_uuid(ir, definition) == external.uuid)
    {
        return None;
    }
    for prior in &ir.model.occurrences {
        if prior.id == occurrence.id {
            break;
        }
        if graph.effective_transform(&prior.id).is_none()
            || !prior
                .effective_transform()
                .is_ok_and(|transform| transform.is_some())
        {
            continue;
        }
        if let OccurrenceParent::Occurrence { occurrence: parent } = &prior.parent {
            let selected = ir
                .model
                .occurrences
                .iter()
                .find(|row| row.id == *parent)
                .is_some_and(|row| {
                    matches!(&row.prototype, PrototypeReference::Local { definition }
                    if ir.model.product_definitions.iter().any(|product| product.id == *definition))
                });
            if !selected {
                continue;
            }
        }
        if let Some(other) = external_parts(prior) {
            if other.uuid == external.uuid {
                return (other.path == external.path).then_some(external);
            }
        }
    }
    Some(external)
}

/// An invalid bounding box explicitly requests recomputation by the consumer.
fn unset_bounds(output: &mut Vec<u8>) {
    for value in [1.0f64, 0.0, 0.0, -1.0, 0.0, 0.0] {
        output.extend(value.to_le_bytes());
    }
}

fn children<'a>(
    ir: &'a CadIr,
    graph: &AssemblyGraph<'_>,
    definition: &ProductDefinition,
) -> Vec<&'a Occurrence> {
    // An occurrence tree repeats nested uses of a shared definition. The
    // native definition owns each source child slot once; each slot retains
    // its own relative placement rather than a composed world placement.
    let parent_instances = ir.model.occurrences.iter().filter(|occurrence| matches!(&occurrence.prototype, PrototypeReference::Local { definition: id } if *id == definition.id)).map(|occurrence| occurrence.id.as_str()).collect::<Vec<_>>();
    let mut result = Vec::new();
    for occurrence in &ir.model.occurrences {
        let OccurrenceParent::Occurrence { occurrence: parent } = &occurrence.parent else {
            continue;
        };
        if graph.effective_transform(&occurrence.id).is_none()
            || !parent_instances.contains(&parent.as_str())
            || !occurrence_written(ir, graph, occurrence)
        {
            continue;
        }
        if !result.iter().any(|existing: &&Occurrence| {
            existing.ordinal == occurrence.ordinal
                && existing.prototype == occurrence.prototype
                && existing.transform == occurrence.transform
                && existing.name == occurrence.name
                && existing.visible == occurrence.visible
        }) {
            result.push(occurrence);
        }
    }
    result
}

pub(in crate::writer) fn is_member(ir: &CadIr, identity: &str) -> bool {
    let referenced = referenced_definitions(ir);
    ir.model.product_definitions.iter().any(|definition| {
        referenced.contains(definition.id.as_str())
            && definition
                .bodies
                .iter()
                .any(|body| body.as_str() == identity)
    }) || ir.model.occurrences.iter().any(|occurrence| {
        occurrence.id.as_str() == identity
            && matches!(occurrence.parent, OccurrenceParent::Occurrence { .. })
    })
}

/// Whether the reference writer emits an occurrence: an effective
/// transform, a local prototype naming a held definition, and an own
/// placement. The reference writer charges the dropped cases; this shared
/// predicate keeps definition members consistent with the emitted
/// references.
fn occurrence_written(ir: &CadIr, graph: &AssemblyGraph<'_>, occurrence: &Occurrence) -> bool {
    graph.effective_transform(&occurrence.id).is_some()
        && (matches!(&occurrence.prototype, PrototypeReference::Local { definition } if ir.model.product_definitions.iter().any(|row| row.id == *definition))
            || external_definition(ir, graph, occurrence).is_some())
        && occurrence
            .effective_transform()
            .is_ok_and(|transform| transform.is_some())
}

/// Definitions named by a writable occurrence decode with placed members.
pub(in crate::writer) fn referenced_definitions(ir: &CadIr) -> BTreeSet<String> {
    let Ok(graph) = AssemblyGraph::new(&ir.model.occurrences) else {
        return BTreeSet::new();
    };
    ir.model
        .occurrences
        .iter()
        .filter(|occurrence| occurrence_written(ir, &graph, occurrence))
        .filter_map(|occurrence| match &occurrence.prototype {
            PrototypeReference::Local { definition } => Some(definition.as_str().to_owned()),
            _ => None,
        })
        .collect()
}

pub(in crate::writer) fn definition_written(
    ir: &CadIr,
    referenced: &BTreeSet<String>,
    definition: &ProductDefinition,
) -> bool {
    if referenced.contains(definition.id.as_str()) {
        return true;
    }
    if !definition.bodies.is_empty() {
        return false;
    }
    let native = definition.native_ref.as_deref().and_then(|id| {
        records(ir, "product_definitions")
            .iter()
            .find(|record| record.id() == id)
    });
    native
        .and_then(|record| record.field("member_object_ids"))
        .is_none_or(|members| members.as_array().is_some_and(Vec::is_empty))
}

fn definition_units(
    native: Option<&serde_json::Map<String, Value>>,
) -> Result<(u32, f64, Vec<u8>), CodecError> {
    let units = match native
        .and_then(|fields| fields.get("unit_system"))
        .and_then(Value::as_u64)
    {
        Some(value) => u32::try_from(value).map_err(CodecError::malformed)?,
        None => 2,
    };
    let meters = native
        .and_then(|fields| fields.get("meters_per_unit"))
        .and_then(Value::as_f64)
        .unwrap_or(0.001);
    let name = native
        .and_then(|fields| fields.get("custom_unit_name"))
        .and_then(Value::as_str)
        .unwrap_or("");
    let mut body = units.to_le_bytes().to_vec();
    body.extend(meters.to_le_bytes());
    body.extend(utf16(name)?);
    Ok((units, meters, body))
}

pub(in crate::writer) fn definitions(
    ir: &CadIr,
    written_bodies: &std::collections::BTreeSet<String>,
    version: crate::RhinoArchiveVersion,
) -> Result<Vec<Vec<u8>>, CodecError> {
    let graph = AssemblyGraph::new(&ir.model.occurrences).map_err(CodecError::malformed)?;
    let referenced = referenced_definitions(ir);
    let mut result = Vec::new();
    for (ordinal, definition) in ir.model.product_definitions.iter().enumerate() {
        if !definition_written(ir, &referenced, definition) {
            continue;
        }
        let native = definition
            .native_ref
            .as_deref()
            .and_then(|id| {
                records(ir, "product_definitions")
                    .iter()
                    .find(|record| record.id() == id)
            })
            .map(cadmpeg_ir::NativeRecord::fields)
            .transpose()?;
        let members = definition
            .bodies
            .iter()
            .filter(|body| written_bodies.contains(body.as_str()))
            .map(|body| neutral_object_uuid(body.as_str()))
            .chain(children(ir, &graph, definition).iter().map(|occurrence| {
                occurrence_uuid(ir, occurrence.id.as_str())
                    .unwrap_or_else(|| neutral_object_uuid(occurrence.id.as_str()))
            }))
            .collect::<Vec<_>>();
        if version != crate::RhinoArchiveVersion::V5 {
            let index = match native
                .as_ref()
                .and_then(|fields| fields.get("archive_index"))
                .and_then(Value::as_i64)
            {
                Some(value) => i32::try_from(value).map_err(CodecError::malformed)?,
                None => wire_index(ordinal)?,
            };
            result.push(modern_definition(
                definition,
                index,
                &members,
                native.as_ref(),
            )?);
            continue;
        }
        let mut payload = vec![0x16];
        payload.extend(product_uuid(ir, definition));
        payload.extend(wire_index(members.len())?.to_le_bytes());
        for member in members {
            payload.extend(member);
        }
        payload.extend(utf16(
            definition
                .label
                .as_deref()
                .or(definition.source_name.as_deref())
                .unwrap_or(""),
        )?);
        payload.extend(utf16(definition.description.as_deref().unwrap_or(""))?);
        for name in ["url", "url_tag"] {
            payload.extend(utf16(
                native
                    .as_ref()
                    .and_then(|fields| fields.get(name))
                    .and_then(Value::as_str)
                    .unwrap_or(""),
            )?);
        }
        unset_bounds(&mut payload);
        payload.extend(0u32.to_le_bytes());
        payload.extend(utf16("")?);
        payload.extend([0; 48]);
        let (unit_system, meters_per_unit, units) = definition_units(native.as_ref())?;
        payload.extend(unit_system.to_le_bytes());
        payload.extend(meters_per_unit.to_le_bytes());
        payload.push(0);
        let child_start = payload.len();
        payload.extend(anonymous(1, 0, &units)?);
        let child_end = payload.len();
        payload.extend(0i32.to_le_bytes());
        payload.extend(0u32.to_le_bytes());
        let direct = [&payload[..child_start], &payload[child_end..]].concat();
        result.push(crc_chunk_with_direct(
            0x2000_8076,
            &class_wrapper_with_direct(DEFINITION_CLASS, &payload, &direct)?,
            &[],
        )?);
    }
    let mut external_ids = BTreeSet::new();
    for occurrence in &ir.model.occurrences {
        if !occurrence_written(ir, &graph, occurrence) {
            continue;
        }
        if let Some(external) = external_definition(ir, &graph, occurrence) {
            if external_ids.insert(external.uuid) {
                result.push(linked_definition(
                    external,
                    wire_index(result.len())?,
                    version,
                )?);
            }
        }
    }
    Ok(result)
}

fn modern_definition(
    definition: &ProductDefinition,
    index: i32,
    members: &[[u8; 16]],
    native: Option<&serde_json::Map<String, Value>>,
) -> Result<Vec<u8>, CodecError> {
    let mut attributes = 1_i32.to_le_bytes().to_vec();
    attributes.extend(0_i32.to_le_bytes());
    attributes.extend([0, 1]);
    attributes.extend(uuid(
        native.and_then(|fields| fields.get("source_uuid")),
        definition.id.as_str(),
    ));
    attributes.extend([0, 1]);
    attributes.extend(index.to_le_bytes());
    attributes.push(1);
    attributes.extend(utf16(
        definition
            .label
            .as_deref()
            .or(definition.source_name.as_deref())
            .unwrap_or(""),
    )?);
    let mut payload = crc_chunk_with_direct(0x4000_8002, &attributes, &attributes)?;
    let attributes_span = 0..payload.len();
    let mut children = vec![attributes_span];
    payload.extend(1_u32.to_le_bytes());
    let (_, _, units) = definition_units(native)?;
    let start = payload.len();
    payload.extend(anonymous(1, 0, &units)?);
    children.push(start..payload.len());
    payload.extend(utf16(definition.description.as_deref().unwrap_or(""))?);
    for name in ["url", "url_tag"] {
        payload.extend(utf16(
            native
                .and_then(|fields| fields.get(name))
                .and_then(Value::as_str)
                .unwrap_or(""),
        )?);
    }
    unset_bounds(&mut payload);
    payload.push(1);
    payload.extend(wire_index(members.len())?.to_le_bytes());
    for member in members {
        payload.extend(member);
    }
    payload.push(0);
    let payload = anonymous_with_children(1, 0, &payload, &children)?;
    crc_chunk_with_direct(
        0x2000_8076,
        &class_wrapper_with_direct(DEFINITION_CLASS, &payload, &[])?,
        &[],
    )
}

fn linked_definition(
    external: ExternalDefinition<'_>,
    index: i32,
    version: crate::RhinoArchiveVersion,
) -> Result<Vec<u8>, CodecError> {
    let path = external.path;
    let absolute =
        path.starts_with('/') || path.starts_with("\\\\") || path.as_bytes().get(1) == Some(&b':');
    let mut units = 2_u32.to_le_bytes().to_vec();
    units.extend(0.001_f64.to_le_bytes());
    units.extend(utf16("")?);
    let units = anonymous(1, 0, &units)?;
    let (payload, direct) = if version == crate::RhinoArchiveVersion::V5 {
        let mut payload = vec![0x16];
        payload.extend(external.uuid);
        payload.extend(0_i32.to_le_bytes());
        for _ in 0..4 {
            payload.extend(utf16("")?);
        }
        unset_bounds(&mut payload);
        payload.extend(3_u32.to_le_bytes());
        payload.extend(utf16(path)?);
        payload.extend([0; 48]);
        payload.extend(2_u32.to_le_bytes());
        payload.extend(0.001_f64.to_le_bytes());
        payload.push(u8::from(!absolute));
        let start = payload.len();
        payload.extend(units);
        let end = payload.len();
        payload.extend(0_i32.to_le_bytes());
        payload.extend(0_u32.to_le_bytes());
        let direct = [&payload[..start], &payload[end..]].concat();
        (payload, direct)
    } else {
        let mut attributes = 1_i32.to_le_bytes().to_vec();
        attributes.extend(0_i32.to_le_bytes());
        attributes.extend([0, 1]);
        attributes.extend(external.uuid);
        attributes.extend([0, 1]);
        attributes.extend(index.to_le_bytes());
        attributes.push(1);
        attributes.extend(utf16("")?);
        let mut body = crc_chunk_with_direct(0x4000_8002, &attributes, &attributes)?;
        let mut children = std::iter::once(0..body.len()).collect::<Vec<_>>();
        body.extend(3_u32.to_le_bytes());
        let start = body.len();
        body.extend(units);
        children.push(start..body.len());
        for _ in 0..3 {
            body.extend(utf16("")?);
        }
        unset_bounds(&mut body);
        body.push(0);
        body.push(1);
        let mut linked = file_reference(path, absolute)?;
        let linked_child = 0..linked.len();
        linked.extend(0_i32.to_le_bytes());
        linked.extend(0_u32.to_le_bytes());
        linked.push(0);
        let start = body.len();
        body.extend(anonymous_with_children(1, 0, &linked, &[linked_child])?);
        children.push(start..body.len());
        (anonymous_with_children(1, 0, &body, &children)?, Vec::new())
    };
    crc_chunk_with_direct(
        0x2000_8076,
        &class_wrapper_with_direct(DEFINITION_CLASS, &payload, &direct)?,
        &[],
    )
}

fn file_reference(path: &str, absolute: bool) -> Result<Vec<u8>, CodecError> {
    let digest = anonymous(1, 0, &[0; 20])?;
    let mut hash = vec![0; 24];
    let first = hash.len();
    hash.extend(&digest);
    let first = first..hash.len();
    let second = hash.len();
    hash.extend(digest);
    let second = second..hash.len();
    let hash = anonymous_with_children(1, 0, &hash, &[first, second])?;
    let mut body = utf16(if absolute { path } else { "" })?;
    body.extend(utf16(if absolute { "" } else { path })?);
    let start = body.len();
    body.extend(hash);
    let hash_range = start..body.len();
    body.extend(0_u32.to_le_bytes());
    anonymous_with_children(1, 0, &body, &[hash_range])
}

/// Emitted instance-reference objects with the neutral occurrences they carry.
pub(in crate::writer) struct ReferenceExport {
    pub(in crate::writer) records: Vec<Vec<u8>>,
    pub(in crate::writer) losses: Vec<cadmpeg_ir::report::loss::LossNote>,
    /// Neutral occurrence ids actually emitted as reference objects.
    pub(in crate::writer) written: std::collections::BTreeSet<String>,
}

pub(in crate::writer) fn references(
    ir: &CadIr,
    materials: &super::materials::MaterialPlan,
) -> Result<ReferenceExport, CodecError> {
    let graph = AssemblyGraph::new(&ir.model.occurrences).map_err(CodecError::malformed)?;
    let mut losses = ir
        .model
        .occurrences
        .iter()
        .filter(|occurrence| graph.effective_transform(&occurrence.id).is_none())
        .map(|occurrence| {
            crate::loss::RhinoLossCode::ProductOccurrenceDropped.note(format!(
                "occurrence {} omitted because its placement or an ancestor placement is unknown",
                occurrence.id
            ))
        })
        .collect::<Vec<_>>();
    let mut result = Vec::new();
    let mut written = std::collections::BTreeSet::new();
    let mut selected = ir
        .model
        .occurrences
        .iter()
        .filter(|occurrence| matches!(occurrence.parent, OccurrenceParent::Root {}))
        .collect::<Vec<_>>();
    for definition in &ir.model.product_definitions {
        selected.extend(children(ir, &graph, definition));
    }
    selected.sort_by(|left, right| left.id.cmp(&right.id));
    selected.dedup_by(|left, right| left.id == right.id);
    for occurrence in selected {
        if graph.effective_transform(&occurrence.id).is_none() {
            continue;
        }
        let definition = match &occurrence.prototype {
            PrototypeReference::Local { definition } => match ir
                .model
                .product_definitions
                .iter()
                .find(|row| row.id == *definition)
            {
                Some(row) => product_uuid(ir, row),
                None => {
                    losses.push(crate::loss::RhinoLossCode::ProductOccurrenceDropped.note(format!(
                        "occurrence {} omitted: its prototype does not resolve to a held definition",
                        occurrence.id
                    )));
                    continue;
                }
            },
            PrototypeReference::External { .. } => {
                match external_definition(ir, &graph, occurrence) {
                    Some(external) => {
                        losses.push(
                            cadmpeg_ir::codec::write::ExportLossCode::RecordResidualUnavailable
                                .note(format!(
                        "external occurrence {} uses an unavailable linked-file checksum",
                        occurrence.id
                    )),
                        );
                        external.uuid
                    }
                    None => {
                        losses.push(crate::loss::RhinoLossCode::ProductOccurrenceDropped.note(format!(
                        "occurrence {} omitted: its external prototype has no unique 3DM path and UUID",
                        occurrence.id
                    )));
                        continue;
                    }
                }
            }
            PrototypeReference::Unresolved {} => {
                losses.push(
                    crate::loss::RhinoLossCode::ProductOccurrenceDropped.note(format!(
                    "occurrence {} omitted: its prototype does not resolve to a held definition",
                    occurrence.id
                )),
                );
                continue;
            }
        };
        let mut payload = vec![0x10];
        payload.extend(definition);
        let Some(transform) = occurrence
            .effective_transform()
            .map_err(CodecError::malformed)?
        else {
            losses.push(
                crate::loss::RhinoLossCode::ProductOccurrenceDropped.note(format!(
                    "occurrence {} omitted because its own placement is unknown",
                    occurrence.id
                )),
            );
            continue;
        };
        for value in transform.rows().iter().flatten() {
            payload.extend(value.to_le_bytes());
        }
        unset_bounds(&mut payload);
        let display = crate::writer::presentation::display(
            ir,
            &cadmpeg_ir::appearance::AppearanceTarget::Occurrence(occurrence.id.clone()),
            None,
        );
        let no_attributes = matches!(occurrence.parent, OccurrenceParent::Root {})
            && occurrence_uuid(ir, occurrence.id.as_str()) == Some([0; 16])
            && display.name.is_none()
            && display.color.is_none()
            && display.visible.is_none()
            && super::layer_index(ir, occurrence.id.as_str())? == -1
            && materials
                .material_index(ir, occurrence.id.as_str())
                .is_none();
        result.push(if no_attributes {
            framed_object_record(0x1000, REFERENCE_CLASS, &payload, None, None)?
        } else {
            attributed_object_record(
                ir,
                0x1000,
                REFERENCE_CLASS,
                &payload,
                occurrence.id.as_str(),
                display,
                materials,
            )?
        });
        written.insert(occurrence.id.to_string());
    }
    Ok(ReferenceExport {
        records: result,
        losses,
        written,
    })
}

#[cfg(test)]
mod tests;
