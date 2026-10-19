// SPDX-License-Identifier: Apache-2.0
//! Reconstruct native lane content from typed records, without retained bytes.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::write::{NativeDisposition, NativeDispositions};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::CadIr;

use crate::loss::SldprtLossCode;
use crate::native::SldprtNative;
use crate::records::{FeatureInputLane, FeatureInputScalarRole, SketchInputKind};

use super::native_selection_write::{Event, Omission};
use super::write_generate::{append_generated_object_name, append_scalar_fields};

/// Re-emit an out-of-scope native design from its typed arenas. Current native
/// records are document content; the lane's old byte extent supplies no bytes.
pub(crate) fn regenerate(
    document: &CadIr,
    ir: &CadIr,
    source: &SldprtNative,
    native: &mut SldprtNative,
    losses: &mut Vec<LossNote>,
    dispositions: &mut NativeDispositions,
) -> Result<(), CodecError> {
    coverage::neutral_copies(document, ir, source, native, dispositions);
    let neutral_owners = ir
        .model
        .features
        .iter()
        .filter_map(|feature| Some((feature.native_ref.as_deref()?, feature)))
        .collect::<BTreeMap<_, _>>();
    let neutral_selections = ir
        .model
        .features
        .iter()
        .filter_map(|feature| {
            Some((
                feature.native_ref.as_deref()?,
                serde_json::to_value(feature.evaluation.definition()).ok()?,
            ))
        })
        .collect::<BTreeMap<_, _>>();
    let mut selection_owners = BTreeSet::new();
    for lane in &source.feature_input_lanes {
        for record in &lane.body_selections {
            let value =
                super::component_paths::compact_body_selection_value(&record.local_body_ids);
            if neutral_selections
                .get(record.feature_ref.as_str())
                .is_some_and(|definition| contains_reference(definition, &value))
            {
                selection_owners.insert(record.feature_ref.as_str());
            }
        }
        let mut edges = BTreeMap::<&str, Vec<&crate::records::FeatureInputEdgeSelection>>::new();
        for record in &lane.edge_selections {
            edges.entry(&record.feature_ref).or_default().push(record);
        }
        for (owner, records) in edges {
            let value = super::component_paths::compact_edge_selection_set_value(&records);
            if neutral_selections
                .get(owner)
                .is_some_and(|definition| contains_reference(definition, &value))
            {
                selection_owners.insert(owner);
            }
        }
        for record in &lane.surface_selections {
            let value = super::terminations::compact_surface_selection_value(&record.components);
            if neutral_selections
                .get(record.feature_ref.as_str())
                .is_some_and(|definition| contains_reference(definition, &value))
            {
                selection_owners.insert(record.feature_ref.as_str());
            }
        }
    }
    let allowed_owners = source
        .feature_histories
        .iter()
        .flat_map(|history| &history.features)
        .filter(|feature| {
            !ir.scope.design
                || neutral_owners
                    .get(feature.id.as_str())
                    .is_some_and(|neutral| {
                        matches!(
                            neutral.evaluation.definition(),
                            cadmpeg_ir::features::FeatureDefinition::Operation(
                                cadmpeg_ir::features::FeatureOperation::Native { .. }
                            )
                        )
                    })
        })
        .map(|feature| feature.id.as_str())
        .collect::<BTreeSet<_>>();
    for (ordinal, original) in source.feature_input_lanes.iter().enumerate() {
        let mut selected = original.clone();
        if ir.scope.design {
            retain_owners(&mut selected, source, &allowed_owners, &selection_owners);
        }
        let lane = &selected;
        if lane.classes.is_empty() && lane.names.is_empty() && lane.sketch_entities.is_empty() {
            continue;
        }
        let mut output = super::native_selection_write::events(lane, &native.feature_histories)?;
        let consumed = output
            .events
            .iter()
            .flat_map(|event| &event.records)
            .map(|(_, id)| id.as_str())
            .collect::<BTreeSet<_>>();
        let scalars = lane
            .scalars
            .iter()
            .map(|scalar| (scalar.name.as_str(), scalar))
            .collect::<BTreeMap<_, _>>();
        let feature_ids = native
            .feature_histories
            .iter()
            .flat_map(|history| &history.features)
            .map(|feature| feature.id.as_str())
            .collect::<BTreeSet<_>>();
        let marker_ids = lane
            .sketch_entities
            .iter()
            .map(|marker| marker.id())
            .collect::<BTreeSet<_>>();
        let mut events = Vec::new();
        let mut omitted = Vec::new();
        for class in &lane.classes {
            if consumed.contains(class.id.as_str()) {
                continue;
            }
            let mut bytes = Vec::new();
            append_class(&mut bytes, &class.name)?;
            events.push(Event {
                sequence: class.sequence,
                bytes,
                records: vec![("feature_input_classes".into(), class.id.clone())],
            });
        }
        for name in &lane.names {
            if let Some(scalar) = scalars.get(name.id.as_str()) {
                if scalar
                    .feature_ref
                    .as_deref()
                    .is_some_and(|id| !feature_ids.contains(id))
                {
                    omitted.push(Omission {
                        arena: "feature_input_scalars",
                        id: scalar.id.clone(),
                        reason: "owning native feature no longer resolves".into(),
                    });
                    continue;
                }
                if scalar.operands.iter().any(|operand| {
                    operand
                        .entity_ref
                        .as_deref()
                        .is_some_and(|id| !marker_ids.contains(id))
                }) {
                    omitted.push(Omission {
                        arena: "feature_input_scalars",
                        id: scalar.id.clone(),
                        reason: "typed scalar operand no longer resolves".into(),
                    });
                    continue;
                }
                let operands = scalar
                    .operands
                    .iter()
                    .map(|operand| (operand.kind, operand.entity_index))
                    .collect::<Vec<_>>();
                let mut bytes = Vec::new();
                let value = match scalar_value(ir, scalar, name, &neutral_owners) {
                    Ok(value) => value.unwrap_or(scalar.value.get()),
                    Err(reason) => {
                        omitted.push(Omission {
                            arena: "feature_input_scalars",
                            id: scalar.id.clone(),
                            reason: reason.into(),
                        });
                        continue;
                    }
                };
                append_scalar_fields(&mut bytes, &name.value, value, scalar.object_id, &operands)?;
                // The typed class is a separate event. No guessed quantity
                // or family is introduced when a scalar has no declaration.
                let trailer = super::NAME_MARKER.len()
                    + 1
                    + name.value.encode_utf16().count() * 2
                    + super::SCALAR_HEADER.len()
                    + 8;
                bytes[trailer + 29] = match scalar.role {
                    FeatureInputScalarRole::Driving => 0,
                    FeatureInputScalarRole::Display => 1,
                    FeatureInputScalarRole::Native => 2,
                };
                let mut records = vec![
                    ("feature_input_names".into(), name.id.clone()),
                    ("feature_input_scalars".into(), scalar.id.clone()),
                ];
                records.extend(scalar.operands.iter().map(|operand| {
                    (
                        "feature_input_references".into(),
                        operand.reference_ref.clone(),
                    )
                }));
                events.push(Event {
                    sequence: name.sequence,
                    bytes,
                    records,
                });
            } else {
                let Some(id) = name.object_id else {
                    omitted.push(Omission {
                        arena: "feature_input_names",
                        id: name.id.clone(),
                        reason: "object name has no typed object identity".into(),
                    });
                    continue;
                };
                let mut bytes = Vec::new();
                let Some(id) = id.value() else {
                    omitted.push(Omission {
                        arena: "feature_input_names",
                        id: name.id.clone(),
                        reason: "object name has an opaque native identity".into(),
                    });
                    continue;
                };
                let current_name = if ir.scope.design {
                    source
                        .feature_histories
                        .iter()
                        .flat_map(|history| &history.features)
                        .find(|feature| feature.source_value() == Some(id))
                        .and_then(|original| neutral_owners.get(original.id.as_str()))
                        .and_then(|feature| feature.name.as_deref())
                        .unwrap_or(name.value.as_str())
                } else {
                    name.value.as_str()
                };
                append_generated_object_name(&mut bytes, current_name, id)?;
                events.push(Event {
                    sequence: name.sequence,
                    bytes,
                    records: vec![("feature_input_names".into(), name.id.clone())],
                });
            }
        }
        for marker in &lane.sketch_entities {
            if marker
                .feature_ref
                .as_deref()
                .is_some_and(|id| !feature_ids.contains(id))
                || marker
                    .links()
                    .iter()
                    .any(|link| !marker_ids.contains(link.entity_ref.as_str()))
            {
                omitted.push(Omission {
                    arena: "sketch_input_entities",
                    id: marker.id().into(),
                    reason: "native marker owner or link no longer resolves".into(),
                });
                continue;
            }
            match marker_bytes(marker) {
                Some(bytes) => events.push(Event {
                    sequence: marker.sequence(),
                    bytes,
                    records: vec![("sketch_input_entities".into(), marker.id().into())],
                }),
                None => omitted.push(Omission {
                    arena: "sketch_input_entities",
                    id: marker.id().into(),
                    reason: "marker payload is opaque beyond its typed identity and links".into(),
                }),
            }
        }
        let emitted_scalars = events
            .iter()
            .flat_map(|event| &event.records)
            .filter(|(arena, _)| arena == "feature_input_scalars")
            .map(|(_, id)| id.as_str())
            .collect::<BTreeSet<_>>();
        for binding in &lane.relation_bindings {
            if emitted_scalars.contains(binding.scalar_ref.as_str())
                && lane
                    .classes
                    .iter()
                    .any(|class| class.id == binding.class_ref)
            {
                dispositions.set_record(
                    "sldprt",
                    "feature_input_relation_bindings",
                    &binding.id,
                    NativeDisposition::Regenerated,
                );
            } else {
                omitted.push(Omission {
                    arena: "feature_input_relation_bindings",
                    id: binding.id.clone(),
                    reason: "relation class or scalar no longer resolves".into(),
                });
            }
        }
        for relation in &lane.relation_instances {
            if feature_ids.contains(relation.feature_ref.as_str())
                && relation
                    .scalar_refs()
                    .iter()
                    .all(|id| emitted_scalars.contains(id.as_str()))
            {
                dispositions.set_record(
                    "sldprt",
                    "feature_input_relation_instances",
                    &relation.id,
                    NativeDisposition::Regenerated,
                );
            } else {
                omitted.push(Omission {
                    arena: "feature_input_relation_instances",
                    id: relation.id.clone(),
                    reason: "relation owner or scalar member no longer resolves".into(),
                });
            }
        }
        events.append(&mut output.events);
        omitted.append(&mut output.omitted);
        events.sort_by_key(|event| event.sequence);
        let mut payload = Vec::new();
        for event in events {
            payload.extend_from_slice(&event.bytes);
            for (arena, id) in event.records {
                dispositions.set_record("sldprt", &arena, &id, NativeDisposition::Regenerated);
            }
        }
        for omission in omitted {
            dispositions.set_record(
                "sldprt",
                omission.arena,
                &omission.id,
                NativeDisposition::Dropped,
            );
            losses.push(SldprtLossCode::WriterNativeRecordsOmitted.note(format!(
                "native {} {}: {}",
                omission.arena, omission.id, omission.reason
            )));
        }
        if payload.is_empty() {
            dispositions.set_record(
                "sldprt",
                "feature_input_lanes",
                &lane.id,
                NativeDisposition::Dropped,
            );
            continue;
        }
        let generated = super::write_prepare::generated_native_lane(
            &native.feature_histories,
            &format!("sldprt:generated:native-input#{ordinal}"),
            lane.configuration.clone(),
            payload,
        )?;
        dispositions.set_record(
            "sldprt",
            "feature_input_lanes",
            &lane.id,
            NativeDisposition::Regenerated,
        );
        native.feature_input_lanes.push(generated);
    }
    sketches::merge(document, ir, source, native, losses, dispositions)?;
    Ok(())
}

/// Native operations explicitly retain canonical expression bindings. Their
/// scalar target copies resolve those identities instead of keeping old values.
fn scalar_value(
    ir: &CadIr,
    scalar: &crate::records::FeatureInputScalar,
    name: &crate::records::FeatureInputName,
    owners: &BTreeMap<&str, &cadmpeg_ir::features::Feature>,
) -> Result<Option<f64>, &'static str> {
    use cadmpeg_ir::features::{Dimension, FeatureDefinition, FeatureOperation, ParameterValue};
    let Some(owner) = scalar.feature_ref.as_deref().and_then(|id| owners.get(id)) else {
        return Ok(None);
    };
    let FeatureDefinition::Operation(FeatureOperation::Native { parameters, .. }) =
        owner.evaluation.definition()
    else {
        return Ok(None);
    };
    let Some(dimension) = parameters.get(name.value.as_str()) else {
        return Ok(None);
    };
    let Dimension::Parameter(id) = dimension else {
        return Err("native scalar expression has no canonical evaluated cache");
    };
    let value = ir
        .model
        .parameters
        .iter()
        .find(|parameter| &parameter.id == id)
        .and_then(|parameter| parameter.value.as_ref())
        .ok_or(
            "canonical parameter has no evaluated scalar; an old native cache cannot supply it",
        )?;
    let value = match value {
        ParameterValue::Length(value) => Some(value.get() / 1000.0),
        ParameterValue::Angle(value) => Some(value.get()),
        ParameterValue::Real(value) => Some(value.get()),
        ParameterValue::Integer(value) => {
            crate::history::parameters::eval::exact_integer_f64(*value)
        }
        ParameterValue::Boolean(_) | ParameterValue::String(_) => None,
    }
    .ok_or("canonical parameter has no exact native f64 scalar encoding")?;
    Ok(Some(value))
}

fn contains_reference(value: &serde_json::Value, id: &str) -> bool {
    match value {
        serde_json::Value::String(value) => value == id,
        serde_json::Value::Array(values) => {
            values.iter().any(|value| contains_reference(value, id))
        }
        serde_json::Value::Object(values) => {
            values.values().any(|value| contains_reference(value, id))
        }
        _ => false,
    }
}

fn retain_owners(
    lane: &mut FeatureInputLane,
    source: &SldprtNative,
    allowed: &BTreeSet<&str>,
    selection_owners: &BTreeSet<&str>,
) {
    let permitted = |owner: Option<&str>| owner.is_some_and(|owner| allowed.contains(owner));
    let selection_permitted = |owner: Option<&str>| {
        owner.is_some_and(|owner| allowed.contains(owner) || selection_owners.contains(owner))
    };
    let scalar_names = lane
        .scalars
        .iter()
        .map(|scalar| scalar.name.as_str())
        .collect::<BTreeSet<_>>();
    // Ownership and the declared content sequence survive CADIR. Physical
    // source intervals are not available to synthesis and never infer content.
    lane.classes.retain(|class| {
        class.feature_ref.is_none() || selection_permitted(class.feature_ref.as_deref())
    });
    lane.names.retain(|name| {
        selection_permitted(name.feature_ref.as_deref())
            && (!scalar_names.contains(name.id.as_str()) || permitted(name.feature_ref.as_deref()))
    });
    lane.scalars
        .retain(|record| permitted(record.feature_ref.as_deref()));
    lane.relation_bindings
        .retain(|record| permitted(record.feature_ref.as_deref()));
    lane.relation_instances
        .retain(|record| permitted(Some(&record.feature_ref)));
    lane.body_selections
        .retain(|record| selection_permitted(Some(&record.feature_ref)));
    lane.edge_selections
        .retain(|record| selection_permitted(Some(&record.feature_ref)));
    lane.surface_selections
        .retain(|record| selection_permitted(Some(&record.feature_ref)));
    lane.references
        .retain(|record| permitted(record.feature_ref.as_deref()));
    lane.sketch_entities
        .retain(|record| permitted(record.feature_ref.as_deref()));
    lane.generated_surface_identities.retain(|record| {
        source
            .feature_histories
            .iter()
            .flat_map(|history| &history.features)
            .any(|feature| {
                feature.source_value() == Some(record.feature_source_id.value())
                    && selection_permitted(Some(&feature.id))
            })
    });
}

fn append_class(bytes: &mut Vec<u8>, name: &str) -> Result<(), CodecError> {
    // (O-capacity): a native class declaration has a u16 byte length.
    let length = u16::try_from(name.len())
        .map_err(|_| CodecError::malformed("SLDPRT native class name exceeds u16"))?;
    bytes.extend_from_slice(super::CLASS_MARKER);
    bytes.extend_from_slice(&length.to_le_bytes());
    bytes.extend_from_slice(name.as_bytes());
    Ok(())
}

fn marker_bytes(marker: &crate::records::SketchInputEntity) -> Option<Vec<u8>> {
    if matches!(
        marker.kind(),
        SketchInputKind::Native(_) | SketchInputKind::NativeHandle(_)
    ) {
        // These variants admit a framing identity without decoding the owning
        // extension payload. Synthesis cannot invent its missing content.
        return None;
    }
    let id = marker.local_id()?;
    let mut bytes = marker
        .object_index()
        .map_or_else(Vec::new, |index| index.to_le_bytes().to_vec());
    let start = bytes.len();
    if let Some(coordinates) = marker.coordinates_m.as_ref() {
        let coordinates = coordinates.get();
        super::write_generate::append_coordinate_marker(&mut bytes, marker.kind(), coordinates, id);
        if marker.links().len() > 2 {
            return None;
        }
        if let Some(links) = &marker.links {
            bytes[start + 84..start + 86]
                .copy_from_slice(&u16::try_from(marker.links().len()).ok()?.to_le_bytes());
            for (index, link) in marker.links().iter().enumerate() {
                let cell = start + 86 + index * 12;
                bytes[cell..cell + 2].copy_from_slice(&links.selector().to_le_bytes());
                bytes[cell + 2..cell + 4].copy_from_slice(&link.local_id.to_le_bytes());
                bytes[cell + 4..cell + 8].fill(0xff);
            }
            let end = start + 86 + marker.links().len() * 12;
            bytes[end..end + 6].copy_from_slice(&[0, 0, 0xfe, 0xff, 0xff, 0xff]);
        }
    } else {
        let SketchInputKind::Relation(kind) = marker.kind() else {
            return None;
        };
        let [first, second] = marker.links() else {
            return None;
        };
        super::write_generate::append_reference_marker(
            &mut bytes,
            kind,
            [first.local_id, second.local_id],
            id,
        );
        if let Some(links) = &marker.links {
            bytes[start + 68..start + 70].copy_from_slice(&links.selector().to_le_bytes());
        }
    }
    if let Some(state) = marker.state_value {
        bytes[start + 48..start + 56].copy_from_slice(&state.get().to_le_bytes());
    }
    Some(bytes)
}

mod coverage;
mod sketches;

#[cfg(test)]
mod tests;
