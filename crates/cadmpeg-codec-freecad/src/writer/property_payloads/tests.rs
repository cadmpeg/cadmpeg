// SPDX-License-Identifier: Apache-2.0
//! W1 binary list regeneration uses current native content and omits failed slots.

use crate::native::entry_contents::{EntryContents, F32Values};
use crate::test_support::test_archive::archive_entries;
use crate::FcstdCodec;
use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
use cadmpeg_ir::{Codec, DecodeOptions};
use std::io::Cursor;

const DOCUMENT: &[u8] = b"<Document SchemaVersion='4' FileVersion='1'><Objects Count='1'><Object type='App::Feature' name='A'/></Objects><ObjectData Count='1'><Object name='A'><Properties Count='0'/></Object></ObjectData></Document>";
const GUI: &[u8] = b"<Document SchemaVersion='1'><ViewProviderData Count='1'><ViewProvider name='A'><Properties Count='1'><Property name='Values' type='App::PropertyFloatList'><FloatList file='values.bin'/></Property></Properties></ViewProvider></ViewProviderData></Document>";

fn source(values: &[f64]) -> Vec<u8> {
    let mut payload = u32::try_from(values.len()).unwrap().to_le_bytes().to_vec();
    for value in values {
        payload.extend_from_slice(&value.to_le_bytes());
    }
    archive_entries(&[
        ("Document.xml", DOCUMENT),
        ("GuiDocument.xml", GUI),
        ("values.bin", &payload),
    ])
}

#[test]
fn source_less_gui_binary_float_list_writes_current_typed_rows() {
    let decoded = FcstdCodec
        .decode(
            &mut Cursor::new(source(&[1.0, 2.0])),
            &DecodeOptions::default(),
        )
        .unwrap();
    let mut current = decoded.ir().clone();
    let namespace = current.native.namespace_mut("fcstd");
    let mut rows = namespace
        .arena_as::<crate::native::EntryRecord>("entries")
        .unwrap();
    let values = rows
        .iter_mut()
        .flat_map(|entry| &mut entry.contents)
        .find_map(|content| match content {
            EntryContents::Float { values } => Some(values),
            _ => None,
        })
        .unwrap();
    values[0] = Some(cadmpeg_ir::scalar::FiniteReal::new(9.0).unwrap());
    namespace
        .set_arena(
            &cadmpeg_test_support::service_decode_context(),
            "entries",
            &rows,
        )
        .unwrap();
    let plan = FcstdCodec
        .plan(EncodeInput::new(&current, None), TargetRequest::Inherit)
        .unwrap();
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let output = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let rows = output
        .ir()
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<crate::native::EntryRecord>("entries")
        .unwrap();
    let mut values = rows
        .iter()
        .flat_map(crate::native::EntryRecord::contents)
        .find_map(|content| match content {
            EntryContents::Float { values } => Some(
                values
                    .iter()
                    .enumerate()
                    .filter_map(|(order, value)| value.map(|value| (order, value.get())))
                    .collect::<Vec<_>>(),
            ),
            _ => None,
        })
        .unwrap();
    values.sort_by_key(|(order, _)| *order);
    assert_eq!(values, [(0, 9.0), (1, 2.0)]);
}

#[test]
fn source_less_sparse_binary_list_omits_only_failed_rows_with_a_loss() {
    let decoded = FcstdCodec
        .decode(
            &mut Cursor::new(source(&[1.0, f64::NAN, 3.0])),
            &DecodeOptions::default(),
        )
        .unwrap();
    let plan = FcstdCodec
        .plan(EncodeInput::new(decoded.ir(), None), TargetRequest::Inherit)
        .unwrap();
    assert!(plan
        .report()
        .losses
        .iter()
        .any(|loss| loss.code.local_code() == "source.undecoded-omitted"));
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let output = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let rows = output
        .ir()
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<crate::native::EntryRecord>("entries")
        .unwrap();
    let values = rows
        .iter()
        .flat_map(crate::native::EntryRecord::contents)
        .find_map(|content| match content {
            EntryContents::Float { values } => Some(values),
            _ => None,
        })
        .unwrap();
    assert_eq!(
        values
            .iter()
            .flatten()
            .map(|value| value.get())
            .collect::<Vec<_>>(),
        [1.0, 3.0]
    );
    assert_eq!(values.len(), 2);
}

#[test]
fn source_less_gui_extension_emits_nested_typed_property_under_its_owner() {
    let gui=b"<Document SchemaVersion='1'><ViewProviderData Count='1'><ViewProvider name='A'><Extensions Count='1'><Extension name='One' type='Gui::One'><Properties Count='1'><Property name='Nested' type='App::PropertyInteger'><Integer value='7'/></Property></Properties></Extension></Extensions><Properties Count='0'/></ViewProvider></ViewProviderData></Document>";
    let source = archive_entries(&[("Document.xml", DOCUMENT), ("GuiDocument.xml", gui)]);
    let decoded = FcstdCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let plan = FcstdCodec
        .plan(EncodeInput::new(decoded.ir(), None), TargetRequest::Inherit)
        .unwrap();
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let output = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let native = output.ir().native.namespace("fcstd").unwrap();
    let extensions = native
        .arena_as::<crate::native::ExtensionRecord>("extensions")
        .unwrap();
    let properties = native
        .arena_as::<crate::native::GuiPropertyRecord>("gui_properties")
        .unwrap();
    let extension = extensions
        .iter()
        .find(|extension| extension.name == "One")
        .unwrap();
    let nested = properties
        .iter()
        .find(|property| property.name == "Nested")
        .unwrap();
    assert_eq!(nested.owner, extension.id);
    assert_eq!(nested.values[0].attributes["value"], "7");
    assert!(output
        .ir()
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<crate::native::PropertyRecord>("properties")
        .unwrap()
        .iter()
        .all(|property| property.name != "Nested"));
}

fn auxiliary_source(kind: &str, tag: &str, payload: &[u8]) -> Vec<u8> {
    let gui=format!("<Document SchemaVersion='1'><ViewProviderData Count='1'><ViewProvider name='A'><Properties Count='1'><Property name='Values' type='{kind}'><{tag} file='values.bin'/></Property></Properties></ViewProvider></ViewProviderData></Document>");
    archive_entries(&[
        ("Document.xml", DOCUMENT),
        ("GuiDocument.xml", gui.as_bytes()),
        ("values.bin", payload),
    ])
}

#[test]
fn source_less_f32_vectors_write_current_typed_components() {
    let mut payload = 1_u32.to_le_bytes().to_vec();
    for value in [1.0_f32, 2.0, 3.0] {
        payload.extend_from_slice(&value.to_le_bytes());
    }
    let decoded = FcstdCodec
        .decode(
            &mut Cursor::new(auxiliary_source(
                "Mesh::PropertyNormalList",
                "VectorList",
                &payload,
            )),
            &DecodeOptions::default(),
        )
        .unwrap();
    let mut current = decoded.ir().clone();
    let namespace = current.native.namespace_mut("fcstd");
    let mut rows = namespace
        .arena_as::<crate::native::EntryRecord>("entries")
        .unwrap();
    let values = rows
        .iter_mut()
        .flat_map(|entry| &mut entry.contents)
        .find_map(|content| match content {
            EntryContents::F32 {
                values: F32Values::Vector(values),
            } => Some(values),
            _ => None,
        })
        .unwrap();
    values[0].as_mut().unwrap()[1] = cadmpeg_ir::scalar::FiniteBinary32::new(8.0).unwrap();
    namespace
        .set_arena(
            &cadmpeg_test_support::service_decode_context(),
            "entries",
            &rows,
        )
        .unwrap();
    let plan = FcstdCodec
        .plan(EncodeInput::new(&current, None), TargetRequest::Inherit)
        .unwrap();
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let output = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let rows = output
        .ir()
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<crate::native::EntryRecord>("entries")
        .unwrap();
    let values = rows
        .iter()
        .flat_map(crate::native::EntryRecord::contents)
        .find_map(|content| match content {
            EntryContents::F32 {
                values: F32Values::Vector(values),
            } => Some(values[0].unwrap().map(|value| value.get())),
            _ => None,
        })
        .unwrap();
    assert_eq!(values, [1.0, 8.0, 3.0]);
}

#[test]
fn source_less_mesh_material_writes_held_arrays_and_reports_one_failed_float() {
    let mut payload = 2_u32.to_le_bytes().to_vec();
    for color in [1_u32, 2, 3, 4] {
        payload.extend_from_slice(&1_u32.to_le_bytes());
        payload.extend_from_slice(&color.to_le_bytes());
    }
    payload.extend_from_slice(&2_u32.to_le_bytes());
    payload.extend_from_slice(&0.5_f32.to_le_bytes());
    payload.extend_from_slice(&f32::NAN.to_le_bytes());
    payload.extend_from_slice(&1_u32.to_le_bytes());
    payload.extend_from_slice(&0.25_f32.to_le_bytes());
    let decoded = FcstdCodec
        .decode(
            &mut Cursor::new(auxiliary_source(
                "Mesh::PropertyMaterial",
                "Material",
                &payload,
            )),
            &DecodeOptions::default(),
        )
        .unwrap();
    let plan = FcstdCodec
        .plan(EncodeInput::new(decoded.ir(), None), TargetRequest::Inherit)
        .unwrap();
    assert!(plan
        .report()
        .losses
        .iter()
        .any(|loss| loss.code.local_code() == "source.undecoded-omitted"));
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let output = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let rows = output
        .ir()
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<crate::native::EntryRecord>("entries")
        .unwrap();
    let (binding, ambient, diffuse, specular, emissive, shininess, transparency) = rows
        .iter()
        .flat_map(crate::native::EntryRecord::contents)
        .find_map(|content| match content {
            EntryContents::MeshMaterial {
                binding,
                ambient,
                diffuse,
                specular,
                emissive,
                shininess,
                transparency,
            } => Some((
                binding,
                ambient,
                diffuse,
                specular,
                emissive,
                shininess,
                transparency,
            )),
            _ => None,
        })
        .unwrap();
    assert_eq!(*binding, Some(2));
    assert_eq!(shininess.as_ref().unwrap().len(), 1);
    assert_eq!(
        [ambient, diffuse, specular, emissive]
            .iter()
            .map(|values| values.as_ref().unwrap().iter().flatten().count())
            .sum::<usize>(),
        4
    );
    assert_eq!(transparency.as_ref().unwrap()[0].unwrap().get(), 0.25);
}

mod expression_links;
mod kernels;
