// SPDX-License-Identifier: Apache-2.0
//! Dynamic extension declarations and properties rebuilt from typed records.

use std::collections::HashMap;

use cadmpeg_core::CodecError;
use cadmpeg_ir::{report::loss::LossNote, CadIr};

use super::super::xml::{self, Object};
use crate::{
    loss::FreecadLossCode,
    native::{ExtensionRecord, PropertyRecord},
};

pub(crate) fn write(
    ir: &CadIr,
    objects: &mut [Object],
    names: &HashMap<String, String>,
    properties: &[PropertyRecord],
    losses: &mut Vec<LossNote>,
    outcomes: &mut cadmpeg_ir::codec::write::NativeDispositions,
) -> Result<(), CodecError> {
    let Some(namespace) = ir.native.namespace("fcstd") else {
        return Ok(());
    };
    let mut extensions = namespace.arena_as::<ExtensionRecord>("extensions")?;
    extensions.sort_by_key(|extension| extension.order);
    let owners = namespace.arena_as::<crate::native::ObjectRecord>("objects")?;
    for extension in extensions.into_iter().filter(|extension| {
        owners
            .iter()
            .any(|object| object.id().as_str() == extension.owner.as_str())
    }) {
        let Some(object) = names
            .get(&extension.owner)
            .and_then(|name| objects.iter_mut().find(|object| object.name == *name))
        else {
            losses.push(
                FreecadLossCode::ExportNativePropertyNotTransferred.note(format!(
                    "extension {} has no retained owning object",
                    extension.id
                )),
            );
            continue;
        };
        let mut content = Vec::new();
        for property in properties
            .iter()
            .filter(|property| property.owner == extension.id)
        {
            if let Some(value) = xml::native_named(property, names)? {
                content.push(value);
                super::super::dispositions::record(
                    outcomes,
                    "properties",
                    &property.id,
                    cadmpeg_ir::codec::write::NativeDisposition::Written,
                );
            } else {
                losses.push(
                    FreecadLossCode::ExportNativePropertyNotTransferred.note(format!(
                        "extension property {} has no independently writable typed payload",
                        property.id
                    )),
                );
            }
        }
        object.extensions.push(format!("<Extension name=\"{}\" type=\"{}\"><Properties Count=\"{}\">{}</Properties></Extension>", xml::escaped(&extension.name), xml::escaped(&extension.type_name), content.len(), content.join("")));
        super::super::dispositions::record(
            outcomes,
            "extensions",
            &extension.id,
            cadmpeg_ir::codec::write::NativeDisposition::Written,
        );
    }
    Ok(())
}
