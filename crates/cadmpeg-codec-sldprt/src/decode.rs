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
use cadmpeg_ir::report::{DecodeReport, LossCategory, LossNote, Severity};
use cadmpeg_ir::units::Units;
use cadmpeg_ir::unknown::UnknownRecord;
use cadmpeg_ir::Exactness;

use crate::container::configuration_index;

use crate::brep::{self, Brep};
use crate::container::{self, Block, ContainerScan};
use crate::parasolid::StreamHeader;

struct BodyStream<'a> {
    block: &'a Block,
    payload: &'a [u8],
    header: StreamHeader,
}

struct DecodedBrep {
    selected: usize,
    brep: Brep,
    configuration_bodies: Vec<(usize, Vec<cadmpeg_ir::ids::BodyId>)>,
}

/// Decode one seekable `.sldprt` stream into IR and diagnostics.
///
/// The function reads and retains the complete source image. Container framing
/// or I/O failures return [`CodecError`]; unsupported model records are reported
/// through [`DecodeResult::report`] when a partial result can be represented.
pub fn decode(
    reader: &mut dyn ReadSeek,
    options: &DecodeOptions,
) -> Result<DecodeResult, CodecError> {
    let scan = container::scan(reader)?;

    if options.container_only {
        let ir = build_metadata_ir(&scan)?;
        let report = build_container_report(&scan, true);
        return Ok(DecodeResult::new(ir, report));
    }

    let streams = active_body_streams(&scan);
    if !streams.is_empty() {
        if let Some((decoded, mut report)) = try_decode_brep(&scan, &streams) {
            let ir = build_geometry_ir(
                &scan,
                streams[decoded.selected].block,
                &streams[decoded.selected].header,
                decoded.brep,
                &decoded.configuration_bodies,
            )?;
            append_design_losses(&ir, &mut report);
            return Ok(DecodeResult::new(ir, report));
        }
    }

    let ir = build_metadata_ir(&scan)?;
    let mut report = build_container_report(&scan, false);
    append_design_losses(&ir, &mut report);
    Ok(DecodeResult::new(ir, report))
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
        report.losses.push(LossNote {
            category: LossCategory::Other,
            severity: Severity::Warning,
            message: format!(
                "active configuration identity is unresolved; {active_configurations} of {} configuration records are active.",
                ir.model.configurations.len()
            ),
            provenance: None,
        });
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
        report.losses.push(LossNote {
            category: LossCategory::Other,
            severity: Severity::Warning,
            message: format!(
                "active configuration identity does not resolve to active geometry partition {active_partition}."
            ),
            provenance: None,
        });
    }
    let inferred_configurations = ir
        .model
        .configurations
        .iter()
        .filter(|configuration| configuration.native_ref.is_none())
        .count();
    if inferred_configurations > 0 {
        report.losses.push(LossNote {
            category: LossCategory::Other,
            severity: Severity::Warning,
            message: format!(
                "{inferred_configurations} configuration state(s) are inferred from geometry partitions without native configuration definitions."
            ),
            provenance: None,
        });
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
        report.losses.push(LossNote {
            category: LossCategory::Other,
            severity: Severity::Warning,
            message: format!(
                "{unresolved_configuration_parameter_lanes} configuration-scoped feature-input lane(s) have duplicate or unresolved configuration identity."
            ),
            provenance: None,
        });
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
        report.losses.push(LossNote {
            category: LossCategory::Other,
            severity: Severity::Warning,
            message: format!(
                "{ambiguous_configuration_sources} configuration record(s) share non-unique geometry partition identities."
            ),
            provenance: None,
        });
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
        report.losses.push(LossNote {
            category: LossCategory::Other,
            severity: Severity::Warning,
            message: format!(
                "{empty_configuration_names} configuration record(s) have empty names; {ambiguous_configuration_names} configuration record(s) share non-unique names; {ambiguous_configuration_ordinals} configuration record(s) share regeneration ordinals."
            ),
            provenance: None,
        });
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
                .iter()
                .any(|body| !bodies.insert(body) || !model_body_ids.contains(body))
        })
        .count();
    if incoherent_configuration_bodies > 0 {
        report.losses.push(LossNote {
            category: LossCategory::Other,
            severity: Severity::Warning,
            message: format!(
                "{incoherent_configuration_bodies} configuration record(s) contain missing or repeated body references."
            ),
            provenance: None,
        });
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
    let unresolved_parameter_references =
        crate::history::parameters_with_unresolved_references(&ir.model.parameters, &feature_names);
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
    let incoherent_parameter_dependencies = ir
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
    if incomplete_parameters > 0
        || unresolved_parameter_references > 0
        || incoherent_parameter_dependencies > 0
    {
        report.losses.push(LossNote {
            category: LossCategory::Other,
            severity: Severity::Warning,
            message: format!(
                "{incomplete_parameters} parameter(s) lack an evaluated scalar; {unresolved_parameter_references} parameter expression(s) contain unresolved, ambiguous, or malformed parameter references; {incoherent_parameter_dependencies} parameter record(s) contain missing or non-preceding dependency edges."
            ),
            provenance: None,
        });
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
        report.losses.push(LossNote {
            category: LossCategory::Other,
            severity: Severity::Warning,
            message: format!(
                "{empty_parameter_names} parameter record(s) have empty names; {duplicate_parameter_names} parameter record(s) share owner-local names; {duplicate_parameter_ordinals} parameter record(s) share owner-local ordinals."
            ),
            provenance: None,
        });
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
        report.losses.push(LossNote {
            category: LossCategory::Other,
            severity: Severity::Warning,
            message: format!(
                "{unbound_pmi_dimensions} semantic dimension record(s) are not bound to parameters; {native_pmi_subtypes} parameter dimension(s) retain native subtypes."
            ),
            provenance: None,
        });
    }

    let incomplete_history_references = native.as_ref().map_or(0, |native| {
        crate::history::incomplete_history_reference_features(&native.feature_histories)
    });
    if incomplete_history_references > 0 {
        report.losses.push(LossNote {
            category: LossCategory::Other,
            severity: Severity::Warning,
            message: format!(
                "{incomplete_history_references} feature history record(s) contain duplicate identities or unresolved parent, dependency, dimension, or child references."
            ),
            provenance: None,
        });
    }
    let feature_positions = ir
        .model
        .features
        .iter()
        .map(|feature| (&feature.id, feature.ordinal))
        .collect::<BTreeMap<_, _>>();
    let incoherent_feature_edges = ir
        .model
        .features
        .iter()
        .filter(|feature| {
            let parent_incoherent = feature.parent.as_ref().is_some_and(|parent| {
                feature_positions
                    .get(parent)
                    .is_none_or(|ordinal| *ordinal >= feature.ordinal)
            });
            let mut dependencies = std::collections::HashSet::new();
            parent_incoherent
                || feature.dependencies.iter().any(|dependency| {
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
        report.losses.push(LossNote {
            category: LossCategory::Other,
            severity: Severity::Warning,
            message: format!(
                "{incoherent_feature_edges} feature record(s) contain missing, repeated, or non-preceding parent/dependency edges; {duplicate_feature_ordinals} feature record(s) share regeneration ordinals."
            ),
            provenance: None,
        });
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
        report.losses.push(LossNote {
            category: LossCategory::Other,
            severity: Severity::Warning,
            message: format!(
                "{incoherent_feature_content} feature record(s) contain missing, repeated, misowned, or structurally inconsistent source-content references."
            ),
            provenance: None,
        });
    }

    let unresolved_output_scopes = ir
        .model
        .features
        .iter()
        .filter(|feature| {
            feature
                .source_properties
                .get("Scope")
                .is_some_and(|scope| !scope.trim().is_empty())
                && feature.outputs.is_empty()
        })
        .count();
    if unresolved_output_scopes > 0 {
        report.losses.push(LossNote {
            category: LossCategory::Other,
            severity: Severity::Warning,
            message: format!(
                "{unresolved_output_scopes} feature(s) retain non-empty native output scopes that do not resolve to model bodies."
            ),
            provenance: None,
        });
    }
    let body_ids = ir
        .model
        .bodies
        .iter()
        .map(|body| &body.id)
        .collect::<std::collections::HashSet<_>>();
    let incoherent_feature_outputs = ir
        .model
        .features
        .iter()
        .filter(|feature| {
            let mut outputs = std::collections::HashSet::new();
            feature
                .outputs
                .iter()
                .any(|body| !outputs.insert(body) || !body_ids.contains(body))
        })
        .count();
    if incoherent_feature_outputs > 0 {
        report.losses.push(LossNote {
            category: LossCategory::Other,
            severity: Severity::Warning,
            message: format!(
                "{incoherent_feature_outputs} feature record(s) contain missing or repeated output body references."
            ),
            provenance: None,
        });
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
        report.losses.push(LossNote {
            category: LossCategory::Other,
            severity: Severity::Warning,
            message: format!(
                "{native_constraints} sketch constraint(s) retain native relation kinds and operands without complete neutral geometric semantics."
            ),
            provenance: None,
        });
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
        report.losses.push(LossNote {
            category: LossCategory::Other,
            severity: Severity::Warning,
            message: format!(
                "{native_sketch_geometry} sketch entity geometry record(s) retain native kinds without solved neutral geometry."
            ),
            provenance: None,
        });
    }

    let unprojected_relations = native
        .as_ref()
        .map_or(0, |native| unprojected_sketch_relation_records(ir, native));
    if unprojected_relations > 0 {
        report.losses.push(LossNote {
            category: LossCategory::Other,
            severity: Severity::Warning,
            message: format!(
                "{unprojected_relations} native sketch relation record(s) have no projected neutral constraint."
            ),
            provenance: None,
        });
    }

    let native_features = ir
        .model
        .features
        .iter()
        .filter(|feature| matches!(feature.definition, FeatureDefinition::Native { .. }))
        .count();
    if native_features > 0 {
        report.losses.push(LossNote {
            category: LossCategory::Other,
            severity: Severity::Warning,
            message: format!(
                "{native_features} feature(s) retain their native kind without a complete neutral operation definition."
            ),
            provenance: None,
        });
    }

    let native_edge_selection = |selection: &EdgeSelection| {
        matches!(
            selection,
            EdgeSelection::Unresolved | EdgeSelection::Native(_)
        )
    };
    let native_face_selection = |selection: &FaceSelection| {
        matches!(
            selection,
            FaceSelection::Unresolved | FaceSelection::Native(_)
        )
    };
    let native_body_selection = |selection: &BodySelection| {
        matches!(
            selection,
            BodySelection::Unresolved | BodySelection::Native(_)
        )
    };
    let native_profile =
        |profile: &ProfileRef| matches!(profile, ProfileRef::Unresolved(_) | ProfileRef::Native(_));
    let native_path = |path: &PathRef| matches!(path, PathRef::Native(_));
    let incomplete_extent = |extent: &Extent| {
        matches!(extent, Extent::Unresolved)
            || matches!(extent, Extent::ToFace { face } if native_face_selection(face))
    };
    let incomplete_typed_features = ir
        .model
        .features
        .iter()
        .filter(|feature| match &feature.definition {
            FeatureDefinition::TreeNode { .. }
            | FeatureDefinition::DatumPrincipalPlane { .. }
            | FeatureDefinition::DatumPlane { .. }
            | FeatureDefinition::DatumAxis { .. }
            | FeatureDefinition::DatumPoint { .. }
            | FeatureDefinition::DatumCoordinateSystem { .. }
            | FeatureDefinition::EquationCurve { .. }
            | FeatureDefinition::Helix { .. } => false,
            FeatureDefinition::DatumOffsetPlane { reference, .. } => reference.is_none(),
            FeatureDefinition::ProjectedCurve {
                source,
                target_faces,
                ..
            } => native_path(source) || native_face_selection(target_faces),
            FeatureDefinition::CompositeCurve { segments, .. } => {
                segments.is_empty() || segments.iter().any(native_path)
            }
            FeatureDefinition::HelixNativeAxis { .. } => true,
            FeatureDefinition::Wrap {
                profile,
                face,
                mode,
                depth,
            } => {
                native_profile(profile)
                    || native_face_selection(face)
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
                native_profile(profile)
                    || incomplete_extent(extent)
                    || *op == BooleanOp::Unresolved
            }
            FeatureDefinition::Revolve { construction, op } => {
                construction.profile.as_ref().is_none_or(native_profile)
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
                profile.as_ref().is_none_or(native_profile)
                    || path.as_ref().is_none_or(native_path)
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
                    || profiles.iter().any(native_profile)
                    || guides.iter().any(native_path)
                    || *op == BooleanOp::Unresolved
            }
            FeatureDefinition::Rib { construction, op } => {
                construction.profile.as_ref().is_none_or(native_profile)
                    || construction.direction.is_none()
                    || construction.thickness.is_none()
                    || construction.side.is_none()
                    || matches!(construction.draft, cadmpeg_ir::features::RibDraft::Unresolved)
                    || *op == BooleanOp::Unresolved
            }
            FeatureDefinition::Fillet { edges, radius } => {
                native_edge_selection(edges) || matches!(radius, RadiusSpec::Unresolved { .. })
            }
            FeatureDefinition::Chamfer { edges, spec } => {
                native_edge_selection(edges) || matches!(spec, ChamferSpec::Unresolved { .. })
            }
            FeatureDefinition::Shell {
                removed_faces,
                thickness,
                outward,
            } => {
                native_face_selection(removed_faces) || thickness.is_none() || outward.is_none()
            }
            FeatureDefinition::Thicken {
                faces,
                thickness,
                side,
            } => native_face_selection(faces) || thickness.is_none() || side.is_none(),
            FeatureDefinition::OffsetSurface { faces, .. }
            | FeatureDefinition::KnitSurface { faces, .. }
            | FeatureDefinition::ExtendSurface { faces, .. } => native_face_selection(faces),
            FeatureDefinition::FilledSurface {
                boundary,
                support_faces,
                ..
            } => native_edge_selection(boundary) || native_face_selection(support_faces),
            FeatureDefinition::TrimSurface { faces, tool, .. } => {
                native_face_selection(faces) || native_path(tool)
            }
            FeatureDefinition::RuledSurface {
                edges,
                support_faces,
                ..
            } => native_edge_selection(edges) || native_face_selection(support_faces),
            FeatureDefinition::Draft {
                faces,
                neutral_plane,
                ..
            } => native_face_selection(faces) || native_face_selection(neutral_plane),
            FeatureDefinition::Combine { target, tools, op } => {
                native_body_selection(target)
                    || native_body_selection(tools)
                    || *op == BooleanOp::Unresolved
            }
            FeatureDefinition::CutWithSurface { targets, tools, .. } => {
                native_body_selection(targets) || native_face_selection(tools)
            }
            FeatureDefinition::DeleteBody { bodies, mode } => {
                native_body_selection(bodies) || *mode == BodyRetentionMode::Unresolved
            }
            FeatureDefinition::DeleteFace { faces, .. } => native_face_selection(faces),
            FeatureDefinition::ReplaceFace {
                targets,
                replacements,
            } => native_face_selection(targets) || native_face_selection(replacements),
            FeatureDefinition::MoveFace { faces, .. } => native_face_selection(faces),
            FeatureDefinition::MoveBody { bodies, .. } => native_body_selection(bodies),
            FeatureDefinition::Dome {
                faces,
                height,
                elliptical,
                reverse,
            } => {
                native_face_selection(faces)
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
                native_body_selection(bodies)
                    || center.as_ref().is_none_or(|center| {
                        matches!(center, cadmpeg_ir::features::ScaleCenter::Native(_))
                    })
                    || factors.resolved().is_none()
            }
            FeatureDefinition::Hole {
                face,
                position,
                direction,
                kind,
                diameter,
                extent,
            } => {
                face.as_ref().is_some_and(native_face_selection)
                    || position.is_none()
                    || direction.is_none()
                    || matches!(kind, cadmpeg_ir::features::HoleKind::Unresolved { .. })
                    || diameter.is_none()
                    || extent.as_ref().is_none_or(incomplete_extent)
            }
            FeatureDefinition::Pattern { seeds, pattern } => {
                seeds.is_empty()
                    || matches!(pattern, PatternKind::Unresolved { .. })
                    || matches!(pattern, PatternKind::Linear { direction: None, .. })
                    || matches!(pattern, PatternKind::CurveDriven { path: None, .. })
            }
            FeatureDefinition::Native { .. } => false,
        })
        .count();
    if incomplete_typed_features > 0 {
        report.losses.push(LossNote {
            category: LossCategory::Other,
            severity: Severity::Warning,
            message: format!(
                "{incomplete_typed_features} typed feature(s) retain native or unresolved required operation operands."
            ),
            provenance: None,
        });
    }

    let unresolved_body_modes = ir
        .model
        .features
        .iter()
        .filter(|feature| {
            matches!(
                feature.definition,
                FeatureDefinition::DeleteBody {
                    mode: BodyRetentionMode::Unresolved,
                    ..
                }
            )
        })
        .count();
    if unresolved_body_modes > 0 {
        report.losses.push(LossNote {
            category: LossCategory::Other,
            severity: Severity::Warning,
            message: format!(
                "{unresolved_body_modes} body delete/keep feature(s) retain selected native body identities without a decoded retention mode."
            ),
            provenance: None,
        });
    }
}

fn unprojected_sketch_relation_records(ir: &CadIr, native: &crate::native::SldprtNative) -> usize {
    use crate::records::SketchInputKind;

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

    native
        .feature_input_lanes
        .iter()
        .map(|lane| {
            let instances = lane
                .relation_instances
                .iter()
                .filter(|relation| !projected.contains(&relation.id))
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
                    matches!(
                        marker.kind,
                        SketchInputKind::Relation(_) | SketchInputKind::Native(_)
                    ) && !projected.contains(&marker.id)
                })
                .count();
            instances + bindings + markers
        })
        .sum()
}

/// Decode the active Parasolid stream's B-rep. Returns `None` when the stream
/// frames but yields no geometry, so the caller falls back to metadata.
fn active_body_streams(scan: &ContainerScan) -> Vec<BodyStream<'_>> {
    let mut streams: Vec<_> = scan
        .blocks
        .iter()
        .flat_map(|block| {
            block.ps_streams.iter().filter_map(move |payload| {
                let header = crate::parasolid::stream_header(payload)?;
                let section = block.section.as_deref().unwrap_or("").to_ascii_lowercase();
                if crate::parasolid::is_body_stream(&header)
                    && !section.contains("ghost")
                    && !section.contains("resolvedfeatures")
                {
                    Some(BodyStream {
                        block,
                        payload,
                        header,
                    })
                } else {
                    None
                }
            })
        })
        .collect();
    streams.sort_by_key(|stream| {
        let section = stream
            .block
            .section
            .as_deref()
            .unwrap_or("")
            .to_ascii_lowercase();
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
        sites.entry(site_key(stream.block)).or_default().push(index);
    }
    let mut decoded_sites = Vec::new();
    for (site, indices) in &sites {
        let first = indices[0];
        let name = streams[first]
            .block
            .section
            .clone()
            .unwrap_or_else(|| format!("block@{}", streams[first].block.offset));
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
    if decoded_sites[selected_site].3.faces.is_empty()
        && decoded_sites[selected_site].3.surfaces.is_empty()
        && decoded_sites[selected_site].3.points.is_empty()
    {
        return None;
    }
    let (_, selected, _, mut decoded) = decoded_sites.swap_remove(selected_site);
    let mut configuration_bodies = Vec::new();
    if let Some(index) = streams[selected]
        .block
        .section
        .as_deref()
        .and_then(configuration_index)
    {
        configuration_bodies.push((
            index,
            decoded.bodies.iter().map(|body| body.id.clone()).collect(),
        ));
    }
    for (site, first, _, mut alternate) in decoded_sites {
        alternate.qualify_ids(&site);
        if let Some(index) = streams[first]
            .block
            .section
            .as_deref()
            .and_then(configuration_index)
        {
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
    target.stats.synthetic_body_grouping |= source.stats.synthetic_body_grouping;
}

fn site_key(block: &Block) -> String {
    let mut key = block
        .section
        .clone()
        .unwrap_or_else(|| format!("block@{}", block.offset))
        .to_ascii_lowercase();
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
    block: &Block,
    header: &StreamHeader,
    mut brep: Brep,
    configuration_bodies: &[(usize, Vec<cadmpeg_ir::ids::BodyId>)],
) -> Result<CadIr, CodecError> {
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
    ir.source = Some(source_meta(scan, block, header));
    ir.annotations = std::mem::take(&mut brep.annotations);
    let mut histories = crate::history::histories(scan, &mut ir.annotations);
    let mut lanes = crate::resolved_features::lanes(scan, &mut ir.annotations);
    crate::resolved_features::bind_history_classes(&mut histories, &lanes);
    crate::resolved_features::bind_scalar_operands(&histories, &mut lanes);
    let pmi_dimensions = crate::pmi::dimensions(scan, &mut ir.annotations);
    project_design_history(&mut ir, &histories, &lanes, &pmi_dimensions);
    let (spatial_sketches, spatial_sketch_entities) =
        crate::resolved_features::spatial_sketches(&mut ir.model.features, &histories, &lanes);
    ir.model.spatial_sketches = spatial_sketches;
    ir.model.spatial_sketch_entities = spatial_sketch_entities;
    crate::resolved_features::bind_extrusion_operations(&mut ir.model.features, &histories, &lanes);
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
    stamp_parameter_baseline(&mut ir);
    let (mut sketches, mut sketch_entities, mut sketch_constraints) =
        crate::resolved_features::sketches(scan, &mut ir.annotations);
    crate::resolved_features::bind_sketch_profiles(
        &mut ir.model.features,
        &mut sketches,
        &sketch_entities,
        &ir.model.parameters,
        &histories,
        &lanes,
        &ir.annotations,
    );
    crate::resolved_features::project_compact_sketch_profiles(
        &mut ir.model.features,
        &mut sketches,
        &mut sketch_entities,
        &histories,
        &lanes,
    );
    crate::history::bind_unique_sketch_feature(&mut ir.model.features, &sketches, &histories);
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
    let attributes = crate::metadata::attributes(scan, &mut ir.annotations);
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
    crate::history::order_features_for_regeneration(&mut ir.model.features);
    stamp_feature_baseline(&mut ir);
    assign_configuration_bodies(&mut ir, configuration_bodies);
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
            &mut ir.annotations,
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
        let material_stream = format!("block@{}", material.block_offset);
        crate::annotations::note(
            &mut ir.annotations,
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
        .blocks
        .iter()
        .filter(|block| crate::tessellation::block_summary(block).is_some())
    {
        for (index, mesh) in crate::tessellation::block_meshes(display)
            .into_iter()
            .enumerate()
        {
            let id = format!("sldprt:displaylist:record#{}:{index}", display.offset);
            let display_stream = display
                .section
                .clone()
                .unwrap_or_else(|| format!("block@{}", display.offset));
            crate::annotations::note(
                &mut ir.annotations,
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
                    source_object: None,
                    vertices: mesh.vertices,
                    triangles: mesh.triangles,
                    strip_lengths: mesh.strip_lengths,
                    normals: mesh.normals,
                    channels: mesh.channels,
                });
        }
        let display_id = format!("sldprt:displaylist:record#{}", display.offset);
        crate::annotations::note(
            &mut ir.annotations,
            display_id.clone(),
            display
                .section
                .clone()
                .unwrap_or_else(|| format!("block@{}", display.offset)),
            0,
            "displaylist_tessellation",
            Exactness::Unknown,
        );
        unknowns.push(UnknownRecord {
            id: UnknownId(display_id),
            offset: display.offset as u64,
            byte_len: display.uncomp_sz as u64,
            sha256: sha256_hex(&display.payload),
            data: Some(display.payload.clone()),
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
            &mut ir.annotations,
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
    let partition_id = UnknownId(format!("sldprt:file:block#{}", block.offset));
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
    preserve_source_image(scan, &mut ir, &mut unknowns);
    ir.set_native_unknowns("sldprt", &unknowns)?;
    set_semantic_hash(&mut ir);
    Ok(ir)
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

fn source_meta(scan: &ContainerScan, block: &Block, header: &StreamHeader) -> SourceMeta {
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
    let active_block = container::select_active_parasolid(scan).map_or(block, |(active, _)| active);
    attributes.insert(
        "active_parasolid_block".to_string(),
        active_block
            .section
            .clone()
            .unwrap_or_else(|| format!("block@{}", active_block.offset)),
    );
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
    for block in &scan.blocks {
        match block.family {
            "png-preview" => {
                let payload = &block.payload;
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
                let payload = &block.payload;
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
    for block in &scan.blocks {
        if block.family != "xml" || !block.payload.windows(12).any(|w| w == b"swSolidWorks") {
            continue;
        }
        let Ok(text) = std::str::from_utf8(&block.payload) else {
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
        losses.push(LossNote {
            category: LossCategory::Geometry,
            severity: Severity::Warning,
            message: format!(
                "{} face(s) rest on a support surface this codec does not type (offset, swept, \
                 blended, intersection, or spline-on-surface); \
                 the face, its loops, and trims are emitted with an unknown-geometry surface \
                 linking to the preserved record bytes. Topology is transferred; the underlying \
                 surface shape is not.",
                s.unknown_surface_faces
            ),
            provenance: None,
        });
    }
    if s.unknown_curve_edges > 0 {
        losses.push(LossNote {
            category: LossCategory::Geometry,
            severity: Severity::Warning,
            message: format!(
                "{} edge(s) reference an untyped support curve; topology references an opaque \
                 curve carrier linked to the retained partition.",
                s.unknown_curve_edges
            ),
            provenance: None,
        });
    }
    if s.synthetic_body_grouping {
        losses.push(LossNote {
            category: LossCategory::Topology,
            severity: Severity::Warning,
            message: "No body record was available; one body/region/shell hierarchy was derived."
                .to_string(),
            provenance: None,
        });
    }
    DecodeReport {
        format: "sldprt".to_string(),
        container_only: false,
        geometry_transferred: true,
        losses,
        notes: container::summarize(scan).notes,
    }
}

fn build_metadata_ir(scan: &ContainerScan) -> Result<CadIr, CodecError> {
    let mut ir = CadIr::empty(Units::default());
    let mut histories = crate::history::histories(scan, &mut ir.annotations);
    let mut lanes = crate::resolved_features::lanes(scan, &mut ir.annotations);
    crate::resolved_features::bind_history_classes(&mut histories, &lanes);
    crate::resolved_features::bind_scalar_operands(&histories, &mut lanes);
    let pmi_dimensions = crate::pmi::dimensions(scan, &mut ir.annotations);
    let (sketches, sketch_entities, sketch_constraints) =
        crate::resolved_features::sketches(scan, &mut ir.annotations);
    let model_attributes = crate::metadata::attributes(scan, &mut ir.annotations);
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
            &mut ir.annotations,
            id.clone(),
            block
                .section
                .clone()
                .unwrap_or_else(|| format!("block@{}", block.offset)),
            0,
            "parasolid_stream",
            Exactness::Unknown,
        );
        ir.push_native_unknown(
            "sldprt",
            UnknownRecord {
                id: UnknownId(id),
                offset: block.offset as u64,
                byte_len: block.uncomp_sz as u64,
                sha256: sha256_hex(&block.payload),
                data: Some(block.payload.clone()),
                links: Vec::new(),
            },
        )?;
    }

    ir.source = Some(SourceMeta {
        format: "sldprt".to_string(),
        attributes,
    });
    project_design_history(&mut ir, &histories, &lanes, &pmi_dimensions);
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
    stamp_parameter_baseline(&mut ir);
    crate::resolved_features::bind_sketch_profiles(
        &mut ir.model.features,
        &mut ir.model.sketches,
        &ir.model.sketch_entities,
        &ir.model.parameters,
        &histories,
        &lanes,
        &ir.annotations,
    );
    crate::resolved_features::project_compact_sketch_profiles(
        &mut ir.model.features,
        &mut ir.model.sketches,
        &mut ir.model.sketch_entities,
        &histories,
        &lanes,
    );
    crate::history::bind_unique_sketch_feature(
        &mut ir.model.features,
        &ir.model.sketches,
        &histories,
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
    crate::history::order_features_for_regeneration(&mut ir.model.features);
    stamp_feature_baseline(&mut ir);
    let native = crate::native::SldprtNative {
        version: crate::native::SLDPRT_NATIVE_VERSION,
        feature_histories: histories.clone(),
        feature_input_lanes: lanes,
        pmi_dimensions,
    };
    native.store(ir.native.namespace_mut("sldprt"))?;
    stamp_sketch_baseline(&mut ir, &native);
    mark_active_configuration(&mut ir);
    let mut unknowns = ir.native_unknowns("sldprt")?;
    preserve_source_image(scan, &mut ir, &mut unknowns);
    ir.set_native_unknowns("sldprt", &unknowns)?;
    set_semantic_hash(&mut ir);
    Ok(ir)
}

fn project_design_history(
    ir: &mut CadIr,
    histories: &[crate::records::FeatureHistory],
    lanes: &[crate::records::FeatureInputLane],
    pmi_dimensions: &[crate::records::PmiDimension],
) {
    let mut semantic_projection = histories.to_vec();
    crate::resolved_features::enrich_history_extrusion_terminations(
        &mut semantic_projection,
        lanes,
    );
    crate::resolved_features::enrich_history_combine_selections(&mut semantic_projection, lanes);
    crate::resolved_features::enrich_history_sweep_paths(&mut semantic_projection, lanes);
    crate::resolved_features::enrich_history_parameters(&mut semantic_projection, lanes, true);
    crate::resolved_features::enrich_history_reference_planes(&mut semantic_projection, lanes);
    crate::pmi::enrich_history_parameters(&mut semantic_projection, pmi_dimensions);
    crate::history::apply_evaluated_parameters(&mut semantic_projection);
    ir.model.features = crate::history::project_features(&semantic_projection);
    crate::resolved_features::project_compact_body_selections(&mut ir.model.features, lanes);
    crate::resolved_features::project_compact_combine_paths(
        &mut ir.model.features,
        &semantic_projection,
        lanes,
    );
    crate::resolved_features::project_compact_edge_selections(&mut ir.model.features, lanes);
    crate::resolved_features::project_compact_surface_selections(&mut ir.model.features, lanes);
    crate::resolved_features::project_surface_sweep_profiles(
        &mut ir.model.features,
        &semantic_projection,
        lanes,
    );
    crate::resolved_features::project_helix_axes(
        &mut ir.model.features,
        &semantic_projection,
        lanes,
    );
    crate::resolved_features::project_adjacent_extrusion_profiles(
        &mut ir.model.features,
        &semantic_projection,
        lanes,
    );
    ir.model.configurations = crate::history::project_configurations(&semantic_projection);
    let mut parameter_projection = histories.to_vec();
    crate::resolved_features::enrich_history_parameters(&mut parameter_projection, lanes, false);
    crate::pmi::enrich_history_parameters(&mut parameter_projection, pmi_dimensions);
    ir.model.parameters = crate::history::project_parameters(&parameter_projection);
    project_configuration_parameter_values(ir, histories, lanes, pmi_dimensions);
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
            "sldprt_neutral_configuration_sha256".into(),
            crate::history::configuration_hash(&ir.model.configurations),
        );
        source.attributes.insert(
            "sldprt_configuration_parameter_values_sha256".into(),
            crate::history::configuration_parameter_value_hash(&ir.model.configurations),
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

fn project_configuration_parameter_values(
    ir: &mut CadIr,
    histories: &[crate::records::FeatureHistory],
    lanes: &[crate::records::FeatureInputLane],
    pmi_dimensions: &[crate::records::PmiDimension],
) {
    let mut lanes_by_configuration =
        BTreeMap::<String, Vec<&crate::records::FeatureInputLane>>::new();
    for lane in lanes {
        let Some(configuration) = lane.configuration.as_ref() else {
            continue;
        };
        lanes_by_configuration
            .entry(configuration.clone())
            .or_default()
            .push(lane);
    }
    for (source_key, scoped_lanes) in lanes_by_configuration {
        let [scoped_lane] = scoped_lanes.as_slice() else {
            continue;
        };
        let Ok(source_index) = source_key.parse::<u32>() else {
            continue;
        };
        let candidates = ir
            .model
            .configurations
            .iter()
            .enumerate()
            .filter(|(_, configuration)| {
                configuration.source_index == Some(source_index)
                    || configuration.source_index.is_none() && configuration.ordinal == source_index
            })
            .map(|(position, _)| position)
            .collect::<Vec<_>>();
        let [configuration_index] = candidates.as_slice() else {
            continue;
        };
        let mut projection = histories.to_vec();
        crate::resolved_features::enrich_history_parameters(
            &mut projection,
            std::iter::once(*scoped_lane),
            true,
        );
        crate::pmi::enrich_history_parameters(&mut projection, pmi_dimensions);
        ir.model.configurations[*configuration_index].parameter_values =
            crate::history::project_parameters(&projection)
                .into_iter()
                .filter_map(|parameter| parameter.value.map(|value| (parameter.id, value)))
                .collect();
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
            configuration.bodies = bodies;
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
                configuration.bodies = bodies;
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
            configuration.bodies = bodies;
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
                bodies,
                parameter_values: std::collections::BTreeMap::new(),
                native_ref: None,
            });
    }
}

fn stamp_configuration_baseline(ir: &mut CadIr) {
    let hash = crate::history::configuration_hash(&ir.model.configurations);
    let parameter_value_hash =
        crate::history::configuration_parameter_value_hash(&ir.model.configurations);
    if let Some(source) = &mut ir.source {
        source
            .attributes
            .insert("sldprt_neutral_configuration_sha256".into(), hash);
        source.attributes.insert(
            "sldprt_configuration_parameter_values_sha256".into(),
            parameter_value_hash,
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
        annotations: Annotations::default(),
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

fn preserve_source_image(scan: &ContainerScan, ir: &mut CadIr, unknowns: &mut Vec<UnknownRecord>) {
    crate::annotations::note(
        &mut ir.annotations,
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
    let parasolid_blocks = scan
        .blocks
        .iter()
        .filter(|b| b.family == "parasolid")
        .count();

    let mut losses = vec![
        LossNote {
            category: LossCategory::Geometry,
            severity: Severity::Blocking,
            message: format!(
                "Parasolid B-rep geometry was not transferred: no partition/deltas stream resolved \
                 into a topology graph. {} block(s) were CRC-validated and enumerated, {} of them \
                 Parasolid-family.",
                scan.blocks.len(),
                parasolid_blocks
            ),
            provenance: None,
        },
        LossNote {
            category: LossCategory::Topology,
            severity: Severity::Blocking,
            message:
                "B-rep topology graph (body/region/shell/face/loop/coedge/edge/vertex) was not \
                      built for this file."
                    .to_string(),
            provenance: None,
        },
        LossNote {
            category: LossCategory::Material,
            severity: Severity::Warning,
            message: "Materials/appearances, tessellation, and document/feature metadata were not \
                      transferred."
                .to_string(),
            provenance: None,
        },
    ];

    if container::select_active_parasolid(scan).is_none() {
        losses.push(LossNote {
            category: LossCategory::Geometry,
            severity: Severity::Error,
            message: "no Parasolid partition/deltas stream was located in the container"
                .to_string(),
            provenance: None,
        });
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
        append_design_losses, assign_configuration_bodies, unprojected_sketch_relation_records,
    };
    use crate::native::SldprtNative;
    use crate::records::{
        FeatureInputLane, FeatureInputRelationBinding, FeatureInputRelationFamily,
        FeatureInputRelationInstance, SketchInputEntity, SketchInputKind, SketchRelationKind,
    };
    use cadmpeg_ir::features::{
        Angle, BodySelection, BooleanOp, ConfigurationId, DesignConfiguration, DesignParameter,
        FaceSelection, Feature, FeatureDefinition, FeatureId, FeatureSourceContent,
        FeatureTreeNodeRole, Length, ParameterId, ParameterPmi, ParameterValue,
        PmiDimensionSubtype,
    };
    use cadmpeg_ir::ids::BodyId;
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
            suppressed: false,
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
    fn every_typed_family_participates_in_design_completeness_accounting() {
        let mut ir = CadIr::empty(Units::default());
        let feature = |id: &str, ordinal, definition| Feature {
            id: FeatureId(id.into()),
            ordinal,
            name: None,
            suppressed: false,
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
    fn incomplete_parameter_semantics_are_reported_as_design_losses() {
        let mut ir = CadIr::empty(Units::default());
        let owner = FeatureId("owner".into());
        ir.model.features.push(Feature {
            id: owner.clone(),
            ordinal: 0,
            name: Some("Boss-Extrude1".into()),
            suppressed: false,
            parent: None,
            dependencies: Vec::new(),
            source_properties: BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs: Vec::new(),
            definition: FeatureDefinition::TreeNode {
                role: FeatureTreeNodeRole::History,
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
            value: Some(cadmpeg_ir::features::ParameterValue::Real(1.0)),
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
        for (id, ordinal, name) in [
            ("empty", 6, ""),
            ("shared-a", 7, "Shared"),
            ("shared-b", 8, "Shared"),
            ("ordinal", 8, "Unique"),
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
                == "1 parameter(s) lack an evaluated scalar; 3 parameter expression(s) contain unresolved, ambiguous, or malformed parameter references; 1 parameter record(s) contain missing or non-preceding dependency edges."
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
            suppressed: false,
            parent,
            dependencies,
            source_properties: BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs: Vec::new(),
            definition: FeatureDefinition::TreeNode {
                role: FeatureTreeNodeRole::History,
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
            suppressed: false,
            parent: None,
            dependencies: Vec::new(),
            source_properties: BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs,
            definition: FeatureDefinition::TreeNode {
                role: FeatureTreeNodeRole::History,
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
            bodies: Vec::new(),
            parameter_values: BTreeMap::new(),
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
                bodies: Vec::new(),
                parameter_values: BTreeMap::new(),
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
                bodies: Vec::new(),
                parameter_values: BTreeMap::new(),
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
            bodies: Vec::new(),
            parameter_values: BTreeMap::new(),
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
            native_ref: Some(format!("native:{id}")),
        };
        ir.model.configurations = vec![
            configuration("duplicate", 0, vec![body.clone(), body]),
            configuration("missing", 1, vec![BodyId("missing-body".into())]),
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
                references: Vec::new(),
                sketch_entities: vec![
                    marker(
                        "relation-marker",
                        0,
                        SketchInputKind::Relation(SketchRelationKind::Horizontal),
                    ),
                    marker("geometry-marker", 1, SketchInputKind::Native(99)),
                ],
            }],
            ..SldprtNative::default()
        };

        assert_eq!(unprojected_sketch_relation_records(&ir, &native), 3);
    }

    #[test]
    fn native_dimension_subtypes_are_reported() {
        let mut ir = CadIr::empty(Units::default());
        let owner = FeatureId("owner".into());
        ir.model.features.push(Feature {
            id: owner.clone(),
            ordinal: 0,
            name: Some("Feature".into()),
            suppressed: false,
            parent: None,
            dependencies: Vec::new(),
            source_properties: BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs: Vec::new(),
            definition: FeatureDefinition::TreeNode {
                role: FeatureTreeNodeRole::History,
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
