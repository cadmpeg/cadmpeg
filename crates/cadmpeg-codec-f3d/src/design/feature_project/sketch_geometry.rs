// SPDX-License-Identifier: Apache-2.0
//! Sketch geometry binding of feature selections.

use crate::ids::native_stream;
use crate::records::feature::scope::DesignParameterScope;
use crate::records::sketch_placement::DesignSketchPlacement;
use cadmpeg_core::CodecError;
use std::collections::HashMap;

/// Bind each Sketch history node to geometry in exactly one neutral sketch arena.
pub(crate) fn bind_sketch_feature_geometry(
    features: &mut [cadmpeg_ir::features::Feature],
    scopes: &[DesignParameterScope],
    placements: &[DesignSketchPlacement],
    sketches: &[cadmpeg_ir::sketches::Sketch],
    spatial_sketches: &[cadmpeg_ir::sketches::SpatialSketch],
) -> Result<(), CodecError> {
    use cadmpeg_ir::features::{
        DatumPointConstruction, FeatureDefinition, FeatureOperation, LoftSection, PathRef,
        PlanarProfileRef, ProfileRef, SketchPointSelection,
    };

    for feature in features.iter_mut() {
        let mut display_name_to_apply = None;
        feature.evaluation.edit(|definition, _| {
            if !matches!(
                definition,
                FeatureDefinition::Operation(
                    FeatureOperation::Sketch { .. } | FeatureOperation::SpatialSketch { .. }
                )
            ) {
                return;
            }
            let Some(scope) = feature
                .native_ref
                .as_deref()
                .and_then(|native_ref| scopes.iter().find(|scope| scope.id == native_ref))
            else {
                return;
            };
            let Some(placement) =
                crate::design::sketch_names::exact_scope_placement(scope, placements)
            else {
                return;
            };
            if let Some(name) = placement.display_name.as_deref() {
                display_name_to_apply = Some(name.to_owned());
            }
            let planar = crate::ids::neutral_sketch_id(placement);
            let spatial = crate::ids::neutral_spatial_sketch_id(placement);
            let has_planar = sketches.iter().any(|sketch| sketch.id == planar);
            let has_spatial = spatial_sketches.iter().any(|sketch| sketch.id == spatial);
            *definition = match (has_planar, has_spatial) {
                (true, false) => FeatureDefinition::Operation(FeatureOperation::Sketch {
                    sketch: cadmpeg_ir::features::SketchFeatureBinding::Planar(Some(planar)),
                }),
                (false, true) => FeatureDefinition::Operation(FeatureOperation::SpatialSketch {
                    sketch: Some(spatial),
                }),
                _ => FeatureDefinition::Operation(FeatureOperation::Sketch {
                    sketch: cadmpeg_ir::features::SketchFeatureBinding::Unresolved,
                }),
            };
        });
        if let Some(custom_name) = display_name_to_apply {
            feature.name = Some(custom_name);
        }
    }
    for feature in features.iter_mut() {
        feature.evaluation.edit(|definition, _| {
            'feature_edit: {
                let Some(scope) = feature
                    .native_ref
                    .as_deref()
                    .and_then(|native_ref| scopes.iter().find(|scope| scope.id == native_ref))
                else {
                    break 'feature_edit;
                };
                let FeatureDefinition::Operation(FeatureOperation::Extrude { profile, .. }) =
                    definition
                else {
                    break 'feature_edit;
                };
                let ProfileRef::Planar(PlanarProfileRef::Sketch(sketch)) = profile else {
                    break 'feature_edit;
                };
                if sketches.iter().any(|candidate| candidate.id == *sketch) {
                    break 'feature_edit;
                }
                let mut spatial = None;
                for placement in placements {
                    if crate::ids::neutral_sketch_id(placement) != *sketch {
                        continue;
                    }
                    let spatial_id = crate::ids::neutral_spatial_sketch_id(placement);
                    if let Some(candidate) = spatial_sketches
                        .iter()
                        .find(|candidate| candidate.id == spatial_id)
                    {
                        if spatial.replace(candidate).is_some() {
                            break 'feature_edit;
                        }
                    }
                }
                let Some(spatial) = spatial else {
                    break 'feature_edit;
                };
                let Some(profile_operand) = scope.extrude_profile() else {
                    // Without a profile operand, the selection groups of the
                    // scope resolve the spatial profiles later.
                    break 'feature_edit;
                };
                let Some(stream) = native_stream(&scope.id) else {
                    break 'feature_edit;
                };
                // Keep the exact profile frame as a native selection instead of
                // the provisional planar ID. Profile-selection binding replaces
                // it with spatial profile indices only when the stored region
                // selection and the Extrude transition prove them.
                let sketch_id = spatial.id.clone();
                let selection = format!(
                    "{}{}{}",
                    stream,
                    ":design-record-header#",
                    profile_operand.byte_offset()
                );
                *profile = match ProfileRef::spatial_sketch_selection(sketch_id, vec![selection]) {
                    Ok(profile) => profile,
                    Err(_) => ProfileRef::Planar(PlanarProfileRef::Native(scope.id.clone())),
                };
            }
        });
    }
    let mut sketch_features = HashMap::new();
    let mut spatial_sketch_features = HashMap::new();
    {
        for feature in features.iter() {
            let (index, sketch) = match feature.evaluation.definition() {
                FeatureDefinition::Operation(FeatureOperation::Sketch {
                    sketch: cadmpeg_ir::features::SketchFeatureBinding::Planar(Some(sketch)),
                }) => (&mut sketch_features, sketch.as_str()),
                FeatureDefinition::Operation(FeatureOperation::SpatialSketch {
                    sketch: Some(sketch),
                }) => (&mut spatial_sketch_features, sketch.as_str()),
                _ => continue,
            };
            let key = sketch.to_owned();
            let value = feature.id.clone();
            // discarded-value: duplicate sketch bindings keep the last feature.
            let _ = index.insert(key, value);
        }
    };
    let planar_profile_dependency = |profile: &PlanarProfileRef| match profile {
        PlanarProfileRef::Sketch(sketch)
        | PlanarProfileRef::SketchProfiles { sketch, .. }
        | PlanarProfileRef::SketchRegions { sketch, .. }
        | PlanarProfileRef::SketchEntities { sketch, .. }
        | PlanarProfileRef::SketchSelection { sketch, .. } => sketch_features.get(sketch.as_str()),
        _ => None,
    };
    let profile_dependency = |profile: &ProfileRef| match profile {
        ProfileRef::Planar(
            PlanarProfileRef::Sketch(sketch)
            | PlanarProfileRef::SketchProfiles { sketch, .. }
            | PlanarProfileRef::SketchRegions { sketch, .. }
            | PlanarProfileRef::SketchEntities { sketch, .. }
            | PlanarProfileRef::SketchSelection { sketch, .. },
        ) => sketch_features.get(sketch.as_str()),
        ProfileRef::SpatialSketchProfiles { sketch, .. }
        | ProfileRef::SpatialSketchSelection { sketch, .. } => {
            spatial_sketch_features.get(sketch.as_str())
        }
        ProfileRef::Planar(_) => None,
    };
    let path_dependency = |path: &PathRef| match path {
        PathRef::Sketch(sketch) | PathRef::SketchCurves { sketch, .. } => {
            sketch_features.get(sketch.as_str())
        }
        PathRef::SpatialSketchSelection { sketch, .. }
        | PathRef::SpatialSketchCurves { sketch, .. } => {
            spatial_sketch_features.get(sketch.as_str())
        }
        _ => None,
    };
    let sketch_point_dependency = |point: &SketchPointSelection| match point {
        SketchPointSelection::Planar { sketch, .. } => sketch_features.get(sketch.as_str()),
        SketchPointSelection::Spatial { sketch, .. } => {
            spatial_sketch_features.get(sketch.as_str())
        }
        SketchPointSelection::Unresolved | SketchPointSelection::Native(_) => None,
    };
    for feature in features.iter_mut() {
        let mut add_dependency = |dependency: Option<&cadmpeg_ir::features::FeatureId>| {
            if let Some(dependency) = dependency {
                if dependency != &feature.id {
                    feature.dependencies.insert(dependency.clone());
                }
            }
            Ok::<(), CodecError>(())
        };
        match feature.evaluation.definition() {
            FeatureDefinition::Operation(FeatureOperation::Extrude { profile, .. }) => {
                add_dependency(profile_dependency(profile))?;
            }
            FeatureDefinition::Operation(FeatureOperation::SheetMetalBaseFlange {
                profile,
                ..
            }) => {
                add_dependency(planar_profile_dependency(profile))?;
            }
            FeatureDefinition::Operation(FeatureOperation::SheetMetalContourFlange {
                construction,
            }) => {
                add_dependency(
                    construction
                        .profile
                        .as_ref()
                        .and_then(planar_profile_dependency),
                )?;
            }
            FeatureDefinition::Operation(FeatureOperation::Revolve { construction, .. }) => {
                add_dependency(construction.profile().and_then(planar_profile_dependency))?;
                add_dependency(
                    construction
                        .axis()
                        .and_then(|axis| axis.reference.as_ref())
                        .and_then(path_dependency),
                )?;
            }
            FeatureDefinition::Operation(FeatureOperation::Sweep {
                shape,
                path,
                guide_rail,
                ..
            }) => {
                add_dependency(
                    shape
                        .referenced_profile()
                        .and_then(planar_profile_dependency),
                )?;
                match shape {
                    cadmpeg_ir::features::SweepShape::Unresolved { sections, .. }
                    | cadmpeg_ir::features::SweepShape::Surface { sections, .. } => {
                        for section in sections {
                            add_dependency(
                                section
                                    .referenced_profile()
                                    .and_then(planar_profile_dependency),
                            )?;
                        }
                    }
                    cadmpeg_ir::features::SweepShape::Solid { sections, .. } => {
                        for section in sections {
                            add_dependency(
                                section
                                    .referenced_profile()
                                    .and_then(planar_profile_dependency),
                            )?;
                        }
                    }
                }
                add_dependency(path.as_ref().and_then(path_dependency))?;
                add_dependency(
                    guide_rail
                        .as_ref()
                        .and_then(|guide| path_dependency(&guide.path)),
                )?;
            }
            FeatureDefinition::Operation(FeatureOperation::Loft {
                sections, guidance, ..
            }) => {
                for section in sections {
                    if let LoftSection::Profile(profile) = section {
                        add_dependency(profile_dependency(profile))?;
                    }
                }
                match guidance {
                    cadmpeg_ir::features::LoftGuidance::Guides(paths) => {
                        for path in paths {
                            add_dependency(path_dependency(path))?;
                        }
                    }
                    cadmpeg_ir::features::LoftGuidance::Centerline(path) => {
                        add_dependency(path_dependency(path))?;
                    }
                }
            }
            FeatureDefinition::Operation(FeatureOperation::DatumPoint {
                construction: Some(construction),
                ..
            }) => {
                if let DatumPointConstruction::SketchPoint { point } = construction.as_ref() {
                    add_dependency(sketch_point_dependency(point))?;
                }
            }
            FeatureDefinition::Operation(FeatureOperation::Hole {
                profile: Some(profile),
                ..
            }) => {
                add_dependency(planar_profile_dependency(profile))?;
            }
            _ => {}
        }
    }
    Ok(())
}
