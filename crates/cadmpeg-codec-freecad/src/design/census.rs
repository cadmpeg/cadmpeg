// SPDX-License-Identifier: Apache-2.0
//! A census classification is supplied by its own source operation getters.

use super::*;

pub(super) fn source(
    reader: &crate::property_reader::PropertyReader<'_, '_>,
    objects: &[ObjectRecord],
    properties: &[PropertyRecord],
    model: &cadmpeg_ir::document::Model,
    program_version: Option<&str>,
    bindings: &ExpressionBindings,
) -> Result<Vec<crate::native::DesignCensusRecord>, CodecError> {
    let mut properties_by_owner = HashMap::<&str, Vec<&PropertyRecord>>::new();
    for property in properties {
        properties_by_owner
            .entry(&property.owner)
            .or_default()
            .push(property);
    }
    let feature_ids = model
        .features
        .iter()
        .filter_map(|feature| {
            feature
                .native_ref
                .as_deref()
                .map(|native| (native, feature.id.clone()))
        })
        .collect::<HashMap<_, _>>();
    let sketch_ids = model
        .sketches
        .iter()
        .filter_map(|sketch| {
            sketch
                .native_ref
                .as_deref()
                .map(|native| (native, sketch.id.clone()))
        })
        .collect::<HashMap<_, _>>();
    let properties_all = properties;
    let mut records = Vec::new();
    for object in objects.iter().filter(|object| projects_feature(object)) {
        if !feature_ids.contains_key(object.id().as_str()) {
            return Err(CodecError::malformed(
                "design census owner has no held feature",
            ));
        }
        let id = crate::native::native_child_id("design-census", object.id(), "projection");
        let (actual_owner, actual_kind) =
            reader.begin_feature(object.id(), &id, &object.type_name)?;
        let mut actual = object.clone();
        actual.type_name = actual_kind;
        let properties = properties_by_owner
            .get(object.id().as_str())
            .map_or(&[][..], Vec::as_slice);
        let admission = if is_sketch(&actual.type_name) {
            sketch_admission::facts(reader, properties).map(|_| ())
        } else if is_spreadsheet(&actual.type_name) {
            spreadsheet_admission::facts(reader, properties).map(|_| ())
        } else {
            Ok(())
        };
        let selected = admission.and_then(|()| {
            operation::select(
                reader,
                &actual,
                properties,
                objects,
                &properties_by_owner,
                &feature_ids,
                &sketch_ids,
                model,
                program_version,
            )
        });
        let definition = match selected {
            Ok(definition) => definition,
            Err(CodecError::Malformed(_)) if reader.has_unavailable() => {
                native_definition(reader, &actual.type_name, properties)?
            }
            Err(error) => return Err(error),
        };
        let definition =
            match post_processed_definition(reader, definition, &actual.type_name, properties) {
                Ok(definition) => definition,
                Err(CodecError::Malformed(_)) if reader.has_unavailable() => {
                    native_definition(reader, &actual.type_name, properties)?
                }
                Err(error) => return Err(error),
            };
        let definition = if !is_spreadsheet(&actual.type_name)
            && cycle_sources::affected(
                reader,
                &feature_ids[object.id().as_str()],
                bindings,
                objects,
                properties_all,
                model,
            )? {
            native_definition(reader, &actual.type_name, properties)?
        } else {
            definition
        };
        let (semantic_kind, post_processed) = classification(&definition)?;
        records.push(crate::native::DesignCensusRecord {
            id,
            object: actual_owner,
            type_name: actual.type_name,
            semantic_kind,
            post_processed,
        });
    }
    records.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(records)
}

pub(super) fn classification(definition: &FeatureDefinition) -> Result<(String, bool), CodecError> {
    let (operation, post_processed) = match definition {
        FeatureDefinition::PostProcess { operation, .. } => (operation, true),
        FeatureDefinition::Operation(operation) => (operation, false),
    };
    let value = serde_json::to_value(operation).map_err(CodecError::malformed)?;
    let kind = value
        .get("definition")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| CodecError::malformed("design operation has no semantic family tag"))?;
    Ok((kind.to_owned(), post_processed))
}

#[cfg(test)]
mod tests;
