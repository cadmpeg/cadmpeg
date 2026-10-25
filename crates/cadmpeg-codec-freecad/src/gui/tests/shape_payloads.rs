// SPDX-License-Identifier: Apache-2.0
//! Exact-shape ZIP locators select the actual GUI property without an app surrogate.

use crate::brep::ShapePayloadRecord;
use crate::native::{GuiPropertyRecord, PropertyRecord};
use crate::test_support::test_archive::archive_entries;
use cadmpeg_ir::{Codec, DecodeOptions};
use std::io::Cursor;

#[test]
fn direct_gui_part_shape_uses_the_real_gui_property_as_its_brep_payload_owner() {
    let brep = b"CASCADE Topology V1, (c) Matra-Datavision\nLocations 0\nCurve2ds 0\nCurves 0\nPolygon3D 0\nPolygonOnTriangulations 0\nSurfaces 0\nTriangulations 0\nTShapes 1\nVe 0.001 1 2 3 0 0 1001000 *\n+1 0 *";
    let property =
        "<Property name='Shape' type='Part::PropertyPartShape'><Part file='Shape.brp'/></Property>";
    // First authenticate the exact same bytes and runtime grammar in their
    // application carrier, then demand the Appendix C GUI-side locator too.
    for gui in [false, true] {
        let app_property = if gui { "" } else { property };
        let gui_property = if gui { property } else { "" };
        let document = format!("<Document SchemaVersion='4' FileVersion='1'><Objects Count='1'><Object type='Part::Feature' name='A'/></Objects><ObjectData Count='1'><Object name='A'><Properties Count='{}'>{app_property}</Properties></Object></ObjectData></Document>", usize::from(!gui));
        let gui_document = format!("<Document SchemaVersion='1'><ViewProviderData Count='1'><ViewProvider name='A'><Properties Count='{}'>{gui_property}</Properties></ViewProvider></ViewProviderData></Document>", usize::from(gui));
        let bytes = archive_entries(&[
            ("Document.xml", document.as_bytes()),
            ("GuiDocument.xml", gui_document.as_bytes()),
            ("Shape.brp", brep),
        ]);
        let result = crate::FcstdCodec
            .decode(&mut Cursor::new(&bytes), &DecodeOptions::default())
            .unwrap();
        crate::test_support::framing::assert_xml_parts_agree(&bytes, &result);
        let namespace = result.ir().native.namespace("fcstd").unwrap();
        let owner = if gui {
            namespace
                .arena_as::<GuiPropertyRecord>("gui_properties")
                .unwrap()
                .into_iter()
                .find(|property| property.type_name == "Part::PropertyPartShape")
                .unwrap()
                .id
        } else {
            namespace
                .arena_as::<PropertyRecord>("properties")
                .unwrap()
                .into_iter()
                .find(|property| property.type_name == "Part::PropertyPartShape")
                .unwrap()
                .id
        };
        let payloads = namespace
            .arena_as::<ShapePayloadRecord>("shape_payloads")
            .unwrap();
        assert_eq!(
            payloads.len(),
            1,
            "GUI carrier={gui}: the direct Part locator must open Shape.brp"
        );
        assert_eq!(
            payloads[0].property, owner,
            "actual property owner; GUI carrier={gui}"
        );
        for level in [
            "freecad.brep_text",
            "freecad.brep_text_header",
            "freecad.brep_text_table",
            "freecad.brep_text_tshape",
            "freecad.brep_text_root",
        ] {
            assert!(
                result
                    .source_fidelity()
                    .parts()
                    .values()
                    .any(|part| part.level().as_str() == level),
                "GUI carrier={gui}: {level}"
            );
        }
        assert!(crate::test_support::validate_native(result.ir()).is_empty());
    }
}

fn faceted_gui_source(bound: bool) -> Vec<u8> {
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
    let count = usize::from(bound);
    let document = format!("<Document SchemaVersion='4' FileVersion='1'><Objects Count='{count}'>{object}</Objects><ObjectData Count='{count}'>{data}</ObjectData></Document>");
    let name = if bound { "A" } else { "Unbound" };
    let gui = format!("<Document SchemaVersion='1'><ViewProviderData Count='1'><ViewProvider name='{name}'><Properties Count='1'><Property name='Shape' type='Part::PropertyPartShape'><Part file='Shape.brp'/></Property></Properties></ViewProvider></ViewProviderData></Document>");
    let brep = b"CASCADE Topology V3, (c) Open Cascade\nLocations 0\nCurve2ds 0\nCurves 0\nPolygon3D 0\nPolygonOnTriangulations 0\nSurfaces 0\nTriangulations 1\n3 1 0 0 0.02 0 0 0 1 0 0 0 1 0 1 2 3\nTShapes 3\nFa 0 0.001 0 0 2 1 1001000 *\nSh 1001000 +3 0 *\nSo 1001000 +2 0 *\n+1 0 *";
    archive_entries(&[
        ("Document.xml", document.as_bytes()),
        ("GuiDocument.xml", gui.as_bytes()),
        ("Shape.brp", brep),
    ])
}

fn write_without_source(
    ir: &cadmpeg_ir::CadIr,
    has_geometry: bool,
    context: &str,
) -> cadmpeg_ir::codec::DecodeResult {
    use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
    let plan = crate::FcstdCodec
        .plan(
            EncodeInput::new(ir, None),
            TargetRequest::Explicit("fcstd:schema-4"),
        )
        .unwrap();
    if has_geometry {
        assert!(
            !plan
                .report()
                .losses
                .iter()
                .any(|loss| loss.code.local_code() == "output.empty"),
            "{context}: {:?}",
            plan.report().losses
        );
    }
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let result = crate::FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    crate::test_support::test_archive::assert_valid_document(result.ir());
    assert!(
        crate::test_support::validate_native(result.ir()).is_empty(),
        "{:?}",
        crate::test_support::validate_native(result.ir())
    );
    result
}

#[test]
fn bound_and_unbound_gui_shape_payloads_write_current_mesh_edits_and_deletions() {
    use cadmpeg_core::decode::DecodeDepth;
    for bound in [false, true] {
        for depth in [
            DecodeDepth::Container,
            DecodeDepth::Geometry,
            DecodeDepth::Full,
        ] {
            let bytes = faceted_gui_source(bound);
            let decoded = crate::FcstdCodec
                .decode(
                    &mut Cursor::new(&bytes),
                    &DecodeOptions {
                        depth,
                        ..DecodeOptions::default()
                    },
                )
                .unwrap();
            let namespace = decoded.ir().native.namespace("fcstd").unwrap();
            let properties = namespace
                .arena_as::<GuiPropertyRecord>("gui_properties")
                .unwrap();
            let property = properties
                .iter()
                .find(|property| property.type_name == "Part::PropertyPartShape")
                .unwrap();
            let payloads = namespace
                .arena_as::<ShapePayloadRecord>("shape_payloads")
                .unwrap();
            assert_eq!(payloads.len(), 1);
            assert_eq!(payloads[0].property, property.id);
            assert!(namespace
                .arena_as::<PropertyRecord>("properties")
                .unwrap()
                .iter()
                .all(|property| property.type_name != "Part::PropertyPartShape"));
            assert!(crate::test_support::validate_native(decoded.ir()).is_empty());
            if depth == DecodeDepth::Container {
                assert!(decoded.ir().model.bodies.is_empty());
                assert!(decoded.ir().model.tessellations.is_empty());
                let output = write_without_source(
                    decoded.ir(),
                    true,
                    &format!("bound={bound}, {depth:?}, native-only"),
                );
                assert_eq!(output.ir().model.bodies.len(), 1, "bound={bound}");
                assert_eq!(output.ir().model.tessellations.len(), 1, "bound={bound}");
                continue;
            }
            assert_eq!(
                decoded.ir().model.bodies.len(),
                1,
                "bound={bound}, {depth:?}"
            );
            assert_eq!(decoded.ir().model.tessellations.len(), 1);
            let association = &decoded.ir().model.tessellations[0].source_object;
            if bound {
                assert_eq!(
                    association.as_ref().unwrap().object_id.as_str(),
                    crate::native::native_id("object", "A")
                );
            } else {
                assert!(
                    association.is_none(),
                    "GUI-only properties have no application object association"
                );
            }
            let mut current = decoded.ir().clone();
            current.model.tessellations[0]
                .edit_vertices(|point| {
                    point.x += 42.0;
                    Ok(())
                })
                .unwrap();
            let output = write_without_source(
                &current,
                true,
                &format!("bound={bound}, {depth:?}, edited-mesh"),
            );
            assert_eq!(output.ir().model.bodies.len(), 1);
            assert_eq!(output.ir().model.tessellations.len(), 1);
            assert_eq!(
                output.ir().model.tessellations[0].vertices(),
                current.model.tessellations[0].vertices()
            );
            let namespace = output.ir().native.namespace("fcstd").unwrap();
            let properties = namespace
                .arena_as::<GuiPropertyRecord>("gui_properties")
                .unwrap();
            assert!(namespace
                .arena_as::<ShapePayloadRecord>("shape_payloads")
                .unwrap()
                .iter()
                .all(|payload| properties
                    .iter()
                    .any(|property| property.id == payload.property)));
            current.model.tessellations.clear();
            let deleted = write_without_source(
                &current,
                false,
                &format!("bound={bound}, {depth:?}, deleted-mesh"),
            );
            assert!(
                deleted.ir().model.tessellations.is_empty(),
                "bound={bound}, {depth:?}"
            );
            assert!(
                deleted.ir().model.bodies.is_empty(),
                "bound={bound}, {depth:?}"
            );
        }
    }
}
