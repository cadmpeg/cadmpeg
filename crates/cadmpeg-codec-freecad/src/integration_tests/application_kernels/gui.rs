// SPDX-License-Identifier: Apache-2.0
//! Source-less GUI kernel edits use current geometry and real native owners.

use super::*;
use crate::native::entry_contents::EntryContents;
use crate::native::EntryRecord;
use crate::native::{GuiPropertyRecord, PropertyRecord};
use std::io::Read;

fn source(bound: bool) -> Vec<u8> {
    let mut archive = zip::ZipArchive::new(Cursor::new(super::source())).unwrap();
    let mut mesh = Vec::new();
    archive
        .by_name("MeshKernel.bms")
        .unwrap()
        .read_to_end(&mut mesh)
        .unwrap();
    let mut cloud = Vec::new();
    archive
        .by_name("Cloud")
        .unwrap()
        .read_to_end(&mut cloud)
        .unwrap();
    let object = if bound {
        "<Object type='App::FeaturePython' name='A'/>"
    } else {
        ""
    };
    let data = if bound {
        "<Object name='A'><Properties Count='0'/></Object>"
    } else {
        ""
    };
    let document = format!("<Document SchemaVersion='4' FileVersion='1'><Objects Count='{}'>{object}</Objects><ObjectData Count='{}'>{data}</ObjectData></Document>", usize::from(bound), usize::from(bound));
    let name = if bound { "A" } else { "Unbound" };
    let gui = format!("<Document SchemaVersion='1'><ViewProviderData Count='1'><ViewProvider name='{name}'><Properties Count='2'><Property name='MeshData' type='Mesh::PropertyMeshKernel'><Mesh file='MeshKernel.bms'/></Property><Property name='PointsData' type='Points::PropertyPointKernel'><Points file='Cloud' mtrx='1 0 0 10 0 1 0 20 0 0 1 30 0 0 0 1'/></Property></Properties></ViewProvider></ViewProviderData></Document>");
    archive_entries(&[
        ("Document.xml", document.as_bytes()),
        ("GuiDocument.xml", gui.as_bytes()),
        ("MeshKernel.bms", &mesh),
        ("Cloud", &cloud),
    ])
}

#[test]
fn bound_and_unbound_gui_kernels_keep_their_real_property_owners_and_current_geometry() {
    for bound in [false, true] {
        let bytes = source(bound);
        for depth in [
            cadmpeg_core::decode::DecodeDepth::Container,
            cadmpeg_core::decode::DecodeDepth::Geometry,
            cadmpeg_core::decode::DecodeDepth::Full,
        ] {
            let decoded = FcstdCodec
                .decode(
                    &mut Cursor::new(&bytes),
                    &DecodeOptions {
                        depth,
                        ..DecodeOptions::default()
                    },
                )
                .unwrap();
            assert!(
                crate::test_support::validate_native(decoded.ir()).is_empty(),
                "{depth:?}: {:?}",
                crate::test_support::validate_native(decoded.ir())
            );
            let namespace = decoded.ir().native.namespace("fcstd").unwrap();
            let properties = namespace
                .arena_as::<GuiPropertyRecord>("gui_properties")
                .unwrap();
            let kernels = namespace.arena_as::<EntryRecord>("entries").unwrap();
            assert_eq!(
                kernels
                    .iter()
                    .flat_map(|entry| entry.contents())
                    .filter(|contents| matches!(
                        contents,
                        EntryContents::Mesh { .. } | EntryContents::Points { .. }
                    ))
                    .count(),
                2
            );
            assert!(kernels
                .iter()
                .filter(|entry| entry.contents().iter().any(|contents| matches!(
                    contents,
                    EntryContents::Mesh { .. } | EntryContents::Points { .. }
                )))
                .all(|entry| properties.iter().any(|property| property
                    .side_entries
                    .iter()
                    .any(|name| name == entry.name()))));
            assert!(namespace
                .arena_as::<PropertyRecord>("properties")
                .unwrap()
                .iter()
                .all(|property| !matches!(
                    property.type_name.as_str(),
                    "Mesh::PropertyMeshKernel" | "Points::PropertyPointKernel"
                )));
            if depth == cadmpeg_core::decode::DecodeDepth::Container {
                assert!(decoded.ir().model.points.is_empty());
                assert!(decoded.ir().model.tessellations.is_empty());
                let native_only = synthesize(decoded.ir());
                assert_eq!(native_only.ir().model.points.len(), 2);
                assert_eq!(native_only.ir().model.tessellations.len(), 1);
                continue;
            }
            assert_eq!(decoded.ir().model.points.len(), 2);
            assert_eq!(decoded.ir().model.tessellations.len(), 1);
            let mut current = decoded.ir().clone();
            current.model.points[0].set_position(
                cadmpeg_ir::features::FinitePoint3::new(cadmpeg_ir::math::Point3::new(
                    42.0, 22.0, 33.0,
                ))
                .unwrap(),
            );
            current.model.tessellations[0]
                .edit_vertices(|point| {
                    point.x += 5.0;
                    Ok(())
                })
                .unwrap();
            let after = synthesize(&current);
            assert_eq!(
                after
                    .ir()
                    .model
                    .points
                    .iter()
                    .map(|point| point.position())
                    .collect::<Vec<_>>(),
                current
                    .model
                    .points
                    .iter()
                    .map(|point| point.position())
                    .collect::<Vec<_>>()
            );
            assert_eq!(
                after.ir().model.tessellations[0].vertices(),
                current.model.tessellations[0].vertices()
            );
            current.model.points.clear();
            current.model.tessellations.clear();
            let namespace = current.native.namespace("fcstd").unwrap();
            assert_eq!(
                namespace
                    .arena_as::<GuiPropertyRecord>("gui_properties")
                    .unwrap(),
                properties
            );
            assert_eq!(
                namespace.arena_as::<EntryRecord>("entries").unwrap(),
                kernels
            );
            {
                let after = synthesize(&current);
                assert!(after.ir().model.points.is_empty());
                assert!(after.ir().model.tessellations.is_empty());
                assert!(after
                    .ir()
                    .native
                    .namespace("fcstd")
                    .unwrap()
                    .arena_as::<EntryRecord>("entries")
                    .unwrap()
                    .iter()
                    .flat_map(|entry| entry.contents())
                    .all(|contents| !matches!(
                        contents,
                        EntryContents::Mesh { .. } | EntryContents::Points { .. }
                    )));
            }
        }
    }
}

#[test]
fn empty_gui_point_kernel_keeps_its_actual_empty_carrier_and_transform_metadata() {
    for bound in [false, true] {
        let bytes =
            crate::test_support::test_archive::rewrite_entry(&source(bound), "Cloud", |_| {
                0_u32.to_le_bytes().to_vec()
            });
        let before = FcstdCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .unwrap();
        assert!(before.ir().model.points.is_empty());
        let before_properties = before
            .ir()
            .native
            .namespace("fcstd")
            .unwrap()
            .arena_as::<GuiPropertyRecord>("gui_properties")
            .unwrap();
        let original = before_properties
            .iter()
            .find(|property| property.name == "PointsData")
            .unwrap();
        let output = synthesize(before.ir());
        assert!(output.ir().model.points.is_empty());
        let namespace = output.ir().native.namespace("fcstd").unwrap();
        let properties = namespace
            .arena_as::<GuiPropertyRecord>("gui_properties")
            .unwrap();
        let property = properties
            .iter()
            .find(|property| property.name == "PointsData")
            .unwrap();
        assert_eq!(
            property.values[0].attributes.get("mtrx"),
            original.values[0].attributes.get("mtrx")
        );
        let rows = namespace.arena_as::<EntryRecord>("entries").unwrap();
        assert!(rows.iter().any(|entry| property
            .side_entries
            .iter()
            .any(|name| name == entry.name())
            && entry.contents().iter().any(
                |contents| matches!(contents, EntryContents::Points { points } if points.is_empty())
            )));
    }
}
