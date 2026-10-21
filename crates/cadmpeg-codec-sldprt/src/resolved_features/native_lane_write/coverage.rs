// SPDX-License-Identifier: Apache-2.0
//! Declare source-native records whose neutral counterpart was regenerated.

use std::collections::BTreeSet;

use cadmpeg_ir::codec::write::{NativeDisposition, NativeDispositions};
use cadmpeg_ir::CadIr;

use crate::native::SldprtNative;

pub(super) fn neutral_copies(
    document: &CadIr,
    ir: &CadIr,
    source: &SldprtNative,
    target: &SldprtNative,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
    dispositions: &mut NativeDispositions,
) {
    // Arena defaults describe records that survived synthesis. Exceptions
    // identify deleted native identities, even when their siblings survive.
    for history in &source.feature_histories {
        if !target
            .feature_histories
            .iter()
            .any(|current| current.id == history.id)
        {
            dispositions.set_record(
                "sldprt",
                "feature_histories",
                &history.id,
                NativeDisposition::Dropped,
            );
        }
        for feature in &history.features {
            if !target
                .feature_histories
                .iter()
                .flat_map(|current| &current.features)
                .any(|current| current.id == feature.id)
            {
                dispositions.set_record(
                    "sldprt",
                    "features",
                    &feature.id,
                    NativeDisposition::Dropped,
                );
            }
        }
        for configuration in &history.configurations {
            if !target
                .feature_histories
                .iter()
                .flat_map(|current| &current.configurations)
                .any(|current| current.id == configuration.id)
            {
                dispositions.set_record(
                    "sldprt",
                    "configurations",
                    &configuration.id,
                    NativeDisposition::Dropped,
                );
            }
        }
    }
    let mut omitted = Vec::new();
    let mut regenerated = |arena: &str, id: &str| {
        dispositions.set_record("sldprt", arena, id, NativeDisposition::Regenerated);
    };
    let owners = ir
        .model
        .features
        .iter()
        .filter_map(|feature| feature.native_ref.as_deref())
        .collect::<BTreeSet<_>>();
    let scalar_ids = document
        .model
        .parameters
        .iter()
        .filter(|parameter| {
            ir.model
                .parameters
                .iter()
                .any(|current| current.id == parameter.id)
        })
        .filter_map(|parameter| parameter.native_ref.as_deref())
        .collect::<BTreeSet<_>>();
    let constraint_refs = document
        .model
        .sketch_constraints
        .iter()
        .filter(|constraint| {
            ir.model
                .sketch_constraints
                .iter()
                .any(|current| current.id == constraint.id)
        })
        .filter_map(|constraint| constraint.native_ref.as_deref())
        .collect::<BTreeSet<_>>();
    let marker_ids = document
        .model
        .sketch_entities
        .iter()
        .filter(|entity| {
            ir.model
                .sketch_entities
                .iter()
                .any(|current| current.id() == entity.id())
        })
        .flat_map(|entity| {
            entity
                .native_ref
                .iter()
                .chain(&entity.geometry_ref)
                .chain(entity.endpoint_refs.iter())
        })
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    for lane in &source.feature_input_lanes {
        for class in &lane.classes {
            if class.feature_ref.as_deref().is_some_and(|owner| {
                owners.contains(owner)
                    && source
                        .feature_histories
                        .iter()
                        .flat_map(|history| &history.features)
                        .any(|feature| {
                            feature.id == owner
                                && feature.input_class.as_deref() == Some(class.name.as_str())
                        })
                    && target
                        .feature_histories
                        .iter()
                        .flat_map(|history| &history.features)
                        .any(|feature| feature.id == owner && feature.input_class.is_some())
            }) {
                regenerated("feature_input_classes", &class.id);
            }
        }
        for name in &lane.names {
            if name
                .feature_ref
                .as_deref()
                .is_some_and(|owner| owners.contains(owner))
                && source
                    .feature_histories
                    .iter()
                    .flat_map(|history| &history.features)
                    .any(|feature| {
                        name.object_id.and_then(|id| id.value()) == feature.source_value()
                            && name.feature_ref.as_deref() == Some(feature.id.as_str())
                    })
            {
                regenerated("feature_input_names", &name.id);
            }
        }
        for marker in &lane.sketch_entities {
            if marker_ids.contains(marker.id()) || constraint_refs.contains(marker.id()) {
                regenerated("sketch_input_entities", marker.id());
            }
        }
        let mut scalars = scalar_ids.clone();
        for relation in &lane.relation_instances {
            if constraint_refs.contains(relation.id.as_str())
                || relation
                    .scalar_refs()
                    .iter()
                    .any(|id| constraint_refs.contains(id.as_str()))
            {
                regenerated("feature_input_relation_instances", &relation.id);
                regenerated("feature_input_classes", &relation.class_ref);
                scalars.extend(relation.scalar_refs().iter().map(String::as_str));
                for operand in &relation.operands {
                    regenerated("feature_input_references", &operand.reference_ref);
                }
            }
        }
        for scalar in &lane.scalars {
            if scalars.contains(scalar.id.as_str()) || constraint_refs.contains(scalar.id.as_str())
            {
                let incompatible = document
                    .model
                    .parameters
                    .iter()
                    .find(|parameter| parameter.native_ref.as_deref() == Some(scalar.id.as_str()))
                    .and_then(|parameter| {
                        ir.model
                            .parameters
                            .iter()
                            .find(|current| current.id == parameter.id)
                    })
                    .and_then(|parameter| parameter.value.as_ref())
                    .is_some_and(|value| {
                        !super::super::parameters::scalar_quantity_matches(lane, scalar, value)
                    });
                if incompatible {
                    // Keywords can regenerate the canonical name/value, but
                    // cannot reproduce this native dimensional statement and
                    // its operand references from an independent real control.
                    regenerated("feature_input_names", &scalar.name);
                    omitted.push(("feature_input_scalars", scalar.id.clone()));
                    omitted.extend(scalar.operands.iter().map(|operand| {
                        ("feature_input_references", operand.reference_ref.clone())
                    }));
                    omitted.extend(
                        lane.relation_bindings
                            .iter()
                            .filter(|binding| binding.scalar_ref == scalar.id)
                            .map(|binding| ("feature_input_relation_bindings", binding.id.clone())),
                    );
                    continue;
                }
                regenerated("feature_input_scalars", &scalar.id);
                regenerated("feature_input_names", &scalar.name);
                for operand in &scalar.operands {
                    regenerated("feature_input_references", &operand.reference_ref);
                }
                for binding in lane
                    .relation_bindings
                    .iter()
                    .filter(|binding| binding.scalar_ref == scalar.id)
                {
                    regenerated("feature_input_relation_bindings", &binding.id);
                    regenerated("feature_input_classes", &binding.class_ref);
                }
            }
        }
    }
    for (arena, id) in omitted {
        dispositions.set_record("sldprt", arena, &id, NativeDisposition::Dropped);
        losses.push(crate::loss::SldprtLossCode::WriterNativeRecordsOmitted.note(format!(
            "native {arena} {id}: canonical parameter quantity does not match the retained native scalar quantity"
        )));
    }
}
