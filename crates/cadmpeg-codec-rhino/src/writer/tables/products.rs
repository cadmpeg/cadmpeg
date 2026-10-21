// SPDX-License-Identifier: Apache-2.0
//! Local prototypes and relative block placements, authored from neutral products.

use super::{
    CadIr, CodecError, Digest, Value, anonymous, attributed_object_record,
    class_wrapper_with_direct, crc_chunk_with_direct, neutral_object_uuid, records, utf16,
    wire_index,
};
use cadmpeg_ir::products::{Occurrence, OccurrenceParent, ProductDefinition, PrototypeReference};

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

fn children<'a>(ir: &'a CadIr, definition: &ProductDefinition) -> Vec<&'a Occurrence> {
    // An occurrence tree repeats nested uses of a shared definition. The
    // native definition owns each source child slot once; each slot retains
    // its own relative placement rather than a composed world placement.
    let parent_instances = ir.model.occurrences.iter().filter(|occurrence| matches!(&occurrence.prototype, PrototypeReference::Local { definition: id } if *id == definition.id)).map(|occurrence| occurrence.id.as_str()).collect::<Vec<_>>();
    let mut result = Vec::new();
    for occurrence in &ir.model.occurrences {
        let OccurrenceParent::Occurrence { occurrence: parent } = &occurrence.parent else {
            continue;
        };
        if !parent_instances.contains(&parent.as_str()) {
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
    ir.model.product_definitions.iter().any(|definition| {
        definition
            .bodies
            .iter()
            .any(|body| body.as_str() == identity)
    }) || ir.model.occurrences.iter().any(|occurrence| {
        occurrence.id.as_str() == identity
            && matches!(occurrence.parent, OccurrenceParent::Occurrence { .. })
    })
}

pub(in crate::writer) fn definitions(
    ir: &CadIr,
    written_bodies: &std::collections::BTreeSet<String>,
) -> Result<Vec<Vec<u8>>, CodecError> {
    let mut result = Vec::new();
    for definition in &ir.model.product_definitions {
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
        let mut payload = vec![0x16];
        payload.extend(neutral_object_uuid(definition.id.as_str()));
        let members = definition
            .bodies
            .iter()
            .filter(|body| written_bodies.contains(body.as_str()))
            .map(|body| neutral_object_uuid(body.as_str()))
            .chain(
                children(ir, definition)
                    .iter()
                    .map(|occurrence| neutral_object_uuid(occurrence.id.as_str())),
            )
            .collect::<Vec<_>>();
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

pub(in crate::writer) fn references(ir: &CadIr) -> Result<Vec<Vec<u8>>, CodecError> {
    let mut result = Vec::new();
    let mut selected = ir
        .model
        .occurrences
        .iter()
        .filter(|occurrence| matches!(occurrence.parent, OccurrenceParent::Root {}))
        .collect::<Vec<_>>();
    for definition in &ir.model.product_definitions {
        selected.extend(children(ir, definition));
    }
    selected.sort_by(|left, right| left.id.cmp(&right.id));
    selected.dedup_by(|left, right| left.id == right.id);
    for occurrence in selected {
        let PrototypeReference::Local { definition } = &occurrence.prototype else {
            continue;
        };
        if !ir
            .model
            .product_definitions
            .iter()
            .any(|row| row.id == *definition)
        {
            continue;
        }
        let mut payload = vec![0x10];
        payload.extend(neutral_object_uuid(definition.as_str()));
        let transform = if let Some(linked) = occurrence.linked_prototype {
            occurrence
                .transform
                .compose(linked)
                .map_err(CodecError::malformed)?
        } else {
            occurrence.transform
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
        )?);
    }
    Ok(result)
}
