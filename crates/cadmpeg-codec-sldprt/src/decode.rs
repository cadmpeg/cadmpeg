// SPDX-License-Identifier: Apache-2.0
//! High-level `.sldprt` decoding.
//!
//! [`decode`] scans the outer [`crate::container`], groups related Parasolid
//! `partition` and `deltas` streams, and selects the group that yields the
//! richest B-rep. It then adds appearances, display meshes, document attributes,
//! feature history, feature-input lanes, provenance, and retained source data.
//!
//! The returned [`DecodeResult`] contains both the IR and its diagnostics.
//! Untyped surface and curve carriers become opaque geometry linked to the
//! retained partition. If no body stream yields geometry, decoding returns a
//! metadata-only IR and blocking loss notes. [`DecodeOptions::container_only`]
//! requests the metadata-only path.

use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_ir::annotations::Annotations;
use cadmpeg_ir::appearance::{Appearance, AppearanceBinding, AppearanceTarget};
use cadmpeg_ir::be::u32_at as be_u32;
use cadmpeg_ir::codec::{CodecError, DecodeOptions, DecodeResult, ReadSeek};
use cadmpeg_ir::document::{CadIr, SourceMeta};
use cadmpeg_ir::geometry::SurfaceGeometry;
use cadmpeg_ir::hash::sha256_hex;
use cadmpeg_ir::ids::{AppearanceId, UnknownId};
use cadmpeg_ir::le::{i32_at as le_i32, u16_at as le_u16, u32_at as le_u32};
use cadmpeg_ir::report::DecodeReport;

use crate::loss::SldprtLossCode;
use cadmpeg_ir::units::Units;
use cadmpeg_ir::unknown::UnknownRecord;
use cadmpeg_ir::Exactness;

use crate::container::configuration_index;

use crate::brep::{self, Brep};
use crate::container::{self, Block, CompoundStream, ContainerScan};
use crate::parasolid::StreamHeader;

struct BodyStream<'a> {
    origin: BodyOrigin<'a>,
    payload: &'a [u8],
    header: StreamHeader,
}

#[derive(Clone, Copy)]
enum BodyOrigin<'a> {
    Block(&'a Block),
    Compound(&'a CompoundStream),
}

impl BodyOrigin<'_> {
    fn name(self) -> String {
        match self {
            Self::Block(block) => block
                .section
                .clone()
                .unwrap_or_else(|| format!("block@{}", block.offset)),
            Self::Compound(stream) => stream.path.clone(),
        }
    }

    fn unknown_id(self) -> UnknownId {
        match self {
            Self::Block(block) => UnknownId(format!("sldprt:file:block#{}", block.offset)),
            Self::Compound(stream) => UnknownId(format!(
                "sldprt:file:compound-stream#{}",
                stream.directory_id
            )),
        }
    }
}

struct DecodedBrep {
    selected: usize,
    brep: Brep,
    configuration_bodies: Vec<(usize, Vec<cadmpeg_ir::ids::BodyId>)>,
}

struct EvaluatedFeatureState<'a> {
    feature: &'a cadmpeg_ir::features::Feature,
    dependencies: &'a [cadmpeg_ir::features::FeatureId],
    outputs: &'a [cadmpeg_ir::ids::BodyId],
    definition: &'a cadmpeg_ir::features::FeatureDefinition,
}

/// Decode one seekable `.sldprt` stream into IR and diagnostics.
///
/// The function reads and retains the complete source image. Container framing
/// or I/O failures return [`CodecError`]; unsupported model records are reported
/// through [`DecodeResult::report`] when a partial result can be represented.
#[allow(clippy::trivially_copy_pass_by_ref)]
pub fn decode(
    reader: &mut dyn ReadSeek,
    options: &DecodeOptions,
) -> Result<DecodeResult, CodecError> {
    decode_inner(reader, options)
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn decode_inner(
    reader: &mut dyn ReadSeek,
    options: &DecodeOptions,
) -> Result<DecodeResult, CodecError> {
    let scan = container::scan(reader)?;

    if options.container_only {
        let (ir, annotations, unknowns) = build_metadata_ir(&scan)?;
        let report = build_container_report(&scan, true);
        return decode_result(ir, report, annotations, unknowns);
    }

    let streams = active_body_streams(&scan);
    if !streams.is_empty() {
        if let Some((decoded, mut report)) = try_decode_brep(&scan, &streams) {
            let (ir, annotations, unknowns) = build_geometry_ir(
                &scan,
                streams[decoded.selected].origin,
                &streams[decoded.selected].header,
                decoded.brep,
                &decoded.configuration_bodies,
            )?;
            append_design_losses(&ir, &mut report);
            return decode_result(ir, report, annotations, unknowns);
        }
    }

    let (ir, annotations, unknowns) = build_metadata_ir(&scan)?;
    let mut report = build_container_report(&scan, false);
    append_design_losses(&ir, &mut report);
    decode_result(ir, report, annotations, unknowns)
}

fn decode_result(
    mut ir: CadIr,
    report: DecodeReport,
    annotations: Annotations,
    mut unknowns: Vec<UnknownRecord>,
) -> Result<DecodeResult, CodecError> {
    let mut source_fidelity = cadmpeg_ir::SourceFidelity {
        annotations,
        ..cadmpeg_ir::SourceFidelity::default()
    };
    let source_image = unknowns
        .iter()
        .position(|record| record.id.0 == "sldprt:file:source-image#0")
        .map(|index| unknowns.remove(index));
    source_fidelity.attach_native_unknown_records(&mut ir, "sldprt", &unknowns)?;
    if let Some(source_image) = source_image {
        source_fidelity.retain_unknown_records("sldprt", std::slice::from_ref(&source_image));
    }
    set_semantic_hash(&mut ir);
    Ok(DecodeResult::with_source_fidelity(
        ir,
        report,
        source_fidelity,
    ))
}

fn append_design_losses(ir: &CadIr, report: &mut DecodeReport) {
    use cadmpeg_ir::features::{
        BodyRetentionMode, BodySelection, BooleanOp, ChamferSpec, EdgeSelection, Extent,
        FaceSelection, FeatureDefinition, FeatureSourceContent, PathRef, PatternKind, ProfileRef,
        RadiusSpec,
    };
    use cadmpeg_ir::sketches::{SketchConstraintDefinition, SketchGeometry, SpatialSketchGeometry};

    let native = ir
        .native
        .namespace("sldprt")
        .and_then(|namespace| crate::native::SldprtNative::load(namespace).ok());

    let active_configurations = ir
        .model
        .configurations
        .iter()
        .filter(|configuration| configuration.active)
        .count();
    if !ir.model.configurations.is_empty() && active_configurations != 1 {
        report.losses.push(SldprtLossCode::ConfigActiveIdentityUnresolved.note(format!(
                "active configuration identity is unresolved; {active_configurations} of {} configuration records are active.",
                ir.model.configurations.len()
            )));
    }
    let active_partition = ir
        .source
        .as_ref()
        .and_then(|source| source.attributes.get("active_parasolid_block"))
        .and_then(|section| crate::container::configuration_index(section))
        .and_then(|index| u32::try_from(index).ok());
    let active_partition_mismatch = active_partition.filter(|active_partition| {
        ir.model
            .configurations
            .iter()
            .find(|configuration| configuration.active)
            .is_some_and(|configuration| {
                configuration.source_index.as_ref() != Some(active_partition)
            })
    });
    if let Some(active_partition) = active_partition_mismatch {
        report.losses.push(SldprtLossCode::ConfigActivePartitionMismatch.note(format!(
                "active configuration identity does not resolve to active geometry partition {active_partition}."
            )));
    }
    let inferred_configurations = ir
        .model
        .configurations
        .iter()
        .filter(|configuration| configuration.native_ref.is_none())
        .count();
    if inferred_configurations > 0 {
        report.losses.push(SldprtLossCode::ConfigInferredWithoutNative.note(format!(
                "{inferred_configurations} configuration state(s) are inferred from geometry partitions without native configuration definitions."
            )));
    }
    let unresolved_configuration_parameter_lanes = native.as_ref().map_or(0, |native| {
        let mut counts = BTreeMap::<&str, usize>::new();
        for key in native
            .feature_input_lanes
            .iter()
            .filter_map(|lane| lane.configuration.as_deref())
        {
            *counts.entry(key).or_default() += 1;
        }
        counts
            .into_iter()
            .map(|(key, count)| {
                let configuration_matches = key.parse::<u32>().ok().map_or(0, |source_index| {
                    ir.model
                        .configurations
                        .iter()
                        .filter(|configuration| {
                            configuration.source_index == Some(source_index)
                                || configuration.source_index.is_none()
                                    && configuration.ordinal == source_index
                        })
                        .count()
                });
                if count == 1 && configuration_matches == 1 {
                    0
                } else {
                    count
                }
            })
            .sum()
    });
    if unresolved_configuration_parameter_lanes > 0 {
        report.losses.push(SldprtLossCode::ConfigLaneIdentityUnresolved.note(format!(
                "{unresolved_configuration_parameter_lanes} configuration-scoped feature-input lane(s) have duplicate or unresolved configuration identity."
            )));
    }
    let mut configuration_source_counts = BTreeMap::new();
    for source_index in ir
        .model
        .configurations
        .iter()
        .filter_map(|configuration| configuration.source_index)
    {
        *configuration_source_counts
            .entry(source_index)
            .or_insert(0usize) += 1;
    }
    let ambiguous_configuration_sources = configuration_source_counts
        .values()
        .filter(|count| **count > 1)
        .copied()
        .sum::<usize>();
    if ambiguous_configuration_sources > 0 {
        report.losses.push(SldprtLossCode::ConfigAmbiguousPartition.note(format!(
                "{ambiguous_configuration_sources} configuration record(s) share non-unique geometry partition identities."
            )));
    }
    let empty_configuration_names = ir
        .model
        .configurations
        .iter()
        .filter(|configuration| configuration.name.is_empty())
        .count();
    let mut configuration_name_counts = BTreeMap::new();
    let mut configuration_ordinal_counts = BTreeMap::new();
    for configuration in &ir.model.configurations {
        *configuration_ordinal_counts
            .entry(configuration.ordinal)
            .or_insert(0usize) += 1;
    }
    for name in ir
        .model
        .configurations
        .iter()
        .map(|configuration| configuration.name.as_str())
        .filter(|name| !name.is_empty())
    {
        *configuration_name_counts.entry(name).or_insert(0usize) += 1;
    }
    let ambiguous_configuration_names = configuration_name_counts
        .values()
        .filter(|count| **count > 1)
        .copied()
        .sum::<usize>();
    let ambiguous_configuration_ordinals = configuration_ordinal_counts
        .values()
        .filter(|count| **count > 1)
        .copied()
        .sum::<usize>();
    if empty_configuration_names > 0
        || ambiguous_configuration_names > 0
        || ambiguous_configuration_ordinals > 0
    {
        report.losses.push(SldprtLossCode::ConfigAmbiguousNaming.note(format!(
                "{empty_configuration_names} configuration record(s) have empty names; {ambiguous_configuration_names} configuration record(s) share non-unique names; {ambiguous_configuration_ordinals} configuration record(s) share regeneration ordinals."
            )));
    }
    let model_body_ids = ir
        .model
        .bodies
        .iter()
        .map(|body| &body.id)
        .collect::<std::collections::HashSet<_>>();
    let incoherent_configuration_bodies = ir
        .model
        .configurations
        .iter()
        .filter(|configuration| {
            let mut bodies = std::collections::HashSet::new();
            configuration
                .bodies
                .resolved()
                .unwrap_or_default()
                .iter()
                .any(|body| !bodies.insert(body) || !model_body_ids.contains(body))
        })
        .count();
    if incoherent_configuration_bodies > 0 {
        report.losses.push(SldprtLossCode::ConfigIncoherentBodyRefs.note(format!(
                "{incoherent_configuration_bodies} configuration record(s) contain missing or repeated body references."
            )));
    }

    let feature_ids = ir
        .model
        .features
        .iter()
        .map(|feature| &feature.id)
        .collect::<std::collections::HashSet<_>>();
    let parameter_ids = ir
        .model
        .parameters
        .iter()
        .map(|parameter| &parameter.id)
        .collect::<std::collections::HashSet<_>>();
    let incomplete_configuration_feature_snapshots = ir
        .model
        .configurations
        .iter()
        .filter(|configuration| {
            !feature_ids.is_empty()
                && (configuration.feature_states.len() != feature_ids.len()
                    || configuration
                        .feature_states
                        .keys()
                        .any(|feature| !feature_ids.contains(feature)))
        })
        .count();
    let incomplete_configuration_parameter_snapshots = ir
        .model
        .configurations
        .iter()
        .filter(|configuration| {
            !parameter_ids.is_empty()
                && (configuration.parameter_values.len() != parameter_ids.len()
                    || configuration
                        .parameter_values
                        .keys()
                        .any(|parameter| !parameter_ids.contains(parameter)))
        })
        .count();
    if incomplete_configuration_feature_snapshots > 0
        || incomplete_configuration_parameter_snapshots > 0
    {
        report.losses.push(SldprtLossCode::ConfigIncompleteSnapshot.note(format!(
                "{incomplete_configuration_feature_snapshots} configuration(s) lack a complete evaluated feature snapshot; {incomplete_configuration_parameter_snapshots} configuration(s) lack a complete evaluated parameter snapshot."
            )));
    }

    let feature_names = ir
        .model
        .features
        .iter()
        .filter_map(|feature| {
            feature
                .name
                .as_ref()
                .map(|name| (feature.id.clone(), name.clone()))
        })
        .collect();
    let global_parameter_owners = crate::history::global_parameter_owners(&ir.model.features);
    let incomplete_parameters = ir
        .model
        .parameters
        .iter()
        .filter(|parameter| {
            parameter.value.is_none()
                && (ir.model.configurations.is_empty()
                    || ir.model.configurations.iter().any(|configuration| {
                        !configuration.parameter_values.contains_key(&parameter.id)
                    }))
        })
        .count();
    let unresolved_parameter_references = crate::history::parameters_with_unresolved_references(
        &ir.model.parameters,
        &feature_names,
        &global_parameter_owners,
    );
    let unevaluable_parameter_expressions = crate::history::parameters_with_unevaluable_expressions(
        &ir.model.parameters,
        &feature_names,
        &global_parameter_owners,
        &ir.model.configurations,
    );
    let feature_ordinals = ir
        .model
        .features
        .iter()
        .map(|feature| (&feature.id, feature.ordinal))
        .collect::<BTreeMap<_, _>>();
    let parameter_positions = ir
        .model
        .parameters
        .iter()
        .map(|parameter| (&parameter.id, (&parameter.owner, parameter.ordinal)))
        .collect::<BTreeMap<_, _>>();
    let invalid_parameter_dependency_order = ir
        .model
        .parameters
        .iter()
        .filter(|parameter| {
            parameter.dependencies.iter().any(|dependency| {
                let Some((owner, ordinal)) = parameter_positions.get(dependency) else {
                    return true;
                };
                if *owner == &parameter.owner {
                    return *ordinal >= parameter.ordinal;
                }
                feature_ordinals
                    .get(*owner)
                    .zip(feature_ordinals.get(&parameter.owner))
                    .is_none_or(|(dependency_owner, consumer_owner)| {
                        dependency_owner >= consumer_owner
                    })
            })
        })
        .count();
    let incoherent_parameter_dependencies = crate::history::parameters_with_incoherent_dependencies(
        &ir.model.parameters,
        &feature_names,
        &global_parameter_owners,
    );
    let incoherent_parameter_values = crate::history::parameters_with_incoherent_evaluated_values(
        &ir.model.parameters,
        &feature_names,
        &global_parameter_owners,
        &ir.model.configurations,
    );
    if incomplete_parameters > 0
        || unresolved_parameter_references > 0
        || unevaluable_parameter_expressions > 0
        || invalid_parameter_dependency_order > 0
        || incoherent_parameter_dependencies > 0
        || incoherent_parameter_values > 0
    {
        report.losses.push(SldprtLossCode::ParameterUnevaluated.note(format!(
                "{incomplete_parameters} parameter(s) lack an evaluated scalar; {unresolved_parameter_references} parameter expression(s) contain unresolved, ambiguous, or malformed parameter references; {unevaluable_parameter_expressions} parameter expression(s) cannot regenerate a finite typed value; {invalid_parameter_dependency_order} parameter record(s) contain missing or non-preceding dependency edges; {incoherent_parameter_dependencies} parameter record(s) have dependency edges inconsistent with their expressions; {incoherent_parameter_values} dependency-driven parameter(s) disagree with their evaluated expressions."
            )));
    }
    let empty_parameter_names = ir
        .model
        .parameters
        .iter()
        .filter(|parameter| parameter.name.is_empty())
        .count();
    let mut parameter_name_counts = BTreeMap::new();
    let mut parameter_ordinal_counts = BTreeMap::new();
    for parameter in &ir.model.parameters {
        if !parameter.name.is_empty() {
            *parameter_name_counts
                .entry((&parameter.owner, parameter.name.as_str()))
                .or_insert(0usize) += 1;
        }
        *parameter_ordinal_counts
            .entry((&parameter.owner, parameter.ordinal))
            .or_insert(0usize) += 1;
    }
    let duplicate_parameter_names = parameter_name_counts
        .values()
        .filter(|count| **count > 1)
        .copied()
        .sum::<usize>();
    let duplicate_parameter_ordinals = parameter_ordinal_counts
        .values()
        .filter(|count| **count > 1)
        .copied()
        .sum::<usize>();
    if empty_parameter_names > 0
        || duplicate_parameter_names > 0
        || duplicate_parameter_ordinals > 0
    {
        report.losses.push(SldprtLossCode::ParameterAmbiguousIdentity.note(format!(
                "{empty_parameter_names} parameter record(s) have empty names; {duplicate_parameter_names} parameter record(s) share owner-local names; {duplicate_parameter_ordinals} parameter record(s) share owner-local ordinals."
            )));
    }

    let bound_pmi = ir
        .model
        .parameters
        .iter()
        .filter_map(|parameter| parameter.pmi.as_ref())
        .map(|pmi| pmi.native_ref.as_str())
        .collect::<std::collections::HashSet<_>>();
    let unbound_pmi_dimensions = native.as_ref().map_or(0, |native| {
        native
            .pmi_dimensions
            .iter()
            .filter(|dimension| !bound_pmi.contains(dimension.id.as_str()))
            .count()
    });
    let native_pmi_subtypes = ir
        .model
        .parameters
        .iter()
        .filter(|parameter| {
            parameter.pmi.as_ref().is_some_and(|pmi| {
                matches!(
                    pmi.subtype,
                    cadmpeg_ir::features::PmiDimensionSubtype::Native(_)
                )
            })
        })
        .count();
    if unbound_pmi_dimensions > 0 || native_pmi_subtypes > 0 {
        report.losses.push(SldprtLossCode::PmiDimensionUnbound.note(format!(
                "{unbound_pmi_dimensions} semantic dimension record(s) are not bound to parameters; {native_pmi_subtypes} parameter dimension(s) retain native subtypes."
            )));
    }

    let incomplete_history_references = native.as_ref().map_or(0, |native| {
        crate::history::incomplete_history_reference_features(&native.feature_histories)
    });
    if incomplete_history_references > 0 {
        report.losses.push(SldprtLossCode::HistoryIncompleteReferences.note(format!(
                "{incomplete_history_references} feature history record(s) contain duplicate identities or unresolved parent, dependency, dimension, or child references."
            )));
    }
    let feature_positions = ir
        .model
        .features
        .iter()
        .map(|feature| (&feature.id, feature.ordinal))
        .collect::<BTreeMap<_, _>>();
    let evaluated_feature_states = if ir
        .model
        .configurations
        .iter()
        .any(|configuration| !configuration.feature_states.is_empty())
    {
        ir.model
            .configurations
            .iter()
            .flat_map(|configuration| {
                ir.model.features.iter().filter_map(move |feature| {
                    configuration.feature_states.get(&feature.id).map(|state| {
                        EvaluatedFeatureState {
                            feature,
                            dependencies: &state.dependencies,
                            outputs: &state.outputs,
                            definition: &state.definition,
                        }
                    })
                })
            })
            .collect::<Vec<_>>()
    } else {
        ir.model
            .features
            .iter()
            .map(|feature| EvaluatedFeatureState {
                feature,
                dependencies: &feature.dependencies,
                outputs: &feature.outputs,
                definition: &feature.definition,
            })
            .collect::<Vec<_>>()
    };
    let incoherent_feature_edges = evaluated_feature_states
        .iter()
        .filter(|state| {
            let feature = state.feature;
            let parent_incoherent = feature.parent.as_ref().is_some_and(|parent| {
                feature_positions
                    .get(parent)
                    .is_none_or(|ordinal| *ordinal >= feature.ordinal)
            });
            let mut dependencies = std::collections::HashSet::new();
            parent_incoherent
                || state.dependencies.iter().any(|dependency| {
                    !dependencies.insert(dependency)
                        || feature_positions
                            .get(dependency)
                            .is_none_or(|ordinal| *ordinal >= feature.ordinal)
                })
        })
        .count();
    let mut feature_ordinal_counts = BTreeMap::new();
    for feature in &ir.model.features {
        *feature_ordinal_counts
            .entry(feature.ordinal)
            .or_insert(0usize) += 1;
    }
    let duplicate_feature_ordinals = feature_ordinal_counts
        .values()
        .filter(|count| **count > 1)
        .copied()
        .sum::<usize>();
    if incoherent_feature_edges > 0 || duplicate_feature_ordinals > 0 {
        report.losses.push(SldprtLossCode::FeatureIncoherentEdges.note(format!(
                "{incoherent_feature_edges} feature record(s) contain missing, repeated, or non-preceding parent/dependency edges; {duplicate_feature_ordinals} feature record(s) share regeneration ordinals."
            )));
    }
    let parameter_owners = ir
        .model
        .parameters
        .iter()
        .map(|parameter| (&parameter.id, &parameter.owner))
        .collect::<BTreeMap<_, _>>();
    let features_by_id = ir
        .model
        .features
        .iter()
        .map(|feature| (&feature.id, feature))
        .collect::<BTreeMap<_, _>>();
    let incoherent_feature_content = ir
        .model
        .features
        .iter()
        .filter(|feature| {
            let mut parameters = std::collections::HashSet::new();
            let mut children = std::collections::HashSet::new();
            feature.source_content.iter().any(|content| match content {
                FeatureSourceContent::Text(_) => false,
                FeatureSourceContent::Parameter(parameter) => {
                    !parameters.insert(parameter)
                        || parameter_owners
                            .get(parameter)
                            .is_none_or(|owner| *owner != &feature.id)
                }
                FeatureSourceContent::Feature(child) => {
                    !children.insert(child)
                        || features_by_id.get(child).is_none_or(|child| {
                            child.ordinal <= feature.ordinal
                                || child.parent.as_ref() != Some(&feature.id)
                        })
                }
            })
        })
        .count();
    if incoherent_feature_content > 0 {
        report.losses.push(SldprtLossCode::FeatureIncoherentContent.note(format!(
                "{incoherent_feature_content} feature record(s) contain missing, repeated, misowned, or structurally inconsistent source-content references."
            )));
    }

    let unresolved_output_scopes = evaluated_feature_states
        .iter()
        .filter(|state| {
            state
                .feature
                .source_properties
                .get("Scope")
                .is_some_and(|scope| !scope.trim().is_empty())
                && state.outputs.is_empty()
        })
        .count();
    if unresolved_output_scopes > 0 {
        report.losses.push(SldprtLossCode::FeatureUnresolvedOutputScope.note(format!(
                "{unresolved_output_scopes} feature(s) retain non-empty native output scopes that do not resolve to model bodies."
            )));
    }
    let body_ids = ir
        .model
        .bodies
        .iter()
        .map(|body| &body.id)
        .collect::<std::collections::HashSet<_>>();
    let incoherent_feature_outputs = evaluated_feature_states
        .iter()
        .filter(|state| {
            let mut outputs = std::collections::HashSet::new();
            state
                .outputs
                .iter()
                .any(|body| !outputs.insert(body) || !body_ids.contains(body))
        })
        .count();
    if incoherent_feature_outputs > 0 {
        report.losses.push(SldprtLossCode::FeatureIncoherentOutputs.note(format!(
                "{incoherent_feature_outputs} feature record(s) contain missing or repeated output body references."
            )));
    }

    let native_constraints = ir
        .model
        .sketch_constraints
        .iter()
        .filter(|constraint| {
            matches!(
                constraint.definition,
                SketchConstraintDefinition::Native { .. }
            )
        })
        .count();
    if native_constraints > 0 {
        report.losses.push(SldprtLossCode::SketchNativeConstraint.note(format!(
                "{native_constraints} sketch constraint(s) retain native relation kinds and operands without complete neutral geometric semantics."
            )));
    }

    let native_sketch_geometry = ir
        .model
        .sketch_entities
        .iter()
        .filter(|entity| matches!(entity.geometry, SketchGeometry::Native { .. }))
        .count()
        + ir.model
            .spatial_sketch_entities
            .iter()
            .filter(|entity| matches!(entity.geometry, SpatialSketchGeometry::Native { .. }))
            .count();
    if native_sketch_geometry > 0 {
        report.losses.push(SldprtLossCode::SketchNativeGeometry.note(format!(
                "{native_sketch_geometry} sketch entity geometry record(s) retain native kinds without solved neutral geometry."
            )));
    }

    let unprojected_relations = native
        .as_ref()
        .map_or(0, |native| unprojected_sketch_relation_records(ir, native));
    if unprojected_relations > 0 {
        report.losses.push(SldprtLossCode::SketchRelationUnprojected.note(format!(
                "{unprojected_relations} native sketch relation record(s) have no projected neutral constraint."
            )));
    }
    let multiply_projected_relations = native.as_ref().map_or(0, |native| {
        multiply_projected_sketch_relation_records(ir, native)
    });
    if multiply_projected_relations > 0 {
        report.losses.push(SldprtLossCode::SketchRelationMultiplyProjected.note(format!(
                "{multiply_projected_relations} native sketch relation record(s) are claimed by multiple neutral objects."
            )));
    }

    let native_features = evaluated_feature_states
        .iter()
        .filter(|state| matches!(state.definition, FeatureDefinition::Native { .. }))
        .count();
    if native_features > 0 {
        report.losses.push(SldprtLossCode::FeatureNativeKindRetained.note(format!(
                "{native_features} feature(s) retain their native kind without a complete neutral operation definition."
            )));
    }
    let unbound_feature_input_objects = native
        .as_ref()
        .map_or(0, unbound_feature_input_operation_objects);
    if unbound_feature_input_objects > 0 {
        report.losses.push(SldprtLossCode::FeatureInputObjectUnbound.note(format!(
                "{unbound_feature_input_objects} native feature-input operation object(s) do not bind uniquely to a history feature."
            )));
    }

    let incomplete_edge_selection = |selection: &EdgeSelection| match selection {
        EdgeSelection::Edges(edges) | EdgeSelection::Resolved { edges, .. } => edges.is_empty(),
        EdgeSelection::Generated { edges, .. } => edges.is_empty(),
        EdgeSelection::All => false,
        EdgeSelection::Unresolved | EdgeSelection::Native(_) => true,
    };
    let incomplete_face_selection = |selection: &FaceSelection| match selection {
        FaceSelection::Faces(faces) | FaceSelection::Resolved { faces, .. } => faces.is_empty(),
        FaceSelection::Generated { faces, .. } => faces.is_empty(),
        FaceSelection::Unresolved | FaceSelection::Native(_) => true,
    };
    let incomplete_optional_face_selection = |selection: &FaceSelection| match selection {
        FaceSelection::Faces(_) | FaceSelection::Resolved { .. } => false,
        FaceSelection::Generated { faces, .. } => faces.is_empty(),
        FaceSelection::Unresolved | FaceSelection::Native(_) => true,
    };
    let incomplete_body_selection = |selection: &BodySelection| match selection {
        BodySelection::Bodies(bodies) | BodySelection::Resolved { bodies, .. } => bodies.is_empty(),
        BodySelection::Generated { bodies, .. } => bodies.is_empty(),
        BodySelection::Local { bodies, .. } => bodies.is_empty(),
        BodySelection::Unresolved | BodySelection::Native(_) => true,
    };
    let incomplete_profile = |profile: &ProfileRef| match profile {
        ProfileRef::Faces(faces) => faces.is_empty(),
        ProfileRef::Generated { curves, .. } => curves.is_empty(),
        ProfileRef::Unresolved(_) | ProfileRef::Native(_) => true,
        ProfileRef::Sketch(_) | ProfileRef::Feature(_) => false,
    };
    let incomplete_path = |path: &PathRef| match path {
        PathRef::Edges(edges) => edges.is_empty(),
        PathRef::Curves(curves) => curves.is_empty(),
        PathRef::Unresolved | PathRef::Native(_) => true,
        PathRef::Sketch(_) => false,
    };
    let incomplete_extent = |extent: &Extent| {
        matches!(extent, Extent::Unresolved)
            || matches!(extent, Extent::ToFace { face } if incomplete_face_selection(face))
    };
    let incomplete_typed_features = evaluated_feature_states
        .iter()
        .filter(|state| match state.definition {
            FeatureDefinition::TreeNode { .. }
            | FeatureDefinition::DatumPrincipalPlane { .. }
            | FeatureDefinition::DatumPlane { .. }
            | FeatureDefinition::DatumAxis { .. }
            | FeatureDefinition::DatumPoint { .. }
            | FeatureDefinition::DatumCoordinateSystem { .. }
            | FeatureDefinition::EquationCurve { .. }
            | FeatureDefinition::Helix { .. } => false,
            FeatureDefinition::CosmeticThread {
                face,
                diameter,
                extent,
            } => {
                incomplete_face_selection(face)
                    || diameter.is_none()
                    || extent.is_none()
            }
            FeatureDefinition::SketchBlockDefinition { sketch } => sketch.is_none(),
            FeatureDefinition::SketchBlockInstance { block, placement } => {
                block.is_none() || placement.is_none()
            }
            FeatureDefinition::DatumOffsetPlane { reference, .. } => reference.is_none(),
            FeatureDefinition::ProjectedCurve {
                source,
                target_faces,
                ..
            } => incomplete_path(source) || incomplete_face_selection(target_faces),
            FeatureDefinition::CompositeCurve { segments, .. } => {
                segments.is_empty() || segments.iter().any(incomplete_path)
            }
            FeatureDefinition::HelixNativeAxis { .. } => true,
            FeatureDefinition::Wrap {
                profile,
                face,
                mode,
                depth,
            } => {
                incomplete_profile(profile)
                    || incomplete_face_selection(face)
                    || (*mode != cadmpeg_ir::features::WrapMode::Scribe && depth.is_none())
            }
            FeatureDefinition::Sketch { sketch, .. } => sketch.is_none(),
            FeatureDefinition::SpatialSketch { sketch } => sketch.is_none(),
            FeatureDefinition::Extrude {
                profile,
                extent,
                op,
                ..
            } => {
                incomplete_profile(profile)
                    || incomplete_extent(extent)
                    || *op == BooleanOp::Unresolved
            }
            FeatureDefinition::Revolve { construction, op } => {
                construction.profile.as_ref().is_none_or(incomplete_profile)
                    || construction.axis.is_none()
                    || construction.extent.as_ref().is_none_or(incomplete_extent)
                    || *op == BooleanOp::Unresolved
            }
            FeatureDefinition::Sweep {
                profile,
                path,
                mode,
                ..
            } => {
                profile.as_ref().is_none_or(incomplete_profile)
                    || path.as_ref().is_none_or(incomplete_path)
                    || matches!(mode, cadmpeg_ir::features::SweepMode::Unresolved)
                    || matches!(mode, cadmpeg_ir::features::SweepMode::Solid { op } if *op == BooleanOp::Unresolved)
            }
            FeatureDefinition::Loft {
                profiles,
                guides,
                op,
                ..
            } => {
                profiles.len() < 2
                    || profiles.iter().any(incomplete_profile)
                    || guides.iter().any(incomplete_path)
                    || *op == BooleanOp::Unresolved
            }
            FeatureDefinition::Rib { construction, op } => {
                construction.profile.as_ref().is_none_or(incomplete_profile)
                    || construction.direction.is_none()
                    || construction.thickness.is_none()
                    || construction.side.is_none()
                    || matches!(construction.draft, cadmpeg_ir::features::RibDraft::Unresolved)
                    || *op == BooleanOp::Unresolved
            }
            FeatureDefinition::Fillet { edges, radius } => {
                incomplete_edge_selection(edges) || matches!(radius, RadiusSpec::Unresolved { .. })
            }
            FeatureDefinition::Chamfer { edges, spec, .. } => {
                incomplete_edge_selection(edges) || matches!(spec, ChamferSpec::Unresolved { .. })
            }
            FeatureDefinition::Shell {
                removed_faces,
                thickness,
                outward,
                ..
            } => {
                incomplete_optional_face_selection(removed_faces)
                    || thickness.is_none()
                    || outward.is_none()
            }
            FeatureDefinition::Thicken {
                faces,
                thickness,
                side,
            } => incomplete_face_selection(faces) || thickness.is_none() || side.is_none(),
            FeatureDefinition::OffsetSurface { faces, .. }
            | FeatureDefinition::KnitSurface { faces, .. }
            | FeatureDefinition::ExtendSurface { faces, .. } => incomplete_face_selection(faces),
            FeatureDefinition::FilledSurface {
                boundary,
                support_faces,
                continuity,
                ..
            } => {
                incomplete_edge_selection(boundary)
                    || if *continuity
                        == cadmpeg_ir::features::SurfaceContinuity::Contact
                    {
                        incomplete_optional_face_selection(support_faces)
                    } else {
                        incomplete_face_selection(support_faces)
                    }
            }
            FeatureDefinition::TrimSurface { faces, tool, .. } => {
                incomplete_face_selection(faces) || incomplete_path(tool)
            }
            FeatureDefinition::RuledSurface {
                edges,
                support_faces,
                mode,
            } => {
                incomplete_edge_selection(edges)
                    || if matches!(mode, cadmpeg_ir::features::RuledSurfaceMode::Direction { .. }) {
                        incomplete_optional_face_selection(support_faces)
                    } else {
                        incomplete_face_selection(support_faces)
                    }
            }
            FeatureDefinition::Draft {
                faces,
                neutral_plane,
                ..
            } => incomplete_face_selection(faces) || incomplete_face_selection(neutral_plane),
            FeatureDefinition::Combine { target, tools, op } => {
                incomplete_body_selection(target)
                    || incomplete_body_selection(tools)
                    || *op == BooleanOp::Unresolved
            }
            FeatureDefinition::CutWithSurface { targets, tools, .. } => {
                incomplete_body_selection(targets) || incomplete_face_selection(tools)
            }
            FeatureDefinition::DeleteBody { bodies, mode } => {
                incomplete_body_selection(bodies) || *mode == BodyRetentionMode::Unresolved
            }
            FeatureDefinition::DeleteFace { faces, .. } => incomplete_face_selection(faces),
            FeatureDefinition::ReplaceFace {
                targets,
                replacements,
            } => incomplete_face_selection(targets) || incomplete_face_selection(replacements),
            FeatureDefinition::MoveFace { faces, .. } => incomplete_face_selection(faces),
            FeatureDefinition::MoveBody { bodies, .. } => incomplete_body_selection(bodies),
            FeatureDefinition::Dome {
                faces,
                height,
                elliptical,
                reverse,
            } => {
                incomplete_face_selection(faces)
                    || height.is_none()
                    || elliptical.is_none()
                    || reverse.is_none()
            }
            FeatureDefinition::Flex { axis, mode } => {
                axis.is_none()
                    || matches!(mode, cadmpeg_ir::features::FlexMode::Unresolved { .. })
            }
            FeatureDefinition::Scale {
                bodies,
                center,
                factors,
            } => {
                incomplete_body_selection(bodies)
                    || center.as_ref().is_none_or(|center| {
                        matches!(center, cadmpeg_ir::features::ScaleCenter::Native(_))
                    })
                    || factors.resolved().is_none()
            }
            FeatureDefinition::Hole {
                face,
                placements,
                kind,
                diameter,
                extent,
                ..
            } => {
                face.as_ref().is_some_and(incomplete_face_selection)
                    || placements.is_empty()
                    || matches!(kind, cadmpeg_ir::features::HoleKind::Unresolved { .. })
                    || diameter.is_none()
                    || extent.as_ref().is_none_or(incomplete_extent)
            }
            FeatureDefinition::Pattern { seeds, pattern } => {
                seeds.is_empty()
                    || seeds.iter().any(|seed| match seed {
                        cadmpeg_ir::features::PatternSeed::Feature(_) => false,
                        cadmpeg_ir::features::PatternSeed::Faces(faces) => {
                            incomplete_face_selection(faces)
                        }
                        cadmpeg_ir::features::PatternSeed::Bodies(bodies) => {
                            incomplete_body_selection(bodies)
                        }
                    })
                    || matches!(pattern, PatternKind::Unresolved { .. })
                    || matches!(pattern, PatternKind::Linear { direction: None, .. })
                    || matches!(pattern, PatternKind::CurveDriven { path: None, .. })
            }
            FeatureDefinition::Native { .. } => false,
            _ => false,
        })
        .count();
    if incomplete_typed_features > 0 {
        report.losses.push(SldprtLossCode::FeatureTypedOperandIncomplete.note(format!(
                "{incomplete_typed_features} typed feature(s) retain native or unresolved required operation operands."
            )));
    }

    let unresolved_body_modes = evaluated_feature_states
        .iter()
        .filter(|state| {
            matches!(
                state.definition,
                FeatureDefinition::DeleteBody {
                    mode: BodyRetentionMode::Unresolved,
                    ..
                }
            )
        })
        .count();
    if unresolved_body_modes > 0 {
        report.losses.push(SldprtLossCode::FeatureBodyRetentionUnresolved.note(format!(
                "{unresolved_body_modes} body delete/keep feature(s) retain selected native body identities without a decoded retention mode."
            )));
    }
}

fn unbound_feature_input_operation_objects(native: &crate::native::SldprtNative) -> usize {
    use crate::records::FeatureInputClassRole;

    let mut source_counts = BTreeMap::<u32, usize>::new();
    let mut binding_counts = BTreeMap::<(u32, &str), usize>::new();
    for feature in native
        .feature_histories
        .iter()
        .flat_map(|history| &history.features)
    {
        let Some(source) = feature
            .source_id
            .as_deref()
            .and_then(|source| source.parse::<u32>().ok())
        else {
            continue;
        };
        *source_counts.entry(source).or_default() += 1;
        if let Some(class) = feature.input_class.as_deref() {
            *binding_counts.entry((source, class)).or_default() += 1;
        }
    }
    native
        .feature_input_lanes
        .iter()
        .flat_map(|lane| {
            lane.classes
                .iter()
                .filter(|class| class.role == FeatureInputClassRole::Feature)
                .filter_map(|class| {
                    let name_offset = class.offset + 6 + class.name.len() as u64;
                    lane.names
                        .iter()
                        .find(|name| name.offset == name_offset)
                        .map(|name| (class, name))
                })
        })
        .filter(|(class, name)| {
            name.object_id.is_none_or(|id| {
                source_counts.get(&id).copied() != Some(1)
                    || binding_counts.get(&(id, class.name.as_str())).copied() != Some(1)
            })
        })
        .count()
}

fn unprojected_sketch_relation_records(ir: &CadIr, native: &crate::native::SldprtNative) -> usize {
    let projected = ir
        .model
        .sketch_constraints
        .iter()
        .filter_map(|constraint| constraint.native_ref.clone())
        .chain(
            ir.model
                .sketch_entities
                .iter()
                .filter_map(|entity| entity.native_ref.clone()),
        )
        .chain(
            ir.model
                .spatial_sketch_entities
                .iter()
                .filter_map(|entity| entity.native_ref.clone()),
        )
        .collect::<std::collections::HashSet<_>>();
    let owned_instances = crate::resolved_features::owned_relation_parameters(
        &ir.model.features,
        &ir.model.parameters,
        &native.feature_input_lanes,
    );

    native
        .feature_input_lanes
        .iter()
        .map(|lane| {
            let markers_by_id = lane
                .sketch_entities
                .iter()
                .map(|marker| (marker.id.as_str(), marker))
                .collect();
            let instances = lane
                .relation_instances
                .iter()
                .filter(|relation| {
                    owned_instances.contains_key(&relation.id) && !projected.contains(&relation.id)
                })
                .count();
            let bindings = lane
                .relation_bindings
                .iter()
                .filter(|binding| {
                    !lane.relation_instances.iter().any(|relation| {
                        relation.class_ref == binding.class_ref
                            && relation.scalar_refs.contains(&binding.scalar_ref)
                    })
                })
                .count();
            let markers = lane
                .sketch_entities
                .iter()
                .filter(|marker| {
                    crate::resolved_features::marker_owns_constraint(marker, &markers_by_id)
                        && !projected.contains(&marker.id)
                })
                .count();
            instances + bindings + markers
        })
        .sum()
}

fn multiply_projected_sketch_relation_records(
    ir: &CadIr,
    native: &crate::native::SldprtNative,
) -> usize {
    let native_relation_ids = native
        .feature_input_lanes
        .iter()
        .flat_map(|lane| {
            let markers_by_id = lane
                .sketch_entities
                .iter()
                .map(|marker| (marker.id.as_str(), marker))
                .collect();
            lane.relation_instances
                .iter()
                .map(|relation| relation.id.as_str())
                .chain(lane.sketch_entities.iter().filter_map(move |marker| {
                    crate::resolved_features::marker_owns_constraint(marker, &markers_by_id)
                        .then_some(marker.id.as_str())
                }))
        })
        .collect::<std::collections::HashSet<_>>();
    let mut projection_counts = BTreeMap::<&str, usize>::new();
    for native_ref in ir
        .model
        .sketch_constraints
        .iter()
        .filter_map(|constraint| constraint.native_ref.as_deref())
        .chain(
            ir.model
                .sketch_entities
                .iter()
                .filter_map(|entity| entity.native_ref.as_deref()),
        )
        .chain(
            ir.model
                .spatial_sketch_entities
                .iter()
                .filter_map(|entity| entity.native_ref.as_deref()),
        )
        .filter(|native_ref| native_relation_ids.contains(native_ref))
    {
        *projection_counts.entry(native_ref).or_default() += 1;
    }
    projection_counts
        .values()
        .filter(|count| **count > 1)
        .count()
}

/// Decode the active Parasolid stream's B-rep. Returns `None` when the stream
/// frames but yields no geometry, so the caller falls back to metadata.
fn active_body_streams(scan: &ContainerScan) -> Vec<BodyStream<'_>> {
    let block_streams = scan.blocks.iter().flat_map(|block| {
        block.ps_streams.iter().filter_map(move |payload| {
            let header = crate::parasolid::stream_header(payload)?;
            let section = block.section.as_deref().unwrap_or("").to_ascii_lowercase();
            if crate::parasolid::is_body_stream(&header)
                && !section.contains("ghost")
                && !section.contains("resolvedfeatures")
            {
                Some(BodyStream {
                    origin: BodyOrigin::Block(block),
                    payload,
                    header,
                })
            } else {
                None
            }
        })
    });
    let compound_streams = scan.compound_streams.iter().flat_map(|stream| {
        stream.ps_streams.iter().filter_map(move |payload| {
            let header = crate::parasolid::stream_header(payload)?;
            let section = stream.path.to_ascii_lowercase();
            (crate::parasolid::is_body_stream(&header)
                && !section.contains("ghost")
                && !section.contains("resolvedfeatures"))
            .then_some(BodyStream {
                origin: BodyOrigin::Compound(stream),
                payload,
                header,
            })
        })
    });
    let mut streams = block_streams.chain(compound_streams).collect::<Vec<_>>();
    streams.sort_by_key(|stream| {
        let section = stream.origin.name().to_ascii_lowercase();
        (
            !section.contains("partition"),
            !stream
                .header
                .description
                .to_ascii_lowercase()
                .contains("partition"),
        )
    });
    streams
}

fn try_decode_brep(
    scan: &ContainerScan,
    streams: &[BodyStream<'_>],
) -> Option<(DecodedBrep, DecodeReport)> {
    let mut sites: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (index, stream) in streams.iter().enumerate() {
        sites
            .entry(site_key(&stream.origin.name()))
            .or_default()
            .push(index);
    }
    let mut decoded_sites = Vec::new();
    for (site, indices) in &sites {
        let first = indices[0];
        let name = streams[first].origin.name();
        let bodies: Vec<_> = indices
            .iter()
            .map(|index| (streams[*index].payload, &streams[*index].header))
            .collect();
        let decoded = brep::decode_bodies(&bodies, &name);
        let score = (
            decoded.faces.len(),
            decoded.bodies.len(),
            decoded.points.len(),
        );
        decoded_sites.push((site.clone(), first, score, decoded));
    }
    let selected_site = decoded_sites
        .iter()
        .enumerate()
        .max_by_key(|(index, (_, _, score, _))| (*score, Reverse(*index)))
        .map(|(index, _)| index)?;
    let selected_is_empty_model = decoded_sites[selected_site].3.stats.source_entity_records == 0
        && sites[&decoded_sites[selected_site].0].iter().any(|index| {
            streams[*index]
                .header
                .description
                .to_ascii_lowercase()
                .contains("partition")
        })
        && sites[&decoded_sites[selected_site].0].iter().any(|index| {
            streams[*index]
                .header
                .description
                .to_ascii_lowercase()
                .contains("deltas")
        });
    if !selected_is_empty_model
        && decoded_sites[selected_site].3.faces.is_empty()
        && decoded_sites[selected_site].3.surfaces.is_empty()
        && decoded_sites[selected_site].3.points.is_empty()
    {
        return None;
    }
    let (_, selected, _, mut decoded) = decoded_sites.swap_remove(selected_site);
    let mut configuration_bodies = Vec::new();
    if let Some(index) = configuration_index(&streams[selected].origin.name()) {
        configuration_bodies.push((
            index,
            decoded.bodies.iter().map(|body| body.id.clone()).collect(),
        ));
    }
    for (site, first, _, mut alternate) in decoded_sites {
        alternate.qualify_ids(&site);
        if let Some(index) = configuration_index(&streams[first].origin.name()) {
            configuration_bodies.push((
                index,
                alternate
                    .bodies
                    .iter()
                    .map(|body| body.id.clone())
                    .collect(),
            ));
        }
        merge_brep(&mut decoded, alternate);
    }
    let report = build_geometry_report(scan, &decoded);
    Some((
        DecodedBrep {
            selected,
            brep: decoded,
            configuration_bodies,
        },
        report,
    ))
}

fn merge_brep(target: &mut Brep, mut source: Brep) {
    let stream_base = target.annotations.streams.len() as u32;
    target
        .annotations
        .streams
        .append(&mut source.annotations.streams);
    for provenance in source.annotations.provenance.values_mut() {
        provenance.stream += stream_base;
    }
    target
        .annotations
        .provenance
        .append(&mut source.annotations.provenance);
    target
        .annotations
        .exactness
        .append(&mut source.annotations.exactness);
    target.bodies.append(&mut source.bodies);
    target.regions.append(&mut source.regions);
    target.shells.append(&mut source.shells);
    target.faces.append(&mut source.faces);
    target.loops.append(&mut source.loops);
    target.coedges.append(&mut source.coedges);
    target.edges.append(&mut source.edges);
    target.vertices.append(&mut source.vertices);
    target.points.append(&mut source.points);
    target.surfaces.append(&mut source.surfaces);
    target.curves.append(&mut source.curves);
    target.pcurves.append(&mut source.pcurves);
    target.unknowns.append(&mut source.unknowns);
    target.face_colors.append(&mut source.face_colors);
    target.stats.unknown_surface_faces += source.stats.unknown_surface_faces;
    target.stats.unknown_curve_edges += source.stats.unknown_curve_edges;
    target.stats.source_entity_records += source.stats.source_entity_records;
    target.stats.synthetic_body_grouping |= source.stats.synthetic_body_grouping;
}

fn site_key(name: &str) -> String {
    let mut key = name.to_ascii_lowercase();
    for suffix in ["partition", "deltas"] {
        if let Some(at) = key.rfind(suffix) {
            key.truncate(at);
            break;
        }
    }
    key.trim_end_matches(['-', '/', '_']).to_string()
}

fn build_geometry_ir(
    scan: &ContainerScan,
    origin: BodyOrigin<'_>,
    header: &StreamHeader,
    mut brep: Brep,
    configuration_bodies: &[(usize, Vec<cadmpeg_ir::ids::BodyId>)],
) -> Result<(CadIr, Annotations, Vec<UnknownRecord>), CodecError> {
    let mut ir = CadIr::empty(Units::default());
    let materials = crate::appearance::materials(scan);
    let unique_material = materials.len() == 1;
    if let [material] = materials.as_slice() {
        for body in &mut brep.bodies {
            body.color = Some(material.color);
            if body.name.is_none() {
                body.name = Some(material.name.clone());
            }
        }
    }
    ir.source = Some(source_meta(scan, origin, header));
    let mut annotations = std::mem::take(&mut brep.annotations);
    let mut histories = crate::history::histories(scan, &mut annotations);
    let mut lanes = crate::resolved_features::lanes(scan, &mut annotations);
    crate::resolved_features::bind_history_classes(&mut histories, &lanes);
    crate::resolved_features::bind_scalar_operands(&histories, &mut lanes);
    let pmi_dimensions = crate::pmi::dimensions(scan, &mut annotations);
    project_design_history(&mut ir, &histories, &lanes, &pmi_dimensions, scan);
    let (spatial_sketches, spatial_sketch_entities) =
        crate::resolved_features::spatial_sketches(&mut ir.model.features, &histories, &lanes);
    ir.model.spatial_sketches = spatial_sketches;
    ir.model.spatial_sketch_entities = spatial_sketch_entities;
    crate::resolved_features::bind_extrusion_operations(&mut ir.model.features, &histories, &lanes);
    crate::resolved_features::bind_revolution_operations(
        &mut ir.model.features,
        &histories,
        &lanes,
    );
    crate::resolved_features::bind_sweep_operations(&mut ir.model.features, &histories, &lanes);
    crate::pmi::apply_to_parameters(
        &mut ir.model.parameters,
        &ir.model.features,
        &pmi_dimensions,
    );
    crate::resolved_features::bind_parameter_scalars(
        &mut ir.model.parameters,
        &ir.model.features,
        &histories,
        parameter_identity_lanes(&lanes),
    );
    crate::resolved_features::type_display_relation_parameters(
        &mut ir.model.parameters,
        &ir.model.features,
        &lanes,
    );
    crate::history::align_configuration_parameter_kinds(&mut ir);
    stamp_parameter_baseline(&mut ir);
    let (mut sketches, mut sketch_entities, mut sketch_constraints) =
        crate::resolved_features::sketches(scan, &mut annotations);
    crate::resolved_features::bind_sketch_profiles(
        &mut ir.model.features,
        &mut sketches,
        &sketch_entities,
        &ir.model.parameters,
        &histories,
        &lanes,
        &annotations,
    );
    crate::resolved_features::project_compact_sketch_profiles(
        &mut ir.model.features,
        &mut sketches,
        &mut sketch_entities,
        &histories,
        &lanes,
    );
    crate::resolved_features::project_marker_backed_sketches(
        &mut ir.model.features,
        &mut sketches,
        &mut sketch_entities,
        &histories,
        &lanes,
        crate::container::active_parasolid_modeler_generation(scan),
    );
    crate::history::bind_unique_sketch_feature(&mut ir.model.features, &sketches, &histories);
    crate::resolved_features::project_dissected_sketches(
        &mut ir.model.features,
        &sketches,
        &histories,
    );
    crate::resolved_features::bind_profile_revolution_axes(
        &mut ir.model.features,
        &histories,
        &lanes,
        &sketches,
        &brep.surfaces,
    );
    crate::resolved_features::bind_pattern_inputs(&mut ir.model.features, &histories, &lanes);
    crate::resolved_features::bind_sweep_adjacent_profiles(
        &mut ir.model.features,
        &histories,
        &lanes,
    );
    crate::resolved_features::project_dimensioned_sketch_geometry(
        &mut sketch_entities,
        &sketches,
        &brep.surfaces,
        &ir.model.features,
        &ir.model.parameters,
        &lanes,
    );
    crate::resolved_features::project_marker_dimensioned_circles(
        &mut sketch_entities,
        &mut sketches,
        &ir.model.features,
        &ir.model.parameters,
        &lanes,
    );
    crate::resolved_features::project_relation_point_geometry(
        &mut sketch_entities,
        &sketches,
        &ir.model.features,
        &lanes,
    );
    crate::resolved_features::project_relation_solved_point_geometry(
        &mut sketch_entities,
        &sketches,
        &ir.model.features,
        &ir.model.parameters,
        &lanes,
    );
    crate::resolved_features::project_relation_bindings(
        &mut sketch_constraints,
        &sketches,
        &ir.model.features,
        &sketch_entities,
        &ir.model.parameters,
        &lanes,
    );
    stamp_feature_baseline(&mut ir);
    let mut attributes = crate::metadata::attributes(scan, &mut annotations);
    attributes.extend(crate::history::custom_property_attributes(&histories));
    let mut native = crate::native::SldprtNative {
        version: crate::native::SLDPRT_NATIVE_VERSION,
        feature_histories: histories.clone(),
        feature_input_lanes: lanes,
        pmi_dimensions,
    };
    ir.model.attributes = attributes;
    ir.model.sketches = sketches;
    ir.model.sketch_entities = sketch_entities;
    ir.model.sketch_constraints = sketch_constraints;
    stamp_sketch_baseline(&mut ir, &native);

    ir.model.bodies = brep.bodies;
    ir.model.regions = brep.regions;
    ir.model.shells = brep.shells;
    ir.model.faces = brep.faces;
    ir.model.loops = brep.loops;
    ir.model.coedges = brep.coedges;
    ir.model.edges = brep.edges;
    ir.model.vertices = brep.vertices;
    ir.model.points = brep.points;
    ir.model.surfaces = brep.surfaces;
    ir.model.curves = brep.curves;
    ir.model.pcurves = brep.pcurves;
    crate::history::bind_topology_selections(
        &mut ir.model.features,
        &histories,
        &ir.model.bodies,
        &ir.model.faces,
        &ir.model.edges,
        &ir.model.curves,
    );
    crate::resolved_features::project_hole_position_sketches(
        &mut ir.model.features,
        &ir.model.sketches,
        &ir.model.sketch_entities,
        &histories,
        &native.feature_input_lanes,
    );
    crate::resolved_features::project_spatial_hole_position_sketches(
        &mut ir.model.features,
        &ir.model.spatial_sketches,
        &ir.model.spatial_sketch_entities,
        &ir.model.surfaces,
        &histories,
        &native.feature_input_lanes,
    );
    crate::resolved_features::project_hole_axes(
        &mut ir.model.features,
        &ir.model.surfaces,
        &histories,
        &native.feature_input_lanes,
    );
    crate::history::order_features_for_regeneration(&mut ir.model.features);
    stamp_feature_baseline(&mut ir);
    assign_configuration_bodies(&mut ir, configuration_bodies);
    crate::history::project_configuration_sketch_states(
        &mut ir,
        &histories,
        &native.feature_input_lanes,
        &annotations,
    );
    mark_active_configuration(&mut ir);
    assign_native_configuration_indices(&ir, &mut native);
    if let Some(source) = &mut ir.source {
        source.attributes.insert(
            "sldprt_native_configuration_sha256".into(),
            crate::history::native_configuration_hash(&native.feature_histories),
        );
        source.attributes.insert(
            "sldprt_native_history_sha256".into(),
            crate::history::history_hash(&native.feature_histories),
        );
    }
    native.store(ir.native.namespace_mut("sldprt"))?;
    stamp_configuration_baseline(&mut ir);
    let mut unknowns = brep.unknowns;
    for face_color in brep.face_colors {
        let id = AppearanceId(format!(
            "sldprt:appearance:entity53#{}",
            face_color.color_attr
        ));
        crate::annotations::note(
            &mut annotations,
            id.0.clone(),
            header.description.clone(),
            face_color.offset as u64,
            "00_53_color",
            Exactness::ByteExact,
        );
        if !ir
            .model
            .appearances
            .iter()
            .any(|appearance| appearance.id == id)
        {
            ir.model.appearances.push(Appearance {
                id: id.clone(),
                name: None,
                asset_guid: None,
                visual_guid: None,
                physical_token: None,
                schema: Some("entity-53".into()),
                category: None,
                base_color: Some(face_color.color),
                properties: BTreeMap::new(),
            });
        }
        if let Some(target) = face_color.target {
            let site = target
                .split_once('@')
                .map(|(_, site)| format!("@{site}"))
                .unwrap_or_default();
            let binding_id = format!(
                "sldprt:appearance:binding#face:{}:{}{}",
                face_color.face_attr, face_color.color_attr, site
            );
            if !ir
                .model
                .appearance_bindings
                .iter()
                .any(|binding| binding.id == binding_id)
            {
                ir.model.appearance_bindings.push(AppearanceBinding {
                    id: binding_id,
                    target: AppearanceTarget::Face(cadmpeg_ir::ids::FaceId(target)),
                    appearance: id,
                    source_entity_id: Some(face_color.face_attr.to_string()),
                    object_type: Some("Face".into()),
                    channels: BTreeMap::new(),
                });
            }
        }
    }
    for (index, material) in materials.into_iter().enumerate() {
        let id = AppearanceId(format!("sldprt:appearance:material#{index}"));
        let material_stream = material.source_name;
        crate::annotations::note(
            &mut annotations,
            id.0.clone(),
            material_stream.clone(),
            material.record_offset as u64,
            "moVisualProperties_c",
            Exactness::ByteExact,
        );
        ir.model.appearances.push(Appearance {
            id: id.clone(),
            name: Some(material.name),
            asset_guid: None,
            visual_guid: None,
            physical_token: None,
            schema: Some("moVisualProperties_c".to_string()),
            category: None,
            base_color: Some(material.color),
            properties: BTreeMap::new(),
        });
        if unique_material {
            for (body_index, body) in ir.model.bodies.iter().enumerate() {
                ir.model.appearance_bindings.push(AppearanceBinding {
                    id: format!("sldprt:appearance:binding#body:{body_index}:{index}"),
                    target: AppearanceTarget::Body(body.id.clone()),
                    appearance: id.clone(),
                    source_entity_id: None,
                    object_type: Some("Body".to_string()),
                    channels: BTreeMap::new(),
                });
            }
        }
    }
    for display in scan
        .sections()
        .filter(|section| crate::tessellation::section_summary(*section).is_some())
    {
        for (index, mesh) in crate::tessellation::section_meshes(display)
            .into_iter()
            .enumerate()
        {
            let id = format!("sldprt:displaylist:record#{}:{index}", display.ordinal());
            let display_stream = display.display_name();
            crate::annotations::note(
                &mut annotations,
                id.clone(),
                display_stream,
                0,
                "displaylist_tessellation",
                Exactness::ByteExact,
            );
            ir.model
                .tessellations
                .push(cadmpeg_ir::tessellation::Tessellation {
                    id,
                    body: None,
                    faces: Vec::new(),
                    chordal_deflection: None,
                    source_object: None,
                    vertices: mesh.vertices,
                    triangles: mesh.triangles,
                    strip_lengths: mesh.strip_lengths,
                    normals: mesh.normals,
                    channels: mesh.channels,
                });
        }
        let display_id = format!("sldprt:displaylist:record#{}", display.ordinal());
        crate::annotations::note(
            &mut annotations,
            display_id.clone(),
            display.display_name(),
            0,
            "displaylist_tessellation",
            Exactness::Unknown,
        );
        unknowns.push(UnknownRecord {
            id: UnknownId(display_id),
            offset: 0,
            byte_len: display.payload().len() as u64,
            sha256: sha256_hex(display.payload()),
            data: Some(display.payload().to_vec()),
            links: Vec::new(),
        });
    }
    for source_block in &scan.blocks {
        if unknowns
            .iter()
            .any(|record| record.id.0 == format!("sldprt:file:block#{}", source_block.offset))
        {
            continue;
        }
        let id = format!("sldprt:file:block#{}", source_block.offset);
        crate::annotations::note(
            &mut annotations,
            id.clone(),
            source_block
                .section
                .clone()
                .unwrap_or_else(|| format!("block@{}", source_block.offset)),
            source_block.offset as u64,
            source_block.family,
            Exactness::ByteExact,
        );
        unknowns.push(UnknownRecord {
            id: UnknownId(id),
            offset: 0,
            byte_len: source_block.payload.len() as u64,
            sha256: sha256_hex(&source_block.payload),
            data: Some(source_block.payload.clone()),
            links: Vec::new(),
        });
    }
    for source_stream in &scan.compound_streams {
        let id = format!("sldprt:file:compound-stream#{}", source_stream.directory_id);
        crate::annotations::note(
            &mut annotations,
            id.clone(),
            source_stream.path.clone(),
            0,
            container::payload_family(&source_stream.payload),
            Exactness::ByteExact,
        );
        unknowns.push(UnknownRecord {
            id: UnknownId(id),
            offset: 0,
            byte_len: source_stream.payload.len() as u64,
            sha256: sha256_hex(&source_stream.payload),
            data: Some(source_stream.payload.clone()),
            links: Vec::new(),
        });
    }
    let partition_id = origin.unknown_id();
    let opaque_surfaces = ir
        .model
        .surfaces
        .iter_mut()
        .filter_map(|surface| match &mut surface.geometry {
            SurfaceGeometry::Unknown { record } => {
                *record = Some(partition_id.clone());
                Some(surface.id.0.clone())
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    let opaque_curves = ir
        .model
        .curves
        .iter_mut()
        .filter_map(|curve| match &mut curve.geometry {
            cadmpeg_ir::geometry::CurveGeometry::Unknown { record } => {
                *record = Some(partition_id.clone());
                Some(curve.id.0.clone())
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    if !opaque_surfaces.is_empty() || !opaque_curves.is_empty() {
        let partition = unknowns
            .iter_mut()
            .find(|record| record.id == partition_id)
            .expect("active partition block is retained");
        partition.links.extend(opaque_surfaces);
        partition.links.extend(opaque_curves);
    }
    preserve_source_image(scan, &mut annotations, &mut unknowns);
    set_semantic_hash(&mut ir);
    Ok((ir, annotations, unknowns))
}

fn assign_native_configuration_indices(ir: &CadIr, native: &mut crate::native::SldprtNative) {
    for configuration in &ir.model.configurations {
        let Some(native_ref) = configuration.native_ref.as_deref() else {
            continue;
        };
        if let Some(record) = native
            .feature_histories
            .iter_mut()
            .flat_map(|history| &mut history.configurations)
            .find(|record| record.id == native_ref)
        {
            record.source_index = configuration.source_index;
        }
    }
}

fn source_meta(scan: &ContainerScan, origin: BodyOrigin<'_>, header: &StreamHeader) -> SourceMeta {
    let mut attributes = BTreeMap::new();
    attributes.insert(
        "outer_version".to_string(),
        format!("0x{:08x}", scan.version),
    );
    let display = crate::tessellation::summary(scan);
    if display.vertices > 0 {
        attributes.insert(
            "displaylist_vertices".to_string(),
            display.vertices.to_string(),
        );
        attributes.insert(
            "displaylist_triangles".to_string(),
            display.triangles.to_string(),
        );
    }
    attributes.insert("block_count".to_string(), scan.blocks.len().to_string());
    attributes.insert(
        "compound_stream_count".to_string(),
        scan.compound_streams.len().to_string(),
    );
    let active_name = match origin {
        BodyOrigin::Block(fallback) => container::select_active_parasolid(scan).map_or_else(
            || {
                fallback
                    .section
                    .clone()
                    .unwrap_or_else(|| format!("block@{}", fallback.offset))
            },
            |(block, _)| {
                block
                    .section
                    .clone()
                    .unwrap_or_else(|| format!("block@{}", block.offset))
            },
        ),
        BodyOrigin::Compound(_) => origin.name(),
    };
    attributes.insert("active_parasolid_block".to_string(), active_name);
    attributes.insert("parasolid_schema".to_string(), header.schema.clone());
    attributes.insert(
        "parasolid_description".to_string(),
        header.description.clone(),
    );
    add_preview_metadata(scan, &mut attributes);
    add_solidworks_xml_metadata(scan, &mut attributes);
    SourceMeta {
        format: "sldprt".to_string(),
        attributes,
    }
}

fn add_preview_metadata(scan: &ContainerScan, attributes: &mut BTreeMap<String, String>) {
    let mut png_index = 0;
    let mut bmp_index = 0;
    for section in scan.sections() {
        let payload = section.payload();
        match container::payload_family(payload) {
            "png-preview" => {
                if payload.get(8..16) != Some(&[0, 0, 0, 13, b'I', b'H', b'D', b'R']) {
                    continue;
                }
                let Some(width) = be_u32(payload, 16) else {
                    continue;
                };
                let Some(height) = be_u32(payload, 20) else {
                    continue;
                };
                let Some(fields) = payload.get(24..29) else {
                    continue;
                };
                let prefix = format!("png_preview_{png_index}");
                attributes.insert(format!("{prefix}_width"), width.to_string());
                attributes.insert(format!("{prefix}_height"), height.to_string());
                attributes.insert(format!("{prefix}_bit_depth"), fields[0].to_string());
                attributes.insert(format!("{prefix}_color_type"), fields[1].to_string());
                attributes.insert(format!("{prefix}_compression"), fields[2].to_string());
                attributes.insert(format!("{prefix}_filter"), fields[3].to_string());
                attributes.insert(format!("{prefix}_interlace"), fields[4].to_string());
                png_index += 1;
            }
            "bmp-thumbnail" => {
                let (Some(width), Some(height), Some(image_size)) =
                    (le_i32(payload, 8), le_i32(payload, 12), le_u32(payload, 24))
                else {
                    continue;
                };
                let (Some(planes), Some(bits_per_pixel), Some(compression)) = (
                    le_u16(payload, 16),
                    le_u16(payload, 18),
                    le_u32(payload, 20),
                ) else {
                    continue;
                };
                let prefix = format!("bmp_thumbnail_{bmp_index}");
                attributes.insert(format!("{prefix}_width"), width.to_string());
                attributes.insert(format!("{prefix}_height"), height.to_string());
                attributes.insert(format!("{prefix}_planes"), planes.to_string());
                attributes.insert(format!("{prefix}_bit_count"), bits_per_pixel.to_string());
                attributes.insert(format!("{prefix}_compression"), compression.to_string());
                attributes.insert(format!("{prefix}_image_size"), image_size.to_string());
                bmp_index += 1;
            }
            _ => {}
        }
    }
    attributes.insert("png_preview_count".into(), png_index.to_string());
    attributes.insert("bmp_thumbnail_count".into(), bmp_index.to_string());
}

fn add_solidworks_xml_metadata(scan: &ContainerScan, attributes: &mut BTreeMap<String, String>) {
    for section in scan.sections() {
        let payload = section.payload();
        if container::payload_family(payload) != "xml"
            || !payload.windows(12).any(|w| w == b"swSolidWorks")
        {
            continue;
        }
        let Ok(text) = std::str::from_utf8(payload) else {
            continue;
        };
        let Ok(document) = roxmltree::Document::parse(text) else {
            continue;
        };
        let root = document.root_element();
        if root.tag_name().name() != "swSolidWorks" {
            continue;
        }
        for (source, target) in [
            ("swVersion", "sw_version"),
            ("swCreationTime", "sw_creation_time_unix"),
            ("swPath", "sw_path"),
        ] {
            if let Some(value) = root.attribute(source) {
                attributes.insert(target.into(), value.into());
            }
        }
        if let Some(model) = root.descendants().find(|node| node.has_tag_name("swModel")) {
            if let Some(value) = model.attribute("swName") {
                attributes.insert("sw_name".into(), value.into());
            }
            if let Some(value) = model.attribute("swConfigurationName") {
                attributes.insert("sw_configuration_name".into(), value.into());
            }
        }
        break;
    }
}

fn build_geometry_report(scan: &ContainerScan, decoded: &Brep) -> DecodeReport {
    let s = &decoded.stats;
    let mut losses = Vec::new();

    if s.unknown_surface_faces > 0 {
        losses.push(
            SldprtLossCode::GeometryFaceSupportSurfaceUntyped.note(format!(
                "{} face(s) rest on a support surface this codec does not type (offset, swept, \
                 blended, intersection, or spline-on-surface); \
                 the face, its loops, and trims are emitted with an unknown-geometry surface \
                 linking to the preserved record bytes. Topology is transferred; the underlying \
                 surface shape is not.",
                s.unknown_surface_faces
            )),
        );
    }
    if s.unknown_curve_edges > 0 {
        losses.push(
            SldprtLossCode::GeometryEdgeSupportCurveUntyped.note(format!(
                "{} edge(s) reference an untyped support curve; topology references an opaque \
                 curve carrier linked to the retained partition.",
                s.unknown_curve_edges
            )),
        );
    }
    if s.synthetic_body_grouping {
        losses.push(
            SldprtLossCode::TopologyBodyHierarchyDerived.note(
                "No body record was available; one body/region/shell hierarchy was derived."
                    .to_string(),
            ),
        );
    }
    DecodeReport {
        format: "sldprt".to_string(),
        container_only: false,
        geometry_transferred: true,
        losses,
        notes: container::summarize(scan).notes,
    }
}

fn build_metadata_ir(
    scan: &ContainerScan,
) -> Result<(CadIr, Annotations, Vec<UnknownRecord>), CodecError> {
    let mut ir = CadIr::empty(Units::default());
    let mut unknowns = Vec::new();
    let mut annotations = Annotations::default();
    let mut histories = crate::history::histories(scan, &mut annotations);
    let mut lanes = crate::resolved_features::lanes(scan, &mut annotations);
    crate::resolved_features::bind_history_classes(&mut histories, &lanes);
    crate::resolved_features::bind_scalar_operands(&histories, &mut lanes);
    let pmi_dimensions = crate::pmi::dimensions(scan, &mut annotations);
    let (sketches, sketch_entities, sketch_constraints) =
        crate::resolved_features::sketches(scan, &mut annotations);
    let mut model_attributes = crate::metadata::attributes(scan, &mut annotations);
    model_attributes.extend(crate::history::custom_property_attributes(&histories));
    ir.model.attributes = model_attributes;
    ir.model.sketches = sketches;
    ir.model.sketch_entities = sketch_entities;
    ir.model.sketch_constraints = sketch_constraints;
    let mut attributes = BTreeMap::new();
    attributes.insert(
        "outer_version".to_string(),
        format!("0x{:08x}", scan.version),
    );
    attributes.insert("block_count".to_string(), scan.blocks.len().to_string());
    add_solidworks_xml_metadata(scan, &mut attributes);

    if let Some((block, header)) = container::select_active_parasolid(scan) {
        attributes.insert(
            "active_parasolid_block".to_string(),
            block
                .section
                .clone()
                .unwrap_or_else(|| format!("block@{}", block.offset)),
        );
        attributes.insert("parasolid_schema".to_string(), header.schema.clone());
        let id = format!("sldprt:file:block#{}", block.offset);
        crate::annotations::note(
            &mut annotations,
            id.clone(),
            block
                .section
                .clone()
                .unwrap_or_else(|| format!("block@{}", block.offset)),
            0,
            "parasolid_stream",
            Exactness::Unknown,
        );
        unknowns.push(UnknownRecord {
            id: UnknownId(id),
            offset: block.offset as u64,
            byte_len: block.uncomp_sz as u64,
            sha256: sha256_hex(&block.payload),
            data: Some(block.payload.clone()),
            links: Vec::new(),
        });
    }

    ir.source = Some(SourceMeta {
        format: "sldprt".to_string(),
        attributes,
    });
    project_design_history(&mut ir, &histories, &lanes, &pmi_dimensions, scan);
    let (spatial_sketches, spatial_sketch_entities) =
        crate::resolved_features::spatial_sketches(&mut ir.model.features, &histories, &lanes);
    ir.model.spatial_sketches = spatial_sketches;
    ir.model.spatial_sketch_entities = spatial_sketch_entities;
    crate::pmi::apply_to_parameters(
        &mut ir.model.parameters,
        &ir.model.features,
        &pmi_dimensions,
    );
    crate::resolved_features::bind_parameter_scalars(
        &mut ir.model.parameters,
        &ir.model.features,
        &histories,
        parameter_identity_lanes(&lanes),
    );
    crate::resolved_features::type_display_relation_parameters(
        &mut ir.model.parameters,
        &ir.model.features,
        &lanes,
    );
    crate::history::align_configuration_parameter_kinds(&mut ir);
    stamp_parameter_baseline(&mut ir);
    crate::resolved_features::bind_sketch_profiles(
        &mut ir.model.features,
        &mut ir.model.sketches,
        &ir.model.sketch_entities,
        &ir.model.parameters,
        &histories,
        &lanes,
        &annotations,
    );
    crate::resolved_features::project_compact_sketch_profiles(
        &mut ir.model.features,
        &mut ir.model.sketches,
        &mut ir.model.sketch_entities,
        &histories,
        &lanes,
    );
    crate::resolved_features::project_marker_backed_sketches(
        &mut ir.model.features,
        &mut ir.model.sketches,
        &mut ir.model.sketch_entities,
        &histories,
        &lanes,
        crate::container::active_parasolid_modeler_generation(scan),
    );
    crate::history::bind_unique_sketch_feature(
        &mut ir.model.features,
        &ir.model.sketches,
        &histories,
    );
    crate::resolved_features::project_dissected_sketches(
        &mut ir.model.features,
        &ir.model.sketches,
        &histories,
    );
    crate::resolved_features::bind_profile_revolution_axes(
        &mut ir.model.features,
        &histories,
        &lanes,
        &ir.model.sketches,
        &ir.model.surfaces,
    );
    crate::resolved_features::bind_pattern_inputs(&mut ir.model.features, &histories, &lanes);
    crate::resolved_features::bind_sweep_adjacent_profiles(
        &mut ir.model.features,
        &histories,
        &lanes,
    );
    crate::resolved_features::project_dimensioned_sketch_geometry(
        &mut ir.model.sketch_entities,
        &ir.model.sketches,
        &ir.model.surfaces,
        &ir.model.features,
        &ir.model.parameters,
        &lanes,
    );
    crate::resolved_features::project_relation_point_geometry(
        &mut ir.model.sketch_entities,
        &ir.model.sketches,
        &ir.model.features,
        &lanes,
    );
    crate::resolved_features::project_relation_solved_point_geometry(
        &mut ir.model.sketch_entities,
        &ir.model.sketches,
        &ir.model.features,
        &ir.model.parameters,
        &lanes,
    );
    crate::resolved_features::project_relation_bindings(
        &mut ir.model.sketch_constraints,
        &ir.model.sketches,
        &ir.model.features,
        &ir.model.sketch_entities,
        &ir.model.parameters,
        &lanes,
    );
    crate::resolved_features::project_hole_position_sketches(
        &mut ir.model.features,
        &ir.model.sketches,
        &ir.model.sketch_entities,
        &histories,
        &lanes,
    );
    crate::resolved_features::project_spatial_hole_position_sketches(
        &mut ir.model.features,
        &ir.model.spatial_sketches,
        &ir.model.spatial_sketch_entities,
        &ir.model.surfaces,
        &histories,
        &lanes,
    );
    crate::resolved_features::project_hole_axes(
        &mut ir.model.features,
        &ir.model.surfaces,
        &histories,
        &lanes,
    );
    crate::history::order_features_for_regeneration(&mut ir.model.features);
    crate::history::project_configuration_sketch_states(&mut ir, &histories, &lanes, &annotations);
    stamp_feature_baseline(&mut ir);
    stamp_configuration_baseline(&mut ir);
    let native = crate::native::SldprtNative {
        version: crate::native::SLDPRT_NATIVE_VERSION,
        feature_histories: histories.clone(),
        feature_input_lanes: lanes,
        pmi_dimensions,
    };
    native.store(ir.native.namespace_mut("sldprt"))?;
    stamp_sketch_baseline(&mut ir, &native);
    mark_active_configuration(&mut ir);
    preserve_source_image(scan, &mut annotations, &mut unknowns);
    set_semantic_hash(&mut ir);
    Ok((ir, annotations, unknowns))
}

fn project_design_history(
    ir: &mut CadIr,
    histories: &[crate::records::FeatureHistory],
    lanes: &[crate::records::FeatureInputLane],
    pmi_dimensions: &[crate::records::PmiDimension],
    scan: &ContainerScan,
) {
    let mut semantic_projection = histories.to_vec();
    crate::history::enrich_scene_classes(
        &mut semantic_projection,
        &crate::tessellation::scene_feature_classes(scan),
    );
    crate::history::enrich_history_semantic(
        &mut semantic_projection,
        lanes,
        pmi_dimensions,
        crate::history::HistoryEnrichment::Read,
    );
    ir.model.features = crate::history::project_features(&semantic_projection);
    crate::resolved_features::bind_pattern_inputs(
        &mut ir.model.features,
        &semantic_projection,
        lanes,
    );
    crate::history::project_compact_and_generated(
        &mut ir.model.features,
        &semantic_projection,
        lanes,
    );
    ir.model.configurations = crate::history::project_configurations(&semantic_projection);
    let mut parameter_projection = histories.to_vec();
    crate::history::enrich_history_parameters_values_only(&mut parameter_projection, lanes);
    crate::pmi::enrich_history_parameters(&mut parameter_projection, pmi_dimensions);
    ir.model.parameters = crate::history::project_parameters(&parameter_projection);
    crate::history::project_configuration_design_states(ir, histories, lanes, pmi_dimensions);
    if let Some(source) = &mut ir.source {
        source.attributes.insert(
            "sldprt_neutral_feature_sha256".into(),
            crate::history::feature_hash(&ir.model.features),
        );
        source.attributes.insert(
            "sldprt_native_history_sha256".into(),
            crate::history::history_hash(histories),
        );
        source.attributes.insert(
            "sldprt_native_configuration_sha256".into(),
            crate::history::native_configuration_hash(histories),
        );
        source.attributes.insert(
            "sldprt_neutral_parameter_sha256".into(),
            crate::history::parameter_hash(&ir.model.parameters),
        );
        source.attributes.insert(
            "sldprt_native_parameter_sha256".into(),
            crate::history::native_parameter_hash(histories),
        );
    }
}

fn parameter_identity_lanes(
    lanes: &[crate::records::FeatureInputLane],
) -> Vec<&crate::records::FeatureInputLane> {
    let has_global = lanes.iter().any(|lane| lane.configuration.is_none());
    let scoped_configurations = lanes
        .iter()
        .filter_map(|lane| lane.configuration.as_deref())
        .collect::<BTreeSet<_>>();
    lanes
        .iter()
        .filter(|lane| {
            if has_global {
                lane.configuration.is_none()
            } else {
                scoped_configurations.len() == 1 && lanes.len() == 1
            }
        })
        .collect()
}

fn stamp_parameter_baseline(ir: &mut CadIr) {
    let hash = crate::history::parameter_hash(&ir.model.parameters);
    if let Some(source) = &mut ir.source {
        source
            .attributes
            .insert("sldprt_neutral_parameter_sha256".into(), hash);
    }
}

fn mark_active_configuration(ir: &mut CadIr) {
    let active_name = ir
        .source
        .as_ref()
        .and_then(|source| source.attributes.get("sw_configuration_name"))
        .cloned();
    let active_index = ir
        .source
        .as_ref()
        .and_then(|source| source.attributes.get("active_parasolid_block"))
        .and_then(|section| crate::container::configuration_index(section));
    let by_name = active_name.as_ref().and_then(|name| {
        let matches = ir
            .model
            .configurations
            .iter()
            .enumerate()
            .filter(|(_, configuration)| &configuration.name == name)
            .map(|(position, _)| position)
            .collect::<Vec<_>>();
        (matches.len() == 1).then(|| matches[0])
    });
    let by_index = active_index.and_then(|index| {
        let index = u32::try_from(index).ok()?;
        let matches = ir
            .model
            .configurations
            .iter()
            .enumerate()
            .filter(|(_, configuration)| {
                configuration.source_index == Some(index)
                    || configuration.source_index.is_none() && configuration.ordinal == index
            })
            .map(|(position, _)| position)
            .collect::<Vec<_>>();
        (matches.len() == 1).then(|| matches[0])
    });
    let selected = if active_name.is_some() {
        by_name
    } else if active_index.is_some() {
        by_index
    } else if ir.model.configurations.len() == 1 {
        Some(0)
    } else {
        None
    };
    for (position, configuration) in ir.model.configurations.iter_mut().enumerate() {
        configuration.active = selected == Some(position);
    }
}

fn stamp_feature_baseline(ir: &mut CadIr) {
    let hash = crate::history::feature_hash(&ir.model.features);
    if let Some(source) = &mut ir.source {
        source
            .attributes
            .insert("sldprt_neutral_feature_sha256".into(), hash);
    }
}

fn assign_configuration_bodies(
    ir: &mut CadIr,
    configuration_bodies: &[(usize, Vec<cadmpeg_ir::ids::BodyId>)],
) {
    let mut partition_map = BTreeMap::<u32, Vec<cadmpeg_ir::ids::BodyId>>::new();
    for (index, bodies) in configuration_bodies {
        let Ok(index) = u32::try_from(*index) else {
            continue;
        };
        let merged = partition_map.entry(index).or_default();
        for body in bodies {
            if !merged.contains(body) {
                merged.push(body.clone());
            }
        }
    }

    let mut assigned = vec![false; ir.model.configurations.len()];
    for (configuration, is_assigned) in ir.model.configurations.iter_mut().zip(&mut assigned) {
        let Some(source_index) = configuration.source_index else {
            continue;
        };
        if let Some(bodies) = partition_map.remove(&source_index) {
            configuration.bodies = cadmpeg_ir::ConfigurationBodies::Resolved(bodies);
            *is_assigned = true;
        }
    }
    let active_name = ir
        .source
        .as_ref()
        .and_then(|source| source.attributes.get("sw_configuration_name"));
    let active_index = ir
        .source
        .as_ref()
        .and_then(|source| source.attributes.get("active_parasolid_block"))
        .and_then(|section| crate::container::configuration_index(section))
        .and_then(|index| u32::try_from(index).ok());
    if let (Some(active_name), Some(active_index)) = (active_name, active_index) {
        let matches = ir
            .model
            .configurations
            .iter()
            .enumerate()
            .filter(|(position, configuration)| {
                !assigned[*position]
                    && configuration.source_index.is_none()
                    && &configuration.name == active_name
            })
            .map(|(position, _)| position)
            .collect::<Vec<_>>();
        if matches.len() == 1 {
            if let Some(bodies) = partition_map.remove(&active_index) {
                let position = matches[0];
                let configuration = &mut ir.model.configurations[position];
                configuration.source_index = Some(active_index);
                configuration.bodies = cadmpeg_ir::ConfigurationBodies::Resolved(bodies);
                assigned[position] = true;
            }
        }
    }
    for (configuration, is_assigned) in ir.model.configurations.iter_mut().zip(&mut assigned) {
        if *is_assigned || configuration.source_index.is_some() {
            continue;
        }
        let source_index = configuration.ordinal;
        if let Some(bodies) = partition_map.remove(&source_index) {
            configuration.source_index = Some(source_index);
            configuration.bodies = cadmpeg_ir::ConfigurationBodies::Resolved(bodies);
            *is_assigned = true;
        }
    }

    for (source_index, bodies) in partition_map {
        let ordinal = ir
            .model
            .configurations
            .iter()
            .map(|configuration| configuration.ordinal)
            .max()
            .map_or(0, |ordinal| ordinal.saturating_add(1));
        ir.model
            .configurations
            .push(cadmpeg_ir::features::DesignConfiguration {
                id: cadmpeg_ir::features::ConfigurationId(format!(
                    "sldprt:model:configuration#partition:{source_index}"
                )),
                ordinal,
                active: false,
                source_index: Some(source_index),
                name: format!("Config-{source_index}"),
                material: None,
                properties: std::collections::BTreeMap::new(),
                bodies: cadmpeg_ir::ConfigurationBodies::Resolved(bodies),
                parameter_values: std::collections::BTreeMap::new(),
                feature_states: std::collections::BTreeMap::new(),
                native_ref: None,
            });
    }
    for configuration in &mut ir.model.configurations {
        if configuration.bodies.is_unresolved() {
            configuration.bodies = cadmpeg_ir::ConfigurationBodies::Resolved(Vec::new());
        }
    }
}

fn stamp_configuration_baseline(ir: &mut CadIr) {
    let hash = crate::history::configuration_hash(&ir.model.configurations);
    let parameter_value_hash =
        crate::history::configuration_parameter_value_hash(&ir.model.configurations);
    let feature_state_hash =
        crate::history::configuration_feature_state_hash(&ir.model.configurations);
    if let Some(source) = &mut ir.source {
        source
            .attributes
            .insert("sldprt_neutral_configuration_sha256".into(), hash);
        source.attributes.insert(
            "sldprt_configuration_parameter_values_sha256".into(),
            parameter_value_hash,
        );
        source.attributes.insert(
            "sldprt_configuration_feature_states_sha256".into(),
            feature_state_hash,
        );
    }
}

fn stamp_sketch_baseline(ir: &mut CadIr, native: &crate::native::SldprtNative) {
    let neutral_hash = crate::resolved_features::sketch_hash(ir);
    let constraint_hash = crate::resolved_features::constraint_hash(ir);
    let native_hash = crate::resolved_features::lane_hash(native);
    if let Some(source) = &mut ir.source {
        source
            .attributes
            .insert("sldprt_neutral_sketch_sha256".into(), neutral_hash);
        source
            .attributes
            .insert("sldprt_native_sketch_sha256".into(), native_hash);
        source.attributes.insert(
            "sldprt_neutral_sketch_constraint_sha256".into(),
            constraint_hash,
        );
    }
}

fn set_semantic_hash(ir: &mut CadIr) {
    ir.finalize();
    let brep_hash = brep_semantic_hash(ir);
    if let Some(source) = &mut ir.source {
        source
            .attributes
            .insert("brep_semantic_sha256".into(), brep_hash);
    }
    let hash = semantic_hash(ir);
    if let Some(source) = &mut ir.source {
        source.attributes.insert("semantic_sha256".into(), hash);
    }
}

pub(crate) fn brep_semantic_hash(ir: &CadIr) -> String {
    use cadmpeg_ir::appearance::AppearanceTarget;

    // Normalize with a field-by-field clone so the dropped namespaces (source
    // image, native records, annotations) are never copied.
    let mut normalized = CadIr {
        ir_version: ir.ir_version.clone(),
        source: None,
        units: ir.units.clone(),
        tolerances: ir.tolerances,
        model: ir.model.clone(),
        native: cadmpeg_ir::Native::default(),
    };
    normalized.model.bodies.iter_mut().for_each(|body| {
        body.name = None;
        body.color = None;
    });
    let face_appearances = normalized
        .model
        .appearance_bindings
        .iter()
        .filter_map(|binding| {
            matches!(binding.target, AppearanceTarget::Face(_))
                .then_some(binding.appearance.clone())
        })
        .collect::<std::collections::HashSet<_>>();
    normalized
        .model
        .appearance_bindings
        .retain(|binding| matches!(binding.target, AppearanceTarget::Face(_)));
    normalized
        .model
        .appearances
        .retain(|appearance| face_appearances.contains(&appearance.id));
    normalized.model.tessellations.clear();
    normalized.model.attributes.clear();
    normalized.model.features.clear();
    normalized.model.parameters.clear();
    normalized.model.sketches.clear();
    normalized.model.sketch_entities.clear();
    normalized.model.sketch_constraints.clear();
    sha256_hex(
        normalized
            .to_canonical_json()
            .expect("CadIr serialization")
            .as_bytes(),
    )
}

pub(crate) fn semantic_hash(ir: &CadIr) -> String {
    // Normalize with a field-by-field clone so the retained source image (the
    // largest single payload) is filtered out instead of copied and dropped.
    let mut normalized = ir.clone();
    normalized.finalize();
    normalized.source = ir.source.as_ref().map(|source| {
        let mut source = source.clone();
        source.attributes.remove("semantic_sha256");
        source
    });
    let unknowns = ir
        .native_unknowns("sldprt")
        .unwrap_or_default()
        .into_iter()
        .filter(|record| record.id.0 != "sldprt:file:source-image#0")
        .collect::<Vec<_>>();
    normalized
        .set_native_unknowns("sldprt", &unknowns)
        .expect("SLDPRT unknown records serialize");
    sha256_hex(
        normalized
            .to_canonical_json()
            .expect("CadIr serialization")
            .as_bytes(),
    )
}

fn preserve_source_image(
    scan: &ContainerScan,
    annotations: &mut Annotations,
    unknowns: &mut Vec<UnknownRecord>,
) {
    crate::annotations::note(
        annotations,
        "sldprt:file:source-image#0",
        "source",
        0,
        "source_image",
        Exactness::ByteExact,
    );
    unknowns.push(UnknownRecord {
        id: UnknownId("sldprt:file:source-image#0".into()),
        offset: 0,
        byte_len: scan.source_image.len() as u64,
        sha256: sha256_hex(&scan.source_image),
        data: Some(scan.source_image.clone()),
        links: Vec::new(),
    });
}

fn build_container_report(scan: &ContainerScan, container_only: bool) -> DecodeReport {
    let summary = container::summarize(scan);
    let parasolid_sources = scan
        .blocks
        .iter()
        .filter(|b| b.family == "parasolid")
        .count()
        + scan
            .compound_streams
            .iter()
            .filter(|stream| !stream.ps_streams.is_empty())
            .count();
    let payload_sources = scan.blocks.len() + scan.compound_streams.len();

    let mut losses = vec![
        SldprtLossCode::GeometryParasolidNotTransferred.note(format!(
            "Parasolid B-rep geometry was not transferred: no partition/deltas stream resolved \
             into a topology graph. {payload_sources} payload source(s) were enumerated, \
             {parasolid_sources} carrying Parasolid streams."
        )),
        SldprtLossCode::TopologyGraphNotTransferred.note(
            "B-rep topology graph (body/region/shell/face/loop/coedge/edge/vertex) was not built \
             for this file."
                .to_string(),
        ),
        SldprtLossCode::MaterialMetadataNotTransferred.note(
            "Body-bound appearances and tessellation were not transferred because no body graph \
             exists."
                .to_string(),
        ),
    ];

    if !container::has_parasolid_body_stream(scan) {
        losses.push(
            SldprtLossCode::ContainerNoParasolidStream.note(
                "no Parasolid partition/deltas stream was located in the container".to_string(),
            ),
        );
    }

    DecodeReport {
        format: "sldprt".to_string(),
        container_only,
        geometry_transferred: false,
        losses,
        notes: summary.notes,
    }
}

#[cfg(test)]
mod design_loss_tests {
    use super::{
        append_design_losses, assign_configuration_bodies,
        multiply_projected_sketch_relation_records, unbound_feature_input_operation_objects,
        unprojected_sketch_relation_records,
    };
    use crate::native::SldprtNative;
    use crate::records::{
        Feature as NativeFeature, FeatureHistory, FeatureInputClass, FeatureInputClassRole,
        FeatureInputLane, FeatureInputName, FeatureInputRelationBinding,
        FeatureInputRelationFamily, FeatureInputRelationInstance, SketchInputEntity,
        SketchInputKind, SketchInputLink, SketchRelationKind,
    };
    use cadmpeg_ir::features::{
        Angle, BodyRetentionMode, BodySelection, BooleanOp, ConfigurationFeatureState,
        ConfigurationId, DesignConfiguration, DesignParameter, EdgeSelection, FaceSelection,
        Feature, FeatureDefinition, FeatureId, FeatureSourceContent, FeatureTreeNodeRole, Length,
        ParameterId, ParameterPmi, ParameterValue, PathRef, PmiDimensionSubtype, RadiusSpec,
        RuledSurfaceMode, SurfaceContinuity,
    };
    use cadmpeg_ir::ids::{BodyId, EdgeId};
    use cadmpeg_ir::math::{Point3, Vector3};
    use cadmpeg_ir::report::DecodeReport;
    use cadmpeg_ir::sketches::{
        SketchEntity, SketchEntityId, SketchGeometry, SketchId, SpatialSketchEntity,
        SpatialSketchEntityId, SpatialSketchGeometry, SpatialSketchId,
    };
    use cadmpeg_ir::units::Units;
    use cadmpeg_ir::CadIr;
    use std::collections::BTreeMap;

    #[test]
    fn typed_native_operands_are_reported_as_design_losses() {
        let mut ir = CadIr::empty(Units::default());
        ir.model.features.push(Feature {
            id: FeatureId("combine".into()),
            ordinal: 0,
            name: None,
            suppressed: Some(false),
            parent: None,
            dependencies: Vec::new(),
            source_properties: BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs: Vec::new(),
            definition: FeatureDefinition::Combine {
                target: BodySelection::Native("target".into()),
                tools: BodySelection::Native("tools".into()),
                op: BooleanOp::Unresolved,
            },
            native_ref: None,
        });
        let mut report = DecodeReport {
            format: "sldprt".into(),
            container_only: false,
            geometry_transferred: true,
            losses: Vec::new(),
            notes: Vec::new(),
        };

        append_design_losses(&ir, &mut report);

        assert!(report.losses.iter().any(|loss| {
            loss.message
                == "1 typed feature(s) retain native or unresolved required operation operands."
        }));
    }

    #[test]
    fn configuration_feature_states_drive_design_completeness_accounting() {
        let mut ir = CadIr::empty(Units::default());
        let feature_id = FeatureId("configured".into());
        ir.model.features.push(Feature {
            id: feature_id.clone(),
            ordinal: 0,
            name: None,
            suppressed: Some(false),
            parent: None,
            dependencies: Vec::new(),
            source_properties: BTreeMap::from([("Scope".into(), "Body1".into())]),
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs: Vec::new(),
            definition: FeatureDefinition::TreeNode {
                role: FeatureTreeNodeRole::History,
                children: Vec::new(),
                active_child: None,
            },
            native_ref: None,
        });
        for (ordinal, definition) in [
            (
                0,
                FeatureDefinition::Native {
                    kind: "Unprojected".into(),
                    parameters: BTreeMap::new(),
                    properties: BTreeMap::new(),
                },
            ),
            (
                1,
                FeatureDefinition::Combine {
                    target: BodySelection::Native("target".into()),
                    tools: BodySelection::Native("tools".into()),
                    op: BooleanOp::Unresolved,
                },
            ),
            (
                2,
                FeatureDefinition::DeleteBody {
                    bodies: BodySelection::Native("bodies".into()),
                    mode: BodyRetentionMode::Unresolved,
                },
            ),
        ] {
            ir.model.configurations.push(DesignConfiguration {
                id: ConfigurationId(format!("configuration-{ordinal}")),
                ordinal,
                active: ordinal == 0,
                source_index: Some(ordinal),
                name: format!("Configuration {ordinal}"),
                material: None,
                properties: BTreeMap::new(),
                bodies: cadmpeg_ir::ConfigurationBodies::Resolved(Vec::new()),
                parameter_values: BTreeMap::new(),
                feature_states: BTreeMap::from([(
                    feature_id.clone(),
                    ConfigurationFeatureState {
                        suppressed: Some(false),
                        dependencies: (ordinal == 0)
                            .then(|| FeatureId("missing-dependency".into()))
                            .into_iter()
                            .collect(),
                        outputs: (ordinal == 0)
                            .then(|| BodyId("missing-output".into()))
                            .into_iter()
                            .collect(),
                        definition,
                    },
                )]),
                native_ref: None,
            });
        }
        let mut report = DecodeReport {
            format: "sldprt".into(),
            container_only: false,
            geometry_transferred: true,
            losses: Vec::new(),
            notes: Vec::new(),
        };

        append_design_losses(&ir, &mut report);

        for expected in [
            "1 feature record(s) contain missing, repeated, or non-preceding parent/dependency edges; 0 feature record(s) share regeneration ordinals.",
            "2 feature(s) retain non-empty native output scopes that do not resolve to model bodies.",
            "1 feature record(s) contain missing or repeated output body references.",
            "1 feature(s) retain their native kind without a complete neutral operation definition.",
            "2 typed feature(s) retain native or unresolved required operation operands.",
            "1 body delete/keep feature(s) retain selected native body identities without a decoded retention mode.",
        ] {
            assert!(report.losses.iter().any(|loss| loss.message == expected));
        }
    }

    #[test]
    fn incomplete_configuration_snapshots_are_reported_as_design_losses() {
        let mut ir = CadIr::empty(Units::default());
        let feature_id = FeatureId("feature".into());
        ir.model.features.push(Feature {
            id: feature_id.clone(),
            ordinal: 0,
            name: None,
            suppressed: Some(false),
            parent: None,
            dependencies: Vec::new(),
            source_properties: BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs: Vec::new(),
            definition: FeatureDefinition::TreeNode {
                role: FeatureTreeNodeRole::History,
                children: Vec::new(),
                active_child: None,
            },
            native_ref: None,
        });
        ir.model.parameters.push(DesignParameter {
            id: ParameterId("parameter".into()),
            owner: feature_id,
            ordinal: 0,
            name: "D1".into(),
            expression: "1".into(),
            display: None,
            value: Some(ParameterValue::Integer(1)),
            dependencies: Vec::new(),
            properties: BTreeMap::new(),
            pmi: None,
            native_ref: None,
        });
        ir.model.configurations.push(DesignConfiguration {
            id: ConfigurationId("configuration".into()),
            ordinal: 0,
            active: true,
            source_index: Some(0),
            name: "Configuration".into(),
            material: None,
            properties: BTreeMap::new(),
            bodies: cadmpeg_ir::ConfigurationBodies::Resolved(Vec::new()),
            parameter_values: BTreeMap::new(),
            feature_states: BTreeMap::new(),
            native_ref: None,
        });
        let mut report = DecodeReport {
            format: "sldprt".into(),
            container_only: false,
            geometry_transferred: true,
            losses: Vec::new(),
            notes: Vec::new(),
        };

        append_design_losses(&ir, &mut report);

        assert!(report.losses.iter().any(|loss| {
            loss.message
                == "1 configuration(s) lack a complete evaluated feature snapshot; 1 configuration(s) lack a complete evaluated parameter snapshot."
        }));
    }

    #[test]
    fn every_typed_family_participates_in_design_completeness_accounting() {
        let mut ir = CadIr::empty(Units::default());
        let feature = |id: &str, ordinal, definition| Feature {
            id: FeatureId(id.into()),
            ordinal,
            name: None,
            suppressed: Some(false),
            parent: None,
            dependencies: Vec::new(),
            source_properties: BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs: Vec::new(),
            definition,
            native_ref: None,
        };
        ir.model.features.push(feature(
            "complete-helix",
            0,
            FeatureDefinition::Helix {
                axis_origin: Point3::new(0.0, 0.0, 0.0),
                axis_direction: Vector3::new(0.0, 0.0, 1.0),
                radius: Length(1.0),
                pitch: Length(2.0),
                revolutions: 3.0,
                start_angle: Angle(0.0),
                clockwise: false,
                radial_growth: None,
                cone_angle: None,
                segment_turns: None,
                construction_style: None,
            },
        ));
        ir.model.features.push(feature(
            "incomplete-dome",
            1,
            FeatureDefinition::Dome {
                faces: FaceSelection::Native("face".into()),
                height: None,
                elliptical: None,
                reverse: None,
            },
        ));
        let mut report = DecodeReport {
            format: "sldprt".into(),
            container_only: false,
            geometry_transferred: true,
            losses: Vec::new(),
            notes: Vec::new(),
        };

        append_design_losses(&ir, &mut report);

        assert!(report.losses.iter().any(|loss| {
            loss.message
                == "1 typed feature(s) retain native or unresolved required operation operands."
        }));
    }

    #[test]
    fn empty_required_operands_are_incomplete_design_semantics() {
        let mut ir = CadIr::empty(Units::default());
        let feature = |ordinal, definition| Feature {
            id: FeatureId(format!("feature-{ordinal}")),
            ordinal,
            name: None,
            suppressed: Some(false),
            parent: None,
            dependencies: Vec::new(),
            source_properties: BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs: Vec::new(),
            definition,
            native_ref: None,
        };
        ir.model.features.extend([
            feature(
                0,
                FeatureDefinition::Fillet {
                    edges: EdgeSelection::Edges(Vec::new()),
                    radius: RadiusSpec::Constant {
                        radius: Length(1.0),
                    },
                },
            ),
            feature(
                1,
                FeatureDefinition::DeleteFace {
                    faces: FaceSelection::Faces(Vec::new()),
                    heal: false,
                },
            ),
            feature(
                2,
                FeatureDefinition::DeleteBody {
                    bodies: BodySelection::Bodies(Vec::new()),
                    mode: BodyRetentionMode::DeleteSelected,
                },
            ),
            feature(
                3,
                FeatureDefinition::CompositeCurve {
                    segments: vec![PathRef::Edges(Vec::new())],
                    closed: false,
                },
            ),
            feature(
                4,
                FeatureDefinition::Shell {
                    removed_faces: FaceSelection::Faces(Vec::new()),
                    thickness: Some(Length(1.0)),
                    outward: Some(false),
                    mode: None,
                    join: None,
                    resolve_intersections: None,
                    allow_self_intersections: None,
                },
            ),
            feature(
                5,
                FeatureDefinition::FilledSurface {
                    boundary: EdgeSelection::Edges(vec![EdgeId("boundary".into())]),
                    support_faces: FaceSelection::Faces(Vec::new()),
                    continuity: SurfaceContinuity::Contact,
                    merge_result: false,
                },
            ),
            feature(
                6,
                FeatureDefinition::RuledSurface {
                    edges: EdgeSelection::Edges(vec![EdgeId("boundary".into())]),
                    support_faces: FaceSelection::Faces(Vec::new()),
                    mode: RuledSurfaceMode::Direction {
                        direction: Vector3::new(0.0, 0.0, 1.0),
                        distance: Length(1.0),
                    },
                },
            ),
        ]);
        let mut report = DecodeReport {
            format: "sldprt".into(),
            container_only: false,
            geometry_transferred: true,
            losses: Vec::new(),
            notes: Vec::new(),
        };

        append_design_losses(&ir, &mut report);

        assert!(report.losses.iter().any(|loss| {
            loss.message
                == "4 typed feature(s) retain native or unresolved required operation operands."
        }));
    }

    #[test]
    fn incomplete_parameter_semantics_are_reported_as_design_losses() {
        let mut ir = CadIr::empty(Units::default());
        let owner = FeatureId("owner".into());
        ir.model.features.push(Feature {
            id: owner.clone(),
            ordinal: 0,
            name: Some("Boss-Extrude1".into()),
            suppressed: Some(false),
            parent: None,
            dependencies: Vec::new(),
            source_properties: BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs: Vec::new(),
            definition: FeatureDefinition::TreeNode {
                role: FeatureTreeNodeRole::History,
                children: Vec::new(),
                active_child: None,
            },
            native_ref: None,
        });
        ir.model.parameters.push(DesignParameter {
            id: ParameterId("base-parameter".into()),
            owner: owner.clone(),
            ordinal: 0,
            name: "D0".into(),
            expression: "1mm".into(),
            display: None,
            value: Some(cadmpeg_ir::features::ParameterValue::Length(Length(1.0))),
            dependencies: Vec::new(),
            properties: BTreeMap::new(),
            pmi: None,
            native_ref: None,
        });
        ir.model.parameters.push(DesignParameter {
            id: ParameterId("parameter".into()),
            owner: owner.clone(),
            ordinal: 1,
            name: "D1".into(),
            expression: "\"D0@Boss-Extrude1\" + Missing@Sketch1".into(),
            display: None,
            value: None,
            dependencies: Vec::new(),
            properties: BTreeMap::new(),
            pmi: None,
            native_ref: None,
        });
        ir.model.parameters.push(DesignParameter {
            id: ParameterId("bare-reference".into()),
            owner: owner.clone(),
            ordinal: 2,
            name: "D2".into(),
            expression: "D99 + 1".into(),
            display: None,
            value: Some(cadmpeg_ir::features::ParameterValue::Real(1.0)),
            dependencies: Vec::new(),
            properties: BTreeMap::new(),
            pmi: None,
            native_ref: None,
        });
        ir.model.parameters.push(DesignParameter {
            id: ParameterId("malformed-reference".into()),
            owner: owner.clone(),
            ordinal: 3,
            name: "D3".into(),
            expression: "\"D0@Boss-Extrude1".into(),
            display: None,
            value: Some(cadmpeg_ir::features::ParameterValue::Real(1.0)),
            dependencies: Vec::new(),
            properties: BTreeMap::new(),
            pmi: None,
            native_ref: None,
        });
        let future = ParameterId("future".into());
        ir.model.parameters.push(DesignParameter {
            id: ParameterId("forward-reference".into()),
            owner: owner.clone(),
            ordinal: 4,
            name: "D4".into(),
            expression: "D5".into(),
            display: None,
            value: Some(cadmpeg_ir::features::ParameterValue::Real(2.0)),
            dependencies: vec![future.clone()],
            properties: BTreeMap::new(),
            pmi: None,
            native_ref: None,
        });
        ir.model.parameters.push(DesignParameter {
            id: future,
            owner: owner.clone(),
            ordinal: 5,
            name: "D5".into(),
            expression: "1".into(),
            display: None,
            value: Some(cadmpeg_ir::features::ParameterValue::Real(1.0)),
            dependencies: Vec::new(),
            properties: BTreeMap::new(),
            pmi: None,
            native_ref: None,
        });
        ir.model.parameters.push(DesignParameter {
            id: ParameterId("omitted-dependency".into()),
            owner: owner.clone(),
            ordinal: 6,
            name: "D6".into(),
            expression: "D0 + 1mm".into(),
            display: None,
            value: Some(cadmpeg_ir::features::ParameterValue::Length(Length(2.0))),
            dependencies: Vec::new(),
            properties: BTreeMap::new(),
            pmi: None,
            native_ref: None,
        });
        ir.model.parameters.push(DesignParameter {
            id: ParameterId("cached-unsupported-expression".into()),
            owner: owner.clone(),
            ordinal: 7,
            name: "D7".into(),
            expression: "unsupported(1)".into(),
            display: None,
            value: Some(cadmpeg_ir::features::ParameterValue::Real(1.0)),
            dependencies: Vec::new(),
            properties: BTreeMap::new(),
            pmi: None,
            native_ref: None,
        });
        for (id, ordinal, name) in [
            ("empty", 8, ""),
            ("shared-a", 9, "Shared"),
            ("shared-b", 10, "Shared"),
            ("ordinal", 10, "Unique"),
        ] {
            ir.model.parameters.push(DesignParameter {
                id: ParameterId(format!("identity:{id}")),
                owner: owner.clone(),
                ordinal,
                name: name.into(),
                expression: "1".into(),
                display: None,
                value: Some(cadmpeg_ir::features::ParameterValue::Real(1.0)),
                dependencies: Vec::new(),
                properties: BTreeMap::new(),
                pmi: None,
                native_ref: None,
            });
        }
        let mut report = DecodeReport {
            format: "sldprt".into(),
            container_only: false,
            geometry_transferred: true,
            losses: Vec::new(),
            notes: Vec::new(),
        };

        append_design_losses(&ir, &mut report);

        assert!(report.losses.iter().any(|loss| {
            loss.message
                == "1 parameter(s) lack an evaluated scalar; 3 parameter expression(s) contain unresolved, ambiguous, or malformed parameter references; 4 parameter expression(s) cannot regenerate a finite typed value; 1 parameter record(s) contain missing or non-preceding dependency edges; 2 parameter record(s) have dependency edges inconsistent with their expressions; 1 dependency-driven parameter(s) disagree with their evaluated expressions."
        }));
        assert!(report.losses.iter().any(|loss| {
            loss.message
                == "1 parameter record(s) have empty names; 2 parameter record(s) share owner-local names; 2 parameter record(s) share owner-local ordinals."
        }));
    }

    #[test]
    fn incoherent_feature_graph_is_reported_as_design_loss() {
        let mut ir = CadIr::empty(Units::default());
        let first = FeatureId("first".into());
        let second = FeatureId("second".into());
        let missing = FeatureId("missing".into());
        let feature = |id, ordinal, parent, dependencies| Feature {
            id,
            ordinal,
            name: None,
            suppressed: Some(false),
            parent,
            dependencies,
            source_properties: BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs: Vec::new(),
            definition: FeatureDefinition::TreeNode {
                role: FeatureTreeNodeRole::History,
                children: Vec::new(),
                active_child: None,
            },
            native_ref: None,
        };
        ir.model
            .features
            .push(feature(first.clone(), 0, None, vec![second.clone()]));
        ir.model
            .features
            .push(feature(second, 1, Some(first.clone()), vec![first]));
        ir.model.features.push(feature(
            FeatureId("third".into()),
            1,
            Some(missing),
            Vec::new(),
        ));
        ir.model.features[0].source_content = vec![
            FeatureSourceContent::Feature(FeatureId("second".into())),
            FeatureSourceContent::Feature(FeatureId("second".into())),
        ];
        ir.model.features[1].source_content =
            vec![FeatureSourceContent::Feature(FeatureId("third".into()))];
        ir.model.features[2].source_content = vec![FeatureSourceContent::Parameter(ParameterId(
            "missing-parameter".into(),
        ))];
        let mut report = DecodeReport {
            format: "sldprt".into(),
            container_only: false,
            geometry_transferred: true,
            losses: Vec::new(),
            notes: Vec::new(),
        };

        append_design_losses(&ir, &mut report);

        assert!(report.losses.iter().any(|loss| {
            loss.message
                == "2 feature record(s) contain missing, repeated, or non-preceding parent/dependency edges; 2 feature record(s) share regeneration ordinals."
        }));
        assert!(report.losses.iter().any(|loss| {
            loss.message
                == "3 feature record(s) contain missing, repeated, misowned, or structurally inconsistent source-content references."
        }));
    }

    #[test]
    fn incoherent_feature_outputs_are_reported_as_design_loss() {
        let mut ir = cadmpeg_ir::examples::unit_cube();
        ir.model.features.clear();
        ir.model.parameters.clear();
        let body = ir.model.bodies[0].id.clone();
        let feature = |id: &str, ordinal: u64, outputs: Vec<BodyId>| Feature {
            id: FeatureId(id.into()),
            ordinal,
            name: None,
            suppressed: Some(false),
            parent: None,
            dependencies: Vec::new(),
            source_properties: BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs,
            definition: FeatureDefinition::TreeNode {
                role: FeatureTreeNodeRole::History,
                children: Vec::new(),
                active_child: None,
            },
            native_ref: None,
        };
        ir.model
            .features
            .push(feature("duplicate", 0, vec![body.clone(), body]));
        ir.model
            .features
            .push(feature("missing", 1, vec![BodyId("missing-body".into())]));
        let mut report = DecodeReport {
            format: "sldprt".into(),
            container_only: false,
            geometry_transferred: true,
            losses: Vec::new(),
            notes: Vec::new(),
        };

        append_design_losses(&ir, &mut report);

        assert!(report.losses.iter().any(|loss| {
            loss.message
                == "2 feature record(s) contain missing or repeated output body references."
        }));
    }

    #[test]
    fn configuration_partitions_require_explicit_source_identity() {
        let mut ir = CadIr::empty(Units::default());
        let configuration = |id: &str, ordinal, source_index| DesignConfiguration {
            id: ConfigurationId(id.into()),
            ordinal,
            active: false,
            source_index,
            name: id.into(),
            material: None,
            properties: BTreeMap::new(),
            bodies: cadmpeg_ir::ConfigurationBodies::Resolved(Vec::new()),
            parameter_values: BTreeMap::new(),
            feature_states: BTreeMap::new(),
            native_ref: Some(format!("native:{id}")),
        };
        ir.model
            .configurations
            .push(configuration("explicit", 0, Some(5)));
        ir.model
            .configurations
            .push(configuration("inferred", 9, None));
        let first = BodyId("body:first".into());
        let second = BodyId("body:second".into());
        let third = BodyId("body:third".into());

        assign_configuration_bodies(
            &mut ir,
            &[
                (7, vec![third.clone()]),
                (5, vec![first.clone()]),
                (5, vec![second.clone()]),
            ],
        );

        assert_eq!(ir.model.configurations[0].source_index, Some(5));
        assert_eq!(ir.model.configurations[0].bodies, vec![first, second]);
        assert_eq!(ir.model.configurations[1].source_index, None);
        assert!(ir.model.configurations[1].bodies.is_empty());
        assert_eq!(ir.model.configurations[2].source_index, Some(7));
        assert_eq!(ir.model.configurations[2].bodies, vec![third]);
        assert!(ir.model.configurations[2].native_ref.is_none());
    }

    #[test]
    fn duplicate_configuration_partition_identities_are_reported() {
        let mut ir = CadIr::empty(Units::default());
        for id in ["first", "second"] {
            ir.model.configurations.push(DesignConfiguration {
                id: ConfigurationId(id.into()),
                ordinal: ir.model.configurations.len() as u32,
                active: false,
                source_index: Some(5),
                name: id.into(),
                material: None,
                properties: BTreeMap::new(),
                bodies: cadmpeg_ir::ConfigurationBodies::Resolved(Vec::new()),
                parameter_values: BTreeMap::new(),
                feature_states: BTreeMap::new(),
                native_ref: Some(format!("native:{id}")),
            });
        }
        let mut report = DecodeReport {
            format: "sldprt".into(),
            container_only: false,
            geometry_transferred: true,
            losses: Vec::new(),
            notes: Vec::new(),
        };

        append_design_losses(&ir, &mut report);

        assert!(report.losses.iter().any(|loss| {
            loss.message
                == "2 configuration record(s) share non-unique geometry partition identities."
        }));
    }

    #[test]
    fn incomplete_configuration_names_are_reported() {
        let mut ir = CadIr::empty(Units::default());
        for (position, (ordinal, name)) in [(0, ""), (1, "Shared"), (2, "Shared"), (2, "Unique")]
            .into_iter()
            .enumerate()
        {
            ir.model.configurations.push(DesignConfiguration {
                id: ConfigurationId(format!("configuration:{position}")),
                ordinal,
                active: position == 1,
                source_index: Some(position as u32),
                name: name.into(),
                material: None,
                properties: BTreeMap::new(),
                bodies: cadmpeg_ir::ConfigurationBodies::Resolved(Vec::new()),
                parameter_values: BTreeMap::new(),
                feature_states: BTreeMap::new(),
                native_ref: Some(format!("native:{position}")),
            });
        }
        let mut report = DecodeReport {
            format: "sldprt".into(),
            container_only: false,
            geometry_transferred: true,
            losses: Vec::new(),
            notes: Vec::new(),
        };

        append_design_losses(&ir, &mut report);

        assert!(report.losses.iter().any(|loss| {
            loss.message
                == "1 configuration record(s) have empty names; 2 configuration record(s) share non-unique names; 2 configuration record(s) share regeneration ordinals."
        }));
    }

    #[test]
    fn active_configuration_partition_disagreement_is_reported() {
        let mut ir = CadIr::empty(Units::default());
        ir.source = Some(cadmpeg_ir::document::SourceMeta {
            format: "sldprt".into(),
            attributes: BTreeMap::from([(
                "active_parasolid_block".into(),
                "Contents/Config-3-Partition".into(),
            )]),
        });
        ir.model.configurations.push(DesignConfiguration {
            id: ConfigurationId("configuration".into()),
            ordinal: 0,
            active: true,
            source_index: Some(5),
            name: "Default".into(),
            material: None,
            properties: BTreeMap::new(),
            bodies: cadmpeg_ir::ConfigurationBodies::Resolved(Vec::new()),
            parameter_values: BTreeMap::new(),
            feature_states: BTreeMap::new(),
            native_ref: Some("native:configuration".into()),
        });
        let mut report = DecodeReport {
            format: "sldprt".into(),
            container_only: false,
            geometry_transferred: true,
            losses: Vec::new(),
            notes: Vec::new(),
        };

        append_design_losses(&ir, &mut report);

        assert!(report.losses.iter().any(|loss| {
            loss.message
                == "active configuration identity does not resolve to active geometry partition 3."
        }));
    }

    #[test]
    fn incoherent_configuration_bodies_are_reported() {
        let mut ir = cadmpeg_ir::examples::unit_cube();
        let body = ir.model.bodies[0].id.clone();
        let configuration = |id: &str, ordinal, bodies| DesignConfiguration {
            id: ConfigurationId(id.into()),
            ordinal,
            active: ordinal == 0,
            source_index: Some(ordinal),
            name: id.into(),
            material: None,
            properties: BTreeMap::new(),
            bodies,
            parameter_values: BTreeMap::new(),
            feature_states: BTreeMap::new(),
            native_ref: Some(format!("native:{id}")),
        };
        ir.model.configurations = vec![
            configuration(
                "duplicate",
                0,
                cadmpeg_ir::ConfigurationBodies::Resolved(vec![body.clone(), body]),
            ),
            configuration(
                "missing",
                1,
                cadmpeg_ir::ConfigurationBodies::Resolved(vec![BodyId("missing-body".into())]),
            ),
        ];
        let mut report = DecodeReport {
            format: "sldprt".into(),
            container_only: false,
            geometry_transferred: true,
            losses: Vec::new(),
            notes: Vec::new(),
        };

        append_design_losses(&ir, &mut report);

        assert!(report.losses.iter().any(|loss| {
            loss.message == "2 configuration record(s) contain missing or repeated body references."
        }));
    }

    #[test]
    fn native_planar_and_spatial_sketch_geometry_is_reported() {
        let mut ir = CadIr::empty(Units::default());
        ir.model.sketch_entities.push(SketchEntity {
            id: SketchEntityId("planar-entity".into()),
            sketch: SketchId("planar-sketch".into()),
            construction: false,
            native_ref: Some("native:planar".into()),
            geometry_ref: None,
            endpoint_refs: Vec::new(),
            geometry: SketchGeometry::Native {
                native_kind: "SplineHandle".into(),
            },
        });
        ir.model.spatial_sketch_entities.push(SpatialSketchEntity {
            id: SpatialSketchEntityId("spatial-entity".into()),
            sketch: SpatialSketchId("spatial-sketch".into()),
            construction: false,
            native_ref: Some("native:spatial".into()),
            geometry_ref: None,
            endpoint_refs: Vec::new(),
            geometry: SpatialSketchGeometry::Native {
                native_kind: "ReferenceCurve".into(),
            },
        });
        let mut report = DecodeReport {
            format: "sldprt".into(),
            container_only: false,
            geometry_transferred: true,
            losses: Vec::new(),
            notes: Vec::new(),
        };

        append_design_losses(&ir, &mut report);

        assert!(report.losses.iter().any(|loss| {
            loss.message
                == "2 sketch entity geometry record(s) retain native kinds without solved neutral geometry."
        }));
    }

    #[test]
    fn retained_relation_records_without_constraints_are_counted() {
        let mut ir = CadIr::empty(Units::default());
        ir.model.sketch_entities.push(SketchEntity {
            id: SketchEntityId("represented-geometry".into()),
            sketch: SketchId("sketch".into()),
            construction: false,
            native_ref: Some("geometry-marker".into()),
            geometry_ref: None,
            endpoint_refs: Vec::new(),
            geometry: SketchGeometry::Native {
                native_kind: "UnknownGeometry".into(),
            },
        });
        let marker = |id: &str, ordinal, kind| SketchInputEntity {
            id: id.into(),
            parent: "lane".into(),
            feature_ref: Some("feature".into()),
            ordinal,
            offset: u64::from(ordinal),
            object_index: None,
            local_id: None,
            kind,
            state_value: None,
            coordinates_m: None,
            links: Vec::new(),
            link_selector: None,
        };
        let relation = FeatureInputRelationInstance {
            id: "relation-instance".into(),
            parent: "lane".into(),
            ordinal: 0,
            offset: 0,
            family: FeatureInputRelationFamily::PointPointDistance,
            class_ref: "class".into(),
            feature_ref: "feature".into(),
            scalar_refs: vec!["scalar".into()],
            parameter_scalar_ref: Some("scalar".into()),
            display_scalar_ref: None,
            operands: Vec::new(),
        };
        let binding =
            |id: &str, class_ref: &str, scalar_ref: &str, ordinal| FeatureInputRelationBinding {
                id: id.into(),
                parent: "lane".into(),
                ordinal,
                offset: u64::from(ordinal),
                class_ref: class_ref.into(),
                family: FeatureInputRelationFamily::PointPointDistance,
                scalar_ref: scalar_ref.into(),
                feature_ref: Some("feature".into()),
            };
        let mut relation_marker = marker(
            "relation-marker",
            0,
            SketchInputKind::Relation(SketchRelationKind::Horizontal),
        );
        relation_marker.links.push(SketchInputLink {
            local_id: 1,
            entity_ref: "geometry-marker".into(),
        });
        let native = SldprtNative {
            feature_input_lanes: vec![FeatureInputLane {
                id: "lane".into(),
                configuration: None,
                native_payload: Vec::new(),
                classes: Vec::new(),
                names: Vec::new(),
                scalars: Vec::new(),
                relation_bindings: vec![
                    binding("grouped-binding", "class", "scalar", 0),
                    binding("orphan-binding", "other-class", "other-scalar", 1),
                ],
                relation_instances: vec![relation],
                body_selections: Vec::new(),
                edge_selections: Vec::new(),
                surface_selections: Vec::new(),
                generated_surface_identities: Vec::new(),
                references: Vec::new(),
                sketch_entities: vec![
                    relation_marker,
                    marker(
                        "dimension-handle",
                        1,
                        SketchInputKind::Relation(SketchRelationKind::Distance),
                    ),
                    marker("geometry-marker", 2, SketchInputKind::Native(99)),
                    marker(
                        "operandless-relation-marker",
                        3,
                        SketchInputKind::Relation(SketchRelationKind::Vertical),
                    ),
                ],
            }],
            ..SldprtNative::default()
        };

        assert_eq!(unprojected_sketch_relation_records(&ir, &native), 3);
    }

    #[test]
    fn native_relation_records_have_at_most_one_neutral_owner() {
        let mut ir = CadIr::empty(Units::default());
        let entity = |id: &str, native_ref: &str| SketchEntity {
            id: SketchEntityId(id.into()),
            sketch: SketchId("sketch".into()),
            construction: false,
            native_ref: Some(native_ref.into()),
            geometry_ref: None,
            endpoint_refs: Vec::new(),
            geometry: SketchGeometry::Native {
                native_kind: "UnknownGeometry".into(),
            },
        };
        ir.model.sketch_entities = vec![
            entity("first", "relation-marker"),
            entity("second", "relation-marker"),
            entity("profile", "profile-stream-record"),
        ];
        let native = SldprtNative {
            feature_input_lanes: vec![FeatureInputLane {
                id: "lane".into(),
                configuration: None,
                native_payload: Vec::new(),
                classes: Vec::new(),
                names: Vec::new(),
                scalars: Vec::new(),
                relation_bindings: Vec::new(),
                relation_instances: Vec::new(),
                body_selections: Vec::new(),
                edge_selections: Vec::new(),
                surface_selections: Vec::new(),
                generated_surface_identities: Vec::new(),
                references: Vec::new(),
                sketch_entities: vec![
                    SketchInputEntity {
                        id: "relation-marker".into(),
                        parent: "lane".into(),
                        feature_ref: Some("feature".into()),
                        ordinal: 0,
                        offset: 0,
                        object_index: None,
                        local_id: None,
                        kind: SketchInputKind::Relation(SketchRelationKind::Horizontal),
                        state_value: None,
                        coordinates_m: None,
                        links: vec![SketchInputLink {
                            local_id: 1,
                            entity_ref: "geometry-marker".into(),
                        }],
                        link_selector: None,
                    },
                    SketchInputEntity {
                        id: "geometry-marker".into(),
                        parent: "lane".into(),
                        feature_ref: Some("feature".into()),
                        ordinal: 1,
                        offset: 1,
                        object_index: None,
                        local_id: Some(1),
                        kind: SketchInputKind::Native(99),
                        state_value: None,
                        coordinates_m: None,
                        links: Vec::new(),
                        link_selector: None,
                    },
                ],
            }],
            ..SldprtNative::default()
        };

        assert_eq!(multiply_projected_sketch_relation_records(&ir, &native), 1);
    }

    #[test]
    fn direct_feature_input_operations_require_unique_history_bindings() {
        let class_name = "moExtrusion_c";
        let mut lane = FeatureInputLane {
            id: "lane".into(),
            configuration: None,
            native_payload: Vec::new(),
            classes: vec![FeatureInputClass {
                id: "class".into(),
                parent: "lane".into(),
                ordinal: 0,
                offset: 10,
                name: class_name.into(),
                role: FeatureInputClassRole::Feature,
            }],
            names: vec![FeatureInputName {
                id: "name".into(),
                parent: "lane".into(),
                ordinal: 0,
                offset: 10 + 6 + class_name.len() as u64,
                object_id: Some(42),
                value: "Boss".into(),
            }],
            scalars: Vec::new(),
            relation_bindings: Vec::new(),
            relation_instances: Vec::new(),
            body_selections: Vec::new(),
            edge_selections: Vec::new(),
            surface_selections: Vec::new(),
            generated_surface_identities: Vec::new(),
            references: Vec::new(),
            sketch_entities: Vec::new(),
        };
        let mut native = SldprtNative {
            feature_input_lanes: vec![lane.clone()],
            ..SldprtNative::default()
        };
        assert_eq!(unbound_feature_input_operation_objects(&native), 1);

        native.feature_histories.push(FeatureHistory {
            id: "history".into(),
            part_name: None,
            properties: BTreeMap::new(),
            content: Vec::new(),
            configurations: Vec::new(),
            features: vec![NativeFeature {
                id: "feature".into(),
                parent: "history".into(),
                xml_tag: "Extrusion".into(),
                tree_parent: None,
                source_id: Some("42".into()),
                parent_source_id: None,
                ordinal: 0,
                name: "Boss".into(),
                kind: "Extrusion".into(),
                input_class: Some(class_name.into()),
                suppressed: false,
                parameters: BTreeMap::new(),
                dimension_properties: BTreeMap::new(),
                properties: BTreeMap::new(),
                text: None,
                content: Vec::new(),
            }],
        });
        assert_eq!(unbound_feature_input_operation_objects(&native), 0);
        native.feature_histories[0].features[0].input_class = Some("moSweep_c".into());
        assert_eq!(unbound_feature_input_operation_objects(&native), 1);
        native.feature_histories[0].features[0].input_class = Some(class_name.into());
        let mut duplicate = native.feature_histories[0].features[0].clone();
        duplicate.id = "duplicate-feature".into();
        native.feature_histories[0].features.push(duplicate);
        assert_eq!(unbound_feature_input_operation_objects(&native), 1);

        lane.names[0].offset += 1;
        native.feature_input_lanes = vec![lane];
        assert_eq!(unbound_feature_input_operation_objects(&native), 0);
    }

    #[test]
    fn native_dimension_subtypes_are_reported() {
        let mut ir = CadIr::empty(Units::default());
        let owner = FeatureId("owner".into());
        ir.model.features.push(Feature {
            id: owner.clone(),
            ordinal: 0,
            name: Some("Feature".into()),
            suppressed: Some(false),
            parent: None,
            dependencies: Vec::new(),
            source_properties: BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs: Vec::new(),
            definition: FeatureDefinition::TreeNode {
                role: FeatureTreeNodeRole::History,
                children: Vec::new(),
                active_child: None,
            },
            native_ref: None,
        });
        ir.model.parameters.push(DesignParameter {
            id: ParameterId("parameter".into()),
            owner,
            ordinal: 0,
            name: "D1".into(),
            expression: "1".into(),
            display: None,
            value: Some(ParameterValue::Real(1.0)),
            dependencies: Vec::new(),
            properties: BTreeMap::new(),
            pmi: Some(ParameterPmi {
                subtype: PmiDimensionSubtype::Native("Ordinate".into()),
                precision: 3,
                display_text: None,
                basic: false,
                inspection: false,
                reference_only: false,
                native_ref: "native:pmi".into(),
            }),
            native_ref: None,
        });
        let mut report = DecodeReport {
            format: "sldprt".into(),
            container_only: false,
            geometry_transferred: true,
            losses: Vec::new(),
            notes: Vec::new(),
        };

        append_design_losses(&ir, &mut report);

        assert!(report.losses.iter().any(|loss| {
            loss.message
                == "0 semantic dimension record(s) are not bound to parameters; 1 parameter dimension(s) retain native subtypes."
        }));
    }
}
