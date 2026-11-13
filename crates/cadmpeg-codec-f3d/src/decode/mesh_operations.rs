// SPDX-License-Identifier: Apache-2.0

use crate::container::ContainerScan;
use crate::design::decode::mesh_operations::{
    read_scope, MeshOperationFields, MeshOperationKind, MeshOperationRecords, MeshOperationScope,
};
use crate::loss::F3dLossCode;
use crate::records::feature::scope::DesignParameterScope;
use crate::records::parameters::{DesignParameter, DesignParameterOwner};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::text::NonBlankString;
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::mesh::{
    MeshFaceDistribution, MeshModification, MeshRebuildMode, MeshReduceTarget, MeshRepair,
    MeshSelection,
};
use cadmpeg_ir::features::{
    Feature, FeatureDefinition, FeatureOperation, NativeSelections, SelectionReference,
};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::scalar::{Fraction, Length, NonNegativeLength, PositiveLength, PositiveReal};
use std::collections::HashMap;

const MILLIMETRES_PER_DESIGN_LENGTH: f64 = 10.0;
const PERCENT: f64 = 100.0;

#[derive(Debug, Clone, Copy, PartialEq)]
struct OwnerValue {
    value: f64,
    length: bool,
}

struct Notes<'a> {
    source: &'a str,
    properties: Vec<(String, String)>,
    losses: Vec<LossNote>,
}

impl Notes<'_> {
    fn property(&mut self, key: impl Into<String>, value: impl ToString) {
        self.properties.push((key.into(), value.to_string()));
    }

    fn selector<T>(&mut self, name: &str, value: u32, ruled: Option<T>) -> Option<T> {
        self.property(format!("mesh-selector:{name}"), value);
        if ruled.is_none() {
            self.losses.push(
                F3dLossCode::FeatureMeshOperationSelectorUnruled.note(format!(
                    "Mesh operation {} retains {name} selector {value} in its source properties.",
                    self.source
                )),
            );
        }
        ruled
    }

    fn bound<T>(&mut self, name: &str, owner: u32, value: Option<T>) -> Option<T> {
        if value.is_none() {
            self.losses.push(
                F3dLossCode::FeatureMeshOperationParameterUnbound.note(format!(
                    "Mesh operation {} has no usable {name} value from parameter owner {owner}.",
                    self.source
                )),
            );
        }
        value
    }
}

fn distribution(value: u32) -> Option<MeshFaceDistribution> {
    match value {
        0 => Some(MeshFaceDistribution::Adaptive),
        1 => Some(MeshFaceDistribution::Uniform),
        _ => None,
    }
}

fn length(value: Option<OwnerValue>) -> Option<f64> {
    value
        .filter(|value| value.length)
        .map(|value| value.value * MILLIMETRES_PER_DESIGN_LENGTH)
}

fn number(value: Option<OwnerValue>) -> Option<f64> {
    value.filter(|value| !value.length).map(|value| value.value)
}

fn face_count(value: f64) -> Option<std::num::NonZeroU32> {
    (value.fract() == 0.0)
        .then(|| cadmpeg_core::convert::truncate_f64_to_u32(value))
        .flatten()
        .and_then(std::num::NonZeroU32::new)
}

fn modification(
    fields: &MeshOperationFields,
    value: &dyn Fn(u32) -> Option<OwnerValue>,
    notes: &mut Notes<'_>,
) -> Option<MeshModification> {
    Some(match *fields {
        MeshOperationFields::Section { .. } => return None,
        MeshOperationFields::Reduce {
            tolerance_owner,
            distribution: distribution_selector,
            proportion_owner,
            target,
            face_count_owner,
        } => {
            let active = match target {
                0 => Some((
                    "tolerance",
                    tolerance_owner,
                    length(value(tolerance_owner))
                        .and_then(NonNegativeLength::new)
                        .map(MeshReduceTarget::MaximumDeviation),
                )),
                1 => Some((
                    "proportion",
                    proportion_owner,
                    number(value(proportion_owner))
                        .and_then(|percent| Fraction::new(percent / PERCENT))
                        .map(MeshReduceTarget::FaceProportion),
                )),
                2 => Some((
                    "face count",
                    face_count_owner,
                    number(value(face_count_owner))
                        .and_then(face_count)
                        .map(MeshReduceTarget::FaceCount),
                )),
                _ => None,
            };
            let target = notes
                .selector("target", target, active)
                .and_then(|(name, owner, value)| notes.bound(name, owner, value));
            MeshModification::Reduce {
                distribution: notes.selector(
                    "distribution",
                    distribution_selector,
                    distribution(distribution_selector),
                ),
                target,
            }
        }
        MeshOperationFields::Repair {
            density_owner,
            offset_owner,
            rebuild_mode,
            method,
        } => {
            let ruled = match method {
                0 => Some(MeshRepair::CloseHoles),
                1 => Some(MeshRepair::StitchAndRemove),
                3 => Some(MeshRepair::Rebuild {
                    mode: None,
                    density: None,
                    offset: None,
                }),
                _ => None,
            };
            let mut method = notes.selector("method", method, ruled);
            if let Some(MeshRepair::Rebuild {
                mode,
                density,
                offset,
            }) = &mut method
            {
                *mode = notes.selector(
                    "rebuild",
                    rebuild_mode,
                    match rebuild_mode {
                        0 => Some(MeshRebuildMode::Fast),
                        2 => Some(MeshRebuildMode::Accurate),
                        _ => None,
                    },
                );
                *density = notes.bound(
                    "density",
                    density_owner,
                    number(value(density_owner)).and_then(PositiveReal::new),
                );
                *offset = notes.bound(
                    "offset",
                    offset_owner,
                    length(value(offset_owner)).and_then(Length::new),
                );
            } else {
                notes.property("mesh-selector:rebuild", rebuild_mode);
            }
            MeshModification::Repair { method }
        }
        MeshOperationFields::Remesh {
            distribution: distribution_selector,
            shape_preservation_owner,
            edge_length_owner,
        } => MeshModification::Remesh {
            distribution: notes.selector(
                "distribution",
                distribution_selector,
                distribution(distribution_selector),
            ),
            edge_length: notes.bound(
                "edge length",
                edge_length_owner,
                length(value(edge_length_owner)).and_then(PositiveLength::new),
            ),
            shape_preservation: shape_preservation_owner.and_then(|owner| {
                notes.bound(
                    "shape preservation",
                    owner,
                    number(value(owner)).and_then(Fraction::new),
                )
            }),
        },
        MeshOperationFields::Smooth { smoothness_owner } => MeshModification::Smooth {
            smoothness: notes.bound(
                "smoothness",
                smoothness_owner,
                number(value(smoothness_owner)).and_then(Fraction::new),
            ),
        },
        MeshOperationFields::Shell { thickness_owner } => MeshModification::Shell {
            thickness: notes.bound(
                "thickness",
                thickness_owner,
                length(value(thickness_owner)).and_then(PositiveLength::new),
            ),
        },
        MeshOperationFields::ReverseNormals => MeshModification::ReverseNormals,
    })
}

fn unresolved_modification(kind: MeshOperationKind) -> Option<MeshModification> {
    Some(match kind {
        MeshOperationKind::Section => return None,
        MeshOperationKind::Reduce => MeshModification::Reduce {
            distribution: None,
            target: None,
        },
        MeshOperationKind::Repair => MeshModification::Repair { method: None },
        MeshOperationKind::Remesh => MeshModification::Remesh {
            distribution: None,
            edge_length: None,
            shape_preservation: None,
        },
        MeshOperationKind::Smooth => MeshModification::Smooth { smoothness: None },
        MeshOperationKind::Shell => MeshModification::Shell { thickness: None },
        MeshOperationKind::ReverseNormals => MeshModification::ReverseNormals,
    })
}

fn source_selection(
    scope: &MeshOperationScope,
    bodies: Result<Vec<u32>, impl std::fmt::Display>,
    notes: &mut Notes<'_>,
) -> Result<MeshSelection, CodecError> {
    notes.property("mesh-source-group", scope.source_group);
    notes.property("mesh-input-geometry", scope.input_geometry);
    if let Some(output) = scope.output_geometry {
        notes.property("mesh-output-geometry", output);
    }
    let members = match bodies {
        Ok(bodies) => {
            for (ordinal, body) in bodies.iter().enumerate() {
                notes.property(format!("mesh-body-reference:{ordinal}"), body);
            }
            let selectors = bodies.iter().map(u32::to_string).collect::<Vec<_>>();
            match NativeSelections::try_from(selectors) {
                Ok(members) => {
                    notes.losses.push(
                        F3dLossCode::FeatureMeshOperationSourceUnresolved.note(format!(
                            "Mesh operation {} retains source body records {bodies:?} and input geometry record {}; their tessellation remains unresolved.",
                            notes.source, scope.input_geometry
                        )),
                    );
                    Some(members)
                }
                Err(error) => {
                    notes.losses.push(
                        F3dLossCode::FeatureMeshOperationScopeUnparsed.note(format!(
                            "Mesh operation {} keeps its scope as the mesh source because source group {} gives an invalid body selection: {error}.",
                            notes.source, scope.source_group
                        )),
                    );
                    None
                }
            }
        }
        Err(error) => {
            notes.losses.push(
                F3dLossCode::FeatureMeshOperationScopeUnparsed.note(format!(
                    "Mesh operation {} keeps its scope as the mesh source because source group {} is unresolved at its {error}.",
                    notes.source, scope.source_group
                )),
            );
            None
        }
    };
    Ok(MeshSelection::Native {
        native: SelectionReference::try_from(notes.source.to_owned())
            .map_err(CodecError::malformed)?,
        members,
    })
}

struct Plan {
    feature: usize,
    definition: FeatureDefinition,
    properties: Vec<(String, String)>,
}

fn scope_bytes<'a>(
    scan: &ContainerScan<'a>,
    scope: &DesignParameterScope,
) -> Option<(String, &'a [u8], &'a [u8])> {
    let stream = crate::ids::native_stream(&scope.id)?.strip_prefix("f3d:")?;
    let entry = crate::ids::decode_identity_key_component(stream)?;
    let bytes = scan.entry_view(&entry)?.window();
    let start = usize::try_from(scope.byte_offset()).ok()?;
    let end = start.checked_add(usize::try_from(scope.frame_length()).ok()?)?;
    Some((entry, bytes, bytes.get(start..end)?))
}

pub(super) fn bind_features(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan<'_>,
    features: &mut [Feature],
    scopes: &[DesignParameterScope],
    owners: &[DesignParameterOwner],
    parameters: &[DesignParameter],
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    let mut records = HashMap::<String, MeshOperationRecords<'_>>::new();
    let mut plans = Vec::new();
    for (index, feature) in features.iter().enumerate() {
        let Some(kind) = feature
            .source_tag
            .as_deref()
            .and_then(MeshOperationKind::from_name)
        else {
            continue;
        };
        let scope = feature
            .native_ref
            .as_deref()
            .and_then(|reference| scopes.iter().find(|scope| scope.id == reference));
        let source = scope.map_or_else(|| feature.id.to_string(), |scope| scope.id.clone());
        let mut notes = Notes {
            source: &source,
            properties: Vec::new(),
            losses: Vec::new(),
        };
        if let FeatureDefinition::Operation(FeatureOperation::Native { parameters, .. }) =
            feature.evaluation.definition()
        {
            for (name, expression) in parameters {
                notes.property(format!("parameter:{name}"), expression);
            }
        }
        let parsed = scope
            .and_then(|scope| Some((scope, scope_bytes(scan, scope)?)))
            .map(|(scope, (entry, bytes, frame))| (scope, entry, bytes, read_scope(kind, frame)));
        let (selection, modification, sketch) = match parsed {
            Some((scope, entry, bytes, Ok(operation))) => {
                if let Some(word) = operation.prefix_word {
                    notes.property("mesh-scope-prefix-word", word);
                    notes
                        .losses
                        .push(F3dLossCode::MeshScopePrefixUninterpreted.note(format!(
                            "Mesh operation {source} retains prefix word {word} in its source properties."
                        )));
                }
                for (field, value) in &operation.deviations {
                    notes.property(format!("mesh-field:{field}"), value);
                }
                if !operation.deviations.is_empty() {
                    notes.losses.push(
                        F3dLossCode::FeatureMeshOperationFieldsUninterpreted.note(format!(
                            "Mesh operation {source} retains uninterpreted field values {:?} in its source properties.",
                            operation.deviations
                        )),
                    );
                }
                let stream = crate::ids::native_stream(&scope.id);
                let value = |owner: u32| {
                    let owner = owners.iter().find(|candidate| {
                        crate::ids::native_stream(candidate.id()) == stream
                            && candidate.record_index() == owner
                    })?;
                    let parameter = parameters.iter().find(|candidate| {
                        crate::ids::native_stream(&candidate.id) == stream
                            && candidate.record_index == owner.parameter_record_index()
                    })?;
                    Some(OwnerValue {
                        value: parameter.evaluated_value().get(),
                        length: parameter.unit().is_some_and(|unit| {
                            crate::design::feature_project::design_length_unit(unit.value.as_str())
                        }),
                    })
                };
                let modification = modification(&operation.fields, &value, &mut notes);
                let sketch = match operation.fields {
                    MeshOperationFields::Section {
                        section_group,
                        sketch_scope,
                    } => {
                        notes.property("mesh-section-group", section_group);
                        notes.property("mesh-section-sketch-scope", sketch_scope);
                        let sketch = scopes
                            .iter()
                            .find(|candidate| {
                                crate::ids::native_stream(&candidate.id) == stream
                                    && candidate.record_index == sketch_scope
                            })
                            .and_then(|sketch| {
                                features.iter().find(|candidate| {
                                    candidate.native_ref.as_deref() == Some(sketch.id.as_str())
                                })
                            })
                            .map(|sketch| sketch.id.clone());
                        notes.losses.push(
                            F3dLossCode::FeatureMeshSectionCurvesUnresolved.note(match &sketch {
                                Some(sketch) => format!(
                                    "Mesh section {source} names sketch feature {sketch}; its section curves remain in the native sketch records."
                                ),
                                None => format!(
                                    "Mesh section {source} retains sketch scope record {sketch_scope} in its source properties; no feature projects that scope."
                                ),
                            }),
                        );
                        sketch
                    }
                    _ => None,
                };
                let bodies = records
                    .entry(entry)
                    .or_insert_with(|| MeshOperationRecords::index(bytes))
                    .group_bodies(ctx, operation.source_group)?;
                (
                    source_selection(&operation, bodies, &mut notes)?,
                    modification,
                    sketch,
                )
            }
            other => {
                let reason = match other {
                    Some((.., Err(error))) => format!("its {error} is unresolved"),
                    _ => "its scope frame is unavailable".to_owned(),
                };
                notes
                    .losses
                    .push(F3dLossCode::FeatureMeshOperationScopeUnparsed.note(format!(
                        "Mesh operation {source} retains its originating feature identity because {reason}."
                    )));
                (
                    MeshSelection::Unresolved {
                        feature: feature.id.clone(),
                    },
                    unresolved_modification(kind),
                    None,
                )
            }
        };
        let operation = match modification {
            Some(modification) => FeatureOperation::MeshModify {
                source: selection,
                modification,
            },
            None => FeatureOperation::MeshSection {
                source: selection,
                sketch,
            },
        };
        losses.append(&mut notes.losses);
        plans.push(Plan {
            feature: index,
            definition: FeatureDefinition::Operation(operation),
            properties: notes.properties,
        });
    }
    for plan in plans {
        let feature = &mut features[plan.feature];
        for (key, value) in plan.properties {
            let key = NonBlankString::new(key)
                .ok_or_else(|| CodecError::malformed("blank mesh operation property key"))?;
            feature.source_properties.insert(key, value);
        }
        feature.evaluation.set_definition(plan.definition);
    }
    Ok(())
}

#[cfg(test)]
mod tests;
