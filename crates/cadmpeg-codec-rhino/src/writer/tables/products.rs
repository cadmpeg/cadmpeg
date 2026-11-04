// SPDX-License-Identifier: Apache-2.0
//! Local prototypes and relative block placements, authored from neutral products.

use super::{
    anonymous, anonymous_with_children, attributed_object_record, class_wrapper_with_direct,
    crc_chunk_with_direct, neutral_object_uuid, records, utf16, wire_index, CadIr, CodecError,
    Value,
};
use cadmpeg_ir::products::{
    AssemblyGraph, Occurrence, OccurrenceParent, ProductDefinition, PrototypeReference,
};
use std::collections::BTreeSet;

const DEFINITION_CLASS: [u8; 16] = [
    0xf6, 0xbf, 0xf8, 0x26, 0x18, 0x26, 0x7f, 0x41, 0xa1, 0x58, 0x15, 0x3d, 0x64, 0xa9, 0x49, 0x89,
];
const REFERENCE_CLASS: [u8; 16] = [
    0x38, 0xb6, 0xcf, 0xf9, 0xd4, 0xb9, 0x40, 0x43, 0x87, 0xe3, 0xc5, 0x6e, 0x78, 0x65, 0xd9, 0x6a,
];

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
fn occurrence_written(
    ir: &CadIr,
    graph: &AssemblyGraph<'_>,
    occurrence: &Occurrence,
) -> bool {
    graph.effective_transform(&occurrence.id).is_some()
        && matches!(&occurrence.prototype, PrototypeReference::Local { definition } if ir.model.product_definitions.iter().any(|row| row.id == *definition))
        && occurrence
            .effective_transform()
            .is_ok_and(|transform| transform.is_some())
}

/// Definitions named by a writable occurrence: only these decode with
/// placed members. Members of any other definition would suppress their
/// object on re-decode, so the writer leaves those definitions out and
/// their bodies decode as free geometry.
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

pub(in crate::writer) fn definitions(
    ir: &CadIr,
    written_bodies: &std::collections::BTreeSet<String>,
    version: crate::RhinoArchiveVersion,
) -> Result<Vec<Vec<u8>>, CodecError> {
    let graph = AssemblyGraph::new(&ir.model.occurrences).map_err(CodecError::malformed)?;
    let referenced = referenced_definitions(ir);
    let mut result = Vec::new();
    for (ordinal, definition) in ir.model.product_definitions.iter().enumerate() {
        if !referenced.contains(definition.id.as_str()) {
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
            .chain(
                children(ir, &graph, definition)
                    .iter()
                    .map(|occurrence| neutral_object_uuid(occurrence.id.as_str())),
            )
            .collect::<Vec<_>>();
        // A definition no written reference names would suppress its
        // members on re-decode; leave it out so they decode as free
        // geometry. The native record keeps a dropped disposition, so
        // the wrapper names it.
        if !referenced.contains(definition.id.as_str()) {
            continue;
        }
        if version != crate::RhinoArchiveVersion::V5 {
            result.push(modern_definition(
                definition,
                wire_index(ordinal)?,
                &members,
                native.as_ref(),
            )?);
            continue;
        }
        let mut payload = vec![0x16];
        payload.extend(neutral_object_uuid(definition.id.as_str()));
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
        payload.extend(2u32.to_le_bytes());
        payload.extend(0.001f64.to_le_bytes());
        payload.push(0);
        let child_start = payload.len();
        let mut units = 2u32.to_le_bytes().to_vec();
        units.extend(0.001f64.to_le_bytes());
        units.extend(utf16("")?);
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
    attributes.extend(neutral_object_uuid(definition.id.as_str()));
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
    let mut units = 2_u32.to_le_bytes().to_vec();
    units.extend(0.001_f64.to_le_bytes());
    units.extend(utf16("")?);
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
        let PrototypeReference::Local { definition } = &occurrence.prototype else {
            losses.push(
                crate::loss::RhinoLossCode::ProductOccurrenceDropped.note(format!(
                    "occurrence {} omitted because its prototype names a document its definition does not hold",
                    occurrence.id
                )),
            );
            continue;
        };
        if !ir
            .model
            .product_definitions
            .iter()
            .any(|row| row.id == *definition)
        {
            losses.push(
                crate::loss::RhinoLossCode::ProductOccurrenceDropped.note(format!(
                    "occurrence {} omitted because its prototype names a definition the document does not hold",
                    occurrence.id
                )),
            );
            continue;
        }
        let mut payload = vec![0x10];
        payload.extend(neutral_object_uuid(definition.as_str()));
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
        result.push(attributed_object_record(
            ir,
            0x1000,
            REFERENCE_CLASS,
            &payload,
            occurrence.id.as_str(),
            display,
            materials,
        )?);
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
