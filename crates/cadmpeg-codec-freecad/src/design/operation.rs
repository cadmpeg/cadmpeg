// SPDX-License-Identifier: Apache-2.0
//! Operation selection shared by feature and census source producers.

use super::*;

pub(super) fn select(
    ctx: &crate::property_reader::PropertyReader<'_, '_>,
    object: &ObjectRecord,
    properties: &[&PropertyRecord],
    objects: &[ObjectRecord],
    properties_by_owner: &HashMap<&str, Vec<&PropertyRecord>>,
    feature_ids: &HashMap<&str, FeatureId>,
    sketch_ids: &HashMap<&str, SketchId>,
    model: &cadmpeg_ir::document::Model,
    program_version: Option<&str>,
) -> Result<FeatureDefinition, CodecError> {
    let selected = operation_sources::selected(ctx, object, properties, objects)?;
    let rebound = properties
        .iter()
        .map(|property| selected.properties.get(&property.id).unwrap_or(property))
        .collect::<Vec<_>>();
    let properties = rebound.as_slice();
    let definition = if is_spreadsheet(&object.type_name) {
        if model.spreadsheets.iter().any(|sheet| {
            sheet.native_ref.as_deref() == Some(object.id().as_str())
                || properties
                    .iter()
                    .any(|property| sheet.native_ref.as_deref() == Some(property.id.as_str()))
        }) {
            FeatureDefinition::Operation(FeatureOperation::TreeNode {
                role: FeatureTreeNodeRole::Equations,
                children: TreeChildren::default(),
            })
        } else {
            native_definition(ctx, &object.type_name, properties)?
        }
    } else if is_body(&object.type_name) {
        body_definition(properties, feature_ids)?
            .map_or_else(|| native_definition(ctx, &object.type_name, properties), Ok)?
    } else if is_datum(&object.type_name) {
        datum_definition(&object.type_name, properties)
            .map_or_else(|| native_definition(ctx, &object.type_name, properties), Ok)?
    } else if is_sketch(&object.type_name) {
        if let Some(sketch) = model
            .sketches
            .iter()
            .find(|sketch| sketch.native_ref.as_deref() == Some(object.id().as_str()))
        {
            FeatureDefinition::Operation(FeatureOperation::Sketch {
                sketch: cadmpeg_ir::features::SketchFeatureBinding::Planar(Some(sketch.id.clone())),
            })
        } else {
            native_definition(ctx, &object.type_name, properties)?
        }
    } else if is_stored_geometry_feature(&object.type_name) {
        FeatureDefinition::Operation(FeatureOperation::StoredGeometry {})
    } else if object.type_name == "PartDesign::FeatureBase" {
        feature_base_definition(properties, feature_ids)
            .map_or_else(|| native_definition(ctx, &object.type_name, properties), Ok)?
    } else if is_imported_geometry(&object.type_name) {
        imported_geometry_definition(ctx, &object.type_name, properties)?
            .map_or_else(|| native_definition(ctx, &object.type_name, properties), Ok)?
    } else if is_part_construction_geometry(&object.type_name) {
        part_construction_geometry_definition(ctx, &object.type_name, properties)?
            .map_or_else(|| native_definition(ctx, &object.type_name, properties), Ok)?
    } else if is_primitive(&object.type_name) {
        primitive_definition(ctx, &object.type_name, properties)?
            .map_or_else(|| native_definition(ctx, &object.type_name, properties), Ok)?
    } else if is_boolean(&object.type_name) {
        boolean_definition(ctx, &object.type_name, properties)?
            .or_else(|| {
                (object.type_name != "PartDesign::Boolean" && !ctx.has_unavailable())
                    .then(|| cached_shape_definition(properties))
                    .flatten()
            })
            .map_or_else(|| native_definition(ctx, &object.type_name, properties), Ok)?
    } else if is_loft(&object.type_name) {
        loft_definition(ctx, &object.type_name, properties, sketch_ids)?
            .or_else(|| {
                (!ctx.has_unavailable())
                    .then(|| cached_shape_definition(properties))
                    .flatten()
            })
            .map_or_else(|| native_definition(ctx, &object.type_name, properties), Ok)?
    } else if is_sweep(&object.type_name) {
        sweep_definition(ctx, &object.type_name, properties, sketch_ids)?
            .map_or_else(|| native_definition(ctx, &object.type_name, properties), Ok)?
    } else if is_helical_sweep(&object.type_name) {
        helical_sweep_definition(
            ctx,
            &object.type_name,
            object.id(),
            properties,
            sketch_ids,
            objects,
            properties_by_owner,
        )?
        .map_or_else(|| native_definition(ctx, &object.type_name, properties), Ok)?
    } else if matches!(object.type_name.as_str(), "Part::Helix" | "Part::Spiral") {
        parametric_helix_definition(ctx, &object.type_name, properties)?
            .map_or_else(|| native_definition(ctx, &object.type_name, properties), Ok)?
    } else if is_binder(&object.type_name) {
        binder_definition(ctx, &object.type_name, properties, feature_ids)?
            .map_or_else(|| native_definition(ctx, &object.type_name, properties), Ok)?
    } else if is_pattern(&object.type_name) {
        pattern_definition(
            ctx,
            &object.type_name,
            object.id(),
            properties,
            feature_ids,
            PatternSources {
                objects,
                properties_by_owner: properties_by_owner,
            },
        )?
        .map_or_else(|| native_definition(ctx, &object.type_name, properties), Ok)?
    } else if object.type_name == "Part::Scale" {
        scale_definition(ctx, properties)?
            .map_or_else(|| native_definition(ctx, &object.type_name, properties), Ok)?
    } else if is_hole(&object.type_name) {
        hole_definition(
            ctx,
            object.id(),
            properties,
            sketch_ids,
            objects,
            properties_by_owner,
            program_version,
        )?
        .map_or_else(|| native_definition(ctx, &object.type_name, properties), Ok)?
    } else if is_extrusion(&object.type_name) {
        let profile = match profile_ref(object.id(), properties, sketch_ids) {
            ProfileRef::Planar(PlanarProfileRef::Unresolved(_)) => {
                ["Profile", "Sketch", "Base", "Source"]
                    .iter()
                    .find_map(|name| property(properties, name))
                    .map_or_else(
                        || ProfileRef::Planar(PlanarProfileRef::Unresolved(object.id().clone())),
                        |property| {
                            ProfileRef::Planar(PlanarProfileRef::Native(property.id.clone()))
                        },
                    )
            }
            profile => profile,
        };
        let profile_normal = profile_target(properties)
            .and_then(|(_, target)| objects.iter().find(|object| object.id().as_str() == target))
            .map(|profile_object| {
                let profile_properties = properties_by_owner
                    .get(profile_object.id().as_str())
                    .map(Vec::as_slice)
                    .unwrap_or_default();
                sketch_frame(ctx, profile_properties).map(|frame| frame.1)
            })
            .transpose()?;
        extrusion_definition(
            ctx,
            &object.type_name,
            properties,
            profile,
            profile_normal,
            &model.sketches,
        )?
        .map_or_else(|| native_definition(ctx, &object.type_name, properties), Ok)?
    } else if is_revolution(&object.type_name) {
        revolution_definition(ctx, &object.type_name, object.id(), properties, sketch_ids)?
            .map_or_else(|| native_definition(ctx, &object.type_name, properties), Ok)?
    } else if matches!(
        object.type_name.as_str(),
        "PartDesign::Thickness" | "Part::Thickness"
    ) {
        thickness_definition(ctx, &object.type_name, properties)?
            .map_or_else(|| native_definition(ctx, &object.type_name, properties), Ok)?
    } else if matches!(object.type_name.as_str(), "Part::Offset" | "Part::Offset2D") {
        offset_shape_definition(ctx, &object.type_name, properties)?
            .map_or_else(|| native_definition(ctx, &object.type_name, properties), Ok)?
    } else if matches!(
        object.type_name.as_str(),
        "Part::Compound" | "Part::Compound2" | "Part::Refine" | "Part::Reverse"
    ) {
        derived_shape_definition(&object.type_name, properties, &selected.arities)
            .map_or_else(|| native_definition(ctx, &object.type_name, properties), Ok)?
    } else if object.type_name == "Part::RuledSurface" {
        ruled_surface_definition(ctx, properties)?
            .map_or_else(|| native_definition(ctx, &object.type_name, properties), Ok)?
    } else if object.type_name == "Part::Section" {
        section_shape_definition(ctx, properties)?
            .map_or_else(|| native_definition(ctx, &object.type_name, properties), Ok)?
    } else if object.type_name == "Part::Mirroring" {
        mirror_shape_definition(ctx, properties)?
            .map_or_else(|| native_definition(ctx, &object.type_name, properties), Ok)?
    } else if object.type_name == "Part::ProjectOnSurface" {
        project_on_surface_definition(ctx, properties)?
            .map_or_else(|| native_definition(ctx, &object.type_name, properties), Ok)?
    } else if object.type_name == "PartDesign::Draft" {
        draft_definition(ctx, properties, objects, properties_by_owner)?
            .map_or_else(|| native_definition(ctx, &object.type_name, properties), Ok)?
    } else if is_fillet(&object.type_name) {
        fillet_definition(ctx, &object.type_name, properties)?
            .or_else(|| {
                (!ctx.has_unavailable())
                    .then(|| cached_shape_definition(properties))
                    .flatten()
            })
            .map_or_else(|| native_definition(ctx, &object.type_name, properties), Ok)?
    } else if is_chamfer(&object.type_name) {
        chamfer_definition(ctx, &object.type_name, properties, program_version)?
            .or_else(|| {
                (!ctx.has_unavailable())
                    .then(|| cached_shape_definition(properties))
                    .flatten()
            })
            .map_or_else(|| native_definition(ctx, &object.type_name, properties), Ok)?
    } else {
        native_definition(ctx, &object.type_name, properties)?
    };
    Ok(definition)
}
