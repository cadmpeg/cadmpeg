// SPDX-License-Identifier: Apache-2.0
//! Source numeric formats and genuine object-attribute failure ownership.

use super::*;

#[test]
fn fixed_numeric_attributes_preserve_registered_float32_forms_and_curvature_slots() {
    use crate::test_support::test_archive::archive_entries;
    use cadmpeg_ir::attributes::AttributeNumericList;
    let document = "<Document SchemaVersion='4'><Properties Count='4'><Property name='Grey' type='Points::PropertyGreyValueList'><FloatList count='2'><F v='0.25'/><F v='0.5'/></FloatList></Property><Property name='Distance' type='Inspection::PropertyDistanceList'><FloatList file='distance'/></Property><Property name='Normals' type='Mesh::PropertyNormalList'><VectorList file='normal'/></Property><Property name='Curvature' type='Points::PropertyCurvatureList'><CurvatureList file='curvature'/></Property></Properties><Objects Count='0'/><ObjectData Count='0'/></Document>";
    let array = |values: &[f32]| {
        let mut bytes = 1_u32.to_le_bytes().to_vec();
        for value in values {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes
    };
    let distance = array(&[-0.5]);
    let normal = array(&[0., 1., 0.]);
    let curvature = array(&[2., -1., 1., 0., 0., 0., 1., 0.]);
    let input = archive_entries(&[
        ("Document.xml", document.as_bytes()),
        ("distance", &distance),
        ("normal", &normal),
        ("curvature", &curvature),
    ]);
    let decoded = FcstdCodec
        .decode(&mut Cursor::new(input), &DecodeOptions::default())
        .unwrap();
    let mut ir = decoded.ir().clone();
    assert_eq!(ir.model.attributes.len(), 4);
    assert!(ir.model.attributes.iter().any(|attribute| matches!(attribute.values.as_slice(), [AttributeValue::NumericList(AttributeNumericList::Curvature(Some(values)))] if values.len() == 1 && values[0].as_ref().unwrap().get() == [2.,-1.,1.,0.,0.,0.,1.,0.])));
    let expected = ir
        .model
        .attributes
        .iter()
        .map(|attribute| (attribute.name.clone(), attribute.values.clone()))
        .collect::<BTreeMap<_, _>>();
    let roundtrip = |ir: &cadmpeg_ir::CadIr| {
        let plan = FcstdCodec
            .plan(
                EncodeInput::new(ir, None),
                TargetRequest::Explicit("fcstd:schema-4"),
            )
            .unwrap();
        let losses = plan.report().losses.clone();
        let mut bytes = Vec::new();
        plan.write_to(&mut bytes).unwrap();
        (
            FcstdCodec
                .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
                .unwrap(),
            losses,
        )
    };
    let (written, losses) = roundtrip(&ir);
    assert!(!losses
        .iter()
        .any(|loss| loss.code.local_code() == "geometry.coordinate-quantized"));
    for attribute in &written.ir().model.attributes {
        assert_eq!(attribute.values, expected[&attribute.name]);
    }
    let properties = written
        .ir()
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<crate::native::PropertyRecord>("properties")
        .unwrap();
    for (name, runtime) in [
        ("Grey", "Points::PropertyGreyValueList"),
        ("Distance", "Inspection::PropertyDistanceList"),
        ("Normals", "Mesh::PropertyNormalList"),
        ("Curvature", "Points::PropertyCurvatureList"),
    ] {
        assert_eq!(
            properties
                .iter()
                .find(|property| property.name == name)
                .unwrap()
                .type_name,
            runtime
        );
    }
    let grey = properties
        .iter()
        .find(|property| property.name == "Grey")
        .unwrap();
    assert!(!grey.values()[0].attributes.contains_key("file"));
    assert_eq!(
        grey.values()
            .iter()
            .filter(|value| value.tag == "F")
            .count(),
        2
    );
    let distance = ir
        .model
        .attributes
        .iter_mut()
        .find(|attribute| attribute.name == "Distance")
        .unwrap();
    distance.values = vec![AttributeValue::NumericList(AttributeNumericList::Float(
        Some(vec![Some(
            cadmpeg_ir::scalar::FiniteReal::new(0.1).unwrap(),
        )]),
    ))];
    let (written, losses) = roundtrip(&ir);
    assert!(losses
        .iter()
        .any(|loss| loss.code.local_code() == "geometry.coordinate-quantized"));
    let distance = written
        .ir()
        .model
        .attributes
        .iter()
        .find(|attribute| attribute.name == "Distance")
        .unwrap();
    assert!(
        matches!(distance.values.as_slice(), [AttributeValue::NumericList(AttributeNumericList::Float(Some(values)))] if values[0].unwrap().get() == f64::from(0.1_f32))
    );
    let distance = ir
        .model
        .attributes
        .iter_mut()
        .find(|attribute| attribute.name == "Distance")
        .unwrap();
    let distance_id = distance.id.to_string();
    distance.values = vec![AttributeValue::NumericList(AttributeNumericList::Float(
        Some(vec![Some(
            cadmpeg_ir::scalar::FiniteReal::new(f64::MAX).unwrap(),
        )]),
    ))];
    let (written, losses) = roundtrip(&ir);
    assert!(!written
        .ir()
        .model
        .attributes
        .iter()
        .any(|attribute| attribute.name == "Distance"));
    assert_eq!(
        losses
            .iter()
            .filter(|loss| loss.code.local_code() == "attribute.not-transferred"
                && loss.message.contains(&distance_id))
            .count(),
        1
    );
    assert!(!losses
        .iter()
        .any(|loss| loss.code.local_code() == "source.undecoded-omitted"
            && loss.message.contains(&distance_id)));
}

#[test]
fn missing_object_collection_attribute_uses_its_actual_failed_child_home() {
    use cadmpeg_ir::source_fidelity::parts::PartState;
    for (kind, root, child) in [
        (
            "App::PropertyMap",
            "Map",
            "<Item key='key' value='goodvalue'/>",
        ),
        (
            "App::PropertyStringList",
            "StringList",
            "<String value='goodvalue'/>",
        ),
    ] {
        let document = format!("<Document SchemaVersion='4'><Objects Count='1'><Object name='Box' type='Part::Box'/></Objects><ObjectData Count='1'><Object name='Box'><Properties Count='5'><Property name='Length' type='App::PropertyLength'><Float value='2'/></Property><Property name='Width' type='App::PropertyLength'><Float value='3'/></Property><Property name='Height' type='App::PropertyLength'><Float value='4'/></Property><Property name='Extra' type='{kind}'><{root} count='1'>{child}</{root}></Property><Property name='Peer' type='App::PropertyString'><String value='unchanged'/></Property></Properties></Object></ObjectData></Document>");
        let before = FcstdCodec
            .decode(
                &mut Cursor::new(archive(&document)),
                &DecodeOptions::default(),
            )
            .unwrap();
        let original = before
            .ir()
            .model
            .attributes
            .iter()
            .find(|attribute| attribute.name == "Extra")
            .unwrap();
        let id = original.id.to_string();
        let damaged = document.replace("goodvalue", "&missing;");
        let after = FcstdCodec
            .decode(
                &mut Cursor::new(archive(&damaged)),
                &DecodeOptions::default(),
            )
            .unwrap();
        assert!(!after
            .ir()
            .model
            .attributes
            .iter()
            .any(|attribute| attribute.id.as_str() == id));
        assert!(after
            .ir()
            .model
            .attributes
            .iter()
            .any(|attribute| attribute.name == "Peer"
                && attribute.values == [AttributeValue::String("unchanged".into())]));
        let failures = after
            .source_fidelity()
            .parts()
            .values()
            .filter(|part| part.state() == &PartState::GivenUp {})
            .collect::<Vec<_>>();
        assert_eq!(failures.len(), 1);
        assert_eq!(failures[0].level().as_str(), "freecad.value_element");
        let errors = after
            .report()
            .losses
            .iter()
            .filter(|loss| loss.code.local_code() == "record.unreadable")
            .collect::<Vec<_>>();
        assert_eq!(errors.len(), 1);
        assert!(errors[0].message.contains(&id), "{:?}", errors[0]);
    }
}

#[test]
fn authored_null_peer_does_not_sponsor_an_attribute_that_could_not_form() {
    let document="<Document SchemaVersion='4'><Properties Count='2'><Property name='Links' type='App::PropertyLinkList'><LinkList count='2'><Link value=''/><Link value='Box'/></LinkList></Property><Property name='Peer' type='App::PropertyString'><String value='unchanged'/></Property></Properties><Objects Count='1'><Object name='Box' type='Part::Feature'/></Objects><ObjectData Count='1'><Object name='Box'><Properties Count='0'/></Object></ObjectData></Document>";
    let before = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert!(!before
        .ir()
        .model
        .attributes
        .iter()
        .any(|attribute| attribute.name == "Links"));
    let damaged = document.replace("<Link value='Box'/>", "<Link value='&missing;'/>");
    let after = FcstdCodec
        .decode(
            &mut Cursor::new(archive(&damaged)),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert!(!after
        .ir()
        .model
        .attributes
        .iter()
        .any(|attribute| attribute.name == "Links"));
    let properties = after
        .ir()
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<crate::native::PropertyRecord>("properties")
        .unwrap();
    let links = properties
        .iter()
        .find(|property| property.name == "Links")
        .unwrap();
    let unformed = crate::native::model_id("attribute", &links.id, "property");
    let errors = after
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code.local_code() == "record.unreadable")
        .collect::<Vec<_>>();
    assert_eq!(errors.len(), 1);
    assert!(!errors[0].message.contains(&unformed));
    assert!(after
        .ir()
        .model
        .attributes
        .iter()
        .any(|attribute| attribute.name == "Peer"));
}

#[test]
fn generic_value_carriers_keep_their_actual_attribute_failure_identity() {
    use cadmpeg_ir::source_fidelity::parts::PartState;
    for object in [false, true] {
        for (kind, value, damaged) in [
            (
                "Path::PropertyPath",
                "<Path value='first'/>",
                "<Path value='&missing;'/>",
            ),
            (
                "App::PropertyStringList",
                "<StringList count='2'><String value='first'/><String value='last'/></StringList>",
                "<StringList count='2'><String value='first'/><String value='&missing;'/></StringList>",
            ),
        ] {
            let properties = format!("<Properties Count='2'><Property name='Extra' type='{kind}'>{value}</Property><Property name='Peer' type='App::PropertyString'><String value='unchanged'/></Property></Properties>");
            let document = if object {
                format!("<Document SchemaVersion='4'><Objects Count='1'><Object name='Box' type='Part::Feature'/></Objects><ObjectData Count='1'><Object name='Box'>{properties}</Object></ObjectData></Document>")
            } else {
                format!("<Document SchemaVersion='4'>{properties}<Objects Count='0'/><ObjectData Count='0'/></Document>")
            };
            let before = FcstdCodec.decode(&mut Cursor::new(archive(&document)), &DecodeOptions::default()).unwrap();
            let attribute = before.ir().model.attributes.iter().find(|attribute| attribute.name == "Extra").unwrap();
            let id = attribute.id.to_string();
            if kind == "Path::PropertyPath" {
                assert_eq!(attribute.values, [AttributeValue::String("first".into())]);
            } else {
                assert_eq!(attribute.values, [AttributeValue::String("first".into()), AttributeValue::String("last".into())]);
            }
            let after = FcstdCodec.decode(&mut Cursor::new(archive(&document.replace(value, damaged))), &DecodeOptions::default()).unwrap();
            assert!(!after.ir().model.attributes.iter().any(|attribute| attribute.id.as_str() == id));
            assert!(after.ir().model.attributes.iter().any(|attribute| attribute.name == "Peer" && attribute.values == [AttributeValue::String("unchanged".into())]));
            let failed = after.source_fidelity().parts().values().filter(|part| part.state() == &PartState::GivenUp {}).collect::<Vec<_>>();
            assert_eq!(failed.len(), 1, "{object} {kind}: {:?}", after.report().losses);
            assert_eq!(failed[0].level().as_str(), if kind == "Path::PropertyPath" {"freecad.property_value"} else {"freecad.value_element"});
            let errors = after.report().losses.iter().filter(|loss| loss.code.local_code() == "record.unreadable").collect::<Vec<_>>();
            assert_eq!(errors.len(), 1);
            assert!(errors[0].message.contains(&id), "{object} {kind}: {:?}", errors[0]);
        }
    }
}

#[test]
fn attribute_value_fields_read_only_the_selected_local_name_winner() {
    let document = "<Document SchemaVersion='4'><Properties Count='1'><Property name='Meta' type='App::PropertyFloat'><Float xmlns:q='urn:vendor' value='97' q:value='2.5'/></Property></Properties><Objects Count='0'/><ObjectData Count='0'/></Document>";
    let decoded = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .unwrap();
    let attribute = decoded
        .ir()
        .model
        .attributes
        .iter()
        .find(|attribute| attribute.name == "Meta")
        .unwrap();
    assert_eq!(attribute.values, [AttributeValue::float(2.5).unwrap()]);
    let layout = &decoded.source_fidelity().record_layout()[attribute.id.as_str()];
    let field = |key: &str, value: &str| {
        let start = document.find(&format!("{key}='{value}'")).unwrap();
        let key_range = u64::try_from(start).unwrap()..u64::try_from(start + key.len()).unwrap();
        let start = start + key.len() + 2;
        (
            key_range,
            u64::try_from(start).unwrap()..u64::try_from(start + value.len()).unwrap(),
        )
    };
    let tag = document.find("<Float").unwrap() + 1;
    let mut expected =
        vec![u64::try_from(tag).unwrap()..u64::try_from(tag + "Float".len()).unwrap()];
    for (key, value) in [
        ("name", "Meta"),
        ("type", "App::PropertyFloat"),
        ("q:value", "2.5"),
    ] {
        let (key, value) = field(key, value);
        expected.extend([key, value]);
    }
    expected.sort_by_key(|range| range.start);
    assert_eq!(layout.reads()[layout.stream()], expected);
}

#[test]
fn identity_only_unknown_property_bodies_do_not_sponsor_generic_attributes() {
    for value in ["2.5", "&missing;"] {
        let document = format!("<Document SchemaVersion='4'><Properties Count='1'><Property name='Opaque' type='Vendor::PropertyFloat'><Float value='{value}'/></Property></Properties><Objects Count='0'/><ObjectData Count='0'/></Document>");
        let decoded = FcstdCodec
            .decode(
                &mut Cursor::new(archive(&document)),
                &DecodeOptions::default(),
            )
            .unwrap();
        assert!(decoded.ir().model.attributes.is_empty());
        let properties = decoded
            .ir()
            .native
            .namespace("fcstd")
            .unwrap()
            .arena_as::<crate::native::PropertyRecord>("properties")
            .unwrap();
        let property = properties
            .iter()
            .find(|property| property.name == "Opaque")
            .unwrap();
        assert_eq!(property.family, crate::native::PropertyFamily::Unknown);
        assert!(property.values().is_empty());
        let attribute = crate::native::model_id("attribute", &property.id, "property");
        assert!(!decoded
            .source_fidelity()
            .record_layout()
            .contains_key(&attribute));
        assert!(!decoded
            .report()
            .losses
            .iter()
            .any(|loss| loss.code.local_code() == "record.unreadable"));
        assert!(decoded
            .source_fidelity()
            .b1_spans()
            .values()
            .any(|spans| !spans.spans().is_empty()));
    }
}
