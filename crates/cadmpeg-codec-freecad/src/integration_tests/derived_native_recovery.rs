// SPDX-License-Identifier: Apache-2.0
//! Late native projections preserve source-part recovery at every decode depth.
use super::*;
use cadmpeg_core::decode::DecodeDepth;

fn document(kind: &str, body: &str, count: usize) -> String {
    format!("<Document SchemaVersion='4' FileVersion='1'><Objects Count='2'><Object name='Target' type='App::FeaturePython'/><Object name='Owner' type='{kind}'/></Objects><ObjectData Count='2'><Object name='Target'><Properties Count='0'/></Object><Object name='Owner'><Properties Count='{count}'>{body}</Properties></Object></ObjectData></Document>")
}

fn depths() -> [DecodeDepth; 3] {
    [
        DecodeDepth::Container,
        DecodeDepth::Geometry,
        DecodeDepth::Full,
    ]
}

fn named_omission(result: &cadmpeg_ir::DecodeResult, id: &str) {
    assert_eq!(
        result
            .report()
            .losses
            .iter()
            .filter(
                |loss| loss.code.local_code() == "record.unreadable" && loss.message.contains(id)
            )
            .count(),
        1,
        "exactly one actual source failure sponsors {id}"
    );
    assert!(result
        .source_fidelity()
        .parts()
        .values()
        .any(|part| matches!(
            part.state(),
            cadmpeg_ir::source_fidelity::parts::PartState::GivenUp { .. }
        )));
}

#[test]
fn failed_attachment_placement_keeps_held_support_mode_and_offset_without_identity_frame() {
    let xml = document("App::FeaturePython", "<Property name='AttachmentSupport' type='App::PropertyLinkSubList'><LinkSubList count='1'><Link obj='Target' sub='Face1'/></LinkSubList></Property><Property name='MapMode' type='App::PropertyEnumeration'><Integer value='5'/></Property><Property name='Placement' type='App::PropertyPlacement'><PropertyPlacement Px='bad' Py='0' Pz='0' Q0='0' Q1='0' Q2='0' Q3='1'/></Property><Property name='AttachmentOffset' type='App::PropertyPlacement'><PropertyPlacement Px='2' Py='0' Pz='0' Q0='0' Q1='0' Q2='0' Q3='1'/></Property>",4);
    for depth in depths() {
        let result = FcstdCodec
            .decode(
                &mut Cursor::new(archive(&xml)),
                &DecodeOptions {
                    depth,
                    ..DecodeOptions::default()
                },
            )
            .unwrap();
        let rows = result
            .ir()
            .native
            .namespace("fcstd")
            .unwrap()
            .arena_as::<crate::native::AttachmentRecord>("attachments")
            .unwrap();
        assert_eq!(rows.len(), 1);
        let row = &rows[0];
        assert_eq!(row.map_mode.unwrap().to_string(), "5");
        assert_eq!(
            row.supports.as_ref().unwrap()[0].as_ref().unwrap().object(),
            Some("fcstd:native:object#Target")
        );
        assert!(row.placement().is_none());
        assert_eq!(row.offset().unwrap().rows()[0][3], 2.0);
        assert!(row.effective_frame().is_none());
        assert_valid_document(result.ir());
    }
}

#[test]
fn refused_only_attachment_headers_do_not_create_a_default_identity_attachment() {
    let property =
        "<Property name='MapMode' type='App::PropertyEnumeration'><Integer value='5'/></Property>";
    let xml = document("App::FeaturePython", &property.repeat(2), 2);
    for depth in depths() {
        let result = FcstdCodec
            .decode(
                &mut Cursor::new(archive(&xml)),
                &DecodeOptions {
                    depth,
                    ..DecodeOptions::default()
                },
            )
            .unwrap();
        let rows = result
            .ir()
            .native
            .namespace("fcstd")
            .unwrap()
            .arena_as::<crate::native::AttachmentRecord>("attachments")
            .unwrap_or_default();
        assert!(rows.is_empty());
        named_omission(&result, "fcstd:native:attachment#Owner");
        assert_valid_document(result.ir());
    }
}

#[test]
fn failed_literal_annotation_item_does_not_pack_surviving_text_into_a_new_annotation() {
    let xml = document("App::Annotation", "<Property name='LabelText' type='App::PropertyStringList'><StringList count='3'><String value='first'/><String/><String value='last'/></StringList></Property><Property name='Position' type='App::PropertyVector'><PropertyVector valueX='1' valueY='2' valueZ='3'/></Property>",2);
    for depth in depths() {
        let result = FcstdCodec
            .decode(
                &mut Cursor::new(archive(&xml)),
                &DecodeOptions {
                    depth,
                    ..DecodeOptions::default()
                },
            )
            .unwrap();
        let namespace = result.ir().native.namespace("fcstd").unwrap();
        assert!(namespace
            .arena_as::<crate::native::SemanticAnnotationRecord>("annotations")
            .unwrap_or_default()
            .is_empty());
        let properties = namespace
            .arena_as::<crate::native::PropertyRecord>("properties")
            .unwrap();
        let text = properties
            .iter()
            .find(|property| property.name == "LabelText")
            .unwrap();
        assert_eq!(
            text.values()
                .iter()
                .filter_map(|value| value.attributes.get("value"))
                .map(String::as_str)
                .collect::<Vec<_>>(),
            ["first", "last"]
        );
        assert!(properties
            .iter()
            .find(|property| property.name == "Position")
            .unwrap()
            .body
            .is_some());
        named_omission(&result, "fcstd:native:annotation#Owner");
        assert!(result.ir().model.semantic_annotations.is_empty());
        assert_valid_document(result.ir());
    }
}

fn joint_document(index: usize, extra: &str) -> String {
    document("App::FeaturePython",&format!("<Property name='JointType' type='App::PropertyEnumeration'><Integer value='{index}' CustomEnum='true'/><CustomEnumList count='3'><Enum value='Fixed'/><Enum/><Enum value='Revolute'/></CustomEnumList></Property><Property name='Reference1' type='App::PropertyXLinkSubHidden'><XLink name='Target' file=''/></Property><Property name='Reference2' type='App::PropertyXLinkSubHidden'><XLink name='Target' file=''/></Property>{extra}"),3+usize::from(!extra.is_empty()))
}

#[test]
fn custom_joint_kind_uses_original_enum_position_and_only_the_failed_selected_item_is_unavailable()
{
    for index in [0, 1, 2] {
        for depth in depths() {
            let result = FcstdCodec
                .decode(
                    &mut Cursor::new(archive(&joint_document(index, ""))),
                    &DecodeOptions {
                        depth,
                        ..DecodeOptions::default()
                    },
                )
                .unwrap();
            let rows = result
                .ir()
                .native
                .namespace("fcstd")
                .unwrap()
                .arena_as::<crate::native::joint::JointRecord>("joints")
                .unwrap_or_default();
            if index == 1 {
                assert!(rows.is_empty());
                named_omission(&result, "fcstd:native:joint#Owner");
            } else {
                assert_eq!(rows.len(), 1);
                let crate::native::joint::JointBody::Pair { kind, .. } = &rows[0].body else {
                    panic!("paired joint");
                };
                assert_eq!(kind.as_str(), if index == 0 { "Fixed" } else { "Revolute" });
            }
            assert_valid_document(result.ir());
        }
    }
}

#[test]
fn failed_joint_detach_keeps_native_relation_without_inventing_neutral_false() {
    let extra = "<Property name='Detach1' type='App::PropertyBool'><Bool value='bad'/></Property>";
    for depth in depths() {
        let result = FcstdCodec
            .decode(
                &mut Cursor::new(archive(&joint_document(0, extra))),
                &DecodeOptions {
                    depth,
                    ..DecodeOptions::default()
                },
            )
            .unwrap();
        let rows = result
            .ir()
            .native
            .namespace("fcstd")
            .unwrap()
            .arena_as::<crate::native::joint::JointRecord>("joints")
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].parameters().bool_value("Detach1"), None);
        assert!(result.ir().model.assembly_joints.is_empty());
        if depth != DecodeDepth::Container {
            named_omission(
                &result,
                &crate::native::model_id("joint", "fcstd:native:object#Owner", "constraint"),
            );
        }
        assert_valid_document(result.ir());
    }
}

#[test]
fn failed_optional_drawing_coordinate_keeps_readable_peer_without_a_default_position() {
    let xml = document("TechDraw::DrawViewPart", "<Property name='X' type='App::PropertyDistance'><Float value='nan'/></Property><Property name='Y' type='App::PropertyDistance'><Float value='12'/></Property><Property name='Scale' type='App::PropertyFloatConstraint'><Float value='2'/></Property>",3);
    for depth in depths() {
        let result = FcstdCodec
            .decode(
                &mut Cursor::new(archive(&xml)),
                &DecodeOptions {
                    depth,
                    ..DecodeOptions::default()
                },
            )
            .unwrap();
        let rows = result
            .ir()
            .native
            .namespace("fcstd")
            .unwrap()
            .arena_as::<crate::native::DrawingRecord>("drawings")
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert!(!rows[0].parameters.contains_key("X"));
        assert_eq!(rows[0].parameters["Y"], "12.0");
        assert_eq!(rows[0].parameters["Scale"], "2.0");
        for row in &result.ir().model.drawings {
            assert!(row.position.is_none());
            assert_eq!(row.scale.unwrap().get(), 2.0);
        }
        assert_valid_document(result.ir());
    }
}

#[test]
fn failed_optional_annotation_coordinate_keeps_literal_text_and_held_coordinate() {
    let xml = document("TechDraw::DrawViewAnnotation", "<Property name='Text' type='App::PropertyStringList'><StringList count='1'><String value='NOTE'/></StringList></Property><Property name='X' type='App::PropertyDistance'><Float value='nan'/></Property><Property name='Y' type='App::PropertyDistance'><Float value='12'/></Property>",3);
    for depth in depths() {
        let result = FcstdCodec
            .decode(
                &mut Cursor::new(archive(&xml)),
                &DecodeOptions {
                    depth,
                    ..DecodeOptions::default()
                },
            )
            .unwrap();
        let rows = result
            .ir()
            .native
            .namespace("fcstd")
            .unwrap()
            .arena_as::<crate::native::SemanticAnnotationRecord>("annotations")
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].text, ["NOTE"]);
        assert!(!rows[0].parameters.contains_key("X"));
        assert_eq!(rows[0].parameters["Y"], "12.0");
        for row in &result.ir().model.semantic_annotations {
            assert!(row.position.is_none());
            assert_eq!(row.text, ["NOTE"]);
        }
        assert_valid_document(result.ir());
    }
}

#[test]
fn failed_required_joint_frame_does_not_become_an_identity_connector() {
    let extra = "<Property name='Placement1' type='App::PropertyPlacement'><PropertyPlacement Px='bad' Py='0' Pz='0' Q0='0' Q1='0' Q2='0' Q3='1'/></Property>";
    for depth in depths() {
        let result = FcstdCodec
            .decode(
                &mut Cursor::new(archive(&joint_document(0, extra))),
                &DecodeOptions {
                    depth,
                    ..DecodeOptions::default()
                },
            )
            .unwrap();
        let rows = result
            .ir()
            .native
            .namespace("fcstd")
            .unwrap()
            .arena_as::<crate::native::joint::JointRecord>("joints")
            .unwrap_or_default();
        assert!(rows.is_empty());
        assert!(result.ir().model.assembly_joints.is_empty());
        named_omission(&result, "fcstd:native:joint#Owner");
        assert_valid_document(result.ir());
    }
}

#[test]
fn native_local_link_validation_uses_current_typed_identities_and_keeps_external_names_literal() {
    use cadmpeg_ir::schema::rewrite::identities;
    let xml = document(
        "App::FeaturePython",
        "<Property name='Link' type='App::PropertyLink'><Link value='Target'/></Property>",
        1,
    );
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive(&xml)),
            &DecodeOptions {
                depth: DecodeDepth::Container,
                ..DecodeOptions::default()
            },
        )
        .unwrap();
    assert!(crate::test_support::validate_native(result.ir()).is_empty());
    let mut ir = result.ir().clone();
    let ctx = cadmpeg_test_support::service_decode_context();
    let original_applications = result
        .ir()
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<serde_json::Value>("applications")
        .unwrap();
    let mut census_ids = std::collections::BTreeMap::new();
    for (index, application) in original_applications.iter().enumerate() {
        census_ids.insert(
            application["id"].as_str().unwrap().to_owned(),
            format!("authored:census:node#application-{index}"),
        );
        for (property, record) in application["property_records"]
            .as_array()
            .unwrap()
            .iter()
            .enumerate()
        {
            census_ids.insert(
                record["id"].as_str().unwrap().to_owned(),
                format!("authored:census:node#property-{index}-{property}"),
            );
        }
    }
    let rename = |id: &str| {
        Ok(match id {
            "fcstd:native:object#Target" => "authored:graph:node#target".to_owned(),
            "fcstd:native:object#Owner" => "authored:graph:node#owner".to_owned(),
            _ => census_ids.get(id).cloned().unwrap_or_else(|| id.to_owned()),
        })
    };
    let namespace = ir.native.namespace_mut("fcstd");
    let objects = namespace
        .arena_as::<crate::native::ObjectRecord>("objects")
        .unwrap()
        .into_iter()
        .map(|object| {
            let object = identities(
                &ctx,
                crate::native::object_declaration::ObjectDeclaration::new(object).unwrap(),
                rename,
            )
            .unwrap();
            serde_json::from_value::<crate::native::ObjectRecord>(
                serde_json::to_value(object).unwrap(),
            )
            .unwrap()
        })
        .collect::<Vec<_>>();
    namespace.set_arena(&ctx, "objects", &objects).unwrap();
    let properties = namespace
        .arena_as::<crate::native::PropertyRecord>("properties")
        .unwrap()
        .into_iter()
        .map(|property| {
            let property = identities(
                &ctx,
                crate::native::document_value::DocumentValue::new(property).unwrap(),
                rename,
            )
            .unwrap();
            serde_json::from_value::<crate::native::PropertyRecord>(
                serde_json::to_value(property).unwrap(),
            )
            .unwrap()
        })
        .collect::<Vec<_>>();
    namespace
        .set_arena(&ctx, "properties", &properties)
        .unwrap();
    let applications = namespace
        .arena_as::<crate::application::ApplicationRecord>("applications")
        .unwrap()
        .into_iter()
        .map(|record| identities(&ctx, record, rename).unwrap())
        .collect::<Vec<_>>();
    namespace
        .set_arena(&ctx, "applications", &applications)
        .unwrap();
    let actual_applications = namespace
        .arena_as::<serde_json::Value>("applications")
        .unwrap();
    for original in &original_applications {
        let current = actual_applications
            .iter()
            .find(|current| {
                current["id"].as_str()
                    == Some(rename(original["id"].as_str().unwrap()).unwrap().as_str())
            })
            .unwrap();
        assert_eq!(
            current["object"],
            rename(original["object"].as_str().unwrap()).unwrap()
        );
        for (current, original) in current["property_records"]
            .as_array()
            .unwrap()
            .iter()
            .zip(original["property_records"].as_array().unwrap())
        {
            assert_eq!(
                current["id"],
                rename(original["id"].as_str().unwrap()).unwrap()
            );
            assert_eq!(current["property"], original["property"]);
            assert_eq!(current["type_name"], original["type_name"]);
        }
    }
    assert!(crate::test_support::validate_native(&ir).is_empty());
    let mut invalid_census = actual_applications;
    let nested = invalid_census
        .iter_mut()
        .find_map(|application| {
            application["property_records"]
                .as_array_mut()
                .unwrap()
                .first_mut()
        })
        .unwrap();
    nested["id"] = "invalid application property identity".into();
    let mut invalid_ir = ir.clone();
    invalid_ir
        .native
        .namespace_mut("fcstd")
        .set_arena(&ctx, "applications", &invalid_census)
        .unwrap();
    assert!(crate::test_support::validate_native(&invalid_ir)
        .iter()
        .any(|finding| {
            finding
                .message
                .contains("application records do not match their typed objects")
        }));
    let mut broken = properties;
    let property = broken
        .iter_mut()
        .find(|property| property.name == "Link")
        .unwrap();
    let property_id = property.id.clone();
    let crate::native::PropertyBody::Persisted { links, .. } = property.body.as_mut().unwrap()
    else {
        panic!("held link");
    };
    links[0].as_mut().unwrap().set_object(
        cadmpeg_core::text::NonBlankString::new("authored:graph:node#missing").unwrap(),
    );
    ir.native
        .namespace_mut("fcstd")
        .set_arena(&ctx, "properties", &broken)
        .unwrap();
    assert!(crate::test_support::validate_native(&ir)
        .iter()
        .any(
            |finding| finding.entity.as_deref() == Some(property_id.as_str())
                && finding
                    .message
                    .contains("missing link target authored:graph:node#missing")
        ));

    let external=document("App::FeaturePython","<Property name='Link' type='App::PropertyXLink'><XLink file='external.FCStd' name='fcstd:native:object#NotLocal'/></Property>",1);
    let external = FcstdCodec
        .decode(
            &mut Cursor::new(archive(&external)),
            &DecodeOptions {
                depth: DecodeDepth::Container,
                ..DecodeOptions::default()
            },
        )
        .unwrap();
    assert!(crate::test_support::validate_native(external.ir()).is_empty());
    let properties = external
        .ir()
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<crate::native::PropertyRecord>("properties")
        .unwrap();
    let link = properties[0].links()[0].as_ref().unwrap();
    assert_eq!(link.object(), Some("fcstd:native:object#NotLocal"));
    assert_eq!(link.document_name(), Some("external.FCStd"));
    let applications = external
        .ir()
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<crate::application::ApplicationRecord>("applications")
        .unwrap();
    for record in applications {
        let original = serde_json::to_value(&record).unwrap();
        let current = identities(&ctx, record, |id| {
            Ok(if id == "fcstd:native:object#NotLocal" {
                "authored:graph:node#must-stay-external".into()
            } else {
                id.to_owned()
            })
        })
        .unwrap();
        assert_eq!(serde_json::to_value(current).unwrap(), original);
    }
}

#[test]
fn native_application_side_entry_references_require_an_actual_current_entry() {
    let xml=document("App::FeaturePython","<Property name='Blob' type='App::PropertyFileIncluded'><FileIncluded file='Payload.bin'/></Property>",1);
    let bytes = archive_entries(&[
        ("Document.xml", xml.as_bytes()),
        ("Payload.bin", b"actual payload"),
    ]);
    let decoded = FcstdCodec
        .decode(
            &mut Cursor::new(bytes),
            &DecodeOptions {
                depth: DecodeDepth::Container,
                ..DecodeOptions::default()
            },
        )
        .unwrap();
    assert!(crate::test_support::validate_native(decoded.ir()).is_empty());
    let namespace = decoded.ir().native.namespace("fcstd").unwrap();
    let properties = namespace
        .arena_as::<crate::native::PropertyRecord>("properties")
        .unwrap();
    let blob = properties
        .iter()
        .find(|property| property.name == "Blob")
        .unwrap();
    assert_eq!(blob.side_entries(), ["Payload.bin"]);
    let entries = namespace
        .arena_as::<crate::native::EntryRecord>("entries")
        .unwrap()
        .into_iter()
        .filter(|entry| entry.name() != "Payload.bin")
        .collect::<Vec<_>>();
    let mut broken = decoded.ir().clone();
    broken
        .native
        .namespace_mut("fcstd")
        .set_arena(
            &cadmpeg_test_support::service_decode_context(),
            "entries",
            &entries,
        )
        .unwrap();
    assert!(crate::test_support::validate_native(&broken)
        .iter()
        .any(
            |finding| finding.entity.as_deref() == Some(blob.id.as_str())
                && finding.message.contains("missing side entry Payload.bin")
        ));
}

#[test]
fn failed_array_rows_keep_prototypes_and_other_original_occurrence_slots() {
    for placement in [true, false] {
        let (role, runtime, tag) = if placement {
            (
                "PlacementList",
                "App::PropertyPlacementList",
                "PlacementList",
            )
        } else {
            ("ScaleList", "App::PropertyVectorList", "VectorList")
        };
        let extra = if placement {
            ""
        } else {
            "<Property name='PlacementList' type='App::PropertyPlacementList'><PlacementList file='GoodFrames'/></Property>"
        };
        let xml = document("App::Link", &format!("<Property name='LinkedObject' type='App::PropertyXLink'><XLink file='' name='Target'/></Property><Property name='LinkPlacement' type='App::PropertyPlacement'><PropertyPlacement Px='10' Py='0' Pz='0' Q0='0' Q1='0' Q2='0' Q3='1'/></Property><Property name='ElementCount' type='App::PropertyIntegerConstraint'><Integer value='3'/></Property><Property name='{role}' type='{runtime}'><{tag} file='Rows'/></Property>{extra}"), if placement { 4 } else { 5 });
        let mut payload = 3_u32.to_le_bytes().to_vec();
        for index in 0..3 {
            let scalars = if placement {
                vec![
                    if index == 1 {
                        f64::NAN
                    } else {
                        f64::from(index)
                    },
                    0.0,
                    0.0,
                    0.0,
                    0.0,
                    0.0,
                    1.0,
                ]
            } else {
                vec![if index == 1 { f64::NAN } else { 1.0 }, 1.0, 1.0]
            };
            for scalar in scalars {
                payload.extend_from_slice(&scalar.to_le_bytes());
            }
        }
        let mut good_frames = 3_u32.to_le_bytes().to_vec();
        for index in 0..3 {
            for value in [f64::from(index), 0.0, 0.0, 0.0, 0.0, 0.0, 1.0] {
                good_frames.extend_from_slice(&value.to_le_bytes());
            }
        }
        let mut entries = vec![("Document.xml", xml.as_bytes()), ("Rows", &payload)];
        if !placement {
            entries.push(("GoodFrames", &good_frames));
        }
        let bytes = archive_entries(&entries);
        for depth in depths() {
            let result = FcstdCodec
                .decode(
                    &mut Cursor::new(&bytes),
                    &DecodeOptions {
                        depth,
                        ..DecodeOptions::default()
                    },
                )
                .unwrap();
            let native = result.ir().native.namespace("fcstd").unwrap();
            let products = native
                .arena_as::<crate::native::ProductNodeRecord>("product_nodes")
                .unwrap();
            let owner = crate::native::native_id("object", "Owner");
            let product = products
                .iter()
                .find(|product| product.object == owner)
                .expect("partial native product survives");
            assert!(product.element_transforms_known());
            assert!(product.element_scales_known());
            if placement {
                assert_eq!(product.element_transforms().len(), 3);
                assert_eq!(product.element_transforms()[0].unwrap().rows()[0][3], 0.0);
                assert!(product.element_transforms()[1].is_none());
                assert_eq!(product.element_transforms()[2].unwrap().rows()[0][3], 2.0);
            } else {
                assert_eq!(product.element_scales().len(), 3);
                assert_eq!(product.element_scales()[0].unwrap().get(), [1.0; 3]);
                assert!(product.element_scales()[1].is_none());
                assert_eq!(product.element_scales()[2].unwrap().get(), [1.0; 3]);
            }
            if depth != DecodeDepth::Container {
                let occurrences = result
                    .ir()
                    .model
                    .occurrences
                    .iter()
                    .filter(|row| row.native_ref.as_deref() == Some(&owner))
                    .collect::<Vec<_>>();
                assert_eq!(
                    occurrences
                        .iter()
                        .map(|row| row.ordinal)
                        .collect::<Vec<_>>(),
                    [0, 1, 2]
                );
                for (index, occurrence) in occurrences.iter().enumerate() {
                    assert_eq!(
                        occurrence.id.as_str(),
                        crate::native::model_id("occurrence", &owner, index.to_string())
                    );
                    assert!(
                        matches!(&occurrence.prototype, cadmpeg_ir::products::PrototypeReference::Local { definition } if definition.as_str() == crate::native::model_id("product_definition", "fcstd:native:object#Target", "definition"))
                    );
                    if index == 1 {
                        assert!(occurrence.transform.is_none());
                    } else {
                        assert_eq!(
                            occurrence.transform.unwrap().affine_rows(),
                            [
                                [1.0, 0.0, 0.0, 10.0 + index as f64],
                                [0.0, 1.0, 0.0, 0.0],
                                [0.0, 0.0, 1.0, 0.0],
                            ]
                        );
                    }
                }
                assert!(result
                    .ir()
                    .model
                    .product_definitions
                    .iter()
                    .any(|row| row.native_ref.as_deref() == Some("fcstd:native:object#Target")));
                let failures = result
                    .report()
                    .losses
                    .iter()
                    .filter(|loss| loss.code.local_code() == "record.unreadable")
                    .collect::<Vec<_>>();
                assert_eq!(failures.len(), 1);
                assert_eq!(
                    failures[0].provenance.as_ref().unwrap().offset,
                    if placement { 60 } else { 28 }
                );
            }
            assert!(crate::test_support::validate_native(result.ir()).is_empty());
            assert_valid_document(result.ir());
        }
    }
}

#[test]
fn failed_array_header_without_a_known_count_retains_unknown_arity() {
    let xml = document("App::Link", "<Property name='LinkedObject' type='App::PropertyXLink'><XLink file='' name='Target'/></Property><Property name='PlacementList' type='App::PropertyPlacementList'><PlacementList file='Rows'/></Property>", 2);
    for expanded in [true, false] {
        let xml = if expanded {
            xml.clone()
        } else {
            xml.replace("<Properties Count='2'><Property name='LinkedObject'", "<Properties Count='3'><Property name='ShowElement' type='App::PropertyBool'><Bool value='false'/></Property><Property name='LinkedObject'")
        };
        let bytes = archive_entries(&[("Document.xml", xml.as_bytes()), ("Rows", &[1, 2, 3])]);
        for depth in depths() {
            let result = FcstdCodec
                .decode(
                    &mut Cursor::new(&bytes),
                    &DecodeOptions {
                        depth,
                        ..DecodeOptions::default()
                    },
                )
                .unwrap();
            let products = result
                .ir()
                .native
                .namespace("fcstd")
                .unwrap()
                .arena_as::<crate::native::ProductNodeRecord>("product_nodes")
                .unwrap();
            let product = products
                .iter()
                .find(|row| row.object == "fcstd:native:object#Owner")
                .unwrap();
            assert!(!product.element_transforms_known());
            assert!(product.element_scales_known());
            assert_eq!(
                product.element_cardinality(),
                Some(crate::native::LinkArrayCardinality::Unknown)
            );
            let wire = serde_json::to_value(product).unwrap();
            assert!(wire["element_transforms"].is_null());
            assert_eq!(wire["element_scales"], serde_json::json!([]));
            assert_eq!(
                serde_json::from_value::<crate::native::ProductNodeRecord>(wire).unwrap(),
                *product
            );
            let occurrences = result
                .ir()
                .model
                .occurrences
                .iter()
                .filter(|row| row.native_ref.as_deref() == Some("fcstd:native:object#Owner"))
                .collect::<Vec<_>>();
            if expanded && depth != DecodeDepth::Container {
                assert_eq!(occurrences.len(), 1);
                assert_eq!(occurrences[0].name.as_deref(), Some("Owner"));
                assert_eq!(occurrences[0].ordinal, 0);
                assert_eq!(occurrences[0].transform, None);
                assert!(occurrences[0]
                    .link
                    .as_ref()
                    .is_none_or(|link| link.element_component().is_none()));
            } else {
                assert!(occurrences.is_empty());
            }
            if depth != DecodeDepth::Container {
                assert!(result
                    .ir()
                    .model
                    .product_definitions
                    .iter()
                    .any(|row| row.native_ref.as_deref() == Some("fcstd:native:object#Target")));
            }
            assert!(crate::test_support::validate_native(result.ir()).is_empty());
            assert_valid_document(result.ir());
        }
    }
}

#[test]
fn parameter_controls_and_bound_engine_text_have_their_own_exact_source_reads() {
    use cadmpeg_ir::features::{ExpressionToken, ParameterValue};
    for expression in ["Driver * 2", "Driver + 1"] {
        let control =
            "<Property name='Target' type='App::PropertyFloat'><Float value='10'/></Property>";
        let binding = format!(
            "<Expression path='Target' expression='{expression}' comment='unconsumed comment'/>"
        );
        let xml = format!("<Document SchemaVersion='4' FileVersion='1'><Objects Count='1'><Object name='Globals' type='App::VarSet'/></Objects><ObjectData Count='1'><Object name='Globals'><Properties Count='3'><Property name='Driver' type='App::PropertyFloat'><Float value='5'/></Property>{control}<Property name='ExpressionEngine' type='App::PropertyExpressionEngine'><ExpressionEngine count='1'>{binding}</ExpressionEngine></Property></Properties></Object></ObjectData></Document>");
        let decoded = FcstdCodec
            .decode(&mut Cursor::new(archive(&xml)), &DecodeOptions::default())
            .unwrap();
        let target = decoded
            .ir()
            .model
            .parameters
            .iter()
            .find(|parameter| parameter.name == "Target")
            .unwrap();
        let driver = decoded
            .ir()
            .model
            .parameters
            .iter()
            .find(|parameter| parameter.name == "Driver")
            .unwrap();
        assert!(matches!(target.value, Some(ParameterValue::Real(value)) if value.get() == 10.0));
        assert!(target
            .expression
            .tokens
            .iter()
            .any(|token| matches!(token, ExpressionToken::Parameter(id) if id == &driver.id)));
        let layout = &decoded.source_fidelity().record_layout()[target.id.as_str()];
        let origin = layout.offset() - u64::try_from(xml.find(control).unwrap()).unwrap();
        let mut expected = Vec::new();
        for (fragment, tokens) in [
            (
                control,
                vec![
                    "name",
                    "Target",
                    "type",
                    "App::PropertyFloat",
                    "Float",
                    "value",
                    "10",
                ],
            ),
            (
                "<Property name='ExpressionEngine' type='App::PropertyExpressionEngine'>",
                vec![
                    "Property",
                    "name",
                    "ExpressionEngine",
                    "type",
                    "App::PropertyExpressionEngine",
                ],
            ),
            (
                "<ExpressionEngine count='1'>",
                vec!["ExpressionEngine", "count", "1"],
            ),
            (
                binding.as_str(),
                vec!["Expression", "path", "Target", "expression", expression],
            ),
        ] {
            let base = xml.find(fragment).unwrap();
            let mut cursor = 0;
            for token in tokens {
                let start = cursor + fragment[cursor..].find(token).unwrap();
                let end = start + token.len();
                expected.push(
                    origin + u64::try_from(base + start).unwrap()
                        ..origin + u64::try_from(base + end).unwrap(),
                );
                cursor = end;
            }
        }
        expected.sort_by_key(|range| range.start);
        let mut merged: Vec<std::ops::Range<u64>> = Vec::new();
        for range in expected {
            if let Some(previous) = merged
                .last_mut()
                .filter(|previous| previous.end >= range.start)
            {
                previous.end = previous.end.max(range.end);
            } else {
                merged.push(range);
            }
        }
        assert_eq!(layout.reads()[layout.stream()], merged);
        let negative = &decoded.source_fidelity().record_layout()[driver.id.as_str()];
        for (fragment, token) in [("count='1'", "1"), ("path='Target'", "Target")] {
            let start = origin
                + u64::try_from(xml.find(fragment).unwrap() + fragment.find(token).unwrap())
                    .unwrap();
            let end = start + u64::try_from(token.len()).unwrap();
            assert!(
                negative.reads()[negative.stream()]
                    .iter()
                    .any(|range| range.start <= start && range.end >= end),
                "literal requires complete negative binding proof"
            );
        }
        let expression_start = origin
            + u64::try_from(
                xml.find(binding.as_str()).unwrap()
                    + binding.find("expression='").unwrap()
                    + "expression='".len(),
            )
            .unwrap();
        let expression_end = expression_start + u64::try_from(expression.len()).unwrap();
        assert!(
            negative.reads()[negative.stream()]
                .iter()
                .all(|range| range.end <= expression_start || range.start >= expression_end),
            "a nonmatching expression is not an input to Driver"
        );
        assert_eq!(target.expression.tokens.iter().filter(|token| matches!(token, ExpressionToken::Text(text) if text.contains(if expression.contains('*') { "* 2" } else { "+ 1" }))).count(), 1);
    }
}

#[test]
fn feature_labels_and_scalar_controls_use_original_fields_under_the_feature_owner() {
    for raw in ["A&amp;B", "Current"] {
        let label = format!("<Property name='Label' type='App::PropertyString'><String value='{raw}' extra='unconsumed label attribute'/></Property>");
        let length =
            "<Property name='Length' type='App::PropertyLength'><Float value='7'/></Property>";
        let xml = document("Part::Box", &format!("{label}{length}<Property name='Width' type='App::PropertyLength'><Float value='3'/></Property><Property name='Height' type='App::PropertyLength'><Float value='4'/></Property>"), 4);
        let decoded = FcstdCodec
            .decode(&mut Cursor::new(archive(&xml)), &DecodeOptions::default())
            .unwrap();
        let feature = decoded
            .ir()
            .model
            .features
            .iter()
            .find(|feature| {
                feature.native_ref.as_deref()
                    == Some(crate::native::native_id("object", "Owner").as_str())
            })
            .unwrap();
        assert_eq!(
            feature.name.as_deref(),
            Some(if raw == "A&amp;B" { "A&B" } else { raw })
        );
        let layout = &decoded.source_fidelity().record_layout()[feature.id.as_str()];
        let declaration = "<Object name='Owner' type='Part::Box'/>";
        let origin = layout.offset() - u64::try_from(xml.find(declaration).unwrap()).unwrap();
        for (fragment, value) in [(label.as_str(), raw), (length, "7")] {
            let start = xml.find(fragment).unwrap()
                + fragment.find(&format!("value='{value}'")).unwrap()
                + "value='".len();
            let range = origin + u64::try_from(start).unwrap()
                ..origin + u64::try_from(start + value.len()).unwrap();
            assert!(
                layout.reads()[layout.stream()]
                    .iter()
                    .any(|read| read.start <= range.start && range.end <= read.end),
                "feature must read its actual used field {value}"
            );
        }
        let opaque = xml.find("unconsumed label attribute").unwrap();
        let opaque = origin + u64::try_from(opaque).unwrap()
            ..origin + u64::try_from(opaque + "unconsumed label attribute".len()).unwrap();
        assert!(layout.reads()[layout.stream()]
            .iter()
            .all(|read| read.end <= opaque.start || opaque.end <= read.start));
    }
}

#[test]
fn same_owner_point_aliases_emit_the_current_union_once_without_applying_matrices_twice() {
    use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
    for translated in [false, true] {
        for edit in [false, true] {
            let document = "<Document SchemaVersion='4' FileVersion='1'><Objects Count='1'><Object name='Cloud' type='Points::Feature'/></Objects><ObjectData Count='1'><Object name='Cloud'><Properties Count='1'><Property name='Points' type='Points::PropertyPointKernel'><Points file='Cloud' mtrx='1 0 0 10 0 1 0 20 0 0 1 30 0 0 0 1'/></Property></Properties></Object></ObjectData></Document>";
            let matrix = if translated {
                "1 0 0 40 0 1 0 50 0 0 1 60 0 0 0 1"
            } else {
                "1 0 0 10 0 1 0 20 0 0 1 30 0 0 0 1"
            };
            let gui = format!("<Document SchemaVersion='1'><ViewProviderData Count='1'><ViewProvider name='Cloud'><Properties Count='1'><Property name='PointsData' type='Points::PropertyPointKernel'><Points file='Cloud' mtrx='{matrix}'/></Property></Properties></ViewProvider></ViewProviderData></Document>");
            let mut cloud = 2_u32.to_le_bytes().to_vec();
            for value in [1.0_f32, 2.0, 3.0, -1.0, -2.0, -3.0] {
                cloud.extend_from_slice(&value.to_le_bytes());
            }
            let source = crate::test_support::test_archive::archive_entries(&[
                ("Document.xml", document.as_bytes()),
                ("GuiDocument.xml", gui.as_bytes()),
                ("Cloud", &cloud),
            ]);
            let decoded = FcstdCodec
                .decode(&mut Cursor::new(source), &DecodeOptions::default())
                .unwrap();
            assert_eq!(decoded.ir().model.points.len(), 4);
            let mut current = decoded.ir().clone();
            if edit {
                current.model.points[0].set_position(
                    cadmpeg_ir::features::FinitePoint3::new(cadmpeg_ir::math::Point3::new(
                        75.0, 22.0, 33.0,
                    ))
                    .unwrap(),
                );
            }
            let expected = current
                .model
                .points
                .iter()
                .map(|point| point.position())
                .collect::<Vec<_>>();
            let plan = FcstdCodec
                .plan(
                    EncodeInput::new(&current, None),
                    TargetRequest::Explicit("fcstd:schema-4"),
                )
                .unwrap();
            assert_eq!(
                plan.report()
                    .losses
                    .iter()
                    .filter(|loss| loss.code.local_code() == "kernel.association-reduced")
                    .count(),
                1
            );
            let mut output = Vec::new();
            plan.write_to(&mut output).unwrap();
            let output = FcstdCodec
                .decode(&mut Cursor::new(output), &DecodeOptions::default())
                .unwrap();
            assert_eq!(
                output
                    .ir()
                    .model
                    .points
                    .iter()
                    .map(|point| point.position())
                    .collect::<Vec<_>>(),
                expected
            );
            let namespace = output.ir().native.namespace("fcstd").unwrap();
            assert!(namespace
                .arena_as::<crate::native::GuiPropertyRecord>("gui_properties")
                .unwrap()
                .iter()
                .all(|property| property.type_name != "Points::PropertyPointKernel"));
            assert_valid_document(output.ir());
        }
    }
}

#[test]
fn spreadsheet_and_census_admission_read_only_their_own_used_cell_and_dimension_fields() {
    let cell = "<Cell address='B2' content='5' alias='width' rowSpan='2' colSpan='3' opaque='not used by admission'/>";
    let dimensions = "<Property name='columnWidths' type='Spreadsheet::PropertyColumnWidths'><ColumnInfo Count='1'><Column name='B' width='81'/></ColumnInfo></Property>";
    let xml = document("Spreadsheet::Sheet", &format!("<Property name='cells' type='Spreadsheet::PropertySheet'><Cells Count='1'>{cell}</Cells></Property>{dimensions}"), 2);
    let result = FcstdCodec
        .decode(&mut Cursor::new(archive(&xml)), &DecodeOptions::default())
        .unwrap();
    assert_eq!(result.ir().model.spreadsheets.len(), 1);
    let sheet = &result.ir().model.spreadsheets[0];
    assert_eq!(sheet.cells()[0].address.a1(), "B2");
    assert_eq!(sheet.column_widths()[0].pixels, 81);
    assert_eq!(sheet.merged_ranges()[0].end().a1(), "D3");
    let census = result
        .ir()
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<crate::native::DesignCensusRecord>("design_census")
        .unwrap();
    let census = census
        .iter()
        .find(|record| record.object == "fcstd:native:object#Owner")
        .unwrap();
    for id in [sheet.id.as_str(), census.id.as_str()] {
        let layout = &result.source_fidelity().record_layout()[id];
        let declaration = "<Object name='Owner' type='Spreadsheet::Sheet'/>";
        let origin = layout.offset() - u64::try_from(xml.find(declaration).unwrap()).unwrap();
        for token in [
            "address='B2'",
            "rowSpan='2'",
            "colSpan='3'",
            "name='B'",
            "width='81'",
        ] {
            let start = xml.find(token).unwrap();
            let value = start + token.find('=').unwrap() + 2;
            let end = start + token.len() - 1;
            assert!(
                layout.reads()[layout.stream()]
                    .iter()
                    .any(|read| read.start <= origin + u64::try_from(value).unwrap()
                        && origin + u64::try_from(end).unwrap() <= read.end),
                "{id} must use its original {token}"
            );
        }
        for opaque in [
            "content='5'",
            "alias='width'",
            "opaque='not used by admission'",
        ] {
            let start = xml.find(opaque).unwrap();
            let range = origin + u64::try_from(start).unwrap()
                ..origin + u64::try_from(start + opaque.len()).unwrap();
            assert!(
                layout.reads()[layout.stream()]
                    .iter()
                    .all(|read| read.end <= range.start || range.end <= read.start),
                "{id} must not claim optional {opaque}"
            );
        }
    }
}

#[test]
fn shared_point_entries_keep_other_objects_raw_geometry_when_one_owner_is_edited_or_deleted() {
    use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
    let document = "<Document SchemaVersion='4' FileVersion='1'><Objects Count='2'><Object name='A' type='Points::Feature'/><Object name='B' type='Points::Feature'/></Objects><ObjectData Count='2'><Object name='A'><Properties Count='1'><Property name='Points' type='Points::PropertyPointKernel'><Points file='Cloud' mtrx='1 0 0 10 0 1 0 20 0 0 1 30 0 0 0 1'/></Property></Properties></Object><Object name='B'><Properties Count='1'><Property name='Points' type='Points::PropertyPointKernel'><Points file='Cloud' mtrx='1 0 0 40 0 1 0 50 0 0 1 60 0 0 0 1'/></Property></Properties></Object></ObjectData></Document>";
    let mut cloud = 2_u32.to_le_bytes().to_vec();
    for value in [1.0_f32, 2.0, 3.0, -1.0, -2.0, -3.0] {
        cloud.extend_from_slice(&value.to_le_bytes());
    }
    let source = crate::test_support::test_archive::archive_entries(&[
        ("Document.xml", document.as_bytes()),
        ("Cloud", &cloud),
    ]);
    let result = FcstdCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert_eq!(result.ir().model.points.len(), 4);
    for delete in [false, true] {
        let mut current = result.ir().clone();
        if delete {
            current.model.points.retain(|point| {
                point.source_object.as_ref().unwrap().object_id.as_str() != "fcstd:native:object#A"
            });
        } else {
            current
                .model
                .points
                .iter_mut()
                .find(|point| {
                    point.source_object.as_ref().unwrap().object_id.as_str()
                        == "fcstd:native:object#A"
                })
                .unwrap()
                .set_position(
                    cadmpeg_ir::features::FinitePoint3::new(cadmpeg_ir::math::Point3::new(
                        75.0, 22.0, 33.0,
                    ))
                    .unwrap(),
                );
        }
        let plan = FcstdCodec
            .plan(
                EncodeInput::new(&current, None),
                TargetRequest::Explicit("fcstd:schema-4"),
            )
            .unwrap();
        assert!(plan
            .report()
            .losses
            .iter()
            .all(|loss| loss.code.local_code() != "kernel.association-reduced"));
        let mut bytes = Vec::new();
        plan.write_to(&mut bytes).unwrap();
        let output = FcstdCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .unwrap();
        assert_eq!(
            output
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
            output
                .ir()
                .model
                .points
                .iter()
                .filter(
                    |point| point.source_object.as_ref().unwrap().object_id.as_str()
                        == "fcstd:native:object#B"
                )
                .count(),
            2
        );
    }
}

#[test]
fn same_owner_mesh_aliases_preserve_every_current_mesh_and_generate_extra_target_carriers() {
    use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
    let document = "<Document SchemaVersion='4' FileVersion='1'><Objects Count='1'><Object name='Mesh' type='Mesh::Feature'/></Objects><ObjectData Count='1'><Object name='Mesh'><Properties Count='1'><Property name='Mesh' type='Mesh::PropertyMeshKernel'><Mesh file='Shared.bms'/></Property></Properties></Object></ObjectData></Document>";
    let gui = "<Document SchemaVersion='1'><ViewProviderData Count='1'><ViewProvider name='Mesh'><Properties Count='1'><Property name='MeshData' type='Mesh::PropertyMeshKernel'><Mesh file='Shared.bms'/></Property></Properties></ViewProvider></ViewProviderData></Document>";
    let mut bytes = 0xa0b0_c0d0_u32.to_le_bytes().to_vec();
    bytes.extend_from_slice(&0x0001_0000_u32.to_le_bytes());
    bytes.resize(crate::layout::mesh_kernel_side_entry_header::LEN, 0);
    bytes.extend_from_slice(&3_u32.to_le_bytes());
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    for value in [0.0_f32, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    for value in [0_u32, 1, 2, u32::MAX, u32::MAX, u32::MAX] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    for value in [0.0_f32, 1.0, 0.0, 1.0, 0.0, 0.0] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    let source = archive_entries(&[
        ("Document.xml", document.as_bytes()),
        ("GuiDocument.xml", gui.as_bytes()),
        ("Shared.bms", &bytes),
    ]);
    let decoded = FcstdCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert_eq!(decoded.ir().model.tessellations.len(), 2);
    for population in [1, 2, 3] {
        let mut current = decoded.ir().clone();
        current.model.tessellations[0]
            .edit_vertices(|point| {
                point.x += 5.0;
                Ok(())
            })
            .unwrap();
        if population == 1 {
            current.model.tessellations.pop();
        } else if population == 3 {
            let mut extra = current.model.tessellations[1].clone();
            extra.id =
                cadmpeg_ir::tessellation::TessellationId::mint("synthetic:test:tessellation#extra")
                    .unwrap();
            extra
                .edit_vertices(|point| {
                    point.y += 9.0;
                    Ok(())
                })
                .unwrap();
            current.model.tessellations.push(extra);
        }
        let expected = current
            .model
            .tessellations
            .iter()
            .map(|mesh| (mesh.vertices(), mesh.triangles()))
            .collect::<Vec<_>>();
        let plan = FcstdCodec
            .plan(
                EncodeInput::new(&current, None),
                TargetRequest::Explicit("fcstd:schema-4"),
            )
            .unwrap();
        assert_eq!(
            plan.report()
                .losses
                .iter()
                .filter(|loss| loss.code.local_code() == "kernel.association-reduced")
                .count(),
            1
        );
        let mut bytes = Vec::new();
        plan.write_to(&mut bytes).unwrap();
        let output = FcstdCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .unwrap();
        let actual = output
            .ir()
            .model
            .tessellations
            .iter()
            .map(|mesh| (mesh.vertices(), mesh.triangles()))
            .collect::<Vec<_>>();
        assert_eq!(actual.len(), expected.len());
        for mesh in &expected {
            assert_eq!(actual.iter().filter(|actual| *actual == mesh).count(), 1);
        }
        assert!(output.ir().model.tessellations.iter().all(|mesh| mesh
            .source_object
            .as_ref()
            .unwrap()
            .object_id
            .as_str()
            == "fcstd:native:object#Mesh"));
        assert_valid_document(output.ir());
    }
    let mut unknown = bytes.clone();
    let start = crate::layout::mesh_kernel_side_entry_header::LEN + 8;
    unknown[start..start + 36 + 24].fill(0xff);
    for depth in [DecodeDepth::Container, DecodeDepth::Full] {
        let source = archive_entries(&[
            ("Document.xml", document.as_bytes()),
            ("GuiDocument.xml", gui.as_bytes()),
            ("Shared.bms", &unknown),
        ]);
        let current = FcstdCodec
            .decode(
                &mut Cursor::new(source),
                &DecodeOptions {
                    depth,
                    ..DecodeOptions::default()
                },
            )
            .unwrap();
        let expected = if depth == DecodeDepth::Full { 2 } else { 0 };
        assert_eq!(current.ir().model.tessellations.len(), expected);
        assert!(current
            .ir()
            .model
            .tessellations
            .iter()
            .all(|mesh| mesh.vertices().is_empty() && mesh.triangles().is_empty()));
        let plan = FcstdCodec
            .plan(
                EncodeInput::new(current.ir(), None),
                TargetRequest::Explicit("fcstd:schema-4"),
            )
            .unwrap();
        assert!(plan
            .report()
            .losses
            .iter()
            .any(|loss| loss.code.local_code() == "source.undecoded-omitted"));
        let mut output = Vec::new();
        plan.write_to(&mut output).unwrap();
        let output = FcstdCodec
            .decode(&mut Cursor::new(output), &DecodeOptions::default())
            .unwrap();
        assert_eq!(output.ir().model.tessellations.len(), expected);
        assert!(output
            .ir()
            .model
            .tessellations
            .iter()
            .all(|mesh| mesh.vertices().is_empty() && mesh.triangles().is_empty()));
    }
}

#[test]
fn qualified_control_winners_drive_features_parameters_and_engine_bindings_without_shadow_reads() {
    use cadmpeg_ir::features::{FeatureOperation, ParameterValue, PrimitiveSolidKind};
    let length = "<Property name='Length' type='App::PropertyLength'><Float value='7' n:value='8'/></Property>";
    let target = "<Property name='Target' type='App::PropertyFloat'><Float value='10' n:value='11'/></Property>";
    let binding = "<Expression path='wrong' n:path='Target' expression='Driver * 0' n:expression='Driver * 2'/>";
    let document = format!("<Document xmlns:n='urn:control' SchemaVersion='4' FileVersion='1'><Objects Count='2'><Object name='Box' type='Part::Box'/><Object name='Globals' type='App::VarSet'/></Objects><ObjectData Count='2'><Object name='Box'><Properties Count='3'>{length}<Property name='Width' type='App::PropertyLength'><Float value='3'/></Property><Property name='Height' type='App::PropertyLength'><Float value='4'/></Property></Properties></Object><Object name='Globals'><Properties Count='3'><Property name='Driver' type='App::PropertyFloat'><Float value='5'/></Property>{target}<Property name='ExpressionEngine' type='App::PropertyExpressionEngine'><ExpressionEngine count='0' n:count='1'>{binding}</ExpressionEngine></Property></Properties></Object></ObjectData></Document>");
    let decoded = FcstdCodec
        .decode(
            &mut Cursor::new(archive(&document)),
            &DecodeOptions::default(),
        )
        .unwrap();
    let feature = decoded
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.native_ref.as_deref() == Some("fcstd:native:object#Box"))
        .unwrap();
    let FeatureOperation::Primitive { solid, .. } = feature.definition().operation() else {
        panic!("known Box primitive");
    };
    let PrimitiveSolidKind::Box { length: actual, .. } = solid.kind() else {
        panic!("Box kind");
    };
    assert_eq!(decoded.ir().dimension_value(actual).unwrap().get(), 8.0);
    let parameter = decoded
        .ir()
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.name == "Target")
        .unwrap();
    assert!(matches!(parameter.value, Some(ParameterValue::Real(value)) if value.get() == 11.0));
    assert!(parameter.expression.tokens.iter().any(|token| matches!(token, cadmpeg_ir::features::ExpressionToken::Text(text) if text.contains("* 2"))));
    let parameter_reads = &decoded.source_fidelity().record_layout()[parameter.id.as_str()];
    let feature_reads = &decoded.source_fidelity().record_layout()[feature.id.as_str()];
    let origin = parameter_reads.offset() - u64::try_from(document.find(target).unwrap()).unwrap();
    for (layout, fragment, winner, shadow) in [
        (feature_reads, length, "n:value='8'", "value='7'"),
        (parameter_reads, target, "n:value='11'", "value='10'"),
        (
            parameter_reads,
            "<ExpressionEngine count='0' n:count='1'>",
            "n:count='1'",
            "count='0'",
        ),
        (parameter_reads, binding, "n:path='Target'", "path='wrong'"),
        (
            parameter_reads,
            binding,
            "n:expression='Driver * 2'",
            "expression='Driver * 0'",
        ),
    ] {
        let base = document.find(fragment).unwrap();
        let start = base + fragment.find(winner).unwrap();
        let value = start + winner.find('=').unwrap() + 2;
        let end = start + winner.len() - 1;
        assert!(layout.reads()[layout.stream()]
            .iter()
            .any(
                |range| range.start <= origin + u64::try_from(start).unwrap()
                    && origin + u64::try_from(start + winner.find('=').unwrap()).unwrap()
                        <= range.end
            ));
        assert!(layout.reads()[layout.stream()]
            .iter()
            .any(
                |range| range.start <= origin + u64::try_from(value).unwrap()
                    && origin + u64::try_from(end).unwrap() <= range.end
            ));
        let start = base + fragment.find(shadow).unwrap();
        let range = origin + u64::try_from(start).unwrap()
            ..origin + u64::try_from(start + shadow.len()).unwrap();
        assert!(layout.reads()[layout.stream()]
            .iter()
            .all(|read| read.end <= range.start || range.end <= read.start));
    }
}

#[test]
fn expression_only_cycles_resume_each_actual_feature_owner_for_native_fallback() {
    let mut xml = String::from("<Document SchemaVersion='4'><Objects Count='2'><Object name='First' type='Part::Box'/><Object name='Second' type='Part::Box'/></Objects><ObjectData Count='2'>");
    for (name, other, angle) in [("First", "Second", "41"), ("Second", "First", "82")] {
        xml.push_str(&format!("<Object name='{name}'><Properties Count='5'><Property name='Length' type='App::PropertyLength'><Float value='10'/></Property><Property name='Width' type='App::PropertyLength'><Float value='2'/></Property><Property name='Height' type='App::PropertyLength'><Float value='3'/></Property><Property name='Angle' type='App::PropertyFloat'><Float value='{angle}'/></Property><Property name='ExpressionEngine' type='App::PropertyExpressionEngine'><ExpressionEngine count='1'><Expression path='Length' expression='{other}.Length'/></ExpressionEngine></Property></Properties></Object>"));
    }
    xml.push_str("</ObjectData></Document>");
    let result = FcstdCodec
        .decode(&mut Cursor::new(archive(&xml)), &DecodeOptions::default())
        .unwrap();
    assert_eq!(result.ir().model.features.len(), 2);
    assert_eq!(
        result
            .report()
            .losses
            .iter()
            .filter(|loss| loss.code.local_code() == "feature.cyclic-history")
            .count(),
        2
    );
    for (name, angle, other_angle) in [("First", "41", "82"), ("Second", "82", "41")] {
        let object = crate::native::native_id("object", name);
        let feature = result
            .ir()
            .model
            .features
            .iter()
            .find(|feature| feature.native_ref.as_deref() == Some(&object))
            .unwrap();
        assert!(feature.dependencies.is_empty());
        assert!(
            matches!(feature.definition().operation(), cadmpeg_ir::features::FeatureOperation::Native { kind, parameters } if kind.as_str() == "Part::Box" && parameters.contains_key("Angle"))
        );
        let layout = &result.source_fidelity().record_layout()[feature.id.as_str()];
        for (value, held) in [(angle, true), (other_angle, false)] {
            let field = format!("value='{value}'");
            let start = u64::try_from(xml.find(&field).unwrap() + "value='".len()).unwrap();
            let end = start + u64::try_from(value.len()).unwrap();
            let credited = layout.reads().get(layout.stream()).is_some_and(|ranges| {
                ranges
                    .iter()
                    .any(|range| range.start <= start && range.end >= end)
            });
            assert_eq!(credited, held, "fallback source field belongs to {name}");
        }
        let census = result
            .ir()
            .native
            .namespace("fcstd")
            .unwrap()
            .arena_as::<crate::native::DesignCensusRecord>("design_census")
            .unwrap()
            .into_iter()
            .find(|record| record.object == object)
            .unwrap();
        assert_eq!(census.semantic_kind, "native");
        assert!(!census.post_processed);
        let census_layout = &result.source_fidelity().record_layout()[&census.id];
        let census_reads = &census_layout.reads()[census_layout.stream()];
        for (value, held) in [(angle, true), (other_angle, false)] {
            let field = format!("value='{value}'");
            let start = u64::try_from(xml.find(&field).unwrap() + "value='".len()).unwrap();
            let end = start + u64::try_from(value.len()).unwrap();
            assert_eq!(
                census_reads
                    .iter()
                    .any(|read| read.start <= start && read.end >= end),
                held
            );
        }
        for formula in ["First.Length", "Second.Length"] {
            let field = format!("expression='{formula}'");
            let start = u64::try_from(xml.find(&field).unwrap() + "expression='".len()).unwrap();
            let end = start + u64::try_from(formula.len()).unwrap();
            assert!(census_reads
                .iter()
                .any(|read| read.start <= start && read.end >= end));
        }
        let parameter = result
            .ir()
            .model
            .parameters
            .iter()
            .find(|parameter| {
                parameter.owner.as_ref() == Some(&feature.id) && parameter.name == "Length"
            })
            .unwrap();
        let target = parameter.expression.dependencies().next().unwrap();
        assert_ne!(target, &parameter.id);
        assert!(result
            .ir()
            .model
            .parameters
            .iter()
            .any(|parameter| &parameter.id == target));
    }
    assert_valid_document(result.ir());
    let findings = crate::test_support::validate_native(result.ir());
    assert!(findings.is_empty(), "{findings:?}");
}

#[test]
fn unavailable_engine_bindings_unform_actual_parameters_and_required_cross_owner_references() {
    let xml = "<Document SchemaVersion='4'><Objects Count='3'><Object name='A' type='App::VarSet'/><Object name='B' type='App::VarSet'/><Object name='Independent' type='PartDesign::Feature'/></Objects><ObjectData Count='3'><Object name='A'><Properties Count='3'><Property name='Driver' type='App::PropertyFloat'><Float value='5'/></Property><Property name='Target' type='App::PropertyFloat'><Float value='10'/></Property><Property name='ExpressionEngine' type='App::PropertyExpressionEngine'><ExpressionEngine count='1'><Expression path='Target' expression='Driver*2'/></ExpressionEngine></Property></Properties></Object><Object name='B'><Properties Count='2'><Property name='Dependent' type='App::PropertyFloat'><Float value='8'/></Property><Property name='ExpressionEngine' type='App::PropertyExpressionEngine'><ExpressionEngine count='1'><Expression path='Dependent' expression='A.Target+1'/></ExpressionEngine></Property></Properties></Object><Object name='Independent'><Properties Count='1'><Property name='Keep' type='App::PropertyFloat' group='Controls'><Float value='13'/></Property></Properties></Object></ObjectData></Document>";
    let before = FcstdCodec
        .decode(&mut Cursor::new(archive(xml)), &DecodeOptions::default())
        .unwrap();
    assert_eq!(before.ir().model.parameters.len(), 4);
    let kept_before = before
        .ir()
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.name == "Keep")
        .unwrap();
    let before_properties = before
        .ir()
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<crate::native::PropertyRecord>("properties")
        .unwrap();
    for (old, new, level) in [
        (
            "<ExpressionEngine count='1'>",
            "<ExpressionEngine count='x'>",
            "freecad.property_value",
        ),
        (
            "expression='Driver*2'",
            "expression='Driver&2'",
            "freecad.value_element",
        ),
        (
            "type='App::PropertyExpressionEngine'",
            "sype='App::PropertyExpressionEngine'",
            "freecad.property",
        ),
    ] {
        let changed = xml.replacen(old, new, 1);
        let after = FcstdCodec
            .decode(
                &mut Cursor::new(archive(&changed)),
                &DecodeOptions::default(),
            )
            .unwrap();
        assert_eq!(after.ir().model.parameters.len(), 1, "{level}");
        assert_eq!(
            serde_json::to_value(&after.ir().model.parameters[0]).unwrap(),
            serde_json::to_value(kept_before).unwrap(),
            "unrelated owner stays unchanged for {level}"
        );
        for name in ["Driver", "Target", "Dependent"] {
            let parameter = before
                .ir()
                .model
                .parameters
                .iter()
                .find(|parameter| parameter.name == name)
                .unwrap();
            named_omission(&after, parameter.id.as_str());
        }
        let failed = after
            .source_fidelity()
            .parts()
            .values()
            .filter(|part| {
                part.state() == &cadmpeg_ir::source_fidelity::parts::PartState::GivenUp {}
            })
            .collect::<Vec<_>>();
        assert_eq!(failed.len(), 1);
        assert_eq!(failed[0].level().as_str(), level);
        let errors = after
            .report()
            .losses
            .iter()
            .filter(|loss| loss.severity == cadmpeg_ir::report::Severity::Error)
            .collect::<Vec<_>>();
        assert_eq!(errors.len(), 1);
        let provenance = errors[0].provenance.as_ref().unwrap();
        assert_eq!(provenance.stream_name(), Some(failed[0].stream()));
        assert_eq!(provenance.offset, failed[0].offset());
        assert_eq!(provenance.byte_len(), Some(failed[0].byte_len()));
        let after_properties = after
            .ir()
            .native
            .namespace("fcstd")
            .unwrap()
            .arena_as::<crate::native::PropertyRecord>("properties")
            .unwrap();
        for property in before_properties.iter().filter(|property| {
            matches!(
                property.name.as_str(),
                "Driver" | "Target" | "Dependent" | "Keep"
            )
        }) {
            assert_eq!(
                after_properties
                    .iter()
                    .find(|after| after.id == property.id),
                Some(property)
            );
        }
        assert!(!after
            .report()
            .losses
            .iter()
            .any(|loss| loss.code.local_code() == "expression.reference-unresolved"));
        assert_valid_document(after.ir());
        assert!(crate::test_support::validate_native(after.ir()).is_empty());
    }
}

#[test]
fn unavailable_engine_expression_keeps_dimension_parameter_references_until_required_closure() {
    let xml = "<Document SchemaVersion='4'><Objects Count='1'><Object name='Box' type='Part::Box'/></Objects><ObjectData Count='1'><Object name='Box'><Properties Count='4'><Property name='Length' type='App::PropertyLength'><Float value='10'/></Property><Property name='Width' type='App::PropertyLength'><Float value='2'/></Property><Property name='Height' type='App::PropertyLength'><Float value='3'/></Property><Property name='ExpressionEngine' type='App::PropertyExpressionEngine'><ExpressionEngine count='1'><Expression path='Length' expression='Width*2'/></ExpressionEngine></Property></Properties></Object></ObjectData></Document>";
    let before = FcstdCodec
        .decode(&mut Cursor::new(archive(xml)), &DecodeOptions::default())
        .unwrap();
    assert_eq!(before.ir().model.parameters.len(), 3);
    assert_eq!(before.ir().model.features.len(), 1);
    let changed = xml.replacen("count='1'", "count='x'", 1);
    let after = FcstdCodec
        .decode(
            &mut Cursor::new(archive(&changed)),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert!(after.ir().model.parameters.is_empty());
    assert!(
        after.ir().model.features.is_empty(),
        "a required Parameter is not replaced with a literal dimension"
    );
    named_omission(&after, before.ir().model.features[0].id.as_str());
    for parameter in &before.ir().model.parameters {
        named_omission(&after, parameter.id.as_str());
    }
    assert_valid_document(after.ir());
    assert!(crate::test_support::validate_native(after.ir()).is_empty());
}
