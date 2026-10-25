// SPDX-License-Identifier: Apache-2.0
//! Current neutral attribute values own source-less property emission.

use crate::test_support::test_archive::archive;
use crate::FcstdCodec;
use cadmpeg_ir::attributes::AttributeValue;
use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
use cadmpeg_ir::{Codec, DecodeOptions};
use std::collections::BTreeMap;
use std::io::Cursor;

#[test]
fn source_less_attributes_follow_native_refs_after_id_rewrite_and_value_edits() {
    let matrix = (1..=4)
        .flat_map(|row| (1..=4).map(move |column| format!("a{row}{column}='{}'", row * column)))
        .collect::<Vec<_>>()
        .join(" ");
    let document = format!(
        "<Document SchemaVersion='4' FileVersion='1'><Properties Count='8'>\
         <Property name='Matrix' type='App::PropertyMatrix'><PropertyMatrix {matrix}/></Property>\
         <Property name='Rotation' type='App::PropertyRotation'><PropertyRotation Ox='0' Oy='1' Oz='0' A='90'/></Property>\
         <Property name='Bits' type='App::PropertyBoolList'><BoolList value='101'/></Property>\
         <Property name='Ints' type='App::PropertyIntegerList'><IntegerList count='2'><I v='1'/><I v='2'/></IntegerList></Property>\
         <Property name='Set' type='App::PropertyIntegerSet'><IntegerSet count='2'><I v='1'/><I v='2'/></IntegerSet></Property>\
         <Property name='Strings' type='App::PropertyStringList'><StringList count='1'><String value='before'/></StringList></Property>\
         <Property name='Json' type='App::PropertyPythonObject'><Python json='yes' value='4'/></Property>\
         <Property name='Text' type='App::PropertyString'><String>before</String></Property>\
         </Properties><Objects Count='0'/><ObjectData Count='0'/></Document>"
    );
    let decoded = FcstdCodec
        .decode(
            &mut Cursor::new(archive(&document)),
            &DecodeOptions::default(),
        )
        .unwrap();
    let mut ir = decoded.ir().clone();
    assert_eq!(ir.model.attributes.len(), 8);
    let expected = BTreeMap::from([
        (
            "Matrix",
            vec![AttributeValue::vector((0..16).map(|n| n as f64 + 0.25)).unwrap()],
        ),
        (
            "Rotation",
            vec![AttributeValue::vector([1.0, 0.0, 0.0, 45.0]).unwrap()],
        ),
        (
            "Bits",
            vec![
                AttributeValue::Boolean(false),
                AttributeValue::Boolean(true),
            ],
        ),
        (
            "Ints",
            vec![
                AttributeValue::Integer(9),
                AttributeValue::Integer(-8),
                AttributeValue::Integer(7),
            ],
        ),
        ("Set", Vec::new()),
        (
            "Strings",
            vec![
                AttributeValue::String("a&b".into()),
                AttributeValue::String("second".into()),
            ],
        ),
        (
            "Json",
            vec![AttributeValue::String("current scalar".into())],
        ),
        ("Text", vec![AttributeValue::String("new <text>".into())]),
    ]);
    for (index, attribute) in ir.model.attributes.iter_mut().enumerate() {
        attribute.id =
            cadmpeg_ir::ids::AttributeId::mint(format!("test:model:attribute#rewritten{index}"))
                .unwrap();
        attribute.values = expected[attribute.name.as_str()].clone();
    }
    let plan = FcstdCodec
        .plan(
            EncodeInput::new(&ir, None),
            TargetRequest::Explicit("fcstd:schema-4"),
        )
        .unwrap();
    assert!(
        plan.report()
            .losses
            .iter()
            .all(|loss| loss.code.local_code() != "attribute.not-transferred"),
        "{:?}",
        plan.report().losses
    );
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let output = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let actual = output
        .ir()
        .model
        .attributes
        .iter()
        .map(|attribute| (attribute.name.as_str(), attribute.values.clone()))
        .collect::<BTreeMap<_, _>>();
    assert_eq!(actual, expected);
    assert!(crate::test_support::validate_native(output.ir()).is_empty());
}

#[test]
fn source_less_reference_attribute_updates_the_current_typed_native_target() {
    let document = "<Document SchemaVersion='4' FileVersion='1'><Properties Count='1'><Property name='Selected' type='App::PropertyLink'><Link value='First'/></Property></Properties><Objects Count='2'><Object name='First' type='App::Part'/><Object name='Second' type='App::Part'/></Objects><ObjectData Count='2'><Object name='First'><Properties Count='0'/></Object><Object name='Second'><Properties Count='0'/></Object></ObjectData></Document>";
    let decoded = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .unwrap();
    let mut ir = decoded.ir().clone();
    let target = ir
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<crate::native::ObjectRecord>("objects")
        .unwrap()
        .iter()
        .find(|object| object.name() == "Second")
        .unwrap()
        .id()
        .as_str()
        .to_owned();
    let attribute = ir
        .model
        .attributes
        .iter_mut()
        .find(|attribute| attribute.name == "Selected")
        .unwrap();
    attribute.id =
        cadmpeg_ir::ids::AttributeId::mint("test:model:attribute#new-reference-id").unwrap();
    attribute.values = vec![AttributeValue::Reference(target)];
    let plan = FcstdCodec
        .plan(
            EncodeInput::new(&ir, None),
            TargetRequest::Explicit("fcstd:schema-4"),
        )
        .unwrap();
    assert!(plan
        .report()
        .losses
        .iter()
        .all(|loss| loss.code.local_code() != "attribute.not-transferred"));
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let output = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let native = output.ir().native.namespace("fcstd").unwrap();
    let second = native
        .arena_as::<crate::native::ObjectRecord>("objects")
        .unwrap()
        .into_iter()
        .find(|object| object.name() == "Second")
        .unwrap();
    let selected = native
        .arena_as::<crate::native::PropertyRecord>("properties")
        .unwrap()
        .into_iter()
        .find(|property| property.name == "Selected")
        .unwrap();
    assert_eq!(
        selected.links()[0].as_ref().unwrap().object(),
        Some(second.id().as_str())
    );
    assert!(crate::test_support::validate_native(output.ir()).is_empty());
}

#[test]
fn source_less_attribute_outside_native_scalar_range_warns_without_reviving_old_value() {
    let document = "<Document SchemaVersion='4' FileVersion='1'><Properties Count='1'><Property name='PackedColor' type='App::PropertyColor'><PropertyColor value='7'/></Property></Properties><Objects Count='0'/><ObjectData Count='0'/></Document>";
    let decoded = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .unwrap();
    let mut ir = decoded.ir().clone();
    let attribute = ir
        .model
        .attributes
        .iter_mut()
        .find(|attribute| attribute.name == "PackedColor")
        .unwrap();
    attribute.values = vec![AttributeValue::Integer(-1)];
    let attribute_id = attribute.id.to_string();
    let plan = FcstdCodec
        .plan(
            EncodeInput::new(&ir, None),
            TargetRequest::Explicit("fcstd:schema-4"),
        )
        .unwrap();
    assert!(plan
        .report()
        .losses
        .iter()
        .any(|loss| loss.code.local_code() == "attribute.not-transferred"
            && loss.message.contains(&attribute_id)));
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let output = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert!(!output
        .ir()
        .model
        .attributes
        .iter()
        .any(|attribute| attribute.name == "PackedColor"));
}

#[test]
fn source_less_native_labels_keep_current_values_without_competing_with_link_transform_policy() {
    let decoded = FcstdCodec
        .decode(
            &mut Cursor::new(crate::test_support::test_archive::CORE_DESIGN_PRODUCT),
            &DecodeOptions::default(),
        )
        .unwrap();
    let mut ir = decoded.ir().clone();
    assert!(!ir
        .model
        .attributes
        .iter()
        .any(|attribute| attribute.name == "LinkTransform"));
    let labels = [
        "X-axis", "Y-axis", "Z-axis", "XY-plane", "XZ-plane", "YZ-plane",
    ];
    let native = ir.native.namespace("fcstd").unwrap();
    let objects = native
        .arena_as::<crate::native::ObjectRecord>("objects")
        .unwrap();
    let origins = objects
        .iter()
        .filter(|object| object.type_name == "App::Origin")
        .map(|object| object.id().clone())
        .collect::<std::collections::BTreeSet<_>>();
    assert!(!origins.is_empty());
    let mut properties = native
        .arena_as::<crate::native::PropertyRecord>("properties")
        .unwrap();
    let mut expected = BTreeMap::new();
    for property in &mut properties {
        if property.name == "Label" {
            let is_origin = origins.contains(&property.owner);
            if let Some(value) = property.values_mut().and_then(|values| values.first_mut()) {
                if let Some(label) = value.attributes.get_mut("value") {
                    if is_origin || labels.contains(&label.as_str()) {
                        *label = format!("edited {label}");
                        expected.insert(property.id.clone(), property.values().to_vec());
                    }
                }
            }
        }
    }
    assert!(expected.len() > labels.len());
    assert!(!ir.model.attributes.iter().any(|attribute| {
        attribute
            .native_ref
            .as_ref()
            .is_some_and(|id| expected.contains_key(id))
    }));
    ir.native
        .namespace_mut("fcstd")
        .set_arena(
            &cadmpeg_test_support::service_decode_context(),
            "properties",
            &properties,
        )
        .unwrap();
    let occurrence_owner = objects
        .iter()
        .find(|object| object.name() == "Occurrence")
        .unwrap()
        .id()
        .as_str()
        .to_owned();
    let box_owner = objects
        .iter()
        .find(|object| object.name() == "Box")
        .unwrap()
        .id()
        .as_str()
        .to_owned();
    let before = ir
        .model
        .occurrences
        .iter()
        .find(|occurrence| occurrence.native_ref.as_deref() == Some(occurrence_owner.as_str()))
        .unwrap()
        .effective_transform()
        .unwrap();
    let feature = ir
        .model
        .features
        .iter()
        .find(|feature| feature.native_ref.as_deref() == Some(box_owner.as_str()))
        .unwrap();
    let feature_id = feature.id.to_string();
    let label_property = properties
        .iter()
        .find(|property| {
            property.name == "Label"
                && feature.native_ref.as_deref() == Some(property.owner.as_str())
        })
        .unwrap();
    let collision_id = "test:model:attribute#label-collision";
    ir.model
        .attributes
        .push(cadmpeg_ir::attributes::SourceAttribute {
            id: cadmpeg_ir::ids::AttributeId::mint(collision_id).unwrap(),
            native_ref: Some(label_property.id.clone()),
            target: cadmpeg_ir::attributes::AttributeTarget::Record {
                arena: cadmpeg_ir::ArenaName::parse("features").unwrap(),
                id: feature_id,
            },
            name: "Label".into(),
            values: vec![AttributeValue::String("competing label".into())],
        });
    let plan = FcstdCodec
        .plan(
            EncodeInput::new(&ir, None),
            TargetRequest::Explicit("fcstd:schema-4"),
        )
        .unwrap();
    assert!(plan.report().losses.iter().any(|loss| {
        loss.code.local_code() == "attribute.not-transferred" && loss.message.contains(collision_id)
    }));
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let written = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let written_properties = written
        .ir()
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<crate::native::PropertyRecord>("properties")
        .unwrap();
    for (native_ref, values) in expected {
        assert_eq!(
            written_properties
                .iter()
                .find(|property| property.id == native_ref)
                .unwrap()
                .values()
                .to_vec(),
            values
        );
        assert!(!written
            .ir()
            .model
            .attributes
            .iter()
            .any(|attribute| attribute.native_ref.as_deref() == Some(native_ref.as_str())));
    }
    assert!(!written
        .ir()
        .model
        .attributes
        .iter()
        .any(|attribute| attribute.name == "LinkTransform"));
    assert!(written
        .ir()
        .model
        .features
        .iter()
        .any(|feature| feature.native_ref.as_deref() == Some(box_owner.as_str())));
    let written_objects = written
        .ir()
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<crate::native::ObjectRecord>("objects")
        .unwrap();
    let occurrence_owner = written_objects
        .iter()
        .find(|object| object.name() == "Occurrence")
        .unwrap()
        .id()
        .as_str()
        .to_owned();
    let after = written
        .ir()
        .model
        .occurrences
        .iter()
        .find(|occurrence| occurrence.native_ref.as_deref() == Some(occurrence_owner.as_str()))
        .unwrap()
        .effective_transform()
        .unwrap();
    assert_eq!(after, before);
    assert!(crate::test_support::validate_native(written.ir()).is_empty());
}

#[test]
fn structured_document_attributes_keep_order_selectors_and_current_native_envelopes() {
    use cadmpeg_ir::attributes::{AttributeMapEntry, AttributeTarget};
    let document = "<Document SchemaVersion='4' FileVersion='1'><Properties Count='6'><Property name='Repeated' type='App::PropertyMap' group='Authored' doc='keep envelope'><Map count='3'><Item key='b' value='2' Vendor='keep row' name='keep item name'/><Item key='a' value='1'/><Item key='b' value='3'/></Map></Property><Property name='Selected' type='App::PropertyLinkSub'><LinkSub value='Box' count='2'><Sub value='Face1'/><Sub value='Edge2'/></LinkSub></Property><Property name='Remote' type='App::PropertyXLink'><XLink file='parts.FCStd' name='RemoteBox' sub='Face3'/></Property><Property name='Meta' type='App::PropertyMap'><Map count='2'><Item key='a' value='1'/><Item key='b' value='2'/></Map></Property><Property name='Shape' type='App::PropertyFloat'><Float value='7'/></Property><Property name='Placement' type='App::PropertyFloat'><Float value='8'/></Property></Properties><Objects Count='1'><Object name='Box' type='App::Feature'/></Objects><ObjectData Count='1'><Object name='Box'><Properties Count='0'/></Object></ObjectData></Document>";
    let decoded = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .unwrap();
    let mut ir = decoded.ir().clone();
    let properties = ir
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<crate::native::PropertyRecord>("properties")
        .unwrap();
    let meta = properties
        .iter()
        .find(|property| property.name == "Repeated")
        .unwrap();
    let attribute = ir
        .model
        .attributes
        .iter()
        .find(|attribute| attribute.name == "Repeated")
        .unwrap();
    assert_eq!(attribute.target, AttributeTarget::Document);
    assert_eq!(attribute.native_ref.as_deref(), Some(meta.id.as_str()));
    assert_eq!(
        attribute.values,
        [AttributeValue::Map(vec![
            AttributeMapEntry {
                key: "b".into(),
                value: "2".into()
            },
            AttributeMapEntry {
                key: "a".into(),
                value: "1".into()
            },
            AttributeMapEntry {
                key: "b".into(),
                value: "3".into()
            }
        ])]
    );
    assert!(
        matches!(ir.model.attributes.iter().find(|attribute| attribute.name == "Selected").unwrap().values.as_slice(), [AttributeValue::SubReference { object, subelements }] if object == &crate::native::native_id("object", "Box") && subelements == &["Face1", "Edge2"])
    );
    assert!(
        matches!(ir.model.attributes.iter().find(|attribute| attribute.name == "Remote").unwrap().values.as_slice(), [AttributeValue::ExternalReference { document, object, subelements }] if document == "parts.FCStd" && object == "RemoteBox" && subelements == &["Face3"])
    );
    let meta = ir
        .model
        .attributes
        .iter()
        .find(|attribute| attribute.name == "Meta")
        .unwrap();
    assert_eq!(meta.target, AttributeTarget::Document);
    assert_eq!(
        meta.native_ref.as_deref(),
        Some(
            properties
                .iter()
                .find(|property| property.name == "Meta")
                .unwrap()
                .id
                .as_str()
        )
    );
    assert_eq!(
        meta.values,
        [AttributeValue::Map(vec![
            AttributeMapEntry {
                key: "a".into(),
                value: "1".into()
            },
            AttributeMapEntry {
                key: "b".into(),
                value: "2".into()
            }
        ])]
    );
    assert_eq!(
        ir.model
            .attributes
            .iter()
            .find(|attribute| attribute.name == "Shape")
            .unwrap()
            .values,
        [AttributeValue::float(7.).unwrap()]
    );
    assert_eq!(
        ir.model
            .attributes
            .iter()
            .find(|attribute| attribute.name == "Placement")
            .unwrap()
            .values,
        [AttributeValue::float(8.).unwrap()]
    );
    // Current typed edits own W1 even without retained source bytes.
    let AttributeValue::Map(entries) = &mut ir
        .model
        .attributes
        .iter_mut()
        .find(|attribute| attribute.name == "Repeated")
        .unwrap()
        .values[0]
    else {
        panic!("map")
    };
    entries[0].value = "current & value".into();
    let expected = ir
        .model
        .attributes
        .iter()
        .map(|attribute| (attribute.name.clone(), attribute.values.clone()))
        .collect::<BTreeMap<_, _>>();
    let plan = FcstdCodec
        .plan(
            EncodeInput::new(&ir, None),
            TargetRequest::Explicit("fcstd:schema-4"),
        )
        .unwrap();
    assert!(!plan
        .report()
        .losses
        .iter()
        .any(|loss| loss.code.local_code() == "attribute.not-transferred"));
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let output = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    for attribute in &output.ir().model.attributes {
        assert_eq!(attribute.values, expected[&attribute.name]);
    }
    assert_eq!(output.ir().model.attributes.len(), expected.len());
    let properties = output
        .ir()
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<crate::native::PropertyRecord>("properties")
        .unwrap();
    let meta = properties
        .iter()
        .find(|property| property.name == "Repeated")
        .unwrap();
    assert_eq!(
        meta.dynamic.as_ref().unwrap().documentation.as_deref(),
        Some("keep envelope")
    );
    assert_eq!(
        meta.values()
            .iter()
            .find(|value| value.tag == "Item")
            .unwrap()
            .attributes["Vendor"],
        "keep row"
    );
    assert_eq!(
        meta.values()
            .iter()
            .find(|value| value.tag == "Item")
            .unwrap()
            .attributes["name"],
        "keep item name"
    );
}

#[test]
fn unbound_expression_attributes_share_parameter_tokens_and_render_current_names() {
    use cadmpeg_ir::features::ExpressionToken;
    let document = "<Document SchemaVersion='4' FileVersion='1'><Objects Count='2'><Object name='Box' type='Part::Box'/><Object name='Sheet' type='Spreadsheet::Sheet'/></Objects><ObjectData Count='2'><Object name='Sheet'><Properties Count='1'><Property name='cells' type='Spreadsheet::PropertySheet'><Cells Count='1' xlink='1'><Cell address='A1' alias='width' content='5'/></Cells></Property></Properties></Object><Object name='Box'><Properties Count='4'><Property name='Length' type='App::PropertyLength'><Float value='10'/></Property><Property name='Width' type='App::PropertyLength'><Float value='3'/></Property><Property name='Height' type='App::PropertyLength'><Float value='4'/></Property><Property name='ExpressionEngine' type='App::PropertyExpressionEngine'><ExpressionEngine count='2'><Expression path='Length' expression='Sheet.width * 2'/><Expression path='Placement.Base.x' expression='Sheet.width + 1'/></ExpressionEngine></Property></Properties></Object></ObjectData></Document>";
    let decoded = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .unwrap();
    let mut ir = decoded.ir().clone();
    let width = ir
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.name == "width")
        .unwrap()
        .id
        .clone();
    let length = ir
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.name == "Length")
        .unwrap();
    assert!(length
        .expression
        .tokens
        .iter()
        .any(|token| matches!(token, ExpressionToken::Parameter(id) if id == &width)));
    let attribute = ir
        .model
        .attributes
        .iter()
        .find(|attribute| attribute.name == "ExpressionEngine")
        .unwrap();
    let [AttributeValue::Expression(value)] = attribute.values.as_slice() else {
        panic!("one unbound expression")
    };
    assert_eq!(value.path, "Placement.Base.x");
    assert_eq!(value.comment, None);
    assert!(value
        .expression
        .tokens
        .iter()
        .any(|token| matches!(token, ExpressionToken::Parameter(id) if id == &width)));
    ir.model
        .parameters
        .iter_mut()
        .find(|parameter| parameter.id == width)
        .unwrap()
        .name = "current_width".into();
    let plan = FcstdCodec
        .plan(
            EncodeInput::new(&ir, None),
            TargetRequest::Explicit("fcstd:schema-4"),
        )
        .unwrap();
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let output = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let width = output
        .ir()
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.name == "current_width")
        .unwrap();
    let attribute = output
        .ir()
        .model
        .attributes
        .iter()
        .find(|attribute| attribute.name == "ExpressionEngine")
        .unwrap();
    let [AttributeValue::Expression(value)] = attribute.values.as_slice() else {
        panic!("one current unbound expression")
    };
    assert_eq!(value.path, "Placement.Base.x");
    assert!(value
        .expression
        .tokens
        .iter()
        .any(|token| matches!(token, ExpressionToken::Parameter(id) if id == &width.id)));
    let properties = output
        .ir()
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<crate::native::PropertyRecord>("properties")
        .unwrap();
    let expression = properties
        .iter()
        .find(|property| property.name == "ExpressionEngine")
        .unwrap()
        .values()
        .iter()
        .find(|value| {
            value
                .attributes
                .get("path")
                .is_some_and(|path| path == "Placement.Base.x")
        })
        .unwrap();
    assert!(expression.attributes["expression"].contains("current_width"));
}

#[test]
fn numeric_attributes_use_current_rows_and_never_restore_missing_or_deleted_lists() {
    use crate::test_support::test_archive::archive_entries;
    use cadmpeg_ir::attributes::AttributeNumericList;
    let document = "<Document SchemaVersion='4' FileVersion='1'><Properties Count='5'><Property name='Numbers' type='App::PropertyFloatList' group='Numeric' doc='current rows'><FloatList file='numbers.bin'/></Property><Property name='Vectors' type='App::PropertyVectorList'><VectorList file='vectors.bin'/></Property><Property name='Placements' type='App::PropertyPlacementList'><PlacementList file='placements.bin'/></Property><Property name='Colors' type='App::PropertyColorList'><ColorList file='colors.bin'/></Property><Property name='Frame' type='App::PropertyPlacement'><PropertyPlacement Px='1' Py='2' Pz='3' Ox='0' Oy='1' Oz='0' A='45'/></Property></Properties><Objects Count='0'/><ObjectData Count='0'/></Document>";
    let floats = |rows: &[f64], count: u32| {
        let mut bytes = count.to_le_bytes().to_vec();
        for value in rows {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes
    };
    let numbers = floats(&[1.25, 2.5], 2);
    let vectors = floats(&[1., 2., 3., 4., 5., 6.], 2);
    let placements = floats(&[1., 2., 3., 0., 0., 0., 2.], 1);
    let mut colors = 2_u32.to_le_bytes().to_vec();
    colors.extend_from_slice(&0x01020304_u32.to_le_bytes());
    colors.extend_from_slice(&u32::MAX.to_le_bytes());
    let bytes = archive_entries(&[
        ("Document.xml", document.as_bytes()),
        ("numbers.bin", &numbers),
        ("vectors.bin", &vectors),
        ("placements.bin", &placements),
        ("colors.bin", &colors),
    ]);
    let decoded = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let mut ir = decoded.ir().clone();
    let attr = |name: &str| {
        ir.model
            .attributes
            .iter()
            .find(|attribute| attribute.name == name)
            .unwrap()
    };
    assert_eq!(
        attr("Frame").values,
        [AttributeValue::vector([1., 2., 3., 0., 1., 0., 45.]).unwrap()]
    );
    assert!(
        matches!(attr("Numbers").values.as_slice(), [AttributeValue::NumericList(AttributeNumericList::Float(Some(values)))] if values.len() == 2 && values[0].unwrap().get() == 1.25)
    );
    assert!(
        matches!(attr("Placements").values.as_slice(), [AttributeValue::NumericList(AttributeNumericList::Placement(Some(values)))] if values[0].as_ref().unwrap().quaternion.get() == [0.,0.,0.,2.])
    );
    let expected = ir
        .model
        .attributes
        .iter()
        .map(|attribute| (attribute.name.clone(), attribute.values.clone()))
        .collect::<BTreeMap<_, _>>();
    let encode = |ir: &cadmpeg_ir::CadIr| {
        let plan = FcstdCodec
            .plan(
                EncodeInput::new(ir, None),
                TargetRequest::Explicit("fcstd:schema-4"),
            )
            .unwrap();
        let losses = plan.report().losses.clone();
        let mut bytes = Vec::new();
        plan.write_to(&mut bytes).unwrap();
        (bytes, losses)
    };
    let (bytes, _) = encode(&ir);
    let output = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert_eq!(output.ir().model.attributes.len(), expected.len());
    for attribute in &output.ir().model.attributes {
        assert_eq!(attribute.values, expected[&attribute.name]);
    }
    for attribute in &mut ir.model.attributes {
        match attribute.name.as_str() {
            "Numbers" => {
                attribute.values = vec![AttributeValue::NumericList(AttributeNumericList::Float(
                    Some(vec![Some(
                        cadmpeg_ir::scalar::FiniteReal::new(9.5).unwrap(),
                    )]),
                ))]
            }
            "Vectors" => {
                attribute.values = vec![AttributeValue::NumericList(AttributeNumericList::Vector(
                    Some(vec![
                        None,
                        Some(cadmpeg_ir::units::FiniteVector::new([4., 5., 6.]).unwrap()),
                    ]),
                ))]
            }
            "Placements" => {
                attribute.values = vec![AttributeValue::NumericList(
                    AttributeNumericList::Placement(None),
                )]
            }
            "Colors" => {
                attribute.values = vec![AttributeValue::NumericList(AttributeNumericList::Color(
                    Some(Vec::new()),
                ))]
            }
            _ => {}
        }
    }
    let missing = ir
        .model
        .attributes
        .iter()
        .filter(|attribute| matches!(attribute.name.as_str(), "Vectors" | "Placements"))
        .map(|attribute| attribute.id.to_string())
        .collect::<Vec<_>>();
    let (bytes, losses) = encode(&ir);
    for id in &missing {
        assert!(losses
            .iter()
            .any(|loss| loss.code.local_code() == "source.undecoded-omitted"
                && loss.message.contains(id)));
    }
    let output = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert!(!output
        .ir()
        .model
        .attributes
        .iter()
        .any(|attribute| matches!(attribute.name.as_str(), "Vectors" | "Placements")));
    assert!(
        matches!(output.ir().model.attributes.iter().find(|attribute| attribute.name == "Colors").unwrap().values.as_slice(), [AttributeValue::NumericList(AttributeNumericList::Color(Some(values)))] if values.is_empty())
    );
    assert!(
        matches!(output.ir().model.attributes.iter().find(|attribute| attribute.name == "Numbers").unwrap().values.as_slice(), [AttributeValue::NumericList(AttributeNumericList::Float(Some(values)))] if values.len() == 1 && values[0].unwrap().get() == 9.5)
    );
    ir.model
        .attributes
        .retain(|attribute| attribute.name != "Numbers");
    let (bytes, _) = encode(&ir);
    let output = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert!(!output
        .ir()
        .model
        .attributes
        .iter()
        .any(|attribute| attribute.name == "Numbers"));
    assert!(!output
        .ir()
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<crate::native::PropertyRecord>("properties")
        .unwrap()
        .iter()
        .any(|property| property.name == "Numbers"));
}

mod fixed_numeric;
mod mesh_material;

#[test]
fn stored_geometry_dynamic_scalar_keeps_design_ownership_at_both_depths() {
    let document = "<Document SchemaVersion='4' FileVersion='1'><Objects Count='1'>\
        <Object name='Designed' type='PartDesign::Feature'/></Objects><ObjectData Count='1'>\
        <Object name='Designed'><Properties Count='1'><Property name='Length' type='App::PropertyLength' group='Design'>\
        <Float value='5'/></Property></Properties></Object></ObjectData></Document>";
    let bytes = archive(document);
    let full = FcstdCodec
        .decode(&mut Cursor::new(&bytes), &DecodeOptions::default())
        .unwrap();
    assert!(full
        .ir()
        .model
        .parameters
        .iter()
        .any(|p| p.name == "Length"));
    let geometry = FcstdCodec
        .decode(
            &mut Cursor::new(&bytes),
            &DecodeOptions {
                depth: cadmpeg_core::decode::DecodeDepth::Geometry,
                ..DecodeOptions::default()
            },
        )
        .unwrap();
    for decoded in [&full, &geometry] {
        assert!(!decoded
            .ir()
            .model
            .attributes
            .iter()
            .any(|a| a.name == "Length"));
    }
    let resolver = crate::test_support::expression_names::resolver(full.ir())
        .unwrap()
        .unwrap();
    let (left, right) = cadmpeg_test_support::contract::expressions::neutral_views(
        full.ir(),
        geometry.ir(),
        &[cadmpeg_ir::ArenaClass::Design],
        resolver.as_ref(),
    )
    .unwrap();
    assert_eq!(left, right);
}
