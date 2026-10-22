// SPDX-License-Identifier: Apache-2.0
//! Merge native-only sketch relations into the current authored marker graph.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::write::{NativeDisposition, NativeDispositions};
use cadmpeg_ir::features::{
    FeatureDefinition, FeatureOperation, ParameterValue, SketchFeatureBinding,
};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::sketches::{Sketch, SketchLocus};
use cadmpeg_ir::CadIr;

use crate::loss::SldprtLossCode;
use crate::native::SldprtNative;
use crate::records::{
    FeatureInputLane, FeatureInputOperandKind, SketchInputEntity, SketchInputKind,
};

use super::super::scalars::FeatureObjectNames;
use super::super::selections::{operand_accepts_marker, operand_uses_compatible_ordinal};
use super::super::transforms::locus_entity;
use super::super::write_generate::{
    append_coordinate_marker, append_scalar_fields, generated_marker_layout,
};

pub(super) fn merge(
    original: &CadIr,
    ir: &CadIr,
    source: &SldprtNative,
    native: &mut SldprtNative,
    losses: &mut Vec<LossNote>,
    dispositions: &mut NativeDispositions,
) -> Result<(), CodecError> {
    if !ir.scope.design {
        return Ok(());
    }
    for sketch in &ir.model.sketches {
        let Some(owner) = ir.model.features.iter().find(|feature| matches!(feature.evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::Sketch { sketch: SketchFeatureBinding::Planar(Some(id)) }) if id == &sketch.id)) else { continue; };
        let Some(owner_ref) = owner.native_ref.as_deref() else {
            continue;
        };
        let Some(record) = native
            .feature_histories
            .iter()
            .flat_map(|history| &history.features)
            .find(|record| record.id == owner_ref)
        else {
            continue;
        };
        let Some(position) = native.feature_input_lanes.iter().position(|lane| {
            lane.id.starts_with("sldprt:generated:feature-input#")
                && FeatureObjectNames::new(&lane.names).of(record).is_some()
        }) else {
            continue;
        };
        let start = FeatureObjectNames::new(&native.feature_input_lanes[position].names)
            .of(record)
            .and_then(|name| name.offset)
            .ok_or_else(|| CodecError::malformed("authored sketch has no object name"))?;
        let end = native
            .feature_histories
            .iter()
            .flat_map(|history| &history.features)
            .filter_map(|feature| {
                FeatureObjectNames::new(&native.feature_input_lanes[position].names)
                    .of(feature)
                    .and_then(|name| name.offset)
            })
            .filter(|offset| *offset > start)
            .min()
            .map(|offset| {
                usize::try_from(offset).map_err(|_| {
                    CodecError::malformed("sketch object offset exceeds address space")
                })
            })
            .transpose()?
            .unwrap_or(native.feature_input_lanes[position].native_payload.len());
        let existing = native.feature_input_lanes[position]
            .sketch_entities
            .iter()
            .filter(|marker| marker.feature_ref.as_deref() == Some(owner_ref))
            .filter_map(|marker| Some((marker.kind(), u16::try_from(marker.local_id()?).ok()?)))
            .collect::<Vec<_>>();
        let mut addresses = existing;
        let mut next_object_id = native.feature_input_lanes[position]
            .scalars
            .iter()
            .map(|scalar| scalar.object_id)
            .chain(
                native.feature_input_lanes[position]
                    .names
                    .iter()
                    .filter_map(|name| name.object_id.and_then(crate::records::ObjectId::value)),
            )
            .max()
            .unwrap_or(0);
        let layout = generated_marker_layout(ir, sketch)?;
        let mut extras = Vec::new();
        let mut forced = false;
        let represented = represented_constraints(original, ir, sketch);
        for lane in &source.feature_input_lanes {
            if lane
                .configuration
                .as_ref()
                .is_some_and(|configuration| sketch.configuration.as_ref() != Some(configuration))
            {
                continue;
            }
            let relations = lane
                .relation_instances
                .iter()
                .filter(|relation| {
                    relation.feature_ref == owner_ref
                        && !represented.contains(relation.id.as_str())
                        && !relation
                            .scalar_refs()
                            .iter()
                            .any(|id| represented.contains(id.as_str()))
                })
                .collect::<Vec<_>>();
            let free_relation_markers = lane
                .sketch_entities
                .iter()
                .filter(|marker| {
                    marker.feature_ref.as_deref() == Some(owner_ref)
                        && matches!(marker.kind(), SketchInputKind::Relation(_))
                        && !represented.contains(marker.id())
                })
                .collect::<Vec<_>>();
            let scalar_ids = relations
                .iter()
                .flat_map(|relation| relation.scalar_refs().iter().map(String::as_str))
                .collect::<BTreeSet<_>>();
            if relations.is_empty() && free_relation_markers.is_empty() {
                continue;
            }
            if addresses.is_empty() && !forced {
                // Unconstrained neutral geometry otherwise needs only its
                // B-rep. Native-only relations require addressable handles.
                for (_, point, kind, id) in &layout {
                    append_coordinate_marker(
                        &mut extras,
                        *kind,
                        [point.u * 0.001, point.v * 0.001],
                        u32::from(*id),
                    );
                    addresses.push((*kind, *id));
                }
                forced = true;
            }
            let mut map = BTreeMap::<&str, u16>::new();
            for marker in lane
                .sketch_entities
                .iter()
                .filter(|marker| marker.feature_ref.as_deref() == Some(owner_ref))
            {
                if let Some(id) = neutral_marker(original, ir, sketch, marker, &layout) {
                    map.insert(marker.id(), id);
                }
            }
            let mut needed = relations
                .iter()
                .flat_map(|relation| &relation.operands)
                .filter_map(|operand| operand.entity_ref.as_deref())
                .chain(
                    free_relation_markers
                        .iter()
                        .flat_map(|marker| marker.links())
                        .map(|link| link.entity_ref.as_str()),
                )
                .collect::<BTreeSet<_>>();
            // Include the complete typed reference closure, never bytes around
            // a marker. A missing endpoint remains a dangling-reference loss.
            loop {
                let mut grew = false;
                let ids = needed.iter().copied().collect::<Vec<_>>();
                for id in ids {
                    if let Some(marker) =
                        lane.sketch_entities.iter().find(|marker| marker.id() == id)
                    {
                        for link in marker.links() {
                            grew |= needed.insert(link.entity_ref.as_str());
                        }
                    }
                }
                if !grew {
                    break;
                }
            }
            let candidates = lane
                .sketch_entities
                .iter()
                .filter(|marker| {
                    needed.contains(marker.id())
                        || free_relation_markers
                            .iter()
                            .any(|candidate| candidate.id() == marker.id())
                })
                .collect::<Vec<_>>();
            let mut admitted = candidates
                .iter()
                .filter(|marker| super::marker_bytes(marker).is_some())
                .map(|marker| marker.id())
                .collect::<BTreeSet<_>>();
            // Remove the complete dependent closure of an absent or opaque
            // marker before assigning addresses. No emitted operand may point
            // at a marker that was subsequently omitted.
            loop {
                let rejected = candidates
                    .iter()
                    .filter(|marker| {
                        admitted.contains(marker.id())
                            && marker.links().iter().any(|link| {
                                !map.contains_key(link.entity_ref.as_str())
                                    && !admitted.contains(link.entity_ref.as_str())
                            })
                    })
                    .map(|marker| marker.id())
                    .collect::<Vec<_>>();
                if rejected.is_empty() {
                    break;
                }
                for id in rejected {
                    admitted.remove(id);
                }
            }
            for marker in &candidates {
                if map.contains_key(marker.id()) || !admitted.contains(marker.id()) {
                    continue;
                }
                let next = addresses
                    .iter()
                    .map(|(_, id)| *id)
                    .max()
                    .unwrap_or(0)
                    .checked_add(1)
                    .ok_or_else(|| {
                        CodecError::malformed("SLDPRT native marker-local id space is exhausted")
                    })?;
                map.insert(marker.id(), next);
                addresses.push((marker.kind(), next));
            }
            for marker in candidates {
                if neutral_marker(original, ir, sketch, marker, &layout).is_some() {
                    continue;
                }
                let Some(bytes) = admitted
                    .contains(marker.id())
                    .then(|| remapped_marker(marker, &map))
                    .flatten()
                else {
                    omit(
                        losses,
                        dispositions,
                        "sketch_input_entities",
                        marker.id(),
                        "native marker has opaque content or a link that no longer resolves",
                    );
                    continue;
                };
                extras.extend(bytes);
                dispositions.set_record(
                    "sldprt",
                    "sketch_input_entities",
                    marker.id(),
                    NativeDisposition::Regenerated,
                );
            }
            let mut written = BTreeSet::new();
            let mut declared_relation = None;
            for scalar in lane
                .scalars
                .iter()
                .filter(|scalar| scalar_ids.contains(scalar.id.as_str()))
            {
                let Some(name) = lane.names.iter().find(|name| name.id == scalar.name) else {
                    omit(
                        losses,
                        dispositions,
                        "feature_input_scalars",
                        &scalar.id,
                        "native scalar name no longer resolves",
                    );
                    continue;
                };
                let Some(class) = lane
                    .relation_bindings
                    .iter()
                    .find(|binding| binding.scalar_ref == scalar.id)
                    .map(|binding| binding.class_ref.as_str())
                    .or_else(|| {
                        relations
                            .iter()
                            .find(|relation| relation.scalar_refs().contains(&scalar.id))
                            .map(|relation| relation.class_ref.as_str())
                    })
                    .and_then(|id| lane.classes.iter().find(|class| class.id == id))
                else {
                    omit(
                        losses,
                        dispositions,
                        "feature_input_scalars",
                        &scalar.id,
                        "native relation class no longer resolves",
                    );
                    continue;
                };
                let operands = scalar
                    .operands
                    .iter()
                    .map(|operand| {
                        let id = map.get(operand.entity_ref.as_deref()?)?;
                        let address = if operand_uses_compatible_ordinal(operand.kind) {
                            u16::try_from(
                                addresses
                                    .iter()
                                    .filter(|(kind, _)| operand_accepts_marker(operand.kind, *kind))
                                    .position(|(_, candidate)| candidate == id)?,
                            )
                            .ok()?
                        } else {
                            *id
                        };
                        Some((operand.kind, address))
                    })
                    .collect::<Option<Vec<(FeatureInputOperandKind, u16)>>>();
                let Some(operands) = operands else {
                    omit(
                        losses,
                        dispositions,
                        "feature_input_scalars",
                        &scalar.id,
                        "native relation operand no longer resolves in the current sketch",
                    );
                    continue;
                };
                let parameter = original
                    .model
                    .parameters
                    .iter()
                    .find(|parameter| parameter.native_ref.as_deref() == Some(scalar.id.as_str()))
                    .and_then(|parameter| {
                        ir.model
                            .parameters
                            .iter()
                            .find(|current| current.id == parameter.id)
                    });
                let (name, value) = if let Some(parameter) = parameter {
                    let Some(value) =
                        configuration_value(ir, lane, &parameter.id).and_then(native_scalar)
                    else {
                        omit(
                            losses,
                            dispositions,
                            "feature_input_scalars",
                            &scalar.id,
                            "canonical parameter has no exact evaluated native scalar",
                        );
                        continue;
                    };
                    (parameter.name.as_str(), value)
                } else {
                    (name.value.as_str(), scalar.value.get())
                };
                next_object_id = next_object_id.checked_add(1).ok_or_else(|| {
                    CodecError::malformed("SLDPRT scalar object id space is exhausted")
                })?;
                let relation = relations
                    .iter()
                    .find(|relation| relation.scalar_refs().contains(&scalar.id));
                let declaration =
                    relation.map_or(scalar.id.as_str(), |relation| relation.id.as_str());
                if declared_relation != Some(declaration) {
                    super::append_class(&mut extras, &class.name)?;
                    declared_relation = Some(declaration);
                }
                let scalar_start = extras.len();
                append_scalar_fields(&mut extras, name, value, next_object_id, &operands)?;
                let trailer = scalar_start
                    + super::super::NAME_MARKER.len()
                    + 1
                    + name.encode_utf16().count() * 2
                    + super::super::SCALAR_HEADER.len()
                    + 8;
                extras[trailer + 29] = match scalar.role {
                    crate::records::FeatureInputScalarRole::Driving => 0,
                    crate::records::FeatureInputScalarRole::Display => 1,
                    crate::records::FeatureInputScalarRole::Native => 2,
                };
                written.insert(scalar.id.as_str());
                for (arena, id) in [
                    ("feature_input_classes", class.id.as_str()),
                    ("feature_input_names", scalar.name.as_str()),
                    ("feature_input_scalars", scalar.id.as_str()),
                ] {
                    dispositions.set_record("sldprt", arena, id, NativeDisposition::Regenerated);
                }
                for operand in &scalar.operands {
                    dispositions.set_record(
                        "sldprt",
                        "feature_input_references",
                        &operand.reference_ref,
                        NativeDisposition::Regenerated,
                    );
                }
            }
            for relation in relations {
                if relation
                    .scalar_refs()
                    .iter()
                    .all(|id| written.contains(id.as_str()))
                {
                    dispositions.set_record(
                        "sldprt",
                        "feature_input_relation_instances",
                        &relation.id,
                        NativeDisposition::Regenerated,
                    );
                } else {
                    omit(
                        losses,
                        dispositions,
                        "feature_input_relation_instances",
                        &relation.id,
                        "one or more scalar members no longer resolve",
                    );
                }
            }
            for binding in &lane.relation_bindings {
                if written.contains(binding.scalar_ref.as_str()) {
                    dispositions.set_record(
                        "sldprt",
                        "feature_input_relation_bindings",
                        &binding.id,
                        NativeDisposition::Regenerated,
                    );
                }
            }
        }
        if !extras.is_empty() {
            let lane = &native.feature_input_lanes[position];
            let mut payload = lane.native_payload.to_vec();
            payload.splice(end..end, extras);
            native.feature_input_lanes[position] =
                super::super::write_prepare::generated_native_lane(
                    &native.feature_histories,
                    &lane.id,
                    lane.configuration.clone(),
                    payload,
                )?
                .0;
        }
    }
    Ok(())
}

fn represented_constraints<'a>(
    original: &'a CadIr,
    ir: &CadIr,
    sketch: &Sketch,
) -> BTreeSet<&'a str> {
    original
        .model
        .sketch_constraints
        .iter()
        .filter(|constraint| {
            constraint.sketch == sketch.id
                && ir
                    .model
                    .sketch_constraints
                    .iter()
                    .any(|current| current.id == constraint.id)
        })
        .filter_map(|constraint| constraint.native_ref.as_deref())
        .collect()
}

fn neutral_marker(
    original: &CadIr,
    ir: &CadIr,
    sketch: &Sketch,
    marker: &SketchInputEntity,
    layout: &[(SketchLocus, cadmpeg_ir::math::Point2, SketchInputKind, u16)],
) -> Option<u16> {
    for source in original
        .model
        .sketch_entities
        .iter()
        .filter(|entity| entity.sketch == sketch.id)
    {
        if !ir
            .model
            .sketch_entities
            .iter()
            .any(|current| current.id() == source.id())
        {
            continue;
        }
        let locus = if source
            .endpoint_refs
            .first()
            .is_some_and(|id| id == marker.id())
        {
            Some(SketchLocus::Start(source.id().clone()))
        } else if source
            .endpoint_refs
            .get(1)
            .is_some_and(|id| id == marker.id())
        {
            Some(SketchLocus::End(source.id().clone()))
        } else if source.native_ref.as_deref() == Some(marker.id())
            || source.geometry_ref.as_deref() == Some(marker.id())
        {
            None
        } else {
            continue;
        };
        if let Some((_, _, _, id)) = layout.iter().find(|(candidate, _, _, _)| {
            locus.as_ref().map_or_else(
                || locus_entity(candidate) == source.id(),
                |locus| candidate == locus,
            )
        }) {
            return Some(*id);
        }
    }
    None
}

fn remapped_marker(marker: &SketchInputEntity, map: &BTreeMap<&str, u16>) -> Option<Vec<u8>> {
    let mut bytes = super::marker_bytes(marker)?;
    let start = if marker.object_index().is_some() {
        4
    } else {
        0
    };
    let id = u32::from(*map.get(marker.id())?);
    if marker.object_index().is_some() {
        bytes[..4].copy_from_slice(&id.to_le_bytes());
    }
    let tail = if marker.coordinates_m.is_some() {
        138
    } else {
        88
    };
    bytes[start + tail..start + tail + 4].copy_from_slice(&id.to_le_bytes());
    for (index, link) in marker.links().iter().enumerate() {
        let local = map.get(link.entity_ref.as_str())?;
        let offset = start
            + if marker.coordinates_m.is_some() {
                88 + index * 12
            } else {
                64 + index * 2
            };
        bytes[offset..offset + 2].copy_from_slice(&local.to_le_bytes());
    }
    Some(bytes)
}

fn configuration_value<'a>(
    ir: &'a CadIr,
    lane: &FeatureInputLane,
    id: &cadmpeg_ir::features::ParameterId,
) -> Option<&'a ParameterValue> {
    if let Some(configuration) = lane.configuration.as_ref().and_then(|slot| {
        ir.model.configurations.iter().find(|configuration| {
            configuration
                .properties
                .get("id")
                .map_or_else(|| configuration.ordinal.to_string(), Clone::clone)
                == *slot
        })
    }) {
        return configuration.parameter_value(ir, id);
    }
    ir.model
        .parameters
        .iter()
        .find(|parameter| &parameter.id == id)?
        .value
        .as_ref()
}
fn native_scalar(value: &ParameterValue) -> Option<f64> {
    match value {
        ParameterValue::Length(value) => Some(value.get() / 1000.0),
        ParameterValue::Angle(value) => Some(value.get()),
        ParameterValue::Real(value) => Some(value.get()),
        ParameterValue::Integer(value) => {
            crate::history::parameters::eval::exact_integer_f64(*value)
        }
        ParameterValue::Boolean(_) | ParameterValue::String(_) => None,
    }
}
fn omit(
    losses: &mut Vec<LossNote>,
    dispositions: &mut NativeDispositions,
    arena: &str,
    id: &str,
    reason: &str,
) {
    dispositions.set_record("sldprt", arena, id, NativeDisposition::Dropped);
    losses.push(
        SldprtLossCode::WriterNativeRecordsOmitted.note(format!("native {arena} {id}: {reason}")),
    );
}
