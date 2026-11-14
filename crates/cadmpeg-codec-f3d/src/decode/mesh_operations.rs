// SPDX-License-Identifier: Apache-2.0

use crate::container::ContainerScan;
use crate::design::decode::mesh_operations::{
    read_scope, MeshOperationFields, MeshOperationFraming, MeshOperationKind, MeshOperationRecords,
    MeshOperationScope,
};
use crate::loss::F3dLossCode;
use crate::records::feature::scope::DesignParameterScope;
use crate::records::parameters::{DesignParameter, DesignParameterOwner};
use crate::records::sketch_placement::DesignSketchPlacement;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::text::NonBlankString;
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::mesh::{
    MeshFaceDistribution, MeshModification, MeshRebuildMode, MeshReduceTarget, MeshRepair,
    MeshSelection,
};
use cadmpeg_ir::features::{
    BodySelection, FaceSelection, Feature, FeatureDefinition, FeatureId, FeatureOperation,
    FeatureSupportPlaneFrame, NativeSelections, SelectionReference,
};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::scalar::{Fraction, Length, NonNegativeLength, PositiveLength, PositiveReal};
use cadmpeg_ir::sketches::Sketch;
use std::collections::HashMap;

const MILLIMETRES_PER_DESIGN_LENGTH: f64 = 10.0;
const PERCENT: f64 = 100.0;

pub(super) mod convert;

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
    fn property(&mut self, key: impl Into<String>, value: impl ToString + Copy) {
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
        MeshOperationFields::Section { .. }
        | MeshOperationFields::Shell { .. }
        | MeshOperationFields::ReverseNormals => return None,
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
                        .map(|value| MeshReduceTarget::MaximumDeviation { value }),
                )),
                1 => Some((
                    "proportion",
                    proportion_owner,
                    number(value(proportion_owner))
                        .and_then(|percent| Fraction::new(percent / PERCENT))
                        .map(|value| MeshReduceTarget::FaceProportion { value }),
                )),
                2 => Some((
                    "face count",
                    face_count_owner,
                    number(value(face_count_owner))
                        .and_then(face_count)
                        .map(|value| MeshReduceTarget::FaceCount { value }),
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
                0 => Some(MeshRepair::CloseHoles {}),
                1 => Some(MeshRepair::StitchAndRemove {}),
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
    })
}

fn unresolved_modification(kind: MeshOperationKind) -> Option<MeshModification> {
    Some(match kind {
        MeshOperationKind::Section
        | MeshOperationKind::Shell
        | MeshOperationKind::ReverseNormals => return None,
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
    })
}

fn body_selection(selection: MeshSelection) -> BodySelection {
    match selection {
        MeshSelection::Native {
            native,
            members: Some(bodies),
        } => BodySelection::Local { bodies, native },
        MeshSelection::Native { native, .. } => BodySelection::Native(native.as_str().to_owned()),
        MeshSelection::Unresolved { .. } | MeshSelection::Tessellations(_) => {
            BodySelection::Unresolved
        }
    }
}

fn shell(
    source: MeshSelection,
    thickness: Option<PositiveLength>,
    notes: &mut Notes<'_>,
) -> FeatureOperation {
    notes.losses.push(F3dLossCode::FeatureMeshShellControlsUnresolved.note(format!(
        "Mesh shell {} retains unknown outward, construction mode, corner join, intersection resolution and self-intersection controls as unresolved.",
        notes.source
    )));
    FeatureOperation::Shell {
        bodies: Some(body_selection(source)),
        removed_faces: FaceSelection::Faces(Vec::new()),
        thickness,
        outward: None,
        mode: None,
        join: None,
        resolve_intersections: None,
        allow_self_intersections: None,
    }
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
                    notes.losses.push(F3dLossCode::FeatureMeshBodySelectionAssumed.note(format!(
                        "Mesh operation {} interprets source member body references as body selections; member-tail selection controls remain unresolved.", notes.source
                    )));
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

#[derive(Clone, Copy)]
pub(super) struct MeshOperationInputs<'a> {
    pub(super) scopes: &'a [DesignParameterScope],
    pub(super) owners: &'a [DesignParameterOwner],
    pub(super) parameters: &'a [DesignParameter],
    pub(super) placements: &'a [DesignSketchPlacement],
    pub(super) sketches: &'a [Sketch],
}

fn section_binding(
    inputs: &MeshOperationInputs<'_>,
    features: &[Feature],
    stream: Option<&str>,
    sketch_scope: u32,
) -> (Option<FeatureSupportPlaneFrame>, Option<FeatureId>) {
    if sketch_scope == 0 {
        return (None, None);
    }
    let mut scopes = inputs.scopes.iter().filter(|candidate| {
        crate::ids::native_stream(&candidate.id) == stream && candidate.record_index == sketch_scope
    });
    let scope = scopes.next().filter(|_| scopes.next().is_none());
    let feature = scope.and_then(|scope| {
        let mut candidates = features.iter().filter(|candidate| {
            candidate.native_ref.as_deref() == Some(scope.id.as_str())
                && matches!(
                    candidate.evaluation.definition(),
                    FeatureDefinition::Operation(
                        FeatureOperation::Sketch { .. } | FeatureOperation::SpatialSketch { .. }
                    )
                )
        });
        candidates.next().filter(|_| candidates.next().is_none())
    });
    let model_plane = feature.and_then(|feature| {
        let FeatureDefinition::Operation(FeatureOperation::Sketch { sketch }) =
            feature.evaluation.definition()
        else {
            return None;
        };
        let id = sketch.id()?;
        let mut sketches = inputs.sketches.iter().filter(|sketch| &sketch.id == id);
        let sketch = sketches.next().filter(|_| sketches.next().is_none())?;
        let (origin, normal, u_axis) = sketch.placement.resolved()?;
        FeatureSupportPlaneFrame::new(origin.get(), normal.get(), u_axis.get())
    });
    let plane = model_plane.or_else(|| {
        let mut placements = inputs.placements.iter().filter(|placement| {
            crate::ids::native_stream(&placement.id) == stream
                && placement.scope_record_index == Some(sketch_scope)
        });
        let placement = placements.next().filter(|_| placements.next().is_none())?;
        let matrix = placement.transform();
        let scale = crate::design::face_resolve::placement_origin_scale(placement);
        FeatureSupportPlaneFrame::new(
            Point3::new(
                matrix[0][3] * scale,
                matrix[1][3] * scale,
                matrix[2][3] * scale,
            ),
            Vector3::new(matrix[0][2], matrix[1][2], matrix[2][2]),
            Vector3::new(matrix[0][0], matrix[1][0], matrix[2][0]),
        )
    });
    (plane, feature.map(|feature| feature.id.clone()))
}

fn scope_bytes<'a>(
    scan: &ContainerScan<'a>,
    scope: &DesignParameterScope,
) -> Option<(String, &'a [u8])> {
    let stream = crate::ids::native_stream(&scope.id)?.strip_prefix("f3d:")?;
    let entry = crate::ids::decode_identity_key_component(stream)?;
    let bytes = scan.entry_view(&entry)?.window();
    Some((entry, bytes))
}

pub(super) fn bind_features(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan<'_>,
    features: &mut [Feature],
    inputs: MeshOperationInputs<'_>,
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    let MeshOperationInputs {
        scopes,
        owners,
        parameters,
        ..
    } = &inputs;
    let mut records = HashMap::<String, Option<MeshOperationRecords<'_>>>::new();
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
        let parsed = if let Some((scope, (entry, bytes))) =
            scope.and_then(|scope| Some((scope, scope_bytes(scan, scope)?)))
        {
            let indexed = match records.entry(entry) {
                std::collections::hash_map::Entry::Occupied(entry) => entry.into_mut(),
                std::collections::hash_map::Entry::Vacant(entry) => {
                    let meta = match crate::design::decode::meta::metadata_for_bulk_stream(
                        scan,
                        entry.key(),
                    ) {
                        Ok(meta) => meta,
                        Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
                        Err(_) => None,
                    };
                    entry.insert(meta.map(|meta| MeshOperationRecords::index(bytes, meta)))
                }
            };
            let parsed = indexed
                .as_ref()
                .ok_or(MeshOperationFraming("source metadata"))
                .and_then(|records| records.validate_scope(kind, scope.record_index))
                .and_then(|frame| read_scope(kind, frame));
            Some((scope, indexed.as_ref(), parsed))
        } else {
            None
        };
        let operation = match parsed {
            Some((scope, records, Ok(operation))) => {
                if let Some(error) = operation.tail_error {
                    notes.losses.push(F3dLossCode::FeatureMeshOperationTailPartial.note(format!(
                        "Mesh operation {source} retains decoded controls and references before its unresolved {error}."
                    )));
                }
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
                    if owner == 0 {
                        return None;
                    }
                    let owner = owners.iter().find(|candidate| {
                        crate::ids::native_stream(candidate.id()) == stream
                            && candidate.record_index() == owner
                            && candidate.scope_record_index() == scope.record_index
                    })?;
                    let parameter = parameters.iter().find(|candidate| {
                        crate::ids::native_stream(&candidate.id) == stream
                            && candidate.record_index == owner.parameter_record_index()
                    })?;
                    let length = match parameter.unit() {
                        None => false,
                        Some(unit)
                            if crate::design::feature_project::design_length_unit(
                                unit.value.as_str(),
                            ) =>
                        {
                            true
                        }
                        Some(_) => return None,
                    };
                    Some(OwnerValue {
                        value: parameter.evaluated_value().get(),
                        length,
                    })
                };
                let (plane, sketch) = match operation.fields {
                    MeshOperationFields::Section {
                        section_group,
                        sketch_scope,
                    } => {
                        notes.property("mesh-section-group", section_group);
                        notes.property("mesh-section-sketch-scope", sketch_scope);
                        let (plane, sketch) =
                            section_binding(&inputs, features, stream, sketch_scope);
                        notes.losses.push(
                            F3dLossCode::FeatureMeshSectionCurvesUnresolved.note(match &sketch {
                                Some(sketch) => format!(
                                    "Mesh section {source} names sketch feature {sketch}; its section curves remain in the native sketch records."
                                ),
                                None => format!(
                                    "Mesh section {source} retains sketch scope record {sketch_scope} in its source properties; no neutral sketch feature projects that scope."
                                ),
                            }),
                        );
                        (plane, sketch)
                    }
                    _ => (None, None),
                };
                let bodies = match records {
                    Some(records) => records.group_bodies(ctx, operation.source_group)?,
                    None => Err(MeshOperationFraming("source metadata")),
                };
                let source = source_selection(&operation, bodies, &mut notes)?;
                match operation.fields {
                    MeshOperationFields::Section { .. } => FeatureOperation::MeshSection {
                        source,
                        plane,
                        sketch,
                    },
                    MeshOperationFields::Shell { thickness_owner } => {
                        let thickness = notes.bound(
                            "thickness",
                            thickness_owner,
                            length(value(thickness_owner)).and_then(PositiveLength::new),
                        );
                        shell(source, thickness, &mut notes)
                    }
                    MeshOperationFields::ReverseNormals => FeatureOperation::ReverseShape {
                        source: body_selection(source),
                    },
                    _ => FeatureOperation::MeshModify {
                        source,
                        modification: modification(&operation.fields, &value, &mut notes)
                            .ok_or_else(|| {
                                CodecError::malformed("mesh modification has no controls")
                            })?,
                    },
                }
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
                let source = MeshSelection::Unresolved {
                    feature: feature.id.clone(),
                };
                match kind {
                    MeshOperationKind::Section => FeatureOperation::MeshSection {
                        source,
                        plane: None,
                        sketch: None,
                    },
                    MeshOperationKind::Shell => shell(source, None, &mut notes),
                    MeshOperationKind::ReverseNormals => FeatureOperation::ReverseShape {
                        source: body_selection(source),
                    },
                    _ => FeatureOperation::MeshModify {
                        source,
                        modification: unresolved_modification(kind).ok_or_else(|| {
                            CodecError::malformed("mesh modification kind has no controls")
                        })?,
                    },
                }
            }
        };
        if matches!(operation, FeatureOperation::MeshSection { plane: None, .. }) {
            notes.losses.push(F3dLossCode::FeatureMeshSectionPlaneUnresolved.note(format!(
                "Mesh section {source} retains its sketch reference while its model-space plane remains unresolved."
            )));
        }
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
