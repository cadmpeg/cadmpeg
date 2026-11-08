// SPDX-License-Identifier: Apache-2.0
//! Record-level native outcomes from the records actually emitted.

use super::xml::Object;
use crate::native::{ObjectRecord, PropertyRecord};
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::write::{NativeDisposition, NativeDispositions};
use cadmpeg_ir::CadIr;
use std::collections::{BTreeMap, HashMap};

pub(super) fn initial() -> NativeDispositions {
    NativeDispositions::for_namespace("fcstd", crate::native::ARENAS, NativeDisposition::Dropped)
}

pub(super) fn record(
    outcomes: &mut NativeDispositions,
    arena: &str,
    id: &str,
    status: NativeDisposition,
) {
    outcomes.set_record("fcstd", arena, id, status);
}

pub(super) fn emitted(
    ir: &CadIr,
    objects: &[Object],
    properties: &[PropertyRecord],
    names: &HashMap<String, String>,
    document: &indexmap::IndexMap<String, String>,
    entries: &BTreeMap<String, Vec<u8>>,
    outcomes: &mut NativeDispositions,
) -> Result<(), CodecError> {
    let Some(namespace) = ir.native.namespace("fcstd") else {
        return Ok(());
    };
    let native_objects = namespace.arena_as::<ObjectRecord>("objects")?;
    for record_value in &native_objects {
        if names
            .get(record_value.id().as_str())
            .is_some_and(|name| objects.iter().any(|object| &object.name == name))
        {
            let neutral =
                ir.model.features.iter().any(|feature| {
                    feature.native_ref.as_deref() == Some(record_value.id().as_str())
                }) || ir.model.product_definitions.iter().any(|definition| {
                    definition.native_ref.as_deref() == Some(record_value.id().as_str())
                }) || ir.model.occurrences.iter().any(|occurrence| {
                    occurrence.native_ref.as_deref() == Some(record_value.id().as_str())
                });
            record(
                outcomes,
                "objects",
                record_value.id().as_str(),
                if neutral {
                    NativeDisposition::Regenerated
                } else {
                    NativeDisposition::Written
                },
            );
        }
    }
    for property in properties {
        let emitted = if property.owner == crate::native::native_id("document", "0") {
            document.contains_key(&property.name)
        } else {
            names
                .get(&property.owner)
                .and_then(|name| objects.iter().find(|object| &object.name == name))
                .is_some_and(|object| object.properties.contains_key(&property.name))
        };
        if emitted {
            let attribute_id = crate::native::model_id("attribute", &property.id, "property");
            let neutral = (crate::attributes::projection_held(ir)
                && crate::attributes::consumed(ir, property, &native_objects))
                || ir
                    .model
                    .attributes
                    .iter()
                    .any(|attribute| attribute.id.as_str() == attribute_id);
            record(
                outcomes,
                "properties",
                &property.id,
                if neutral {
                    NativeDisposition::Regenerated
                } else {
                    NativeDisposition::Written
                },
            );
        }
    }
    for payload in namespace.arena_as::<crate::brep::ShapePayloadRecord>("shape_payloads")? {
        if properties
            .iter()
            .find(|property| property.id == payload.property)
            .and_then(|property| {
                names
                    .get(&property.owner)
                    .and_then(|name| objects.iter().find(|object| &object.name == name))
                    .map(|object| object.properties.contains_key(&property.name))
            })
            .unwrap_or(false)
        {
            record(
                outcomes,
                "shape_payloads",
                &payload.id,
                NativeDisposition::Regenerated,
            );
            // Polygon tables re-emit from these records with no neutral
            // counterpart (W1), so an emitted payload carries its records
            // as written.
            let (table3d, table_indexed) = match &payload.payload {
                crate::brep::ShapePayload::Empty => (Vec::new(), Vec::new()),
                crate::brep::ShapePayload::Text { facts, .. }
                | crate::brep::ShapePayload::Binary { facts, .. } => (
                    facts.polygons3d.clone(),
                    facts.polygons_on_triangulations.clone(),
                ),
            };
            for id in table3d {
                record(outcomes, "polygons3d", &id, NativeDisposition::Written);
            }
            for id in table_indexed {
                record(
                    outcomes,
                    "polygons_on_triangulations",
                    &id,
                    NativeDisposition::Written,
                );
            }
        }
    }
    for entry in namespace.arena_as::<crate::native::EntryRecord>("entries")? {
        if entries.contains_key(entry.name()) {
            record(
                outcomes,
                "entries",
                entry.id(),
                NativeDisposition::Regenerated,
            );
        }
    }
    for census in namespace.arena_as::<crate::native::CarrierCensusRecord>("carrier_census")? {
        if outcomes.record_disposition("fcstd", "shape_payloads", &census.payload)
            != NativeDisposition::Dropped
        {
            record(
                outcomes,
                "carrier_census",
                &census.id,
                NativeDisposition::Regenerated,
            );
        }
    }
    // These are typed summaries/projections of the emitted object graph. They
    // are regenerated rather than copied as an independent content source.
    for arena in [
        "document",
        "product_nodes",
        "joints",
        "applications",
        "design_census",
    ] {
        if let Some(records) = namespace.arenas().get(arena) {
            for value in records {
                let wire = serde_json::to_value(value).map_err(CodecError::malformed)?;
                let owner = wire.get("object").and_then(serde_json::Value::as_str);
                if owner.is_none_or(|owner| {
                    names
                        .get(owner)
                        .is_some_and(|name| objects.iter().any(|object| &object.name == name))
                }) {
                    record(outcomes, arena, value.id(), NativeDisposition::Regenerated);
                }
            }
        }
    }
    Ok(())
}

/// Keep a written or regenerated claim only for a record that the output
/// decodes to unchanged; every other emitted record is dropped, so the
/// omission loss names it. Synthesis regenerates carriers, entry layout and
/// property framing, which a claim inferred from emission cannot see.
pub(super) fn verify_output(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ir: &CadIr,
    bytes: &[u8],
    outcomes: &mut NativeDispositions,
) -> Result<(), CodecError> {
    let Some(before) = ir.native.namespace("fcstd") else {
        return Ok(());
    };
    let session = cadmpeg_core::encode::EncodeSession::from_context(ctx)?;
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (output_ctx, root) =
        cadmpeg_core::decode::DecodeContext::for_encode_session_root(&arena, &session, bytes)?;
    let decoded = output_ctx
        .run_part(|| {
            Ok(cadmpeg_ir::Codec::decode_with_context(&crate::FcstdCodec, &output_ctx, root).ok())
        })?
        .ok()
        .flatten();
    session.finish()?;
    let after = decoded
        .as_ref()
        .and_then(|decoded| decoded.ir().native.namespace("fcstd"));
    let mut dropped = std::collections::BTreeSet::new();
    for (arena, records) in before.arenas() {
        let peers = after
            .and_then(|after| after.arenas().get(arena))
            .map(|rows| {
                rows.iter()
                    .map(|row| (row.id(), row))
                    .collect::<HashMap<_, _>>()
            })
            .unwrap_or_default();
        for value in records {
            if outcomes.record_disposition("fcstd", arena, value.id()) == NativeDisposition::Dropped
                || peers.get(value.id()).is_some_and(|peer| *peer == value)
            {
                if outcomes.record_disposition("fcstd", arena, value.id())
                    == NativeDisposition::Dropped
                {
                    dropped.insert(value.id().to_owned());
                }
                continue;
            }
            record(outcomes, arena, value.id(), NativeDisposition::Dropped);
            dropped.insert(value.id().to_owned());
        }
    }
    // A record that names a dropped record cannot re-decode with that
    // reference intact, so it is dropped with it.
    let mut kept = Vec::new();
    for (arena, records) in before.arenas() {
        for value in records {
            if dropped.contains(value.id()) {
                continue;
            }
            let mut strings = Vec::new();
            collect_strings(
                &serde_json::to_value(value).map_err(CodecError::malformed)?,
                &mut strings,
            );
            strings.retain(|text| text != value.id());
            kept.push((arena.as_str(), value.id(), strings));
        }
    }
    loop {
        let mut changed = false;
        kept.retain(|(arena, id, strings)| {
            if strings.iter().any(|text| dropped.contains(text)) {
                record(outcomes, arena, id, NativeDisposition::Dropped);
                dropped.insert((*id).to_owned());
                changed = true;
                false
            } else {
                true
            }
        });
        if !changed {
            break;
        }
    }
    Ok(())
}

fn collect_strings(value: &serde_json::Value, output: &mut Vec<String>) {
    match value {
        serde_json::Value::String(text) => output.push(text.clone()),
        serde_json::Value::Array(values) => {
            for value in values {
                collect_strings(value, output);
            }
        }
        serde_json::Value::Object(fields) => {
            for value in fields.values() {
                collect_strings(value, output);
            }
        }
        _ => {}
    }
}

pub(super) fn gui(
    ir: &CadIr,
    names: &HashMap<String, String>,
    output: &str,
    outcomes: &mut NativeDispositions,
) -> Result<(), CodecError> {
    let Some(namespace) = ir.native.namespace("fcstd") else {
        return Ok(());
    };
    if output.is_empty() {
        return Ok(());
    }
    let document = roxmltree::Document::parse(output).map_err(CodecError::malformed)?;
    let providers =
        namespace.arena_as::<crate::native::GuiViewProviderRecord>("gui_view_providers")?;
    let properties = namespace.arena_as::<crate::native::GuiPropertyRecord>("gui_properties")?;
    for provider in &providers {
        let name = provider
            .object
            .as_ref()
            .and_then(|object| names.get(object.as_str()))
            .map_or(provider.name.as_str(), String::as_str);
        let emitted = document
            .descendants()
            .find(|node| node.has_tag_name("ViewProvider") && node.attribute("name") == Some(name));
        let Some(emitted) = emitted else {
            continue;
        };
        let neutral = ir
            .model
            .view_presentations
            .iter()
            .any(|view| view.native_ref.as_deref() == Some(provider.id.as_str()));
        record(
            outcomes,
            "gui_view_providers",
            &provider.id,
            if neutral {
                NativeDisposition::Regenerated
            } else {
                NativeDisposition::Written
            },
        );
        for property in properties
            .iter()
            .filter(|property| property.owner == provider.id)
        {
            if emitted.descendants().any(|node| {
                node.has_tag_name("Property")
                    && node.attribute("name") == Some(property.name.as_str())
            }) {
                record(
                    outcomes,
                    "gui_properties",
                    &property.id,
                    if neutral {
                        NativeDisposition::Regenerated
                    } else {
                        NativeDisposition::Written
                    },
                );
            }
        }
    }
    for source in namespace.arena_as::<crate::native::GuiDocumentRecord>("gui_documents")? {
        let neutral = ir
            .model
            .presentation_documents
            .iter()
            .any(|document| document.native_ref.as_deref() == Some(source.id.as_str()));
        record(
            outcomes,
            "gui_documents",
            &source.id,
            if neutral {
                NativeDisposition::Regenerated
            } else {
                NativeDisposition::Written
            },
        );
    }
    Ok(())
}
