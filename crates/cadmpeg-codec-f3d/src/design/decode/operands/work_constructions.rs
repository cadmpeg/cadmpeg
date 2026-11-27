// SPDX-License-Identifier: Apache-2.0
//! Work point, hole location, work plane, and vertex recipe bindings.

use super::entity_selections::{
    parse_entity_selection_frame, parse_work_point_sketch_point_frame, WorkPointSketchPointFrame,
};
use super::recipe_operands::parse_vertex_recipe;
use crate::container::ContainerScan;
use crate::ids::native_stream;
use crate::records::decal::DesignRecordHeader;
use crate::records::feature::hole::DesignHoleConstruction;
use crate::records::feature::scope::DesignParameterScope;
use crate::records::feature::work_geometry::{
    DesignWorkPlaneConstruction, DesignWorkPointInputCarrier, DesignWorkPointPlaneSelection,
    DesignWorkPointRule, DesignWorkPointSketchPointSelection,
};
use crate::records::parameters::{DesignParameter, DesignParameterOwner};
use crate::records::recipes::ConstructionRecipe;
use crate::records::sketch_geometry::SketchPoint;
use crate::records::topology::edge_identity::DesignEdgeOperand;
use cadmpeg_core::container::ContainerRole;
use cadmpeg_core::CodecError;
use std::collections::HashMap;

/// Bind each `WorkPoint` input to its exact edge, vertex, or `WorkPlane` carrier.
pub(crate) fn bind_work_point_input_carriers(
    scan: &ContainerScan,
    scopes: &mut [DesignParameterScope],
    headers: &[DesignRecordHeader],
    recipes: &[ConstructionRecipe],
    edge_operands: &[DesignEdgeOperand],
    sketch_points: &[SketchPoint],
) -> Result<(), CodecError> {
    let mut header_index = HashMap::new();
    for header in headers {
        let Some(stream) = native_stream(&header.id) else {
            continue;
        };
        let key = (stream, header.record_index);
        header_index.insert(key, header);
    }
    let mut work_planes: HashMap<String, HashMap<u32, u32>> = HashMap::new();
    for scope in scopes.iter().filter(|scope| {
        scope.kind() == crate::records::feature::scope::DesignFeatureKind::WorkPlane
    }) {
        let (Some(stream), Some(preceding_index)) =
            (native_stream(&scope.id), scope.record_index.checked_sub(1))
        else {
            continue;
        };
        work_planes
            .entry(stream.to_owned())
            .or_default()
            .insert(preceding_index, scope.record_index);
    }
    for scope in scopes.iter_mut().filter(|scope| {
        scope.kind() == crate::records::feature::scope::DesignFeatureKind::WorkPoint
    }) {
        let Some(stream) = native_stream(&scope.id) else {
            continue;
        };
        let stream = stream.to_owned();
        let Some(entry) = scan.design_stream_entry_for_scope(ContainerRole::Bulkstream, &stream)
        else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        let records = scan.design_record_offsets(&entry.name)?;
        let scope_record_index = scope.record_index;
        let Some(construction) = scope.work_point_construction_mut() else {
            continue;
        };
        let mut inputs = construction.rule.inputs().to_vec();
        for input in &mut inputs {
            let mut edge_matches = edge_operands.iter().filter(|operand| {
                native_stream(&operand.id) == Some(stream.as_str())
                    && operand.scope_record_index == scope_record_index
                    && operand.record_index() == input.record_index()
            });
            let edge_match = edge_matches.next();
            if let Some(operand) = edge_match.filter(|_| edge_matches.next().is_none()) {
                input
                    .try_set_carrier(Some(Box::new(DesignWorkPointInputCarrier::EdgeRecipe {
                        operand_id: operand.id.clone(),
                    })))
                    .map_err(CodecError::Malformed)?;
                continue;
            }
            let Some(header) = header_index.get(&(stream.as_str(), input.record_index())) else {
                continue;
            };
            if let Some(recipe) = parse_vertex_recipe(bytes, &records, &stream, header, recipes) {
                input
                    .try_set_carrier(Some(Box::new(DesignWorkPointInputCarrier::VertexRecipe {
                        recipe,
                    })))
                    .map_err(CodecError::Malformed)?;
                continue;
            }
            if let Some(frame) =
                parse_work_point_sketch_point_frame(bytes, input.record_index(), header.byte_offset)
            {
                if let Some(selection) = resolve_sketch_point_selection(
                    frame,
                    header,
                    input.record_index(),
                    &stream,
                    sketch_points,
                )? {
                    input
                        .try_set_carrier(Some(Box::new(DesignWorkPointInputCarrier::SketchPoint {
                            selection,
                        })))
                        .map_err(CodecError::Malformed)?;
                }
                continue;
            }
            let Some(selection) = parse_entity_selection_frame(
                bytes,
                input.record_index(),
                header.byte_offset,
                header.class_tag.as_str(),
            ) else {
                continue;
            };
            let Ok(primary_identity) = u32::try_from(selection.primary_identity) else {
                continue;
            };
            let Some(work_plane_scope_record_index) = work_planes
                .get(stream.as_str())
                .and_then(|indices| indices.get(&primary_identity))
                .copied()
            else {
                continue;
            };
            if selection.secondary.is_some() {
                continue;
            }
            let (Ok(asset_id), Ok(context_id)) = (
                crate::records::mesh::DesignRelaxedGuidText::try_from(selection.asset_id),
                crate::records::mesh::DesignRelaxedGuidText::try_from(selection.context_id),
            ) else {
                continue;
            };
            input
                .try_set_carrier(Some(Box::new(DesignWorkPointInputCarrier::WorkPlane {
                    selection: DesignWorkPointPlaneSelection::try_new(
                        input.record_index(),
                        crate::records::feature::work_geometry::DesignWorkPointPlaneSelectionDraft {
                        class_tag: header.class_tag.clone(),
                        asset_id,
                        asset_id_offset: selection.asset_id_offset,
                        context_id,
                        context_id_offset: selection.context_id_offset,
                        serialization_form: selection.serialization_form,
                        identity_record_index: selection.identity_record_index,
                        identity_record_offset: selection.identity_record_offset,
                        primary_identity: selection.primary_identity,
                        primary_identity_offset: selection.primary_identity_offset,
                        work_plane_scope_record_index,
                        next_record_index: selection.next_record_index,
                        next_byte_offset: selection.next_byte_offset,
                        },
                    )
                    .map_err(CodecError::Malformed)?,
                })))
                .map_err(CodecError::Malformed)?;
        }
        construction.rule =
            DesignWorkPointRule::from_serialized(construction.rule.reference_type(), inputs)
                .map_err(CodecError::Malformed)?;
    }
    Ok(())
}

/// Bind each vertex-rule input of a `Hole` location that is a direct
/// sketch-point envelope to the sketch point it names.
pub(crate) fn bind_hole_location_sketch_points(
    scan: &ContainerScan,
    scopes: &mut [DesignParameterScope],
    headers: &[DesignRecordHeader],
    sketch_points: &[SketchPoint],
) -> Result<(), CodecError> {
    let mut header_index = HashMap::new();
    for header in headers {
        let Some(stream) = native_stream(&header.id) else {
            continue;
        };
        header_index.insert((stream, header.record_index), header);
    }
    for scope in scopes.iter_mut() {
        let Some(stream) = native_stream(&scope.id) else {
            continue;
        };
        let stream = stream.to_owned();
        let Some(construction) = scope.hole_construction_mut() else {
            continue;
        };
        if construction
            .locations
            .iter()
            .all(|location| location.vertex_input.is_none())
        {
            continue;
        }
        let Some(entry) = scan.design_stream_entry_for_scope(ContainerRole::Bulkstream, &stream)
        else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        bind_hole_construction_sketch_points(
            bytes,
            &stream,
            construction,
            &header_index,
            sketch_points,
        )?;
    }
    Ok(())
}

pub(super) fn bind_hole_construction_sketch_points(
    bytes: &[u8],
    stream: &str,
    construction: &mut DesignHoleConstruction,
    headers: &HashMap<(&str, u32), &DesignRecordHeader>,
    sketch_points: &[SketchPoint],
) -> Result<(), CodecError> {
    for input in construction
        .locations
        .iter_mut()
        .filter_map(|location| location.vertex_input.as_mut())
    {
        let Some(header) = headers.get(&(stream, input.record_index())) else {
            continue;
        };
        let Some(frame) =
            parse_work_point_sketch_point_frame(bytes, input.record_index(), header.byte_offset)
        else {
            continue;
        };
        if let Some(selection) = resolve_sketch_point_selection(
            frame,
            header,
            input.record_index(),
            stream,
            sketch_points,
        )? {
            input
                .try_set_carrier(Some(Box::new(DesignWorkPointInputCarrier::SketchPoint {
                    selection,
                })))
                .map_err(CodecError::Malformed)?;
        }
    }
    Ok(())
}

/// The selection of the one decoded sketch point that a direct sketch-point
/// envelope at input `record_index` names by its Sketch record and
/// persistent id.
pub(super) fn resolve_sketch_point_selection(
    frame: WorkPointSketchPointFrame,
    header: &DesignRecordHeader,
    record_index: u32,
    stream: &str,
    sketch_points: &[SketchPoint],
) -> Result<Option<DesignWorkPointSketchPointSelection>, CodecError> {
    let (Ok(asset_id), Ok(context_id)) = (
        crate::records::mesh::DesignRelaxedGuidText::try_from(frame.asset_id),
        crate::records::mesh::DesignRelaxedGuidText::try_from(frame.context_id),
    ) else {
        return Ok(None);
    };
    let mut point_matches = sketch_points.iter().filter(|point| {
        native_stream(&point.id) == Some(stream)
            && point.owner_reference == Some(frame.sketch_record_index)
            && point.persistent_id() == Some(frame.point_persistent_id)
    });
    let (Some(point), None) = (point_matches.next(), point_matches.next()) else {
        return Ok(None);
    };
    DesignWorkPointSketchPointSelection::try_new(
        record_index,
        crate::records::feature::work_geometry::DesignWorkPointSketchPointSelectionDraft {
            class_tag: header.class_tag.clone(),
            asset_id,
            asset_id_offset: frame.asset_id_offset,
            context_id,
            context_id_offset: frame.context_id_offset,
            serialization_form: frame.serialization_form,
            identity_record_index: frame.identity_record_index,
            identity_record_offset: frame.identity_record_offset,
            identity_form: frame.identity_form,
            sketch_record_index: frame.sketch_record_index,
            sketch_record_index_offset: frame.sketch_record_index_offset,
            point_persistent_id: frame.point_persistent_id,
            point_persistent_id_offset: frame.point_persistent_id_offset,
            point_native_id: point.id.clone(),
            next_record_index: frame.next_record_index,
            next_byte_offset: frame.next_byte_offset,
        },
    )
    .map(Some)
    .map_err(CodecError::Malformed)
}

/// Bind the exact three-vertex construction carried by a `WorkPlane` scope.
pub(crate) fn bind_work_plane_constructions(
    scan: &ContainerScan,
    scopes: &mut [DesignParameterScope],
    headers: &[DesignRecordHeader],
    recipes: &[ConstructionRecipe],
    owners: &[DesignParameterOwner],
    parameters: &[DesignParameter],
) -> Result<(), CodecError> {
    let mut header_index = HashMap::new();
    for header in headers {
        let Some(stream) = native_stream(&header.id) else {
            continue;
        };
        let key = (stream, header.record_index);
        header_index.insert(key, header);
    }
    for scope in scopes.iter_mut().filter(|scope| {
        scope.kind() == crate::records::feature::scope::DesignFeatureKind::WorkPlane
    }) {
        if let Some(frame) = scope.work_plane_frame_mut() {
            frame.work_plane_construction = None;
        }
        let Some(stream) = native_stream(&scope.id) else {
            continue;
        };
        let stream = stream.to_owned();
        let Some(entry) = scan.design_stream_entry_for_scope(ContainerRole::Bulkstream, &stream)
        else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        let records = scan.design_record_offsets(&entry.name)?;
        let Some([placement_record_index, first, second, third, extra_offset]) =
            scope.reference_members().values_array()
        else {
            continue;
        };
        if scope.work_plane_frame().is_none() || scope.work_plane_reference() != Some(*extra_offset)
        {
            continue;
        }
        let Some(owner) = owners.iter().find(|owner| {
            native_stream(owner.id()) == Some(stream.as_str())
                && owner.record_index() == *extra_offset
                && owner.scope_record_index() == scope.record_index
                && owner.evaluated_value().get() == 0.0
        }) else {
            continue;
        };
        if !parameters.iter().any(|parameter| {
            native_stream(&parameter.id) == Some(stream.as_str())
                && parameter.record_index == owner.parameter_record_index()
                && parameter.owner_record_index() == Some(owner.record_index())
                && parameter.source_kind() == "ExtraOffset"
                && parameter.evaluated_value().get() == 0.0
        }) {
            continue;
        }
        let [Some(first_input), Some(second_input), Some(third_input)] = [first, second, third]
            .map(|record_index| {
                parse_vertex_recipe(
                    bytes,
                    &records,
                    &stream,
                    header_index.get(&(stream.as_str(), *record_index))?,
                    recipes,
                )
            })
        else {
            continue;
        };
        let inputs = [first_input, second_input, third_input];
        let placement_record_index = *placement_record_index;
        if let Some(frame) = scope.work_plane_frame_mut() {
            frame.work_plane_construction = Some(
                DesignWorkPlaneConstruction::try_new(placement_record_index, Box::new(inputs))
                    .map_err(CodecError::malformed)?,
            );
        }
    }
    Ok(())
}
