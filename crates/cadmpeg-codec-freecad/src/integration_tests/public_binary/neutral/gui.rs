// SPDX-License-Identifier: Apache-2.0
//! Independent neutral consequences in the untouched, fresh source projection.
//!
//! Source projection appends each property's held slots in source order. The
//! oracle uses that BEFORE order and asserts the actual alias/slot cardinality;
//! it never consults the decoder's AFTER model or parses an identity spelling.

use crate::native::entry_contents::EntryContents;
use crate::native::{DocumentFacts, EntryRecord, GuiPropertyRecord, GuiViewProviderRecord};
use cadmpeg_core::CodecError;
use cadmpeg_ir::{scalar::FiniteReal, topology::Color, CadIr};
use cadmpeg_test_support::framing::freecad::binary::BinaryKind;

fn rgba(packed: u32, inverse: bool, opacity: Option<f32>) -> Result<Color, CodecError> {
    let [red, green, blue, alpha] = packed.to_be_bytes();
    let alpha = if inverse { 255 - alpha } else { alpha };
    Color::new(
        f32::from(red) / 255.0,
        f32::from(green) / 255.0,
        f32::from(blue) / 255.0,
        opacity.unwrap_or(f32::from(alpha) / 255.0),
    )
    .ok_or_else(|| CodecError::malformed("independent RGBA is outside its domain"))
}
fn scalar(value: u32, inverse: bool) -> FiniteReal {
    FiniteReal::new(f64::from(if inverse { value ^ 0xff } else { value }))
        .expect("u32 is finite as f64")
}

pub(super) fn update(
    ir: &mut CadIr,
    entry: &EntryRecord,
    kind: BinaryKind,
) -> Result<(), CodecError> {
    if !matches!(kind, BinaryKind::Color | BinaryKind::Material { .. }) {
        return Ok(());
    }
    if !ir.scope.model {
        assert_eq!(
            serde_json::to_value(&ir.model).unwrap(),
            serde_json::to_value(cadmpeg_ir::document::Model::default()).unwrap(),
            "a decode without Model scope has no neutral records"
        );
        return Ok(());
    }
    let Some(namespace) = ir.native.namespace("fcstd") else {
        return Ok(());
    };
    let documents = namespace.arena_as::<DocumentFacts>("document")?;
    let inverse = documents
        .first()
        .and_then(|document| document.program_version.as_deref())
        .is_some_and(|version| version.starts_with('0') || version.starts_with("1.0"));
    let properties = namespace.arena_as::<GuiPropertyRecord>("gui_properties")?;
    let providers = namespace.arena_as::<GuiViewProviderRecord>("gui_view_providers")?;
    let aliases = properties
        .iter()
        .filter(|property| {
            let runtime = match kind {
                BinaryKind::Color => {
                    property.type_name == "App::PropertyColorList"
                        && matches!(
                            property.name.as_str(),
                            "DiffuseColor" | "LineColorArray" | "PointColorArray"
                        )
                }
                BinaryKind::Material { .. } => {
                    property.type_name == "App::PropertyMaterialList"
                        && property.name == "ShapeAppearance"
                }
                _ => false,
            };
            runtime
                && property
                    .side_entries
                    .iter()
                    .any(|name| name == entry.name())
                && property.values.iter().any(|value| {
                    value
                        .attributes
                        .get("file")
                        .is_some_and(|name| name == entry.name())
                })
                && providers
                    .iter()
                    .any(|provider| provider.id == property.owner && provider.object.is_some())
        })
        .count();
    let selected = entry
        .contents()
        .iter()
        .find(|contents| {
            matches!(
                (kind, contents),
                (BinaryKind::Color, EntryContents::Color { .. })
                    | (BinaryKind::Material { .. }, EntryContents::Material { .. })
            )
        })
        .ok_or_else(|| CodecError::malformed("independent GUI entry interpretation is absent"))?;
    let slots = match selected {
        EntryContents::Color { values } => values.iter().flatten().count(),
        EntryContents::Material { values, .. } => {
            values.iter().filter(|value| value.fixed.is_some()).count()
        }
        _ => 0,
    };
    let actual = ir
        .model
        .appearances
        .iter()
        .filter(|appearance| appearance.native_ref.as_deref() == Some(entry.id()))
        .count();
    let expected = aliases
        .checked_mul(slots)
        .ok_or_else(|| CodecError::malformed("GUI oracle alias cardinality overflows"))?;
    assert_eq!(
        actual,
        expected,
        "fresh BEFORE source-order appearances must equal actual aliases times held slots for {}",
        entry.name()
    );
    if slots == 0 {
        return Ok(());
    }
    for (index, appearance) in ir
        .model
        .appearances
        .iter_mut()
        .filter(|appearance| appearance.native_ref.as_deref() == Some(entry.id()))
        .enumerate()
    {
        let slot = index % slots;
        match selected {
            EntryContents::Color { values } => {
                let value = values
                    .iter()
                    .flatten()
                    .nth(slot)
                    .expect("cardinality proved");
                appearance.base_color = Some(rgba(*value, inverse, None)?);
            }
            EntryContents::Material { values, .. } => {
                let value = values
                    .iter()
                    .filter(|value| value.fixed.is_some())
                    .nth(slot)
                    .expect("cardinality proved");
                let material = value.fixed.as_ref().expect("held slot");
                let transparency = material.transparency.get();
                appearance.base_color = if (0.0..=1.0).contains(&transparency) {
                    Some(rgba(material.diffuse, inverse, Some(1.0 - transparency))?)
                } else {
                    None
                };
                for (key, value) in [
                    ("ambient_packed", scalar(material.ambient, inverse)),
                    ("diffuse_packed", scalar(material.diffuse, inverse)),
                    ("specular_packed", scalar(material.specular, inverse)),
                    ("emissive_packed", scalar(material.emissive, inverse)),
                    ("shininess", material.shininess.into()),
                    ("transparency", material.transparency.into()),
                ] {
                    let target = appearance
                        .properties
                        .iter_mut()
                        .find(|(name, _)| name.as_str() == key)
                        .ok_or_else(|| {
                            CodecError::malformed(
                                "fresh material projection omits an actual coefficient",
                            )
                        })?;
                    *target.1 = value;
                }
                appearance.asset_guid =
                    value.uuid.as_ref().filter(|uuid| !uuid.is_empty()).cloned();
            }
            _ => return Err(CodecError::malformed("non-GUI interpretation selected")),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadmpeg_ir::{Codec, DecodeOptions};
    use std::io::Cursor;

    #[test]
    fn source_order_oracle_handles_duplicate_values_and_actual_shared_entry_aliases() {
        let document=b"<Document SchemaVersion='4' ProgramVersion='1.1'><Objects Count='2'><Object name='A' type='Part::Feature'/><Object name='B' type='Part::Feature'/></Objects><ObjectData Count='2'><Object name='A'><Properties Count='0'/></Object><Object name='B'><Properties Count='0'/></Object></ObjectData></Document>";
        let gui=b"<Document SchemaVersion='1'><ViewProviderData Count='2'><ViewProvider name='A'><Properties Count='1'><Property name='DiffuseColor' type='App::PropertyColorList'><ColorList file='palette'/></Property></Properties></ViewProvider><ViewProvider name='B'><Properties Count='1'><Property name='DiffuseColor' type='App::PropertyColorList'><ColorList file='palette'/></Property></Properties></ViewProvider></ViewProviderData></Document>";
        let decode = |first: u32| {
            let payload = [
                2_u32.to_le_bytes(),
                first.to_le_bytes(),
                0x11223344_u32.to_le_bytes(),
            ]
            .concat();
            let archive = crate::test_support::test_archive::archive_entries(&[
                ("Document.xml", document),
                ("GuiDocument.xml", gui),
                ("palette", &payload),
            ]);
            crate::FcstdCodec
                .decode(&mut Cursor::new(archive), &DecodeOptions::default())
                .unwrap()
        };
        let before = decode(0x11223344);
        let after = decode(0);
        assert_eq!(before.ir().model.appearances.len(), 4);
        assert_eq!(
            before
                .source_fidelity()
                .parts()
                .values()
                .filter(|part| part.level().as_str() == "freecad.color_list_entry")
                .count(),
            1
        );
        assert_eq!(
            before
                .source_fidelity()
                .parts()
                .values()
                .filter(|part| part.level().as_str() == "freecad.color_list_color")
                .count(),
            2
        );
        let mut expected = before.ir().clone();
        let mut entries = expected
            .native
            .namespace("fcstd")
            .unwrap()
            .arena_as::<EntryRecord>("entries")
            .unwrap();
        let selected = entries
            .iter_mut()
            .find(|entry| entry.name() == "palette")
            .unwrap();
        let EntryContents::Color { values } = selected
            .contents
            .iter_mut()
            .find(|content| matches!(content, EntryContents::Color { .. }))
            .unwrap()
        else {
            unreachable!()
        };
        values[0] = Some(0);
        let selected = selected.clone();
        expected
            .native
            .namespace_mut("fcstd")
            .set_arena(
                &cadmpeg_test_support::service_decode_context(),
                "entries",
                &entries,
            )
            .unwrap();
        update(&mut expected, &selected, BinaryKind::Color).unwrap();
        super::super::super::exact(&expected, after.ir());
    }
}
