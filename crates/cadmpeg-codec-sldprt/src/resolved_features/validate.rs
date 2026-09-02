//! Native lane validation findings.

use super::assembly::is_supplemental_config_lane;
use super::bindings::finalize_lane_bindings;
use cadmpeg_ir::{Check, Finding, Severity};
use std::collections::HashMap;

/// Validate `SolidWorks` native feature-input byte references.
///
/// Re-derives the expected feature classes, scalar indices, relation
/// bindings, reference cells, name structures, and sketch-marker offsets
/// from each lane's `native_payload` and asserts the stored arenas match.
pub(crate) fn validate_native(ir: &cadmpeg_ir::CadIr) -> Vec<Finding> {
    let Some(namespace) = ir.native.namespace("sldprt") else {
        return Vec::new();
    };
    if !crate::native::native_version_supported(namespace.version()) {
        let version = namespace.version();
        return vec![Finding {
            check: Check::Version,
            severity: Severity::Error,
            message: format!("unsupported SolidWorks native namespace version {version}"),
            entity: None,
        }];
    }
    let native = match crate::native::SldprtNative::load(namespace) {
        Ok(native) => native,
        Err(error) => {
            return vec![Finding {
                check: Check::NativeLinks,
                severity: Severity::Error,
                message: format!("invalid SolidWorks native namespace: {error}"),
                entity: None,
            }]
        }
    };
    let mut findings = Vec::new();
    for history in &native.feature_histories {
        if let Err(error) = crate::writer::validate_feature_graph(&history.features) {
            findings.push(Finding {
                check: Check::NativeLinks,
                severity: Severity::Error,
                message: error.to_string(),
                entity: Some(history.id.clone()),
            });
        }
        let mut feature_ordinals = std::collections::HashSet::new();
        for feature in &history.features {
            if !feature_ordinals.insert(feature.ordinal) {
                findings.push(Finding {
                    check: Check::NativeLinks,
                    severity: Severity::Error,
                    message: format!(
                        "SolidWorks history repeats feature ordinal {}",
                        feature.ordinal
                    ),
                    entity: Some(feature.id.clone()),
                });
            }
        }
        let mut configuration_ordinals = std::collections::HashSet::new();
        for configuration in &history.configurations {
            if !configuration_ordinals.insert(configuration.ordinal) {
                findings.push(Finding {
                    check: Check::NativeLinks,
                    severity: Severity::Error,
                    message: format!(
                        "SolidWorks history repeats configuration ordinal {}",
                        configuration.ordinal
                    ),
                    entity: Some(configuration.id.clone()),
                });
            }
        }
        if !history.content.is_empty() {
            let configurations = history
                .configurations
                .iter()
                .map(|configuration| configuration.id.as_str())
                .collect::<std::collections::HashSet<_>>();
            let root_features = history
                .features
                .iter()
                .filter(|feature| {
                    feature.tree_parent.is_none() && feature.parent_source_id.is_none()
                })
                .map(|feature| feature.id.as_str())
                .collect::<std::collections::HashSet<_>>();
            let all_features = history
                .features
                .iter()
                .map(|feature| feature.id.as_str())
                .collect::<std::collections::HashSet<_>>();
            let mut seen_configurations = std::collections::HashSet::new();
            let mut seen_features = std::collections::HashSet::new();
            for item in &history.content {
                let error = match item {
                    crate::records::HistoryContent::Configuration(id) => {
                        if !configurations.contains(id.as_str()) {
                            Some(format!(
                                "SolidWorks history root references missing configuration {id}"
                            ))
                        } else if !seen_configurations.insert(id.as_str()) {
                            Some(format!(
                                "SolidWorks history root repeats configuration {id}"
                            ))
                        } else {
                            None
                        }
                    }
                    crate::records::HistoryContent::Feature(id) => {
                        if !all_features.contains(id.as_str()) {
                            Some(format!(
                                "SolidWorks history root references missing feature {id}"
                            ))
                        } else if !root_features.contains(id.as_str()) {
                            Some(format!(
                                "SolidWorks history root references nested feature {id}"
                            ))
                        } else if !seen_features.insert(id.as_str()) {
                            Some(format!("SolidWorks history root repeats feature {id}"))
                        } else {
                            None
                        }
                    }
                    crate::records::HistoryContent::Text(_) => None,
                };
                if let Some(message) = error {
                    findings.push(Finding {
                        check: Check::NativeLinks,
                        severity: Severity::Error,
                        message,
                        entity: Some(history.id.clone()),
                    });
                }
            }
            for missing in configurations.difference(&seen_configurations) {
                findings.push(Finding {
                    check: Check::NativeLinks,
                    severity: Severity::Error,
                    message: format!("SolidWorks history root omits configuration {missing}"),
                    entity: Some(history.id.clone()),
                });
            }
            for missing in root_features.difference(&seen_features) {
                findings.push(Finding {
                    check: Check::NativeLinks,
                    severity: Severity::Error,
                    message: format!("SolidWorks history root omits feature {missing}"),
                    entity: Some(history.id.clone()),
                });
            }
        }
    }
    let mut expected_histories = native.feature_histories.clone();
    let history_lanes = native
        .feature_input_lanes
        .iter()
        .filter(|lane| !is_supplemental_config_lane(lane))
        .cloned()
        .collect::<Vec<_>>();
    crate::resolved_features::classes::bind_history_classes(
        &mut expected_histories,
        &history_lanes,
    );
    for (history, expected_history) in native.feature_histories.iter().zip(&expected_histories) {
        for (feature, expected_feature) in history.features.iter().zip(&expected_history.features) {
            if feature.input_class != expected_feature.input_class {
                findings.push(Finding {
                    check: Check::NativeLinks,
                    severity: Severity::Error,
                    message:
                        "SolidWorks history feature class does not match its feature-input index"
                            .into(),
                    entity: Some(feature.id.clone()),
                });
            }
        }
    }
    let mut expected_primary_lanes = native
        .feature_input_lanes
        .iter()
        .filter(|lane| !is_supplemental_config_lane(lane))
        .cloned()
        .collect::<Vec<_>>();
    let mut expected_supplemental_lanes = native
        .feature_input_lanes
        .iter()
        .filter(|lane| is_supplemental_config_lane(lane))
        .cloned()
        .collect::<Vec<_>>();
    for lane in expected_primary_lanes
        .iter_mut()
        .chain(&mut expected_supplemental_lanes)
    {
        lane.scalars = crate::resolved_features::scalars::named_scalars(
            &lane.native_payload,
            &lane.id,
            &lane.names,
        );
        lane.relation_bindings = crate::resolved_features::markers::relation_bindings(
            &lane.id,
            &lane.classes,
            &lane.scalars,
        );
        lane.references =
            crate::resolved_features::markers::reference_cells(&lane.scalars, &lane.classes);
    }
    crate::resolved_features::bindings::bind_scalar_operands(
        &native.feature_histories,
        &mut expected_primary_lanes,
    );
    crate::resolved_features::bindings::bind_scalar_operands(
        &native.feature_histories,
        &mut expected_supplemental_lanes,
    );
    let supplemental_lanes = native
        .feature_input_lanes
        .iter()
        .filter(|lane| is_supplemental_config_lane(lane))
        .map(|lane| (lane.id.as_str(), lane))
        .collect::<HashMap<_, _>>();
    for expected_lane in &mut expected_supplemental_lanes {
        let actual_lane = supplemental_lanes
            .get(expected_lane.id.as_str())
            .expect("expected supplemental lanes are cloned from native lanes");
        // Detached supplemental objects acquire owners before later projection
        // can replace an unresolved sketch definition. The final model does not
        // retain that intermediate state. Treat the stored owner partition as
        // derived provenance, then re-derive every byte-backed local link from it.
        for (expected, actual) in expected_lane
            .sketch_entities
            .iter_mut()
            .zip(&actual_lane.sketch_entities)
        {
            expected.feature_ref.clone_from(&actual.feature_ref);
            expected.links.clear();
            expected.link_selector = None;
        }
        for (expected, actual) in expected_lane
            .references
            .iter_mut()
            .zip(&actual_lane.references)
        {
            expected.feature_ref.clone_from(&actual.feature_ref);
        }
        for (expected, actual) in expected_lane.scalars.iter_mut().zip(&actual_lane.scalars) {
            expected.feature_ref.clone_from(&actual.feature_ref);
        }
        finalize_lane_bindings(&native.feature_histories, expected_lane);
    }
    let expected_lanes = expected_primary_lanes
        .iter()
        .chain(&expected_supplemental_lanes)
        .map(|lane| (lane.id.as_str(), lane))
        .collect::<HashMap<_, _>>();
    for lane in &native.feature_input_lanes {
        let expected_classes =
            crate::resolved_features::names::class_declarations(&lane.native_payload, &lane.id);
        if lane.classes != expected_classes {
            findings.push(Finding {
                check: Check::NativeLinks,
                severity: Severity::Error,
                message: "SolidWorks feature-input class index does not match its native payload"
                    .into(),
                entity: Some(lane.id.clone()),
            });
        }
        let expected_names =
            crate::resolved_features::names::object_names(&lane.native_payload, &lane.id);
        if lane.names.len() != expected_names.len()
            || lane
                .names
                .iter()
                .zip(&expected_names)
                .any(|(actual, expected)| {
                    actual.id != expected.id
                        || actual.parent != expected.parent
                        || actual.ordinal != expected.ordinal
                        || actual.offset != expected.offset
                })
        {
            findings.push(Finding {
                check: Check::NativeLinks,
                severity: Severity::Error,
                message:
                    "SolidWorks feature-input name structure does not match its native payload"
                        .into(),
                entity: Some(lane.id.clone()),
            });
        }
        let expected_lane = expected_lanes
            .get(lane.id.as_str())
            .expect("expected lanes are cloned from native lanes");
        if !crate::resolved_features::scalars::scalar_indices_match(
            &lane.scalars,
            &expected_lane.scalars,
        ) {
            let detail = lane
                .scalars
                .iter()
                .zip(&expected_lane.scalars)
                .find(|(actual, expected)| {
                    !crate::resolved_features::scalars::scalar_indices_match(
                        std::slice::from_ref(actual),
                        std::slice::from_ref(expected),
                    )
                })
                .map_or_else(
                    || {
                        format!(
                            "count {} != {}",
                            lane.scalars.len(),
                            expected_lane.scalars.len()
                        )
                    },
                    |(actual, expected)| format!("{actual:?} != {expected:?}"),
                );
            findings.push(Finding {
                check: Check::NativeLinks,
                severity: Severity::Error,
                message: format!(
                    "SolidWorks feature-input scalar index does not match its native payload: {detail}"
                ),
                entity: Some(lane.id.clone()),
            });
        }
        if lane.relation_bindings != expected_lane.relation_bindings {
            findings.push(Finding {
                check: Check::NativeLinks,
                severity: Severity::Error,
                message:
                    "SolidWorks feature-input relation bindings do not match the native payload"
                        .into(),
                entity: Some(lane.id.clone()),
            });
        }
        if lane.relation_instances != expected_lane.relation_instances {
            findings.push(Finding {
                check: Check::NativeLinks,
                severity: Severity::Error,
                message:
                    "SolidWorks feature-input relation instances do not match the native payload"
                        .into(),
                entity: Some(lane.id.clone()),
            });
        }
        if lane.references != expected_lane.references {
            findings.push(Finding {
                check: Check::NativeLinks,
                severity: Severity::Error,
                message:
                    "SolidWorks feature-input reference index does not match its native payload"
                        .into(),
                entity: Some(lane.id.clone()),
            });
        }
        let expected_offsets = (0..lane.native_payload.len())
            .filter(|offset| {
                crate::resolved_features::markers::sketch_marker_at(&lane.native_payload, *offset)
            })
            .map(|offset| offset as u64)
            .collect::<std::collections::HashSet<_>>();
        let mut ordinals = std::collections::HashSet::new();
        let mut offsets = std::collections::HashSet::new();
        let mut previous_offset = None;
        for (index, entity) in lane.sketch_entities.iter().enumerate() {
            let expected_entity = &expected_lane.sketch_entities[index];
            if entity.feature_ref != expected_entity.feature_ref {
                findings.push(Finding {
                    check: Check::NativeLinks,
                    severity: Severity::Error,
                    message: "SolidWorks sketch-input marker has inconsistent feature ownership"
                        .into(),
                    entity: Some(entity.id.clone()),
                });
            }
            if entity.links != expected_entity.links
                || entity.link_selector != expected_entity.link_selector
            {
                findings.push(Finding {
                    check: Check::NativeLinks,
                    severity: Severity::Error,
                    message: "SolidWorks sketch-input marker has inconsistent local links".into(),
                    entity: Some(entity.id.clone()),
                });
            }
            if entity.ordinal != index as u32 {
                findings.push(Finding {
                    check: Check::NativeLinks,
                    severity: Severity::Error,
                    message: format!(
                        "SolidWorks feature-input lane expects entity ordinal {index}, found {}",
                        entity.ordinal
                    ),
                    entity: Some(entity.id.clone()),
                });
            }
            if previous_offset.is_some_and(|offset| entity.offset <= offset) {
                findings.push(Finding {
                    check: Check::NativeLinks,
                    severity: Severity::Error,
                    message: "SolidWorks feature-input entities are not in stream order".into(),
                    entity: Some(entity.id.clone()),
                });
            }
            previous_offset = Some(entity.offset);
            if !ordinals.insert(entity.ordinal) {
                findings.push(Finding {
                    check: Check::NativeLinks,
                    severity: Severity::Error,
                    message: format!(
                        "SolidWorks feature-input lane repeats entity ordinal {}",
                        entity.ordinal
                    ),
                    entity: Some(entity.id.clone()),
                });
            }
            if !offsets.insert(entity.offset) {
                findings.push(Finding {
                    check: Check::NativeLinks,
                    severity: Severity::Error,
                    message: format!(
                        "SolidWorks feature-input lane repeats entity offset {}",
                        entity.offset
                    ),
                    entity: Some(entity.id.clone()),
                });
            }
            let valid = usize::try_from(entity.offset).ok().is_some_and(|offset| {
                crate::resolved_features::markers::sketch_marker_at(&lane.native_payload, offset)
            });
            if !valid {
                findings.push(Finding {
                    check: Check::NativeLinks,
                    severity: Severity::Error,
                    message: "feature-input entity is outside its native payload".into(),
                    entity: Some(lane.id.clone()),
                });
            }
            if usize::try_from(entity.offset).ok().is_some_and(|offset| {
                entity.object_index
                    != crate::resolved_features::markers::marker_object_index(
                        &lane.native_payload,
                        offset,
                    )
            }) {
                findings.push(Finding {
                    check: Check::NativeLinks,
                    severity: Severity::Error,
                    message:
                        "SolidWorks feature-input object index does not match its native payload"
                            .into(),
                    entity: Some(entity.id.clone()),
                });
            }
            if usize::try_from(entity.offset).ok().is_some_and(|offset| {
                entity.local_id
                    != crate::resolved_features::markers::marker_local_id(
                        &lane.native_payload,
                        offset,
                    )
            }) {
                findings.push(Finding {
                    check: Check::NativeLinks,
                    severity: Severity::Error,
                    message:
                        "SolidWorks feature-input local object id does not match its native payload"
                            .into(),
                    entity: Some(entity.id.clone()),
                });
            }
        }
        for offset in expected_offsets.difference(&offsets) {
            findings.push(Finding {
                check: Check::NativeLinks,
                severity: Severity::Error,
                message: format!("SolidWorks feature-input lane omits marker at offset {offset}"),
                entity: Some(lane.id.clone()),
            });
        }
    }
    findings
}

#[cfg(test)]
mod tests;
