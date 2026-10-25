// SPDX-License-Identifier: Apache-2.0
//! GUI kernel carriers keep their actual native owners through source-less output.

use super::*;

#[test]
fn source_less_gui_kernels_keep_gui_only_owners_and_current_neutral_edits() {
    let gui=b"<Document SchemaVersion='1'><ViewProviderData Count='1'><ViewProvider name='GuiOnly'><Properties Count='2'><Property name='Mesh' type='Mesh::PropertyMeshKernel'><Mesh file='mesh'/></Property><Property name='Points' type='Points::PropertyPointKernel'><Points file='points' mtrx='1 0 0 10 0 1 0 20 0 0 1 30 0 0 0 1'/></Property></Properties></ViewProvider></ViewProviderData></Document>";
    let mut mesh = Vec::new();
    mesh.extend_from_slice(&0xa0b0_c0d0_u32.to_le_bytes());
    mesh.extend_from_slice(&0x0001_0000_u32.to_le_bytes());
    mesh.resize(crate::layout::mesh_kernel_side_entry_header::LEN, 0);
    mesh.extend_from_slice(&3_u32.to_le_bytes());
    mesh.extend_from_slice(&1_u32.to_le_bytes());
    for scalar in [0.0_f32, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0] {
        mesh.extend_from_slice(&scalar.to_le_bytes());
    }
    for index in [0_u32, 1, 2, u32::MAX, u32::MAX, u32::MAX] {
        mesh.extend_from_slice(&index.to_le_bytes());
    }
    for scalar in [0.0_f32, 1.0, 0.0, 1.0, 0.0, 0.0] {
        mesh.extend_from_slice(&scalar.to_le_bytes());
    }
    let mut points = 1_u32.to_le_bytes().to_vec();
    for scalar in [1.0_f32, 2.0, 3.0] {
        points.extend_from_slice(&scalar.to_le_bytes());
    }
    let decoded = FcstdCodec
        .decode(
            &mut Cursor::new(archive_entries(&[
                ("Document.xml", DOCUMENT),
                ("GuiDocument.xml", gui),
                ("mesh", &mesh),
                ("points", &points),
            ])),
            &DecodeOptions::default(),
        )
        .unwrap();
    let mut current = decoded.ir().clone();
    assert_eq!(current.model.tessellations.len(), 1);
    assert_eq!(current.model.points.len(), 1);
    assert!(current.model.tessellations[0].source_object.is_none());
    assert!(current.model.points[0].source_object.is_none());
    let namespace = current.native.namespace("fcstd").unwrap();
    assert!(namespace
        .arena_as::<crate::native::PropertyRecord>("properties")
        .unwrap()
        .is_empty());
    let properties = namespace
        .arena_as::<crate::native::GuiPropertyRecord>("gui_properties")
        .unwrap();
    let kernels = namespace
        .arena_as::<crate::native::EntryRecord>("entries")
        .unwrap();
    assert_eq!(
        kernels
            .iter()
            .flat_map(|entry| entry.contents())
            .filter(|contents| matches!(
                contents,
                crate::native::entry_contents::EntryContents::Mesh { .. }
                    | crate::native::entry_contents::EntryContents::Points { .. }
            ))
            .count(),
        2
    );
    assert!(kernels
        .iter()
        .filter(|entry| entry.contents().iter().any(|contents| matches!(
            contents,
            crate::native::entry_contents::EntryContents::Mesh { .. }
                | crate::native::entry_contents::EntryContents::Points { .. }
        )))
        .all(|entry| properties.iter().any(|property| property
            .side_entries
            .iter()
            .any(|name| name == entry.name()))));
    current.model.tessellations[0]
        .edit_vertices(|point| {
            point.x += 5.0;
            Ok(())
        })
        .unwrap();
    current.model.points[0].set_position(
        cadmpeg_ir::features::FinitePoint3::new(cadmpeg_ir::math::Point3::new(42.0, 22.0, 33.0))
            .unwrap(),
    );
    let plan = FcstdCodec
        .plan(EncodeInput::new(&current, None), TargetRequest::Inherit)
        .unwrap();
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let output = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert_eq!(
        output.ir().model.tessellations[0].vertices(),
        current.model.tessellations[0].vertices()
    );
    assert_eq!(
        output.ir().model.points[0].position(),
        current.model.points[0].position()
    );
}
