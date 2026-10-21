// SPDX-License-Identifier: Apache-2.0
//! Reconstruct native lane content from typed records, without retained bytes.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::CodecError;
use cadmpeg_ir::CadIr;
use cadmpeg_ir::codec::write::{NativeDisposition, NativeDispositions};
use cadmpeg_ir::report::loss::LossNote;

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
    coverage::neutral_copies(document, ir, source, native, losses, dispositions);
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
        let marker_ids = reconstructible_marker_ids(lane, &feature_ids);
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
                    omit_scalar(
                        &mut events,
                        &mut omitted,
                        scalar,
                        name,
                        "owning native feature no longer resolves",
                    )?;
                    continue;
                }
                let value = match scalar_value(ir, lane, scalar, name, &neutral_owners) {
                    Ok(value) => value.unwrap_or(scalar.value.get()),
                    Err(reason) => {
                        omit_scalar(&mut events, &mut omitted, scalar, name, reason)?;
                        continue;
                    }
                };
                if scalar.operands.iter().any(|operand| {
                    operand
                        .entity_ref
                        .as_deref()
                        .is_none_or(|id| !marker_ids.contains(id))
                }) {
                    omit_scalar(
                        &mut events,
                        &mut omitted,
                        scalar,
                        name,
                        "typed scalar operand no longer resolves",
                    )?;
                    continue;
                }
                let operands = scalar
                    .operands
                    .iter()
                    .map(|operand| (operand.kind, operand.entity_index))
                    .collect::<Vec<_>>();
                let mut bytes = Vec::new();
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
                let id = u32::from(id);
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
        merge_configuration_lane(native, generated)?;
    }
    sketches::merge(document, ir, source, native, losses, dispositions)?;
    Ok(())
}

/// A patch cannot retain an opaque marker body or dangling scalar index as
/// content. The same typed reconstruction proof governs both output paths.
pub(crate) fn require_reconstructible_content(
    lane: &FeatureInputLane,
    histories: &[crate::records::FeatureHistory],
) -> Result<(), CodecError> {
    let feature_ids = histories
        .iter()
        .flat_map(|history| &history.features)
        .map(|feature| feature.id.as_str())
        .collect::<BTreeSet<_>>();
    let markers = reconstructible_marker_ids(lane, &feature_ids);
    if let Some(marker) = lane
        .sketch_entities
        .iter()
        .find(|marker| !markers.contains(marker.id()))
    {
        return Err(CodecError::UnsupportedContent(format!(
            "native marker {} has no reconstructible typed payload",
            marker.id()
        )));
    }
    if let Some(scalar) = lane.scalars.iter().find(|scalar| {
        scalar.operands.iter().any(|operand| {
            operand
                .entity_ref
                .as_deref()
                .is_none_or(|id| !markers.contains(id))
        })
    }) {
        return Err(CodecError::UnsupportedContent(format!(
            "native scalar {} has an unresolved typed marker operand",
            scalar.id
        )));
    }
    Ok(())
}

fn reconstructible_marker_ids<'a>(
    lane: &'a FeatureInputLane,
    feature_ids: &BTreeSet<&str>,
) -> BTreeSet<&'a str> {
    let mut marker_ids = lane
        .sketch_entities
        .iter()
        .filter(|marker| {
            marker_bytes(marker).is_some()
                && marker
                    .feature_ref
                    .as_deref()
                    .is_none_or(|id| feature_ids.contains(id))
        })
        .map(crate::records::SketchInputEntity::id)
        .collect::<BTreeSet<_>>();
    let mut dependents = BTreeMap::<&str, Vec<&str>>::new();
    let mut unavailable = Vec::new();
    for marker in &lane.sketch_entities {
        if !marker_ids.contains(marker.id()) {
            unavailable.push(marker.id());
        }
        for link in marker.links() {
            dependents
                .entry(link.entity_ref.as_str())
                .or_default()
                .push(marker.id());
            if !marker_ids.contains(link.entity_ref.as_str()) {
                unavailable.push(marker.id());
            }
        }
    }
    while let Some(id) = unavailable.pop() {
        if marker_ids.remove(id) {
            unavailable.extend(dependents.get(id).into_iter().flatten().copied());
        }
    }
    marker_ids
}

/// A configuration slot has one state lane. Combine only bytes authored in
/// this synthesis pass, so native-only statements cannot create a second,
/// ambiguous lane beside the current canonical configuration caches.
fn merge_configuration_lane(
    native: &mut SldprtNative,
    generated: FeatureInputLane,
) -> Result<(), CodecError> {
    let candidates = native
        .feature_input_lanes
        .iter()
        .enumerate()
        .filter(|(_, lane)| {
            generated.configuration.is_some() && lane.configuration == generated.configuration
        })
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    if let [index] = candidates.as_slice() {
        let previous = &native.feature_input_lanes[*index];
        if previous.id.starts_with("sldprt:generated:") && !previous.native_payload.is_empty() {
            let payload = combine_authored_owners(previous, &generated, &native.feature_histories)?;
            let combined = super::write_prepare::generated_native_lane(
                &native.feature_histories,
                &previous.id,
                previous.configuration.clone(),
                payload,
            )?;
            native.feature_input_lanes[*index] = combined;
            return Ok(());
        }
    }
    native.feature_input_lanes.push(generated);
    Ok(())
}

struct AuthoredOwner<'a> {
    id: &'a str,
    start: usize,
    name_start: usize,
    name_end: usize,
    end: usize,
}

/// These positions belong only to fresh target bytes. Each current native
/// feature contributes at most one unique object name in either authored lane.
fn authored_owners<'a>(
    lane: &'a FeatureInputLane,
    histories: &'a [crate::records::FeatureHistory],
) -> Result<Vec<AuthoredOwner<'a>>, CodecError> {
    let names = super::scalars::FeatureObjectNames::new(&lane.names);
    let mut first_classes = BTreeMap::<&str, usize>::new();
    for class in &lane.classes {
        if let (Some(owner), Some(offset)) = (
            class.feature_ref.as_deref(),
            class.offset.and_then(|offset| usize::try_from(offset).ok()),
        ) {
            first_classes
                .entry(owner)
                .and_modify(|first| *first = (*first).min(offset))
                .or_insert(offset);
        }
    }
    let mut owners = Vec::new();
    for feature in histories.iter().flat_map(|history| &history.features) {
        let Some(name) = names.of(feature) else {
            continue;
        };
        let name_start = name
            .offset
            .and_then(|offset| usize::try_from(offset).ok())
            .ok_or_else(|| {
                CodecError::malformed("authored native object has no target position")
            })?;
        let start = first_classes
            .get(feature.id.as_str())
            .copied()
            .filter(|offset| *offset < name_start)
            .unwrap_or(name_start);
        let name_end = name
            .value
            .encode_utf16()
            .count()
            .checked_mul(2)
            .and_then(|length| length.checked_add(super::NAME_MARKER.len() + 1 + 12))
            .and_then(|length| name_start.checked_add(length))
            .ok_or_else(|| CodecError::malformed("authored native object name extent overflows"))?;
        owners.push(AuthoredOwner {
            id: &feature.id,
            start,
            name_start,
            name_end,
            end: lane.native_payload.len(),
        });
    }
    owners.sort_by_key(|owner| owner.start);
    for index in 0..owners.len() {
        if let Some(next) = owners.get(index + 1) {
            owners[index].end = next.start;
        }
        if owners[index].name_end > owners[index].end {
            return Err(CodecError::malformed(
                "authored native object framing overlaps another owner",
            ));
        }
    }
    Ok(owners)
}

fn combine_authored_owners(
    previous: &FeatureInputLane,
    generated: &FeatureInputLane,
    histories: &[crate::records::FeatureHistory],
) -> Result<Vec<u8>, CodecError> {
    let current = authored_owners(previous, histories)?;
    let extra = authored_owners(generated, histories)?;
    let extra_by_owner = extra
        .iter()
        .map(|owner| (owner.id, owner))
        .collect::<BTreeMap<_, _>>();
    let current_ids = current
        .iter()
        .map(|owner| owner.id)
        .collect::<BTreeSet<_>>();
    let capacity = previous
        .native_payload
        .len()
        .checked_add(generated.native_payload.len())
        .ok_or_else(|| CodecError::malformed("authored configuration lane size overflows"))?;
    let mut payload = Vec::new();
    payload
        .try_reserve_exact(capacity)
        .map_err(|_| CodecError::malformed("authored configuration lane allocation failed"))?;
    let prefix_end = current
        .first()
        .map_or(previous.native_payload.len(), |owner| owner.start);
    payload.extend_from_slice(&previous.native_payload[..prefix_end]);
    let extra_prefix_end = extra
        .first()
        .map_or(generated.native_payload.len(), |owner| owner.start);
    payload.extend_from_slice(&generated.native_payload[..extra_prefix_end]);
    for owner in &current {
        payload.extend_from_slice(&previous.native_payload[owner.start..owner.end]);
        if let Some(addition) = extra_by_owner.get(owner.id) {
            // The target already carries this exact native object identity
            // and name. Retain its other statements inside the same owner.
            payload
                .extend_from_slice(&generated.native_payload[addition.start..addition.name_start]);
            payload.extend_from_slice(&generated.native_payload[addition.name_end..addition.end]);
        }
    }
    for owner in extra.iter().filter(|owner| !current_ids.contains(owner.id)) {
        payload.extend_from_slice(&generated.native_payload[owner.start..owner.end]);
    }
    Ok(payload)
}

/// Native operations explicitly retain canonical expression bindings. Their
/// scalar target copies resolve those identities instead of keeping old values.
fn scalar_value(
    ir: &CadIr,
    lane: &crate::records::FeatureInputLane,
    scalar: &crate::records::FeatureInputScalar,
    name: &crate::records::FeatureInputName,
    owners: &BTreeMap<&str, &cadmpeg_ir::features::Feature>,
) -> Result<Option<f64>, &'static str> {
    use cadmpeg_ir::features::{Dimension, FeatureDefinition, FeatureOperation, ParameterValue};
    let Some(owner) = scalar.feature_ref.as_deref().and_then(|id| owners.get(id)) else {
        return Ok(None);
    };
    if let Some(value) = ir
        .model
        .parameters
        .iter()
        .find(|parameter| {
            parameter.native_ref.as_deref() == Some(scalar.id.as_str())
                || (parameter.owner.as_ref() == Some(&owner.id)
                    && parameter.name == name.value.as_str())
        })
        .and_then(|parameter| parameter.value.as_ref())
    {
        if !super::parameters::scalar_quantity_matches(lane, scalar, value) {
            return Err(
                "canonical parameter quantity does not match the retained native scalar quantity",
            );
        }
    }
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
    let permitted = |owner: Option<&str>| owner.is_none_or(|owner| allowed.contains(owner));
    let selection_permitted = |owner: Option<&str>| {
        owner.is_none_or(|owner| allowed.contains(owner) || selection_owners.contains(owner))
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

fn omit_scalar(
    events: &mut Vec<Event>,
    omitted: &mut Vec<Omission>,
    scalar: &crate::records::FeatureInputScalar,
    name: &crate::records::FeatureInputName,
    reason: &str,
) -> Result<(), CodecError> {
    if let Some(id) = name.object_id {
        let mut bytes = Vec::new();
        append_generated_object_name(&mut bytes, &name.value, u32::from(id))?;
        events.push(Event {
            sequence: name.sequence,
            bytes,
            records: vec![("feature_input_names".into(), name.id.clone())],
        });
    } else {
        omitted.push(Omission {
            arena: "feature_input_names",
            id: name.id.clone(),
            reason: reason.into(),
        });
    }
    for (arena, id) in std::iter::once(("feature_input_scalars", scalar.id.as_str())).chain(
        scalar
            .operands
            .iter()
            .map(|operand| ("feature_input_references", operand.reference_ref.as_str())),
    ) {
        omitted.push(Omission {
            arena,
            id: id.into(),
            reason: reason.into(),
        });
    }
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
mod marker_admission_tests;
#[cfg(test)]
mod tests;
