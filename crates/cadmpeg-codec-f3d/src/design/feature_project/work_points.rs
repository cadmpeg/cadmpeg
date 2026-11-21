// SPDX-License-Identifier: Apache-2.0
//! Work point and work plane projection.

use crate::design::feature_project::work_plane_datum_frame;
use crate::design::feature_project::work_plane_recipe_state_id;
use crate::ids;
use crate::ids::native_stream;
use crate::records::feature::scope::DesignParameterScope;
use crate::records::parameters::DesignParameter;
use crate::records::topology::edge_identity::DesignEdgeOperand;
use std::collections::HashMap;

pub(super) fn work_point_edge_operand<'a>(
    scope: &DesignParameterScope,
    input: &crate::records::feature::work_geometry::DesignWorkPointInput,
    edge_operands: &'a [DesignEdgeOperand],
) -> Option<&'a DesignEdgeOperand> {
    let crate::records::feature::work_geometry::DesignWorkPointInputCarrier::EdgeRecipe {
        operand_id,
    } = input.carrier()?
    else {
        return None;
    };
    let stream = native_stream(&scope.id)?;
    let mut matching = edge_operands.iter().filter(|operand| {
        operand.id == *operand_id
            && native_stream(&operand.id) == Some(stream)
            && operand.scope_record_index == scope.record_index
            && operand.record_index() == input.record_index()
    });
    let operand = matching.next()?;
    matching.next().is_none().then_some(operand)
}

pub(super) fn work_point_input_history_state_id(
    scope: &DesignParameterScope,
    input: &crate::records::feature::work_geometry::DesignWorkPointInput,
    edge_operands: &[DesignEdgeOperand],
) -> Option<i64> {
    match input.carrier()? {
        crate::records::feature::work_geometry::DesignWorkPointInputCarrier::EdgeRecipe {
            ..
        } => work_point_edge_operand(scope, input, edge_operands)?.recipe_state_id,
        crate::records::feature::work_geometry::DesignWorkPointInputCarrier::VertexRecipe {
            recipe,
        } => recipe.resolution.map(|resolution| resolution.state_id),
        crate::records::feature::work_geometry::DesignWorkPointInputCarrier::WorkPlane {
            ..
        }
        | crate::records::feature::work_geometry::DesignWorkPointInputCarrier::SketchPoint {
            ..
        } => None,
    }
}

pub(crate) fn work_point_recipe_state_id(
    scope: &DesignParameterScope,
    edge_operands: &[DesignEdgeOperand],
) -> Option<i64> {
    let construction = scope.work_point_construction()?;
    let mut states = construction
        .rule
        .inputs()
        .iter()
        .filter_map(|input| work_point_input_history_state_id(scope, input, edge_operands));
    let state = states.next()?;
    states.all(|candidate| candidate == state).then_some(state)
}

pub(super) fn project_work_point_construction(
    scope: &DesignParameterScope,
    construction: &crate::records::feature::work_geometry::DesignWorkPointConstruction,
    parameters: &[(u32, &DesignParameter)],
    edge_operands: &[DesignEdgeOperand],
    scope_ids: &HashMap<(&str, u32), cadmpeg_ir::features::FeatureId>,
) -> Option<cadmpeg_ir::features::DatumPointConstruction> {
    use crate::records::feature::work_geometry::{
        DesignWorkPointInput, DesignWorkPointInputCarrier, DesignWorkPointRuleForm,
    };
    use cadmpeg_ir::features::{
        DatumPlaneReference, DatumPointConstruction, EdgeSelection, VertexSelection,
    };

    let stream = native_stream(&scope.id)?;
    let edge = |input: &DesignWorkPointInput| -> Option<EdgeSelection> {
        let operand = work_point_edge_operand(scope, input, edge_operands)?;
        let Some((state_id, edge_slot)) = operand
            .recipe_state_id
            .zip(crate::design::edge_resolve::resolved_edge_operand(operand))
        else {
            return Some(EdgeSelection::Native(operand.id.clone()));
        };
        let feature_id = crate::ids::neutral_feature_id(scope);
        let prefix = crate::ids::feature_input_prefix(&feature_id, state_id);
        Some(
            match cadmpeg_ir::features::EdgeSelection::historical(
                crate::ids::feature_input_topology_id(&feature_id, state_id),
                vec![crate::ids::history_input_edge_id(&prefix, edge_slot)],
                operand.id.clone(),
            ) {
                Ok(selection) => selection,
                Err(_) => EdgeSelection::Native(operand.id.clone()),
            },
        )
    };
    let plane = |input: &DesignWorkPointInput| -> Option<DatumPlaneReference> {
        let Some(DesignWorkPointInputCarrier::WorkPlane { selection }) = input.carrier() else {
            return None;
        };
        scope_ids
            .get(&(stream, selection.work_plane_scope_record_index))
            .map(|feature| DatumPlaneReference::Feature {
                feature: feature.clone(),
            })
    };

    let vertex = |input: &DesignWorkPointInput| -> Option<VertexSelection> {
        let Some(DesignWorkPointInputCarrier::VertexRecipe { recipe }) = input.carrier() else {
            return None;
        };
        if recipe.record_index() != input.record_index() {
            return None;
        }
        let vertex = match recipe.resolution {
            None => VertexSelection::native(recipe.recipe_id.clone())
                .unwrap_or(VertexSelection::Unresolved),
            Some(resolution) => {
                let state_id = resolution.state_id;
                let vertex_slot = resolution.vertex_slot();
                let feature_id = crate::ids::neutral_feature_id(scope);
                let prefix = crate::ids::feature_input_prefix(&feature_id, state_id);
                VertexSelection::historical(
                    crate::ids::feature_input_topology_id(&feature_id, state_id),
                    crate::ids::history_input_vertex_id(&prefix, vertex_slot),
                    recipe.recipe_id.clone(),
                )
                .unwrap_or(VertexSelection::Unresolved)
            }
        };
        Some(vertex)
    };

    Some(match construction.rule.form() {
        DesignWorkPointRuleForm::CircleCenter { input } => {
            DatumPointConstruction::CircleCenter { edge: edge(input)? }
        }
        DesignWorkPointRuleForm::TwoEdgeIntersection { inputs } => {
            DatumPointConstruction::TwoEdgeIntersection {
                edges: [edge(&inputs[0])?, edge(&inputs[1])?],
            }
        }
        DesignWorkPointRuleForm::ThreePlaneIntersection { inputs } => {
            DatumPointConstruction::ThreePlaneIntersection {
                planes: Box::new([plane(&inputs[0])?, plane(&inputs[1])?, plane(&inputs[2])?]),
            }
        }
        DesignWorkPointRuleForm::Vertex { input } => DatumPointConstruction::Vertex {
            vertex: vertex(input)?,
        },
        DesignWorkPointRuleForm::Native {
            reference_type: 1,
            inputs,
        } => {
            let [input] = inputs.as_slice() else {
                return None;
            };
            DatumPointConstruction::Vertex {
                vertex: vertex(input)?,
            }
        }
        DesignWorkPointRuleForm::EdgePlaneIntersection { inputs } => {
            DatumPointConstruction::EdgePlaneIntersection {
                edge: edge(&inputs[0])?,
                plane: plane(&inputs[1])?,
            }
        }
        DesignWorkPointRuleForm::DistanceOnEdge { input } => {
            let mut distances = parameters
                .iter()
                .map(|(_, parameter)| *parameter)
                .filter(|parameter| parameter.source_kind() == "PathDistance");
            let distance = distances.next()?;
            if distances.next().is_some()
                || !(0.0..=1.0).contains(&distance.evaluated_value().get())
            {
                return None;
            }
            DatumPointConstruction::DistanceOnEdge {
                edge: edge(input)?,
                fraction: cadmpeg_ir::scalar::Fraction::new(distance.evaluated_value().get())?,
            }
        }
        DesignWorkPointRuleForm::Native { .. } => return None,
    })
}

pub(super) fn project_work_plane(
    scope: &DesignParameterScope,
    transform: [[f64; 4]; 4],
) -> cadmpeg_ir::features::FeatureDefinition {
    use cadmpeg_ir::features::{
        FeatureDefinition, FeatureOperation, UnresolvedFamily, VertexSelection,
    };

    let Some(frame) = work_plane_datum_frame(transform) else {
        return FeatureDefinition::Operation(FeatureOperation::Unresolved {
            family: UnresolvedFamily::DatumPlane,
        });
    };
    let Some(construction) = scope.work_plane_construction() else {
        return FeatureDefinition::Operation(FeatureOperation::DatumPlane { frame });
    };
    let Some(state_id) = work_plane_recipe_state_id(scope) else {
        return FeatureDefinition::Operation(FeatureOperation::Unresolved {
            family: UnresolvedFamily::DatumPlane,
        });
    };
    let feature_id = crate::ids::neutral_feature_id(scope);
    let prefix = crate::ids::feature_input_prefix(&feature_id, state_id);
    let vertex = |recipe: &crate::records::feature::work_geometry::DesignVertexRecipe|
        -> Option<VertexSelection> {
            let resolution = recipe.resolution?;
            let native = recipe.recipe_id.clone();
            let selection = VertexSelection::historical(
                crate::ids::feature_input_topology_id(&feature_id, state_id),
                crate::ids::history_input_vertex_id(&prefix, resolution.vertex_slot()),
                native,
            )
            .unwrap_or(VertexSelection::Unresolved);
            Some(selection)
    };
    let [first, second, third] = construction.inputs();
    let (Some(first), Some(second), Some(third)) = (vertex(first), vertex(second), vertex(third))
    else {
        return FeatureDefinition::Operation(FeatureOperation::Unresolved {
            family: UnresolvedFamily::DatumPlane,
        });
    };
    let Ok(points) =
        cadmpeg_ir::features::ThreePointSelection::try_from(Box::new([first, second, third]))
    else {
        return FeatureDefinition::Operation(FeatureOperation::Unresolved {
            family: UnresolvedFamily::DatumPlane,
        });
    };
    FeatureDefinition::Operation(FeatureOperation::DatumThreePointPlane { frame, points })
}

/// Bind `WorkPoint` inputs that select a sketch point after the sketch arenas
/// have been projected. The Design selection identifies a native point record;
/// the neutral point identity depends on whether that record belongs to a
/// planar or model-space sketch.
pub(crate) fn bind_work_point_sketch_point_constructions(
    features: &mut [cadmpeg_ir::features::Feature],
    scopes: &[DesignParameterScope],
    sketch_entities: &[cadmpeg_ir::sketches::SketchEntity],
    spatial_sketch_entities: &[cadmpeg_ir::sketches::SpatialSketchEntity],
) {
    use cadmpeg_ir::features::{
        DatumPointConstruction, FeatureDefinition, FeatureOperation, SketchPointSelection,
    };

    for feature in features.iter_mut() {
        feature.evaluation.edit(|definition, _| 'feature_edit: {
            let Some(scope) = feature
                .native_ref
                .as_deref()
                .and_then(|native_ref| scopes.iter().find(|scope| scope.id == native_ref))
            else {
                break 'feature_edit;
            };
            let FeatureDefinition::Operation(FeatureOperation::DatumPoint { construction, .. }) =
                definition
            else {
                break 'feature_edit;
            };
            if construction.is_some() {
                break 'feature_edit;
            }
            let Some(point) = scope.work_point_construction() else {
                break 'feature_edit;
            };
            let crate::records::feature::work_geometry::DesignWorkPointRuleForm::Vertex { input } =
                point.rule.form()
            else {
                break 'feature_edit;
            };
            let Some(carrier) = input.carrier() else {
                break 'feature_edit;
            };
            let record_index = input.record_index();
            let crate::records::feature::work_geometry::DesignWorkPointInputCarrier::SketchPoint {
                selection,
            } = carrier
            else {
                break 'feature_edit;
            };
            let stream = native_stream(&scope.id).unwrap_or(ids::DEFAULT_STREAM);
            if let Some(entity) = sketch_entities.iter().find(|entity| {
                entity.native_ref.as_deref() == Some(selection.point_native_id.as_str())
            }) {
                let native = format!("{}{}{}", stream, ":design-record#", u64::from(record_index));
                *construction = Some(Box::new(DatumPointConstruction::SketchPoint {
                    point: SketchPointSelection::Planar {
                        sketch: entity.sketch.clone(),
                        point: entity.id().clone(),
                        native,
                    },
                }));
            } else if let Some(entity) = spatial_sketch_entities.iter().find(|entity| {
                entity.native_ref.as_deref() == Some(selection.point_native_id.as_str())
            }) {
                let native = format!("{}{}{}", stream, ":design-record#", u64::from(record_index));
                *construction = Some(Box::new(DatumPointConstruction::SketchPoint {
                    point: SketchPointSelection::Spatial {
                        sketch: entity.sketch.clone(),
                        point: entity.id().clone(),
                        native,
                    },
                }));
            }
        });
    }
}
