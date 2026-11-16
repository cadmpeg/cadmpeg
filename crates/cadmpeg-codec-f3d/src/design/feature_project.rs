// SPDX-License-Identifier: Apache-2.0
//! Project parameter-design features and dispatch per feature family.

use crate::design::dimensions::expression_identifiers;
use crate::design::edge_resolve::resolved_edge_group;
use crate::design::face_resolve::{
    design_angle, resolved_face_group, resolved_historical_face_group,
    resolved_historical_split_face_target_group_with_updated_faces, resolved_profile_face_group,
};

use crate::design::{design_feature_family, DesignFeatureFamily};
use crate::ids::{self, native_stream};
use pattern_seeds::unprojected_pattern_seeds;

use crate::records::{
    bodies::DesignBodyBinding,
    entity_header::DesignFeatureTimeline,
    feature::{
        scope::DesignParameterScope, surface_ops::DesignSurfaceTrimOperation,
        work_geometry::DesignEdgeTreatmentVertexOperand,
    },
    parameters::{DesignParameter, DesignParameterKind, DesignParameterOwner},
    sketch_geometry::SketchCurveIdentity,
    sketch_placement::DesignSketchPlacement,
    topology::{
        body_recipe::DesignBodyRecipeOperand, construction::DesignConstructionOperandGroup,
        edge_identity::DesignEdgeIdentityOperand, edge_identity::DesignEdgeOperand,
        extrude_selection::DesignOperandRole, face::DesignFaceOperand,
        fillet::DesignFilletRadiusGroup,
    },
};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::math::{Point3, Vector3};
use std::collections::{HashMap, HashSet};
use std::hash::Hash;

pub(crate) mod form_cages;
mod mesh_face_groups;
mod mirror;
mod move_owners;
mod paste_copy;
mod path_pattern;
mod pattern_seeds;
mod reverse_normal;
mod revolve_target;
pub(crate) mod shell_fallback;
mod surface_offset;
mod surface_patch;
mod surface_untrim;
pub(crate) mod sweep_fallback;
pub(crate) mod thicken;
mod unstitch;

/// Millimetres per Design length unit (the Design stream stores centimetres).
const DESIGN_LENGTH_TO_MILLIMETRE: f64 = 10.0;
const EPS_FEATURE_PROJECT_MATRIX_AXIS_ANGLE_E12: f64 = 1.0e-12;
const EPS_FEATURE_PROJECT_MATRIX_AXIS_ANGLE_E8: f64 = 1.0e-8;
const EPS_FEATURE_PROJECT_PROJECT_EXTRUDE_E9: f64 = 1.0e-9;
const EPS_FEATURE_PROJECT_PROJECT_EXTRUDE_E12: f64 = 1.0e-12;
macro_rules! or_none {
    ($value:expr) => {
        match $value {
            Some(value) => value,
            None => return Ok(None),
        }
    };
}

mod bend_lines;
mod body_ops;
mod chamfer;
mod contour;
mod delete_face;
mod direct_face;
mod draft;
mod edge_flange;
mod edge_treatments;
mod emboss;
mod extrude;
mod fold;
mod hole;
mod pattern;
mod primitives;
mod revolve;
mod scope_order;
mod scope_properties;
mod sheet_metal;
mod sketch_geometry;
mod surface_edit;
mod sweep_loft;
mod work_points;

use body_ops::{project_align, project_modify, project_move};
pub(super) use body_ops::{project_combine, project_remove_body};
use direct_face::{project_move_face, project_replace_face, project_split_face};
pub(super) use direct_face::{project_offset_faces, project_shell, project_split};
use draft::project_draft;
use edge_treatments::{project_chamfer, project_fillet_arm, project_fixed_chamfer};
use emboss::project_emboss;
use extrude::{bind_extrude_work_plane_targets, project_extrude};
pub(crate) use hole::bind_hole_sketch_point_profiles;
use hole::project_hole;
use pattern::{project_circular_pattern, project_rectangular_pattern_scalars};
use primitives::{project_coil, project_solid_primitive};
pub(super) use revolve::project_fixed_revolve_with_entities;
pub(crate) use revolve::{bind_revolve_face_axes, orient_revolve_axes};
use scope_order::{
    authored_scope_ordinals, ensure_feature_dependencies_precede, normalize_parameter_ordinals,
};
pub(crate) use scope_order::{
    authored_scope_ordinals_per_stream, ScopeHistoryGraph, ScopeHistoryPredecessor,
};
use scope_properties::{scope_parameter_expressions, scope_properties};
use sheet_metal::{project_base_flange, project_edge_flange, project_hem};
pub(crate) use sketch_geometry::bind_sketch_feature_geometry;
pub(super) use surface_edit::project_surface_stitch;
use surface_edit::{project_boundary_fill, project_ruled_surface, project_surface_trim};
use sweep_loft::project_fixed_loft;
pub(crate) use sweep_loft::{bind_loft_sketch_point_sections, loft_groups};
pub(super) use sweep_loft::{closed_spatial_sketch_profiles, project_fixed_sweep};
pub(crate) use work_points::{
    bind_work_point_sketch_point_constructions, work_point_recipe_state_id,
};
use work_points::{
    project_work_plane, project_work_point_construction, work_point_input_history_state_id,
};

fn unique_feature_match<T>(mut matches: impl Iterator<Item = T>) -> Option<T> {
    let first = matches.next()?;
    matches.next().is_none().then_some(first)
}

/// Design record slices projected together into the neutral construction
/// history: the parameter, owner, and scope tables plus the construction
/// operand, fillet-radius, edge, edge-identity, face, and whole-body recipe
/// operand records and the sketch placements and body bindings each feature
/// scope resolves against.
#[cfg_attr(test, derive(Default))]
pub(crate) struct ProjectInputs<'a> {
    pub(crate) native: &'a [DesignParameter],
    pub(crate) owners: &'a [DesignParameterOwner],
    pub(crate) scopes: &'a [DesignParameterScope],
    pub(crate) timelines: &'a [DesignFeatureTimeline],
    pub(crate) design_types: &'a [crate::records::entity_header::SegmentType],
    pub(crate) construction_groups: &'a [DesignConstructionOperandGroup],
    pub(crate) fillet_radius_groups: &'a [DesignFilletRadiusGroup],
    pub(crate) edge_operands: &'a [DesignEdgeOperand],
    pub(crate) edge_identity_operands: &'a [DesignEdgeIdentityOperand],
    pub(crate) edge_treatment_vertex_operands: &'a [DesignEdgeTreatmentVertexOperand],
    pub(crate) entity_selection_operands:
        &'a [crate::records::topology::entity_selection::DesignEntitySelectionOperand],
    pub(crate) curve_identities: &'a [SketchCurveIdentity],
    pub(crate) face_operands: &'a [DesignFaceOperand],
    pub(crate) body_recipe_operands: &'a [DesignBodyRecipeOperand],
    pub(crate) placements: &'a [DesignSketchPlacement],
    pub(crate) placement_constructions:
        &'a [crate::records::placement_construction::DesignPlacementConstruction],
    pub(crate) body_bindings: &'a [DesignBodyBinding],
    pub(crate) component_naming_spaces: &'a [crate::records::recipes::DesignComponentNamingSpace],
    pub(crate) histories: &'a [crate::history_records::AsmHistory],
    pub(crate) face_boundaries:
        &'a [crate::records::topology::historical_context::AsmFaceBoundary],
    pub(crate) sheet_metal_settings:
        &'a [crate::records::sheet_metal_settings::DesignSheetMetalSettings],
}

/// The millimetre coordinate frame that a `JointOrigin` scope solves.
pub(crate) fn joint_origin_frame(
    scope: &DesignParameterScope,
) -> Option<cadmpeg_ir::features::FeatureCoordinateFrame> {
    let transform = scope.joint_origin_transform()?;
    cadmpeg_ir::features::FeatureCoordinateFrame::new(
        Point3::new(
            transform[0][3] * 10.0,
            transform[1][3] * 10.0,
            transform[2][3] * 10.0,
        ),
        Vector3::new(transform[0][0], transform[1][0], transform[2][0]),
        Vector3::new(transform[0][1], transform[1][1], transform[2][1]),
        Vector3::new(transform[0][2], transform[1][2], transform[2][2]),
    )
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) enum ComponentHistoryNamespace {
    Aggregate,
    Component(u64),
}

/// Neutral features and parameters projected from the Design records.
#[derive(Debug)]
pub(crate) struct ProjectedParameterDesign {
    pub(crate) features: Vec<cadmpeg_ir::features::Feature>,
    pub(crate) parameters: Vec<cadmpeg_ir::features::DesignParameter>,
    /// Owner scopes whose parameter ordering exceeded [`PARAMETER_ORDER_WORK`].
    /// Each keeps its source ordinals and loses its same-scope dependency edges.
    pub(crate) unordered_parameter_scopes: usize,
    /// `BaseFlange` scopes projected with their stored thickness copy because
    /// their sheet-metal rule states no thickness.
    pub(crate) unruled_flange_thicknesses: usize,
    pub(crate) projection_losses: Vec<cadmpeg_ir::report::loss::LossNote>,
}

impl ProjectedParameterDesign {
    /// The loss note for base flanges projected with their stored thickness
    /// copy, when there are any.
    pub(crate) fn unruled_flange_thickness_loss(
        &self,
    ) -> Option<cadmpeg_ir::report::loss::LossNote> {
        (self.unruled_flange_thicknesses != 0).then(|| {
            crate::loss::F3dLossCode::SheetThicknessUnruled.note(format!(
                "{} sheet-metal base flange(s) use the thickness copy their scope stores: their sheet-metal rule states no readable thickness, and the copy can differ from it.",
                self.unruled_flange_thicknesses,
            ))
        })
    }
}

/// Project Design parameters and feature scopes, including fixed edge identities.
pub(crate) fn project_parameter_design_with_edge_identities(
    ctx: &DecodeContext<'_>,
    inputs: &ProjectInputs<'_>,
) -> Result<ProjectedParameterDesign, CodecError> {
    use cadmpeg_ir::features::{
        patterns::PatternKind, DesignParameter as NeutralParameter, DimensionDisplay, Feature,
        FeatureDefinition, FeatureOperation, ParameterId, ParameterValue, UnresolvedFamily,
    };
    use cadmpeg_ir::scalar::{Angle, Length};
    use std::collections::BTreeMap;

    let &ProjectInputs {
        native,
        owners,
        scopes,
        timelines,
        construction_groups,
        edge_operands,
        edge_identity_operands,
        edge_treatment_vertex_operands,
        entity_selection_operands,
        design_types,
        curve_identities,
        face_operands,
        body_recipe_operands,
        placements,
        body_bindings,
        component_naming_spaces,
        histories,
        face_boundaries,
        sheet_metal_settings,
        ..
    } = inputs;
    let mut projection_losses = Vec::new();
    let unruled_flange_thicknesses = std::cell::Cell::new(0_usize);

    let source_ordinals = authored_scope_ordinals(scopes, timelines)?;

    let mut scope_ids = HashMap::new();
    {
        for scope in scopes {
            let Some(stream) = native_stream(&scope.id) else {
                continue;
            };
            if source_ordinals.contains_key(&(stream, scope.record_index)) {
                // discarded-value: duplicate scope keys retain the last feature ID.
                let _ = scope_ids.insert(
                    (stream, scope.record_index),
                    crate::ids::neutral_feature_id(scope),
                );
            }
        }
    }
    let mut owners_by_index = HashMap::new();
    {
        for owner in owners {
            let Some(stream) = native_stream(owner.id()) else {
                continue;
            };
            // discarded-value: duplicate owner keys retain the last source record.
            let _ = owners_by_index.insert((stream, owner.record_index()), owner);
        }
    }
    let extended_assembly_frames = scopes
        .iter()
        .filter(|scope| scope.kind() == crate::records::feature::scope::DesignFeatureKind::Assemble)
        .filter_map(|scope| {
            scope
                .assembly_alignment()
                .filter(|alignment| alignment.additional_tail_reference.is_some())
                .and_then(
                    crate::records::feature::assembly::DesignAssemblyAlignment::operand_frames,
                )
        })
        .flatten()
        .map(|frame| {
            (
                (frame.reference_record_index, frame.transform_offset),
                frame.transform,
            )
        })
        .collect::<HashMap<_, _>>();
    let mut features = scopes
        .iter()
        .filter(|scope| {
            let stream = native_stream(&scope.id).unwrap_or(ids::DEFAULT_STREAM);
            source_ordinals.contains_key(&(stream, scope.record_index))
        })
        .map(|scope| -> Result<Feature, CodecError> {
            let native_scope = native_stream(&scope.id).unwrap_or(ids::DEFAULT_STREAM);
            let mut parameters = Vec::new();
            for owner in owners.iter().filter(|owner| {
                native_stream(owner.id()) == Some(native_scope)
                    && owner.scope_record_index() == scope.record_index
            }) {
                if let Some(parameter) = native.iter().find(|parameter| {
                    native_stream(&parameter.id) == Some(native_scope)
                        && parameter.record_index == owner.parameter_record_index()
                }) {
                    parameters.push((owner.local_ordinal(), parameter));
                }
            }
            let family = design_feature_family(&scope.kind());
            let mut retain_chamfer_records = false;
            let mut inserted_bodies: Vec<cadmpeg_ir::ids::BodyId> = Vec::new();
            let definition = match family {
                Some(DesignFeatureFamily::Sketch) => {
                    FeatureDefinition::Operation(FeatureOperation::Sketch {
                        sketch: cadmpeg_ir::features::SketchFeatureBinding::Unresolved,
                    })
                }
                // The joint is product structure; the feature names it only
                // after product projection has emitted it.
                Some(DesignFeatureFamily::Assemble) => native_scope_definition(scope, &parameters)?,
                Some(DesignFeatureFamily::Extrude) => {
                    let mut definition = project_extrude(
                        ctx,
                        scope,
                        &parameters,
                        construction_groups,
                        face_operands,
                        placements,
                        body_recipe_operands,
                    )?
                    .map_or_else(|| native_scope_definition(scope, &parameters), Ok)?;
                    bind_extrude_work_plane_targets(
                        &mut definition,
                        scope,
                        scopes,
                        construction_groups,
                        entity_selection_operands,
                    );
                    definition
                }
                Some(DesignFeatureFamily::Fillet) => project_fillet_arm(
                    ctx,
                    inputs,
                    scope,
                    parameters.as_slice(),
                    native_scope,
                    &mut projection_losses,
                )?,
                Some(DesignFeatureFamily::Chamfer) => {
                    let projection = chamfer::prepare(scope, inputs);
                    let mut definition = if projection.legacy_compatible() && parameters.is_empty() {
                        project_fixed_chamfer(scope, construction_groups, edge_operands,
                            edge_identity_operands, edge_treatment_vertex_operands, histories,
                            (ctx, &mut projection_losses))?
                    } else { None };
                    if definition.is_none() && projection.legacy_compatible() {
                        definition = project_chamfer(scope, &parameters, inputs, ctx, &mut projection_losses)?;
                    }
                    definition.map_or_else(|| {
                        retain_chamfer_records = true;
                        projection.project(&parameters, ctx, &mut projection_losses)
                    }, Ok)?
                }
                Some(DesignFeatureFamily::Combine) => project_combine(scope, native_scope)
                    .unwrap_or_else(|| {
                        projection_losses.push(crate::loss::F3dLossCode::FeatureCombineDefinitionUnresolved.note(
                            format!("{}: the Combine operation or its operand construction is unresolved; the feature family is retained without a fabricated Boolean operation", scope.id),
                        ));
                        FeatureDefinition::Operation(FeatureOperation::Unresolved {
                            family: UnresolvedFamily::Combine,
                        })
                    }),
                Some(DesignFeatureFamily::Draft) => {
                    project_draft(scope, inputs)?.unwrap_or_else(|| {
                        FeatureDefinition::Operation(FeatureOperation::Native {
                            kind: scope.kind_name().into(),
                            parameters: BTreeMap::new(),
                        })
                    })
                }
                Some(DesignFeatureFamily::ReplaceFace) => project_replace_face(
                    scope,
                    construction_groups,
                    face_operands,
                    body_recipe_operands,
                )
                .map_or_else(|| native_scope_definition(scope, &parameters), Ok)?,
                Some(DesignFeatureFamily::Revolve) => {
                    let mut definition = project_fixed_revolve_with_entities(
                        scope,
                        construction_groups,
                        edge_operands,
                        entity_selection_operands,
                        face_operands,
                        (placements, curve_identities),
                        design_types,
                    )
                    .unwrap_or(FeatureDefinition::Operation(FeatureOperation::Unresolved {
                        family: UnresolvedFamily::Revolve,
                    }));
                    revolve_target::bind_revolve_shape_target(
                        &mut definition,
                        scope,
                        construction_groups,
                        face_operands,
                        body_recipe_operands,
                        histories,
                    );
                    definition
                }
                Some(DesignFeatureFamily::Loft) => {
                    project_fixed_loft(scope, inputs, ctx, &mut projection_losses)?.unwrap_or_else(
                        || {
                            FeatureDefinition::Operation(FeatureOperation::Native {
                                kind: scope.kind_name().into(),
                                parameters: BTreeMap::new(),
                            })
                        },
                    )
                }
                Some(DesignFeatureFamily::Sweep) => project_fixed_sweep(
                    scope,
                    construction_groups,
                    edge_operands,
                    edge_identity_operands,
                    entity_selection_operands,
                    face_operands,
                    ctx,
                )?
                .unwrap_or_else(|| {
                    FeatureDefinition::Operation(FeatureOperation::Native {
                        kind: scope.kind_name().into(),
                        parameters: BTreeMap::new(),
                    })
                }),
                Some(DesignFeatureFamily::Pipe) => native_scope_definition(scope, &parameters)?,
                Some(DesignFeatureFamily::SurfacePatch) => surface_patch::project_surface_patch(
                    ctx,
                    scope,
                    construction_groups,
                    edge_operands,
                    edge_identity_operands,
                    face_boundaries,
                    &mut projection_losses,
                )?
                .unwrap_or_else(|| {
                    FeatureDefinition::Operation(FeatureOperation::Native {
                        kind: scope.kind_name().into(),
                        parameters: BTreeMap::new(),
                    })
                }),
                Some(DesignFeatureFamily::SurfaceExtend) => {
                    if let Some((operation, distance)) =
                        scope.surface_extend_operation().and_then(|operation| {
                            cadmpeg_ir::scalar::PositiveLength::new(operation.distance.get() * 10.0)
                                .map(|distance| (operation, distance))
                        })
                    {
                        FeatureDefinition::Operation(FeatureOperation::ExtendSurface {
                            faces: cadmpeg_ir::features::FaceSelection::Native(format!(
                                "{}{}{}",
                                native_scope,
                                ":design-record#",
                                u64::from(operation.boundary_record_index)
                            )),
                            distance: Some(distance),
                            method: cadmpeg_ir::features::SurfaceExtension::Unresolved,
                        })
                    } else {
                        FeatureDefinition::Operation(FeatureOperation::Native {
                            kind: scope.kind_name().into(),
                            parameters: BTreeMap::new(),
                        })
                    }
                }
                Some(DesignFeatureFamily::SurfaceOffset) => scope
                    .surface_offset_operation()
                    .map(|operation| {
                        surface_offset::project_surface_offset(
                            scope,
                            operation,
                            construction_groups,
                            face_operands,
                            histories,
                        )
                    })
                    .transpose()?
                    .flatten()
                    .unwrap_or_else(|| {
                        FeatureDefinition::Operation(FeatureOperation::Native {
                            kind: scope.kind_name().into(),
                            parameters: BTreeMap::new(),
                        })
                    }),
                Some(DesignFeatureFamily::SurfaceRuled) => project_ruled_surface(
                    scope,
                    owners,
                    native,
                    construction_groups,
                    edge_operands,
                    edge_identity_operands,
                    ctx,
                )?
                .map_or_else(|| native_scope_definition(scope, &parameters), Ok)?,
                Some(DesignFeatureFamily::SurfaceTrim) => project_surface_trim(
                    scope,
                    construction_groups,
                    body_recipe_operands,
                    entity_selection_operands,
                    &mut projection_losses,
                )
                .unwrap_or_else(|| {
                    FeatureDefinition::Operation(FeatureOperation::Native {
                        kind: scope.kind_name().into(),
                        parameters: BTreeMap::new(),
                    })
                }),
                Some(DesignFeatureFamily::BoundaryFill) => {
                    project_boundary_fill(scope, construction_groups).unwrap_or_else(|| {
                        FeatureDefinition::Operation(FeatureOperation::Native {
                            kind: scope.kind_name().into(),
                            parameters: BTreeMap::new(),
                        })
                    })
                }
                Some(DesignFeatureFamily::Hole) => project_hole(
                    scope,
                    &parameters,
                    construction_groups,
                    face_operands,
                    &mut projection_losses,
                )?
                .map_or_else(|| native_scope_definition(scope, &parameters), Ok)?,
                Some(DesignFeatureFamily::Emboss) => project_emboss(
                    scope,
                    &parameters,
                    owners,
                    construction_groups,
                    face_operands,
                    &mut projection_losses,
                )?,
                Some(DesignFeatureFamily::Split) => project_split(
                    scope,
                    scopes,
                    construction_groups,
                    entity_selection_operands,
                    face_operands,
                )?
                .unwrap_or_else(|| {
                    FeatureDefinition::Operation(FeatureOperation::Native {
                        kind: scope.kind_name().into(),
                        parameters: BTreeMap::new(),
                    })
                }),
                Some(DesignFeatureFamily::CircularPattern) => project_circular_pattern(
                    scope,
                    construction_groups,
                    face_operands,
                    entity_selection_operands,
                    scopes,
                    &mut projection_losses,
                )
                .unwrap_or_else(|| {
                    FeatureDefinition::Operation(FeatureOperation::Pattern {
                        seeds: unprojected_pattern_seeds(
                            scope,
                            construction_groups,
                            face_operands,
                            entity_selection_operands,
                            scopes,
                        ),
                        pattern: PatternKind::UNRESOLVED_CIRCULAR,
                    })
                }),
                Some(DesignFeatureFamily::RectangularPattern) => {
                    project_rectangular_pattern_scalars(
                        scope,
                        construction_groups,
                        face_operands,
                        entity_selection_operands,
                        scopes,
                        &mut projection_losses,
                    )
                    .unwrap_or_else(|| {
                        FeatureDefinition::Operation(FeatureOperation::Pattern {
                            seeds: unprojected_pattern_seeds(
                                scope,
                                construction_groups,
                                face_operands,
                                entity_selection_operands,
                                scopes,
                            ),
                            pattern: PatternKind::UNRESOLVED_LINEAR,
                        })
                    })
                }
                Some(DesignFeatureFamily::Mirror) => mirror::project_mirror(
                    scope,
                    construction_groups,
                    face_operands,
                    entity_selection_operands,
                    scopes,
                    &mut projection_losses,
                )
                .unwrap_or_else(|| {
                    FeatureDefinition::Operation(FeatureOperation::Pattern {
                        seeds: unprojected_pattern_seeds(
                            scope,
                            construction_groups,
                            face_operands,
                            entity_selection_operands,
                            scopes,
                        ),
                        pattern: PatternKind::UNRESOLVED_MIRROR,
                    })
                }),
                Some(DesignFeatureFamily::OffsetFaces) => {
                    project_offset_faces(scope, &parameters, face_operands, construction_groups)?
                        .map_or_else(|| native_scope_definition(scope, &parameters), Ok)?
                }
                Some(DesignFeatureFamily::Move) => {
                    project_move(scope, construction_groups, owners, &mut projection_losses)
                        .map_or_else(|| native_scope_definition(scope, &parameters), Ok)?
                }
                Some(DesignFeatureFamily::Shell) => {
                    projection_losses.extend(shell_table_member_loss(scope, construction_groups));
                    let projected = project_shell(scope, face_operands, construction_groups)?;
                    if projected.is_some() && shell_has_independent_walls(scope) {
                        projection_losses.push(crate::loss::F3dLossCode::FeatureShellIndependentWallsNative.note(format!(
                            "Shell scope {} retains unequal inside and outside wall offsets in its native construction and source properties; the neutral Shell states both sides with unresolved total thickness (DR-223).",
                            scope.id
                        )));
                    }
                    projected.map_or_else(|| native_scope_definition(scope, &parameters), Ok)?
                }
                Some(DesignFeatureFamily::Thicken) => {
                    thicken::project_thicken(scope, face_operands, construction_groups, &mut projection_losses)
                        .map_or_else(|| native_scope_definition(scope, &parameters), Ok)?
                }
                Some(DesignFeatureFamily::Coil) => project_coil(
                    scope,
                    &parameters,
                    construction_groups,
                    &mut projection_losses,
                )?
                .map_or_else(|| native_scope_definition(scope, &parameters), Ok)?,
                Some(DesignFeatureFamily::Scale) => {
                    if let Some(operation) = scope.scale_operation() {
                        let factors = match operation.factors {
                            crate::records::feature::body_ops::DesignScaleFactors::Uniform {
                                factor,
                                ..
                            } => cadmpeg_ir::features::ScaleFactors::Uniform {
                                factor: cadmpeg_ir::scalar::NonZeroReal::from(factor),
                            },
                            crate::records::feature::body_ops::DesignScaleFactors::PerAxis {
                                factors,
                                ..
                            } => cadmpeg_ir::features::ScaleFactors::PerAxis {
                                factors: factors.map(cadmpeg_ir::scalar::NonZeroReal::from),
                            },
                        };
                        let scope_body_group = |record_index: u32| {
                            construction_groups.iter().find(|group| {
                                native_stream(&group.id) == Some(native_scope)
                                    && group.scope_record_index == scope.record_index
                                    && group.record_index == record_index
                            })
                        };
                        let listed = std::iter::once(operation.body_group_record_index)
                            .chain(operation.further_body_group_record_indices.iter().copied())
                            .collect::<Vec<_>>();
                        let selected = listed
                            .iter()
                            .map(|record_index| {
                                scope_body_group(*record_index).map(|group| group.id.clone())
                            })
                            .collect::<Option<Vec<_>>>();
                        let bodies = match selected.as_deref() {
                            Some([group]) => {
                                cadmpeg_ir::features::BodySelection::Native(group.clone())
                            }
                            Some([_, _, ..]) => cadmpeg_ir::features::NativeSelections::try_from(
                                selected.clone().unwrap_or_default(),
                            )
                            .map_or(
                                cadmpeg_ir::features::BodySelection::Unresolved,
                                cadmpeg_ir::features::BodySelection::NativeSet,
                            ),
                            _ => cadmpeg_ir::features::BodySelection::Unresolved,
                        };
                        if operation.matrix_offset.is_some() {
                            let unlisted = construction_groups
                                .iter()
                                .filter(|group| {
                                    native_stream(&group.id) == Some(native_scope)
                                        && group.scope_record_index == scope.record_index
                                        && group.role() == DesignOperandRole::BODIES_A
                                        && !listed.contains(&group.record_index)
                                })
                                .map(|group| group.record_index)
                                .collect::<Vec<_>>();
                            if !unlisted.is_empty() {
                                projection_losses.push(
                                    crate::loss::F3dLossCode::FeatureScaleUnlistedGroupsAssumed
                                        .note(format!(
                                            "Scale scope {} scales the bodies of its listed body groups {listed:?}; its unlisted body groups {unlisted:?} are assumed to select the same bodies (DR-464).",
                                            scope.id
                                        )),
                                );
                            }
                        }
                        let center = match operation.center_position.and_then(|center| {
                            cadmpeg_ir::features::FinitePoint3::new(Point3::new(
                                center.value[0].get() * 10.0,
                                center.value[1].get() * 10.0,
                                center.value[2].get() * 10.0,
                            ))
                        }) {
                            Some(center) => cadmpeg_ir::features::ScaleCenter::Point(center),
                            None => cadmpeg_ir::features::ScaleCenter::Native(format!(
                                "{}{}{}",
                                native_scope,
                                ":design-record#",
                                u64::from(operation.center_record_index)
                            )),
                        };
                        FeatureDefinition::Operation(FeatureOperation::Scale {
                            bodies,
                            center: Some(center),
                            factors,
                        })
                    } else {
                        FeatureDefinition::Operation(FeatureOperation::Native {
                            kind: scope.kind_name().into(),
                            parameters: BTreeMap::new(),
                        })
                    }
                }
                Some(DesignFeatureFamily::Thread) => scope
                    .thread_construction()
                    .and_then(|construction| {
                        construction
                            .nominal_size
                            .value()
                            .ok()
                            .and_then(cadmpeg_ir::scalar::PositiveLength::new)
                            .map(|size| (construction, size))
                    })
                    .map_or_else(
                        || {
                            Ok::<_, CodecError>(FeatureDefinition::Operation(
                                FeatureOperation::Unresolved {
                                    family: UnresolvedFamily::Thread,
                                },
                            ))
                        },
                        |(construction, nominal_size)| {
                            let face = project_thread_face_selection(
                                scope,
                                &construction.face_group_record_indices,
                                construction_groups,
                                face_operands,
                            );
                            let has_parameter_owners = owners.iter().any(|owner| {
                                native_stream(owner.id()) == Some(native_scope)
                                    && owner.scope_record_index() == scope.record_index
                            });
                            let face_reference_count =
                                construction.face_group_record_indices.len().checked_mul(2);
                            let full_face_extent = !has_parameter_owners
                                && Some(scope.reference_members().len()) == face_reference_count
                                && construction
                                    .face_group_record_indices
                                    .iter()
                                    .enumerate()
                                    .all(|(group_ordinal, group_record_index)| {
                                        let Some(pair_at) = group_ordinal.checked_mul(2) else {
                                            return false;
                                        };
                                        let Some(member_at) = pair_at.checked_add(1) else {
                                            return false;
                                        };
                                        let Some(member_record_index) = scope
                                            .reference_members()
                                            .values()
                                            .nth(member_at)
                                            .copied()
                                        else {
                                            return false;
                                        };
                                        scope.reference_members().values().nth(pair_at)
                                            == Some(group_record_index)
                                            && construction_groups.iter().any(|group| {
                                                native_stream(&group.id) == Some(native_scope)
                                                    && group.scope_record_index
                                                        == scope.record_index
                                                    && group.record_index == *group_record_index
                                                    && group.role() == DesignOperandRole::ROLE_0X10
                                                    && group
                                                        .members()
                                                        .iter()
                                                        .map(|member| member.value)
                                                        .eq([member_record_index])
                                            })
                                    });
                            let extent = cosmetic_thread_extent(
                                construction
                                    .extended
                                    .as_ref()
                                    .and_then(|extended| extended.full_length),
                                parameters
                                    .iter()
                                    .find(|(ordinal, _)| *ordinal == 1)
                                    .and_then(|(_, parameter)| design_positive_length(parameter)),
                                full_face_extent,
                            );
                            Ok(FeatureDefinition::Operation(
                                FeatureOperation::CosmeticThread {
                                    face,
                                    standard: Some(construction.profile.clone()),
                                    designation: Some(construction.designation.clone()),
                                    diameter: Some(nominal_size),
                                    pitch: cadmpeg_ir::scalar::PositiveLength::new(
                                        construction.pitch.get() * DESIGN_LENGTH_TO_MILLIMETRE,
                                    ),
                                    modeled: construction
                                        .extended
                                        .as_ref()
                                        .map(|extended| extended.modeled),
                                    extent,
                                },
                            ))
                        },
                    )?,
                Some(DesignFeatureFamily::SheetMetalEdgeFlange) => {
                    edge_flange::project(scope, inputs, ctx, &mut projection_losses)?
                }
                Some(DesignFeatureFamily::SheetMetalHem) => project_hem(scope, inputs, ctx)?
                    .map_or_else(|| native_scope_definition(scope, &parameters), Ok)?,
                Some(DesignFeatureFamily::SheetMetalContourFlange) => contour::project(scope, &parameters, inputs, &mut projection_losses),
                Some(DesignFeatureFamily::SheetMetalUnfold) => fold::unfold(scope, inputs, &mut projection_losses)?,
                Some(DesignFeatureFamily::SheetMetalRefold) => fold::refold(scope, inputs, &scope_ids, &source_ordinals, &mut projection_losses),
                Some(DesignFeatureFamily::SheetMetalBend) => {
                    bend_lines::project(scope, &parameters, inputs, &mut projection_losses)?
                }
                None => {
                    if let Some(definition) = super::position::project(scope, &mut projection_losses) {
                        definition
                    } else if let Some(primitive) = project_solid_primitive(
                        scope,
                        inputs.placement_constructions,
                        &mut projection_losses,
                    ) {
                        primitive
                    } else if scope.kind() == crate::records::feature::scope::DesignFeatureKind::Align {
                        project_align(scope, construction_groups, &mut projection_losses)
                            .map_or_else(|| native_scope_definition(scope, &parameters), Ok)?
                    } else if scope.kind()
                        == crate::records::feature::scope::DesignFeatureKind::JointOrigin
                    {
                        joint_origin_frame(scope).map_or_else(
                            || native_scope_definition(scope, &parameters),
                            |frame| {
                                Ok(FeatureDefinition::Operation(
                                    FeatureOperation::DatumCoordinateSystem { frame },
                                ))
                            },
                        )?
                    } else if scope.kind()
                        == crate::records::feature::scope::DesignFeatureKind::WorkPlane
                    {
                        scope.work_plane_transform().map_or_else(
                            || native_scope_definition(scope, &parameters),
                            |transform| Ok(project_work_plane(scope, transform.into())),
                        )?
                    } else if scope.kind()
                        == crate::records::feature::scope::DesignFeatureKind::WorkAxis
                    {
                        scope
                            .work_axis_construction()
                            .and_then(|construction| {
                                let displacement = Vector3::new(
                                    construction.displacement[0].get(),
                                    construction.displacement[1].get(),
                                    construction.displacement[2].get(),
                                );
                                Some((
                                    cadmpeg_ir::features::FinitePoint3::new(Point3::new(
                                        construction.origin[0].get() * 10.0,
                                        construction.origin[1].get() * 10.0,
                                        construction.origin[2].get() * 10.0,
                                    ))?,
                                    cadmpeg_ir::features::FeatureDirection3::from(
                                        cadmpeg_ir::units::UnitVector3::normalized(displacement)?,
                                    ),
                                ))
                            })
                            .map_or_else(
                                || native_scope_definition(scope, &parameters),
                                |(origin, direction)| {
                                    Ok(FeatureDefinition::Operation(FeatureOperation::DatumAxis {
                                        origin,
                                        direction,
                                    }))
                                },
                            )?
                    } else if scope.kind()
                        == crate::records::feature::scope::DesignFeatureKind::WorkPoint
                    {
                        scope
                            .work_point_construction()
                            .and_then(|construction| {
                                Some((
                                    construction,
                                    cadmpeg_ir::features::FinitePoint3::new(Point3::new(
                                        construction.position[0].get() * 10.0,
                                        construction.position[1].get() * 10.0,
                                        construction.position[2].get() * 10.0,
                                    ))?,
                                ))
                            })
                            .map_or_else(
                                || native_scope_definition(scope, &parameters),
                                |(construction, position)| {
                                    Ok(FeatureDefinition::Operation(FeatureOperation::DatumPoint {
                                        position,
                                        construction: project_work_point_construction(
                                            scope,
                                            construction,
                                            &parameters,
                                            edge_operands,
                                            &scope_ids,
                                        )
                                        .map(Box::new),
                                    }))
                                },
                            )?
                    } else if scope.kind()
                        == crate::records::feature::scope::DesignFeatureKind::BaseFlange
                    {
                        project_base_flange(
                            scope,
                            construction_groups,
                            placements,
                            sheet_metal_settings,
                        )
                        .map_or_else(
                            || {
                                FeatureDefinition::Operation(FeatureOperation::Native {
                                    kind: scope.kind_name().into(),
                                    parameters: BTreeMap::new(),
                                })
                            },
                            |(definition, ruled)| {
                                if !ruled {
                                    unruled_flange_thicknesses
                                        .set(unruled_flange_thicknesses.get() + 1);
                                }
                                definition
                            },
                        )
                    } else if scope.kind()
                        == crate::records::feature::scope::DesignFeatureKind::Modify
                    {
                        project_modify(scope, construction_groups, &mut projection_losses)
                            .unwrap_or_else(|| {
                                FeatureDefinition::Operation(FeatureOperation::Native {
                                    kind: scope.kind_name().into(),
                                    parameters: BTreeMap::new(),
                                })
                            })
                    } else if scope.kind()
                        == crate::records::feature::scope::DesignFeatureKind::RemoveBody
                    {
                        project_remove_body(scope, construction_groups).unwrap_or_else(|| {
                            FeatureDefinition::Operation(FeatureOperation::Native {
                                kind: scope.kind_name().into(),
                                parameters: BTreeMap::new(),
                            })
                        })
                    } else if scope.kind()
                        == crate::records::feature::scope::DesignFeatureKind::ReverseNormal
                    {
                        reverse_normal::project_reverse_normal(
                            scope,
                            construction_groups,
                            design_types,
                            &mut projection_losses,
                        )
                        .unwrap_or_else(|| {
                            FeatureDefinition::Operation(FeatureOperation::Native {
                                kind: scope.kind_name().into(),
                                parameters: BTreeMap::new(),
                            })
                        })
                    } else if scope.kind()
                        == crate::records::feature::scope::DesignFeatureKind::SurfaceStitch
                    {
                        project_surface_stitch(scope, construction_groups).unwrap_or_else(|| {
                            FeatureDefinition::Operation(FeatureOperation::Native {
                                kind: scope.kind_name().into(),
                                parameters: BTreeMap::new(),
                            })
                        })
                    } else if scope.kind()
                        == crate::records::feature::scope::DesignFeatureKind::SplitFace
                    {
                        project_split_face(
                            scope,
                            scopes,
                            construction_groups,
                            entity_selection_operands,
                            face_operands,
                            histories,
                        )?
                        .unwrap_or_else(|| {
                            FeatureDefinition::Operation(FeatureOperation::Native {
                                kind: scope.kind_name().into(),
                                parameters: BTreeMap::new(),
                            })
                        })
                    } else if matches!(scope.kind(), crate::records::feature::scope::DesignFeatureKind::GenerateFaceGroups | crate::records::feature::scope::DesignFeatureKind::CombineFaceGroups) {
                        mesh_face_groups::project(scope, &parameters, design_types, &mut projection_losses)
                    } else if scope.kind()
                        == crate::records::feature::scope::DesignFeatureKind::PathPattern
                    {
                        path_pattern::path_pattern_definition(
                            scope,
                            inputs,
                            &parameters,
                            &source_ordinals,
                            &mut projection_losses,
                        )?
                    } else if scope.kind()
                        == crate::records::feature::scope::DesignFeatureKind::MoveFace
                    {
                        project_move_face(scope, construction_groups, &mut projection_losses)?.unwrap_or_else(|| {
                            FeatureDefinition::Operation(FeatureOperation::Native {
                                kind: scope.kind_name().into(),
                                parameters: BTreeMap::new(),
                            })
                        })
                    } else if matches!(
                        scope.kind(),
                        crate::records::feature::scope::DesignFeatureKind::DeleteFace
                            | crate::records::feature::scope::DesignFeatureKind::SurfaceDeleteFace
                    ) {
                        delete_face::project_delete_face(
                            scope,
                            construction_groups,
                            face_operands,
                            &mut projection_losses,
                        )?
                        .unwrap_or_else(|| {
                            FeatureDefinition::Operation(FeatureOperation::Native {
                                kind: scope.kind_name().into(),
                                parameters: BTreeMap::new(),
                            })
                        })
                    } else if scope.kind()
                        == crate::records::feature::scope::DesignFeatureKind::SurfaceUntrim
                    {
                        surface_untrim::project_surface_untrim(
                            scope,
                            construction_groups,
                            face_operands,
                            body_recipe_operands,
                            &mut projection_losses,
                        )
                    } else if scope.kind()
                        == crate::records::feature::scope::DesignFeatureKind::UnStitch
                    {
                        unstitch::project_unstitch(
                            scope,
                            construction_groups,
                            face_operands,
                            &mut projection_losses,
                        )
                    } else if scope.kind()
                        == crate::records::feature::scope::DesignFeatureKind::CopyPasteBodies
                    {
                        if let Some(operation) = scope.copy_paste_bodies_operation() {
                            let selection = design_body_selection(
                                scope,
                                operation
                                    .bodies()
                                    .iter()
                                    .map(|body| u64::from(body.copied.value)),
                                body_bindings,
                            );
                            let bodies = match selection {
                                cadmpeg_ir::features::BodySelection::Resolved {
                                    bodies,
                                    native,
                                } => {
                                    inserted_bodies.extend_from_slice(bodies.as_slice());
                                    cadmpeg_ir::features::InsertedBodies::Resolved { native }
                                }
                                _ => cadmpeg_ir::features::InsertedBodies::Native(scope.id.clone()),
                            };
                            FeatureDefinition::Operation(FeatureOperation::InsertBodies { bodies })
                        } else {
                            FeatureDefinition::Operation(FeatureOperation::Native {
                                kind: scope.kind_name().into(),
                                parameters: BTreeMap::new(),
                            })
                        }
                    } else if scope.kind()
                        == crate::records::feature::scope::DesignFeatureKind::CopyPaste
                    {
                        scope.copy_paste_component_operation().map_or_else(
                            || {
                                FeatureDefinition::Operation(FeatureOperation::Native {
                                    kind: scope.kind_name().into(),
                                    parameters: BTreeMap::new(),
                                })
                            },
                            |operation| {
                                FeatureDefinition::Operation(FeatureOperation::InsertComponent {
                                    occurrence: crate::ids::neutral_component_occurrence_id(
                                        &operation.copied_occurrence_guid,
                                    ),
                                })
                            },
                        )
                    } else if scope.kind() == crate::records::feature::scope::DesignFeatureKind::Form {
                        form_cages::unresolved(scope, &parameters)?
                    } else if scope.kind()
                        == crate::records::feature::scope::DesignFeatureKind::BaseFeature
                    {
                        if let Some((definition, copies)) =
                            scope.base_feature_construction().and_then(|construction| {
                                paste_copy::same_component_paste_copy(
                                    scope,
                                    construction,
                                    body_bindings,
                                )
                            })
                        {
                            inserted_bodies.extend(copies);
                            definition
                        } else if let Some(construction) = scope.base_feature_construction() {
                            if let Some(offset) = construction.null_reference_flag_offset() {
                                projection_losses.push(crate::loss::F3dLossCode::FeatureBaseFeatureNullFlagAssumed.note(format!(
                                    "Base Feature {} stores flag 1 at Design offset {offset} in its marked null reference; its result runs are read as the ordinary result set (DR-241).",
                                    scope.id
                                )));
                            }
                            if matches!(
                                construction,
                                crate::records::feature::base_feature::DesignBaseFeatureConstruction::SuffixResultBodies { .. }
                            ) && construction.body_entity_suffixes().len() > 1
                            {
                                projection_losses.push(crate::loss::F3dLossCode::FeatureBaseFeatureSuffixRunOrderAssumed.note(format!(
                                    "Base Feature {} stores {} bodies in a suffix result-body frame; its body, repeated, and result entries are read as three consecutive runs (DR-646).",
                                    scope.id,
                                    construction.body_entity_suffixes().len()
                                )));
                            }
                            FeatureDefinition::Operation(FeatureOperation::BaseFeature {
                                bodies: design_body_selection(
                                    scope,
                                    construction.body_entity_suffixes(),
                                    body_bindings,
                                ),
                            })
                        } else {
                            FeatureDefinition::Operation(FeatureOperation::Native {
                                kind: scope.kind_name().into(),
                                parameters: BTreeMap::new(),
                            })
                        }
                    } else {
                        native_scope_definition(scope, &parameters)?
                    }
                }
            };
            let extended_joint_origin = scope.joint_origin_frame().is_some_and(|origin| {
                extended_assembly_frames.get(&(
                    scope.record_index, origin.joint_origin_transform_offset,
                )).is_some_and(|transform| *transform == origin.joint_origin_transform)
            });
            let outputs = inserted_bodies;
            Ok(Feature {
                product_definition: None,
                id: scope_ids[&(native_scope, scope.record_index)].clone(),
                ordinal: source_ordinals[&(native_scope, scope.record_index)],
                name: Some(
                    scope
                        .display_name
                        .as_deref()
                        .or_else(|| scope.label.as_ref().map(|l| l.value.as_str()))
                        .map_or_else(
                            || format!("{} {}", scope.source_kind_name(), scope.feature_ordinal),
                            str::to_owned,
                        ),
                ),
                suppressed: Some(
                    family.is_some_and(stateless_scope_is_suppressed)
                        && scope.history_state_id().is_none()
                        && scope.previous_history_state_id().is_none(),
                ),
                dependencies: cadmpeg_ir::features::DistinctMembers::default(),
                source_properties: if matches!(
                    &definition,
                    FeatureDefinition::Operation(FeatureOperation::Native { .. } | FeatureOperation::Form(_) | FeatureOperation::MeshGenerateFaceGroups { .. } | FeatureOperation::MeshCombineFaceGroups { .. })
                ) || matches!(&definition, FeatureDefinition::Operation(FeatureOperation::Pattern { pattern, .. }) if matches!(pattern.definition(), cadmpeg_ir::features::patterns::PatternTransform::CurveDrivenPartial { .. }))
                    || matches!(family, Some(DesignFeatureFamily::Hole))
                    || matches!(family, Some(DesignFeatureFamily::Emboss))
                    || retain_chamfer_records
                    || shell_has_independent_walls(scope)
                    || extended_joint_origin
                {
                    let mut properties = scope_properties(scope, native_scope, placements, &parameters)?;
                    if retain_chamfer_records {
                        properties.insert(
                            cadmpeg_core::nonblank_literal!("chamfer:parameters"),
                            serde_json::to_string(&chamfer::retained_parameters(scope, inputs))
                                .map_err(CodecError::malformed)?,
                        );
                        if let Some(sets) = scope.chamfer_sets() {
                            properties.insert(
                                cadmpeg_core::nonblank_literal!("chamfer:sets"),
                                serde_json::to_string(sets).map_err(CodecError::malformed)?,
                            );
                        }
                    }

                    if shell_has_independent_walls(scope)
                        && matches!(&definition, FeatureDefinition::Operation(FeatureOperation::Shell { .. }))
                    {
                        properties.insert(
                            cadmpeg_core::nonblank_literal!("shell:parameters"),
                            serde_json::to_string(&scope_parameter_expressions(scope, &parameters)?)
                                .map_err(CodecError::malformed)?,
                        );
                        if let crate::records::feature::scope::DesignScopePayload::Shell(Some(operation)) = scope.payload() {
                            properties.insert(
                                cadmpeg_core::nonblank_literal!("shell:construction"),
                                serde_json::to_string(operation).map_err(CodecError::malformed)?,
                            );
                        }
                    }
                    properties
                } else {
                    BTreeMap::new()
                },
                source_tag: Some(scope.source_kind_name().to_owned()),
                source_text: None,
                source_content: cadmpeg_ir::features::FeatureContent::default(),

                evaluation: cadmpeg_ir::features::FeatureEvaluation::new(
                    definition,
                    cadmpeg_ir::features::DistinctMembers::try_from(outputs)
                        .map_err(cadmpeg_core::CodecError::from)?,
                ),
                native_ref: Some(scope.id.clone()),
            })
        })
        .try_fold(Vec::new(), |mut features, feature| {
            features.push(feature?);
            Ok::<_, CodecError>(features)
        })?;
    let scope_history = ScopeHistoryGraph::new(
        scopes,
        body_bindings,
        body_recipe_operands,
        component_naming_spaces,
        histories,
    );
    for feature in &mut features {
        let Some(scope) = feature
            .native_ref
            .as_deref()
            .and_then(|native_ref| scopes.iter().find(|scope| scope.id == native_ref))
        else {
            continue;
        };
        let ScopeHistoryPredecessor::Scope(predecessor_scope) =
            scope_history.predecessor(scope, |candidate| {
                let stream = native_stream(&candidate.id).unwrap_or(ids::DEFAULT_STREAM);
                scope_ids.contains_key(&(stream, candidate.record_index))
            })?
        else {
            continue;
        };
        let stream = native_stream(&predecessor_scope.id).unwrap_or(ids::DEFAULT_STREAM);
        let Some(predecessor) = scope_ids.get(&(stream, predecessor_scope.record_index)) else {
            continue;
        };
        if predecessor != &feature.id {
            feature.dependencies.insert(predecessor.clone());
        }
    }
    for feature in &mut features {
        let FeatureDefinition::Operation(FeatureOperation::Pattern { seeds, .. }) =
            feature.evaluation.definition()
        else {
            continue;
        };
        for dependency in seeds.iter().filter_map(|seed| match seed {
            cadmpeg_ir::features::patterns::PatternSeed::Feature(feature) => Some(feature),
            _ => None,
        }) {
            if dependency != &feature.id {
                feature.dependencies.insert(dependency.clone());
            }
        }
    }
    for feature in &mut features {
        let feature_id = &feature.id;
        let dependencies = &mut feature.dependencies;
        let mut add_dependency = |dependency: &cadmpeg_ir::features::FeatureId| {
            if dependency != feature_id {
                dependencies.insert(dependency.clone());
            }
            Ok::<_, CodecError>(())
        };
        match feature.evaluation.definition() {
            FeatureDefinition::Operation(FeatureOperation::SheetMetalRefold { construction }) => {
                if let Some(source) = &construction.unfold {
                    add_dependency(source)?;
                }
            }
            FeatureDefinition::Operation(FeatureOperation::Draft { anchor, .. }) => {
                if let Some(plane) = anchor.pull().and_then(|pull| pull.plane.as_ref()) {
                    add_dependency(plane)?;
                }
            }
            FeatureDefinition::Operation(
                FeatureOperation::SplitFace {
                    tool: cadmpeg_ir::features::SplitFaceTool::Plane { plane },
                    ..
                }
                | FeatureOperation::SplitBody {
                    tool: cadmpeg_ir::features::SplitBodyTool::Plane { plane },
                    ..
                },
            ) => add_dependency(plane)?,
            FeatureDefinition::Operation(FeatureOperation::SplitFace {
                tool: cadmpeg_ir::features::SplitFaceTool::Planes { planes },
                ..
            }) => {
                for plane in planes {
                    add_dependency(plane)?;
                }
            }
            FeatureDefinition::Operation(FeatureOperation::DatumPoint {
                construction: Some(construction),
                ..
            }) => {
                use cadmpeg_ir::features::{
                    DatumPlaneReference, DatumPointConstruction, VertexSelection,
                };
                match construction.as_ref() {
                    DatumPointConstruction::ThreePlaneIntersection { planes } => {
                        for plane in planes.iter() {
                            if let DatumPlaneReference::Feature { feature } = plane {
                                add_dependency(feature)?;
                            }
                        }
                    }
                    DatumPointConstruction::EdgePlaneIntersection {
                        plane: DatumPlaneReference::Feature { feature },
                        ..
                    } => add_dependency(feature)?,
                    DatumPointConstruction::Vertex {
                        vertex: VertexSelection::Generated { vertex, .. },
                    } => add_dependency(&vertex.feature)?,
                    _ => {}
                }
            }
            FeatureDefinition::Operation(FeatureOperation::DatumThreePointPlane {
                points, ..
            }) => {
                for point in points.iter() {
                    if let cadmpeg_ir::features::VertexSelection::Generated { vertex, .. } = point {
                        add_dependency(&vertex.feature)?;
                    }
                }
            }
            _ => {}
        }
    }
    let mut history_state_features = HashMap::<
        (String, ComponentHistoryNamespace, String, i64),
        Option<cadmpeg_ir::features::FeatureId>,
    >::new();
    {
        for scope in scopes {
            let Some(state_id) = scope.history_state_id() else {
                continue;
            };
            let Some(key) = scope_history.state_key(scope, state_id) else {
                continue;
            };
            let stream = native_stream(&scope.id).unwrap_or(ids::DEFAULT_STREAM);
            let Some(feature_id) = scope_ids.get(&(stream, scope.record_index)) else {
                continue;
            };
            if let Some(candidate) = history_state_features.get_mut(&key) {
                *candidate = None;
            } else {
                let id = feature_id.clone();
                // discarded-value: the vacant-key check admits this state.
                let _ = history_state_features.insert(key, Some(id));
            }
        }
    }
    for feature in &mut features {
        let Some(scope) = feature
            .native_ref
            .as_deref()
            .and_then(|native_ref| scopes.iter().find(|scope| scope.id == native_ref))
        else {
            continue;
        };
        let Some(construction) = scope.work_point_construction() else {
            continue;
        };
        for state_id in construction
            .rule
            .inputs()
            .iter()
            .filter_map(|input| work_point_input_history_state_id(scope, input, edge_operands))
        {
            let Some(key) = scope_history.state_key(scope, state_id) else {
                continue;
            };
            let Some(Some(dependency)) = history_state_features.get(&key) else {
                continue;
            };
            if dependency != &feature.id {
                feature.dependencies.insert(dependency.clone());
            }
        }
    }
    features.sort_by(|a, b| a.id.cmp(&b.id));

    let mut parameters = native
        .iter()
        .map(|parameter| -> NeutralParameter {
            let stream = native_stream(&parameter.id).unwrap_or(ids::DEFAULT_STREAM);
            let native_owner = parameter
                .owner_record_index()
                .and_then(|record_index| owners_by_index.get(&(stream, record_index)));
            let owner =
                native_owner.and_then(|owner| scope_ids.get(&(stream, owner.scope_record_index())));
            let mut properties = BTreeMap::new();
            if parameter.kind() != DesignParameterKind::User {
                properties.insert(
                    cadmpeg_core::nonblank_literal!("source_kind"),
                    parameter.source_kind_str().to_owned(),
                );
            }
            if let (Some(owner_record_index), None) = (parameter.owner_record_index(), owner) {
                properties.insert(
                    cadmpeg_core::nonblank_literal!("owner_record_index"),
                    format!("{owner_record_index}"),
                );
            }
            if let Some(comment) = parameter.comment() {
                properties.insert(
                    cadmpeg_core::nonblank_literal!("comment"),
                    comment.to_owned(),
                );
            }
            let value = match parameter.unit().map(|field| field.value.as_str()) {
                _ if parameter.text_value().is_some() => parameter
                    .text_value()
                    .map(|text| ParameterValue::String(text.to_owned())),
                Some(unit) if design_length_unit(unit) => {
                    Length::new(parameter.evaluated_value().get() * 10.0)
                        .map(ParameterValue::Length)
                }
                Some(unit) if design_angle_unit(unit) => {
                    Angle::new(parameter.evaluated_value().get()).map(ParameterValue::Angle)
                }
                None => Some(ParameterValue::Real(parameter.evaluated_value())),
                Some(unit) => {
                    properties.insert(cadmpeg_core::nonblank_literal!("unit"), unit.to_owned());
                    properties.insert(
                        cadmpeg_core::nonblank_literal!("evaluated_scalar"),
                        format!("{}", parameter.evaluated_value().get()),
                    );
                    None
                }
            };
            NeutralParameter {
                product_definition: None,
                id: crate::ids::neutral_parameter_id(parameter),
                owner: owner.cloned(),
                ordinal: owner
                    .zip(native_owner)
                    .map_or(parameter.source_ordinal, |(_, owner)| owner.local_ordinal()),
                name: parameter.name().to_owned(),
                expression: parameter.expression().to_owned(),
                display: if parameter.source_kind_str().contains("Diameter Dimension") {
                    Some(DimensionDisplay::Diameter)
                } else if parameter.source_kind_str().contains("Radius Dimension") {
                    Some(DimensionDisplay::Radius)
                } else {
                    None
                },
                value,
                dependencies: cadmpeg_ir::features::DistinctMembers::default(),
                properties,
                pmi: None,
                native_ref: Some(parameter.id.clone()),
            }
        })
        .collect::<Vec<_>>();
    let (
        parameter_scopes,
        document_aliases,
        feature_aliases,
        owned_aliases,
        parameter_owners,
        feature_order,
    ) = {
        let mut parameter_scopes = HashMap::new();
        for (source, parameter) in native.iter().zip(&parameters) {
            if let Some(stream) = native_stream(&source.id) {
                let id = parameter.id.clone();
                // discarded-value: duplicate parameter IDs keep the last source stream.
                let _ = parameter_scopes.insert(id, stream);
            }
        }
        let mut document_aliases = HashMap::<(&str, String), Option<ParameterId>>::new();
        let mut feature_aliases =
            HashMap::<(&str, cadmpeg_ir::features::FeatureId, String), Option<ParameterId>>::new();
        let mut owned_aliases = HashMap::<(&str, String), Vec<ParameterId>>::new();
        for parameter in &parameters {
            let scope = parameter_scopes[&parameter.id];
            if let Some(owner) = &parameter.owner {
                let key = (scope, owner.clone(), parameter.name.clone());
                if let Some(candidate) = feature_aliases.get_mut(&key) {
                    *candidate = None;
                } else {
                    let value = parameter.id.clone();
                    // discarded-value: the vacant-key check admits this alias.
                    let _ = feature_aliases.insert(key, Some(value));
                }
                let key = (scope, parameter.name.clone());
                owned_aliases
                    .entry(key)
                    .or_default()
                    .push(parameter.id.clone());
            } else {
                let key = (scope, parameter.name.clone());
                if let Some(candidate) = document_aliases.get_mut(&key) {
                    *candidate = None;
                } else {
                    let value = parameter.id.clone();
                    // discarded-value: the vacant-key check admits this alias.
                    let _ = document_aliases.insert(key, Some(value));
                }
            }
        }
        let mut parameter_owners = HashMap::new();
        for parameter in &parameters {
            let key = parameter.id.clone();
            let owner = parameter.owner.clone();
            // discarded-value: duplicate parameter IDs keep the last owner.
            let _ = parameter_owners.insert(key, owner);
        }
        let mut feature_order = HashMap::new();
        for feature in &features {
            let id = feature.id.clone();
            // discarded-value: duplicate feature IDs keep the last authored ordinal.
            let _ = feature_order.insert(id, feature.ordinal);
        }
        (
            parameter_scopes,
            document_aliases,
            feature_aliases,
            owned_aliases,
            parameter_owners,
            feature_order,
        )
    };
    for parameter in &mut parameters {
        let scope = parameter_scopes[&parameter.id];
        if parameter.properties.contains_key("owner_record_index") {
            continue;
        }
        parameter.dependencies.clear();
        for identifier in expression_identifiers(&parameter.expression) {
            let identifier = identifier.to_string();
            let alias_key = (scope, identifier);
            let preceding_owned = || {
                let consumer = parameter.owner.as_ref()?;
                let consumer_order = feature_order.get(consumer)?;
                let mut candidates = owned_aliases.get(&alias_key)?.iter().filter(|candidate| {
                    parameter_owners
                        .get(*candidate)
                        .and_then(Option::as_ref)
                        .and_then(|owner| feature_order.get(owner))
                        .is_some_and(|order| order < consumer_order)
                });
                let candidate = candidates.next()?;
                candidates.next().is_none().then_some(candidate)
            };
            let candidate = if let Some(owner) = &parameter.owner {
                let owner_key = owner.clone();
                let feature_identifier = alias_key.1.clone();
                match feature_aliases.get(&(scope, owner_key, feature_identifier)) {
                    Some(None) => None,
                    Some(Some(local)) => Some(local),
                    None => match document_aliases.get(&alias_key) {
                        Some(Some(document)) => Some(document),
                        Some(None) => None,
                        None => preceding_owned(),
                    },
                }
            } else {
                document_aliases.get(&alias_key).and_then(Option::as_ref)
            };
            let Some(candidate) = candidate else {
                continue;
            };
            let dependency_owner = parameter_owners.get(candidate);
            let allowed = match (dependency_owner, &parameter.owner) {
                (Some(Some(dependency_owner)), Some(consumer_owner))
                    if dependency_owner != consumer_owner =>
                {
                    feature_order
                        .get(dependency_owner)
                        .zip(feature_order.get(consumer_owner))
                        .is_some_and(|(dependency, consumer)| dependency < consumer)
                }
                (Some(Some(_)), None) => false,
                (Some(_), _) => true,
                (None, _) => false,
            };
            if !allowed || candidate == &parameter.id || parameter.dependencies.contains(candidate)
            {
                continue;
            }
            let dependency = candidate.clone();
            parameter.dependencies.insert(dependency);
        }
    }
    let unordered_parameter_scopes =
        normalize_parameter_ordinals(&mut parameters, &parameter_owners)?;
    for feature in &mut features {
        for parameter in parameters
            .iter()
            .filter(|parameter| parameter.owner.as_ref() == Some(&feature.id))
        {
            for dependency in &parameter.dependencies {
                if let Some(Some(owner)) = parameter_owners.get(dependency) {
                    if owner != &feature.id {
                        feature.dependencies.insert(owner.clone());
                    }
                }
            }
        }
    }
    ensure_feature_dependencies_precede(&features)?;
    parameters.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(ProjectedParameterDesign {
        features,
        parameters,
        unordered_parameter_scopes,
        unruled_flange_thicknesses: unruled_flange_thicknesses.get(),
        projection_losses,
    })
}

pub(crate) fn work_plane_recipe_state_id(scope: &DesignParameterScope) -> Option<i64> {
    scope.work_plane_construction()?.inputs()[0]
        .resolution
        .map(|resolution| resolution.state_id)
}

/// Millimetres per Design length unit (centimetre).
const DESIGN_LENGTH_TO_MM: f64 = 10.0;

/// The millimetre datum frame of a solved `WorkPlane` matrix: column 0 is the
/// u-axis, column 2 the normal, and column 3 the origin in centimetres.
pub(crate) fn work_plane_datum_frame(
    transform: [[f64; 4]; 4],
) -> Option<cadmpeg_ir::features::FeatureDatumPlaneFrame> {
    let (origin, normal, u_axis) = work_plane_frame_columns(transform);
    cadmpeg_ir::features::FeatureDatumPlaneFrame::new(origin, normal, u_axis)
}

/// The same `WorkPlane` frame as a feature support plane.
fn work_plane_support_frame(
    transform: [[f64; 4]; 4],
) -> Option<cadmpeg_ir::features::FeatureSupportPlaneFrame> {
    let (origin, normal, u_axis) = work_plane_frame_columns(transform);
    cadmpeg_ir::features::FeatureSupportPlaneFrame::new(origin, normal, u_axis)
}

fn work_plane_frame_columns(transform: [[f64; 4]; 4]) -> (Point3, Vector3, Vector3) {
    (
        Point3::new(
            transform[0][3] * DESIGN_LENGTH_TO_MM,
            transform[1][3] * DESIGN_LENGTH_TO_MM,
            transform[2][3] * DESIGN_LENGTH_TO_MM,
        ),
        Vector3::new(transform[0][2], transform[1][2], transform[2][2]),
        Vector3::new(transform[0][0], transform[1][0], transform[2][0]),
    )
}

fn native_scope_definition(
    scope: &DesignParameterScope,
    parameters: &[(u32, &DesignParameter)],
) -> Result<cadmpeg_ir::features::FeatureDefinition, CodecError> {
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};
    Ok(FeatureDefinition::Operation(FeatureOperation::Native {
        kind: scope.kind_name().to_owned().into(),
        parameters: scope_parameter_expressions(scope, parameters)?,
    }))
}

fn project_thread_face_selection(
    scope: &DesignParameterScope,
    face_group_record_indices: &[u32],
    groups: &[DesignConstructionOperandGroup],
    face_operands: &[DesignFaceOperand],
) -> cadmpeg_ir::features::FaceSelection {
    use cadmpeg_ir::features::FaceSelection;

    let Some(stream) = native_stream(&scope.id) else {
        return FaceSelection::Unresolved;
    };
    if face_group_record_indices.is_empty() {
        return FaceSelection::Unresolved;
    }
    let mut ordered_groups = Vec::new();
    for record_index in face_group_record_indices {
        let mut matching = groups.iter().filter(|group| {
            native_stream(&group.id) == Some(stream)
                && group.scope_record_index == scope.record_index
                && group.record_index == *record_index
                && group.role() == DesignOperandRole::ROLE_0X10
        });
        let Some(group) = matching.next() else {
            return FaceSelection::Unresolved;
        };
        if matching.next().is_some() {
            return FaceSelection::Unresolved;
        }
        ordered_groups.push(group);
    }
    let native_source = if let [group] = ordered_groups.as_slice() {
        group.id.as_str()
    } else {
        scope.id.as_str()
    };
    let native = native_source.to_owned();
    let mut state = None;
    let mut faces = Vec::new();
    for group in ordered_groups {
        let Some(FaceSelection::Historical {
            state: group_state,
            faces: group_faces,
            ..
        }) = resolved_historical_face_group(
            scope,
            scope.previous_history_state_id(),
            group,
            face_operands,
        )
        else {
            return FaceSelection::Native(native);
        };
        if state
            .as_ref()
            .is_some_and(|expected| expected != &group_state)
        {
            return FaceSelection::Native(native);
        }
        state.get_or_insert(group_state);
        for face in group_faces.as_slice() {
            if !faces.contains(face) {
                faces.push(face.clone());
            }
        }
    }
    let Some(state) = state else {
        return FaceSelection::Native(native);
    };

    let historical_native = native.clone();

    cadmpeg_ir::features::FaceSelection::historical(state, faces, historical_native)
        .unwrap_or(FaceSelection::Native(native))
}

fn design_body_selection(
    scope: &DesignParameterScope,
    entity_suffixes: impl ExactSizeIterator<Item = u64>,
    body_bindings: &[DesignBodyBinding],
) -> cadmpeg_ir::features::BodySelection {
    use cadmpeg_ir::features::BodySelection;

    let stream = native_stream(&scope.id).unwrap_or(ids::DEFAULT_STREAM);
    let expected_count = entity_suffixes.len();
    let mut bodies = Vec::new();
    for suffix in entity_suffixes {
        let mut matches = body_bindings.iter().filter(|binding| {
            native_stream(binding.id()) == Some(stream) && binding.entity_suffix == suffix
        });
        let Some(body) = matches.next().and_then(|binding| binding.body.as_ref()) else {
            return BodySelection::Native(scope.id.clone());
        };
        if matches.any(|candidate| candidate.body.as_ref() != Some(body)) {
            return BodySelection::Native(scope.id.clone());
        }
        bodies.push(body.clone());
    }
    if bodies.len() == expected_count {
        if let Ok(bodies) = cadmpeg_ir::features::DistinctMembers::try_from(bodies) {
            return BodySelection::Resolved {
                bodies,
                native: scope.id.clone(),
            };
        }
    }
    BodySelection::Native(scope.id.clone())
}

/// Native and neutral records that solve a Loft point section.
pub(crate) struct LoftPointSources<'a> {
    pub(crate) groups: &'a [DesignConstructionOperandGroup],
    pub(crate) entity_operands:
        &'a [crate::records::topology::entity_selection::DesignEntitySelectionOperand],
    pub(crate) sketch_points: &'a [crate::records::sketch_geometry::SketchPoint],
    pub(crate) sketches: &'a [cadmpeg_ir::sketches::Sketch],
    pub(crate) sketch_entities: &'a [cadmpeg_ir::sketches::SketchEntity],
}

/// Derive the neutral material-side flag from F3D's signed Draft angle.
///
/// F3D does not store a second outward bit. Keeping this rule in one helper
/// makes every Draft projection branch use the same convention.
const fn draft_outward(angle: f64) -> bool {
    angle < 0.0
}

/// The neutral extent of a Thread: the full-length flag of an extended
/// standard thread, else its length owner, else a selection of whole faces.
fn cosmetic_thread_extent(
    full_length: Option<bool>,
    length: Option<cadmpeg_ir::scalar::PositiveLength>,
    full_face: bool,
) -> Option<cadmpeg_ir::features::CosmeticThreadExtent> {
    use cadmpeg_ir::features::CosmeticThreadExtent;

    if full_length == Some(true) {
        return Some(CosmeticThreadExtent::Through {});
    }
    length
        .map(|length| CosmeticThreadExtent::Blind { length })
        .or_else(|| full_face.then_some(CosmeticThreadExtent::Through {}))
}

fn project_face_selection(
    scope: &DesignParameterScope,
    group: &DesignConstructionOperandGroup,
    face_operands: &[DesignFaceOperand],
    histories: &[crate::history_records::AsmHistory],
) -> Result<cadmpeg_ir::features::FaceSelection, CodecError> {
    let historical = if let Some(previous_state_id) =
        crate::history::effective_scope_previous_history_state_id(scope, histories)
    {
        let updated_face_slots = scope
            .history_state_id()
            .and_then(|state_id| {
                crate::history::unique_history_state_pair(histories, state_id, previous_state_id)
            })
            .and_then(|(_, state, _)| state.transition.as_ref())
            .map_or(&[][..], |transition| {
                transition.topology.faces.updated.as_slice()
            });
        if let Some(selection) =
            resolved_historical_face_group(scope, Some(previous_state_id), group, face_operands)
        {
            Some(selection)
        } else {
            resolved_historical_split_face_target_group_with_updated_faces(
                scope,
                Some(previous_state_id),
                group,
                face_operands,
                updated_face_slots,
            )
        }
    } else {
        None
    };
    if let Some(selection) = historical {
        return Ok(selection);
    }
    match resolved_face_group(group, face_operands)? {
        Some(selection) => Ok(selection),
        None => Ok(cadmpeg_ir::features::FaceSelection::Native(
            group.id.clone(),
        )),
    }
}

fn selected_work_planes<'a>(
    scope: &DesignParameterScope,
    group: &DesignConstructionOperandGroup,
    entity_selection_operands: &[crate::records::topology::entity_selection::DesignEntitySelectionOperand],
    scopes: &'a [DesignParameterScope],
) -> Option<Vec<&'a DesignParameterScope>> {
    let stream = native_stream(&scope.id)?;
    if group.members().is_empty() {
        return None;
    }
    let mut planes = Vec::new();
    let mut target_record_indices = HashSet::new();
    for (ordinal, member) in group
        .members()
        .iter()
        .map(|member| &member.value)
        .enumerate()
    {
        let ordinal = u32::try_from(ordinal).ok()?;
        let selection = unique_feature_match(entity_selection_operands.iter().filter(|operand| {
            native_stream(&operand.id) == Some(stream)
                && operand.scope_record_index == scope.record_index
                && operand.group_record_index == group.record_index
                && operand.group_member_ordinal == ordinal
                && operand.record_index() == *member
        }));
        let selection = selection?;
        if selection.secondary().is_some() {
            return None;
        }
        let target_record_index = u32::try_from(selection.primary_identity)
            .ok()?
            .checked_add(1)?;
        if !target_record_indices.insert(target_record_index) {
            return None;
        }
        let target = unique_feature_match(scopes.iter().filter(|candidate| {
            native_stream(&candidate.id) == Some(stream)
                && candidate.record_index == target_record_index
                && candidate.kind() == crate::records::feature::scope::DesignFeatureKind::WorkPlane
                && candidate.work_plane_transform().is_some()
        }));
        let target = target?;
        planes.push(target);
    }
    Some(planes)
}

/// Return the unique non-empty construction operand group in `scope` carrying
/// `role`. Yields `None` unless exactly one such group exists.
fn single_operand_group<'a>(
    groups: &'a [DesignConstructionOperandGroup],
    scope: &DesignParameterScope,
    role: DesignOperandRole,
) -> Option<&'a DesignConstructionOperandGroup> {
    let mut matching = groups.iter().filter(|group| {
        native_stream(&group.id) == native_stream(&scope.id)
            && group.scope_record_index == scope.record_index
            && group.role() == role
            && !group.members().is_empty()
    });
    let group = matching.next()?;
    matching.next().is_none().then_some(group)
}

pub(super) fn shell_table_member_loss(
    scope: &DesignParameterScope,
    groups: &[DesignConstructionOperandGroup],
) -> Option<cadmpeg_ir::report::loss::LossNote> {
    let crate::records::feature::scope::DesignScopePayload::Shell(Some(operation)) =
        &scope.payload()
    else {
        return None;
    };
    if operation.wall_slots.is_some() {
        return None;
    }
    let group = single_operand_group(groups, scope, DesignOperandRole::ROLE_0X10)
        .or_else(|| single_operand_group(groups, scope, DesignOperandRole::BODIES_A))?;
    let remaining = scope
        .reference_members()
        .values()
        .copied()
        .filter(|record| {
            *record != group.record_index && *record != operation.thickness_record_index
        })
        .collect::<Vec<_>>();
    let members = group
        .members()
        .iter()
        .map(|member| member.value)
        .collect::<Vec<_>>();
    (remaining != members).then(|| {
        crate::loss::F3dLossCode::FeatureShellTableMembersAssumed.note(format!(
            "Shell scope {} stores ordered references {remaining:?} besides face group {} and wall scalar {}, and the face group has members {members:?}; the Shell is projected from its wall slot and face group, assuming the other references leave the walls unchanged (DR-330).",
            scope.id, group.record_index, operation.thickness_record_index
        ))
    })
}

fn shell_has_independent_walls(scope: &DesignParameterScope) -> bool {
    let crate::records::feature::scope::DesignScopePayload::Shell(Some(operation)) =
        scope.payload()
    else {
        return false;
    };
    operation
        .wall_slots
        .as_ref()
        .and_then(|slots| slots.outside.as_ref())
        .is_some_and(|outside| {
            outside.thickness.get().to_bits() != operation.thickness.get().to_bits()
        })
}

fn merge_edge_selections(
    scope: &DesignParameterScope,
    selections: &[cadmpeg_ir::features::EdgeSelection],
) -> cadmpeg_ir::features::EdgeSelection {
    use cadmpeg_ir::features::EdgeSelection;

    let native = || EdgeSelection::Native(scope.id.clone());
    if selections.iter().all(|selection| {
        matches!(
            selection,
            EdgeSelection::Edges(_) | EdgeSelection::Resolved { .. }
        )
    }) {
        let mut seen = HashSet::new();
        let mut resolved = Vec::new();
        for selection in selections {
            let (EdgeSelection::Edges(edges) | EdgeSelection::Resolved { edges, .. }) = selection
            else {
                return native();
            };
            for edge in edges {
                if !seen.insert(edge) {
                    return native();
                }
                resolved.push(edge.clone());
            }
        }
        return EdgeSelection::Resolved {
            edges: resolved,
            native: scope.id.clone(),
        };
    }
    if let Some(EdgeSelection::Historical { state, .. }) = selections.first() {
        if selections.iter().all(|selection| {
            matches!(selection,
            EdgeSelection::Historical { state: candidate, .. } if candidate == state)
        }) {
            let state = state.clone();
            let mut seen = HashSet::new();
            let mut resolved = Vec::new();
            for selection in selections {
                let EdgeSelection::Historical { edges, .. } = selection else {
                    return native();
                };
                for edge in edges {
                    if !seen.insert(edge) {
                        return native();
                    }
                    resolved.push(edge.clone());
                }
            }
            let selected_native = scope.id.clone();
            return cadmpeg_ir::features::EdgeSelection::historical(
                state,
                resolved,
                selected_native,
            )
            .unwrap_or_else(|_| native());
        }
    }
    native()
}

fn matrix_axis_angle(transform: &[[f64; 4]; 4]) -> Option<cadmpeg_ir::features::AxisAngle> {
    use cadmpeg_ir::features::AxisAngle;
    use cadmpeg_ir::scalar::Angle;

    let trace = transform[0][0] + transform[1][1] + transform[2][2];
    let sine = 0.5
        * (transform[2][1] - transform[1][2])
            .hypot(transform[0][2] - transform[2][0])
            .hypot(transform[1][0] - transform[0][1]);
    let angle = sine.atan2(((trace - 1.0) * 0.5).clamp(-1.0, 1.0));
    if angle.abs() <= EPS_FEATURE_PROJECT_MATRIX_AXIS_ANGLE_E12 {
        return None;
    }
    let (x, y, z) =
        if (std::f64::consts::PI - angle).abs() <= EPS_FEATURE_PROJECT_MATRIX_AXIS_ANGLE_E8 {
            // The largest diagonal supplies a nonzero axis component at a half-turn.
            // Recover the other signs from its row; the X component may be zero.
            let pivot = (0..3).max_by(|&a, &b| transform[a][a].total_cmp(&transform[b][b]))?;
            let mut axis = [0.0; 3];
            axis[pivot] = ((transform[pivot][pivot] + 1.0) * 0.5).max(0.0).sqrt();
            if axis[pivot] == 0.0 {
                return None;
            }
            for other in 0..3 {
                if other != pivot {
                    axis[other] =
                        (transform[pivot][other] + transform[other][pivot]) / (4.0 * axis[pivot]);
                }
            }
            // Away from an exact half-turn the skew terms still state the
            // orientation. The diagonal alone cannot distinguish opposite axes.
            let skew = [
                transform[2][1] - transform[1][2],
                transform[0][2] - transform[2][0],
                transform[1][0] - transform[0][1],
            ];
            if axis
                .iter()
                .zip(skew)
                .map(|(axis, skew)| axis * skew)
                .sum::<f64>()
                < 0.0
            {
                axis = axis.map(|value| -value);
            }
            (axis[0], axis[1], axis[2])
        } else {
            let scale = 2.0 * angle.sin();
            (
                (transform[2][1] - transform[1][2]) / scale,
                (transform[0][2] - transform[2][0]) / scale,
                (transform[1][0] - transform[0][1]) / scale,
            )
        };
    let norm = x.hypot(y).hypot(z);
    (norm > EPS_FEATURE_PROJECT_MATRIX_AXIS_ANGLE_E12).then_some(AxisAngle {
        origin: cadmpeg_ir::features::FinitePoint3::ZERO,
        direction: cadmpeg_ir::features::FeatureDirection3::new(Vector3::new(
            x / norm,
            y / norm,
            z / norm,
        ))?,
        angle: Angle::new(angle)?,
    })
}

pub(crate) fn direct_face_selection<'a>(
    scope: &DesignParameterScope,
    operands: impl IntoIterator<Item = &'a DesignFaceOperand>,
) -> Option<cadmpeg_ir::features::FaceSelection> {
    use cadmpeg_ir::features::FaceSelection;

    let mut matching = Vec::new();
    for operand in operands.into_iter().filter(|operand| {
        native_stream(&operand.id) == native_stream(&scope.id)
            && operand.scope_record_index == scope.record_index
    }) {
        matching.push(operand);
    }
    matching.sort_by(|left, right| {
        left.scope_reference_ordinal
            .cmp(&right.scope_reference_ordinal)
    });
    if matching.is_empty() {
        return None;
    }
    let mut members = Vec::new();
    for operand in &matching {
        members.push((operand.id.as_str(), operand.resolved_face_slots.as_slice()));
    }
    let feature_id = crate::ids::neutral_feature_id(scope);
    let historical_face = |previous_state_id, slot| {
        crate::ids::history_input_face_id(
            &crate::ids::feature_input_prefix(&feature_id, previous_state_id),
            slot,
        )
    };
    let faces = match scope.previous_history_state_id() {
        Some(previous_state_id) if members.iter().all(|(_, faces)| !faces.is_empty()) => {
            let mut resolved = Vec::new();
            for slot in members.iter().flat_map(|(_, faces)| faces.iter().copied()) {
                let face = historical_face(previous_state_id, slot);
                if !resolved.contains(&face) {
                    resolved.push(face);
                }
            }
            match cadmpeg_ir::features::FaceSelection::historical(
                crate::ids::feature_input_topology_id(&feature_id, previous_state_id),
                resolved,
                scope.id.clone(),
            ) {
                Ok(selection) => selection,
                Err(_) => FaceSelection::Native(scope.id.clone()),
            }
        }
        Some(previous_state_id) if members.iter().any(|(_, faces)| !faces.is_empty()) => {
            let mut faces = Vec::new();
            let mut unresolved = Vec::new();
            for (identity, slots) in &members {
                if slots.is_empty() {
                    unresolved.push((*identity).to_owned());
                } else {
                    for slot in *slots {
                        let face = historical_face(previous_state_id, *slot);
                        if !faces.contains(&face) {
                            faces.push(face);
                        }
                    }
                }
            }
            match cadmpeg_ir::features::FaceSelection::historical_partial(
                crate::ids::feature_input_topology_id(&feature_id, previous_state_id),
                faces,
                unresolved,
                scope.id.clone(),
            ) {
                Ok(selection) => selection,
                Err(_) => FaceSelection::Native(scope.id.clone()),
            }
        }
        _ => FaceSelection::Native(scope.id.clone()),
    };
    Some(faces)
}

/// Local work cap for ordering the parameters of one owner scope.
///
/// Each ordering round scans every unresolved parameter and its dependencies,
/// so a dependency chain makes the ordering quadratic in the scope's
/// parameter count.
pub(crate) const PARAMETER_ORDER_WORK: usize = 1 << 22;

fn design_positive_length(
    parameter: &DesignParameter,
) -> Option<cadmpeg_ir::scalar::PositiveLength> {
    cadmpeg_ir::scalar::PositiveLength::try_from(design_length(parameter)?).ok()
}

pub(super) fn design_length(parameter: &DesignParameter) -> Option<cadmpeg_ir::scalar::Length> {
    let value = parameter.evaluated_value().get() * 10.0;
    (parameter
        .unit()
        .map(|field| field.value.as_str())
        .is_some_and(design_length_unit))
    .then_some(cadmpeg_ir::scalar::Length::new(value)?)
}

pub(crate) fn design_length_unit(unit: &str) -> bool {
    matches!(unit, "mm" | "cm" | "m" | "in" | "ft")
}

pub(super) fn design_angle_unit(unit: &str) -> bool {
    matches!(unit, "deg" | "rad")
}

pub(super) fn design_dimension_unit(parameter: &DesignParameter) -> bool {
    let unit = parameter.unit().map(|field| field.value.as_str());
    if parameter.source_kind_str().starts_with("Linear Dimension")
        || parameter.source_kind_str().starts_with("Radius Dimension")
        || parameter.source_kind_str().starts_with("Radial Dimension")
        || parameter
            .source_kind_str()
            .starts_with("Diameter Dimension")
    {
        return unit.is_some_and(design_length_unit);
    }
    if parameter.source_kind_str().starts_with("Angular Dimension") {
        return unit.is_some_and(design_angle_unit);
    }
    if parameter.source_kind_str().starts_with("Tangent Dimension") {
        return unit.is_some_and(design_length_unit);
    }
    false
}

fn variable_fillet_law(
    parameters: &[(u32, &DesignParameter)],
) -> Option<(
    cadmpeg_ir::features::edge_treatments::VariableRadii,
    Option<cadmpeg_ir::scalar::FiniteReal>,
)> {
    use cadmpeg_ir::features::edge_treatments::VariableRadius;

    let unique_parameter = |kind: &str| {
        let mut matches = parameters
            .iter()
            .filter_map(|(_, parameter)| (parameter.source_kind() == kind).then_some(*parameter));
        let parameter = matches.next()?;
        matches.next().is_none().then_some(parameter)
    };
    let start = unique_parameter("StartRadius").and_then(design_length)?;
    let end = unique_parameter("EndRadius").and_then(design_length)?;
    let tangency_weight = {
        let mut matches = parameters.iter().filter_map(|(_, parameter)| {
            (parameter.source_kind() == "TangencyWeight").then_some(*parameter)
        });
        match (matches.next(), matches.next()) {
            (None, None) => None,
            (Some(parameter), None) => Some(parameter.evaluated_value()),
            (None, Some(_)) => return None,
            (Some(_), Some(_)) => return None,
        }
    };
    let mut middle_radii = Vec::new();
    let mut middle_parameters = Vec::new();
    for (ordinal, parameter) in parameters {
        match parameter.source_kind_str() {
            "MidRadius" => middle_radii.push((*ordinal, *parameter)),
            "MidParams" => middle_parameters.push((*ordinal, *parameter)),
            _ => {}
        }
    }
    middle_radii.sort_by_key(|left| left.0);
    middle_parameters.sort_by_key(|left| left.0);
    if middle_radii.len() != middle_parameters.len()
        || parameters.iter().any(|(_, parameter)| {
            !matches!(
                parameter.source_kind_str(),
                "StartRadius" | "EndRadius" | "MidRadius" | "MidParams" | "TangencyWeight"
            )
        })
    {
        return None;
    }
    let mut points = Vec::new();
    points.push(VariableRadius {
        parameter: 0.0,
        radius: start,
    });
    for ((_, radius), (_, parameter)) in middle_radii.into_iter().zip(middle_parameters) {
        let radius = design_length(radius)?;
        let parameter = parameter.evaluated_value().get();
        points.push(VariableRadius { parameter, radius });
    }
    points.push(VariableRadius {
        parameter: 1.0,
        radius: end,
    });
    cadmpeg_ir::features::edge_treatments::VariableRadii::new(points)
        .ok()
        .map(|points| (points, tangency_weight))
}

/// Count parameters whose unit token has no settled neutral quantity kind.
pub(crate) fn untyped_parameter_unit_count(parameters: &[DesignParameter]) -> usize {
    parameters
        .iter()
        .filter(|parameter| {
            parameter.text_value().is_none()
                && parameter
                    .unit()
                    .map(|field| field.value.as_str())
                    .is_some_and(|unit| !design_length_unit(unit) && !design_angle_unit(unit))
        })
        .count()
}

/// Whether a scope of this family with null current and preceding
/// history-state identities is suppressed. An active scope of these families
/// changes ASM topology and carries both identities.
fn stateless_scope_is_suppressed(family: DesignFeatureFamily) -> bool {
    matches!(
        family,
        DesignFeatureFamily::Extrude
            | DesignFeatureFamily::Fillet
            | DesignFeatureFamily::Chamfer
            | DesignFeatureFamily::Hole
            | DesignFeatureFamily::Thread
            | DesignFeatureFamily::Combine
    )
}

fn resolved_loft_path(
    group: &DesignConstructionOperandGroup,
    groups: &[DesignConstructionOperandGroup],
    operands: &[DesignEdgeOperand],
    identity_operands: &[DesignEdgeIdentityOperand],
    scope: &DesignParameterScope,
    ctx: &DecodeContext<'_>,
) -> Result<cadmpeg_ir::features::PathRef, CodecError> {
    let selection = resolved_edge_group(
        group,
        groups,
        operands,
        identity_operands,
        scope.previous_history_state_id(),
        &crate::ids::neutral_feature_id(scope),
        ctx,
    )?;
    Ok(loft_path_from_edge_selection(&group.id, selection))
}

fn loft_path_from_edge_selection(
    native: &str,
    selection: cadmpeg_ir::features::EdgeSelection,
) -> cadmpeg_ir::features::PathRef {
    use cadmpeg_ir::features::{EdgeSelection, PathRef};

    match selection {
        EdgeSelection::Edges(edges) | EdgeSelection::Resolved { edges, .. } => {
            PathRef::Edges(edges)
        }
        EdgeSelection::Historical {
            state,
            edges,
            native,
        } => PathRef::HistoricalEdges {
            state,
            edges,
            native,
        },
        EdgeSelection::All
        | EdgeSelection::Unresolved
        | EdgeSelection::Native(_)
        | EdgeSelection::Generated { .. }
        | EdgeSelection::HistoricalPartial { .. } => PathRef::Native(native.to_owned()),
    }
}

/// Bind the exact removed-cell set retained by a decoded `SurfaceTrim`.
///
/// A source trim selects cells for removal. That set is independent of the
/// canonical inside/outside shorthand and must remain explicit when it is not
/// reducible to one of those two regions.
pub(crate) fn bind_surface_trim_cell_selections(
    features: &mut [cadmpeg_ir::features::Feature],
    scopes: &[DesignParameterScope],
    operations: &[DesignSurfaceTrimOperation],
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) {
    let mut native_tables = 0usize;
    for feature in features {
        if !matches!(
            feature.evaluation.definition(),
            cadmpeg_ir::features::FeatureDefinition::Operation(
                cadmpeg_ir::features::FeatureOperation::TrimSurface {
                    keep: cadmpeg_ir::features::TrimRegion::Unresolved,
                    ..
                }
            )
        ) {
            continue;
        }
        let Some(native_ref) = feature.native_ref.as_deref() else {
            continue;
        };
        let Some(scope) = scopes.iter().find(|scope| scope.id == native_ref) else {
            continue;
        };
        let Some(operation) = operations.iter().find(|operation| {
            operation.scope_record_index == scope.record_index
                && native_stream(&operation.id) == native_stream(&scope.id)
        }) else {
            continue;
        };
        let removed = operation
            .cell_entries()
            .iter()
            .map(|entry| entry.ordinal)
            .collect::<Vec<_>>();
        let total = u64::from(operation.trailing_value);
        let selection = u64::try_from(removed.len())
            .is_ok_and(|count| count < total)
            .then(|| cadmpeg_ir::features::TrimCellSelection::new(removed, total))
            .flatten();
        let Some(selection) = selection else {
            native_tables += 1;
            continue;
        };
        feature.evaluation.edit(|definition, _| {
            if let cadmpeg_ir::features::FeatureDefinition::Operation(
                cadmpeg_ir::features::FeatureOperation::TrimSurface { keep, .. },
            ) = definition
            {
                *keep = cadmpeg_ir::features::TrimRegion::Cells(selection);
            }
        });
    }
    if native_tables != 0 {
        losses.push(crate::loss::F3dLossCode::FeatureTrimCellTableNative.note(format!(
            "{native_tables} SurfaceTrim cell table(s) keep native entries because their entry count reaches the trailing value or their entry values repeat."
        )));
    }
}

fn face_scope_frame(scope: &DesignParameterScope) -> Option<(u64, u64)> {
    let count_offset = scope
        .reference_count_offset()
        .checked_sub(scope.byte_offset())?;
    let reference_bytes = u64::try_from(scope.reference_members().len())
        .ok()?
        .checked_mul(11)?;
    let kind_offset = scope
        .reference_count_offset()
        .checked_add(12)?
        .checked_add(reference_bytes)?;
    if scope.kind_offset() != kind_offset {
        return None;
    }
    let kind_bytes = u64::try_from(scope.source_kind_name().encode_utf16().count())
        .ok()?
        .checked_mul(2)?;
    let tail_length = scope
        .paired_byte_offset()
        .checked_sub(kind_offset.checked_add(kind_bytes)?)?;
    Some((count_offset, tail_length))
}

/// Local work cap for the closed-profile chain search of one spatial sketch.
///
/// Each chain step scans every unused open edge, so the search is quadratic
/// in the sketch's open edges. An exhausted cap leaves the sketch with no
/// derived profiles.
pub(super) const SPATIAL_PROFILE_SEARCH_WORK: usize = 1 << 24;

#[cfg(test)]
mod tests;
