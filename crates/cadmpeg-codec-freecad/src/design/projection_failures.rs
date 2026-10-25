// SPDX-License-Identifier: Apache-2.0
//! Never-formed neutral records retain their actual consumed source cause.

use crate::native::{ObjectRecord, PropertyRecord};
use crate::property_reader::PropertyReader;
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::Model;
use cadmpeg_ir::source_fidelity::parts::GivenUpHome;

pub(crate) struct UnformedProjection {
    pub(crate) ids: Vec<String>,
    pub(crate) homes: Vec<GivenUpHome>,
}

pub(super) fn unformed(
    reader: &PropertyReader<'_, '_>,
    object: &ObjectRecord,
    properties: &[&PropertyRecord],
    model: &Model,
) -> Result<UnformedProjection, CodecError> {
    let mut homes = reader.take_unavailable();
    let mut ids = Vec::new();
    if !homes.is_empty() && super::is_sketch(&object.type_name) {
        let id = super::design_identity_text("sketch", object, format_args!(""));
        if !model.sketches.iter().any(|row| row.id.as_str() == id) {
            ids.push(id);
        }
    }
    if !homes.is_empty() && super::is_spreadsheet(&object.type_name) {
        let id = super::design_identity_text("spreadsheet", object, format_args!(""));
        if !model.spreadsheets.iter().any(|row| row.id.as_str() == id) {
            ids.push(id);
        }
    }
    let owned = object.type_name != "App::VarSet";
    // A failed scalar carrier can also prevent its independently named
    // parameter row. Use the exact producer eligibility and identity factory;
    // selectors and opaque/custom payloads do not acquire guessed parameters.
    for property in properties {
        let operation_parameter = owned
            && super::OPERATION_PARAMETER_NAMES.contains(&property.name.as_str())
            && matches!(
                crate::persistence::scalar_value_tag(&property.type_name),
                Some("Float" | "Integer")
            );
        if !operation_parameter && !super::custom_parameter_eligible(property, owned) {
            continue;
        }
        let id = super::parameter_id(object, &property.name)?;
        if model.parameters.iter().any(|parameter| parameter.id == id) {
            continue;
        }
        if reader.unavailable(property)? {
            ids.push(id.as_str().to_owned());
        }
    }
    homes.extend(reader.take_unavailable());
    Ok(UnformedProjection { ids, homes })
}
