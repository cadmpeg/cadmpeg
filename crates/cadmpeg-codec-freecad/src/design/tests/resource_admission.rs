// SPDX-License-Identifier: Apache-2.0
//! Design feature and spreadsheet admission tests.

#[test]
fn design_boolean_scalar_keeps_case_insensitive_values_without_copy() {
    for (value, expected) in [("TrUe", true), ("FaLsE", false)] {
        let property = crate::test_support::source_fidelity::PropertyFixture::new(
            crate::native::PropertyRecord {
                dynamic: None,
                id: "flag".into(),
                owner: "owner".into(),
                name: "Flag".into(),
                type_name: "App::PropertyBool".into(),
                family: crate::native::PropertyFamily::Unknown,
                status: None,
                body: Some(crate::native::PropertyBody::Transient),
                order: 0,
            },
            format!("<Property><Bool value=\"{value}\"/></Property>"),
        );
        crate::test_support::source_fidelity::with_property_reader(
            &[],
            &[&property],
            &cadmpeg_core::decode::DecodePolicy::service(),
            |ctx| {
                assert_eq!(
                    super::super::bool_property(ctx, &[&property], "Flag").expect("admitted XML"),
                    Some(expected)
                );
            },
        );
    }
}

#[test]
fn failed_varset_scalar_names_only_its_parameter_and_keeps_readable_global_peers() {
    use cadmpeg_ir::{Codec, DecodeOptions};
    use std::io::Cursor;
    for (value, held) in [("7", true), ("bad", false)] {
        let xml = format!("<Document SchemaVersion='4' FileVersion='1'><Objects Count='1'><Object name='Globals' type='App::VarSet'/></Objects><ObjectData Count='1'><Object name='Globals'><Properties Count='2'><Property name='Damaged' type='App::PropertyFloat'><Float value='{value}'/></Property><Property name='Peer' type='App::PropertyInteger'><Integer value='9'/></Property></Properties></Object></ObjectData></Document>");
        let result = crate::FcstdCodec
            .decode(
                &mut Cursor::new(crate::test_support::test_archive::archive(&xml)),
                &DecodeOptions::default(),
            )
            .expect("global scalar failure stays local");
        let objects = result
            .ir()
            .native
            .namespace("fcstd")
            .unwrap()
            .arena_as::<crate::native::ObjectRecord>("objects")
            .unwrap();
        let object = &objects[0];
        let id = super::super::parameter_id(object, "Damaged").unwrap();
        assert_eq!(
            result.ir().model.parameters.iter().any(|row| row.id == id),
            held
        );
        let peer = result
            .ir()
            .model
            .parameters
            .iter()
            .find(|row| row.name == "Peer")
            .expect("readable global peer");
        assert!(peer.owner.is_none());
        assert_eq!(
            peer.value,
            Some(cadmpeg_ir::features::ParameterValue::Integer(9))
        );
        assert!(
            result.ir().model.features.is_empty(),
            "VarSet has no invented Feature"
        );
        let named = result
            .report()
            .losses
            .iter()
            .filter(|loss| {
                loss.code.local_code() == "record.unreadable" && loss.message.contains(id.as_str())
            })
            .collect::<Vec<_>>();
        assert_eq!(named.len(), usize::from(!held));
        if let Some(loss) = named.first() {
            let provenance = loss.provenance.as_ref().unwrap();
            assert!(result
                .source_fidelity()
                .parts()
                .values()
                .any(|part| part.level().as_str() == "freecad.property_value"
                    && part.state() == &cadmpeg_ir::source_fidelity::parts::PartState::GivenUp {}
                    && provenance.stream_name() == Some(part.stream())
                    && provenance.offset == part.offset()
                    && provenance.byte_len() == Some(part.byte_len())));
        }
        crate::test_support::test_archive::assert_valid_document(result.ir());
    }
}

#[test]
fn failed_box_length_retains_native_kind_and_every_readable_parameter() {
    use cadmpeg_ir::{Codec, DecodeOptions};
    use std::io::Cursor;
    let xml = "<Document SchemaVersion='4' FileVersion='1'><Objects Count='1'><Object name='Box' type='Part::Box'/></Objects><ObjectData Count='1'><Object name='Box'><Properties Count='3'><Property name='Length' type='App::PropertyLength'><Float value='bad'/></Property><Property name='Width' type='App::PropertyLength'><Float value='2'/></Property><Property name='Height' type='App::PropertyLength'><Float value='3'/></Property></Properties></Object></ObjectData></Document>";
    let result = crate::FcstdCodec
        .decode(
            &mut Cursor::new(crate::test_support::test_archive::archive(xml)),
            &DecodeOptions::default(),
        )
        .expect("#224 retains a Native feature for a given-up Length");
    super::projection_assertions::assert_native_projection_from_failed_control(
        &result, "Box", "Length",
    );
    let feature = &result.ir().model.features[0];
    let cadmpeg_ir::features::FeatureOperation::Native { kind, parameters } =
        feature.definition().operation()
    else {
        panic!("the failed dimension cannot create a default Box");
    };
    assert_eq!(kind.as_str(), "Part::Box");
    assert_eq!(parameters.len(), 2);
    for (name, expected) in [("Width", 2.0), ("Height", 3.0)] {
        let parameter = result
            .ir()
            .model
            .parameters
            .iter()
            .find(|row| row.name == name)
            .expect("readable dimension retained");
        assert_eq!(parameter.owner.as_ref(), Some(&feature.id));
        assert!(
            matches!(parameter.value, Some(cadmpeg_ir::features::ParameterValue::Length(length)) if length.get() == expected)
        );
    }
    assert!(result
        .ir()
        .model
        .parameters
        .iter()
        .all(|row| row.name != "Length"));
    let objects = result
        .ir()
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<crate::native::ObjectRecord>("objects")
        .unwrap();
    let missing = super::super::parameter_id(&objects[0], "Length").unwrap();
    assert_eq!(
        result
            .report()
            .losses
            .iter()
            .filter(|loss| loss.code.local_code() == "record.unreadable"
                && loss.message.contains(missing.as_str()))
            .count(),
        1
    );
}

#[test]
fn detached_partial_sketch_and_sheet_keep_native_without_fabricated_failure_authority() {
    use cadmpeg_ir::{Codec, DecodeOptions};
    use std::io::Cursor;
    for (kind, name, property_kind, root) in [
        (
            "Sketcher::SketchObject",
            "Geometry",
            "Part::PropertyGeometryList",
            "GeometryList",
        ),
        (
            "Spreadsheet::Sheet",
            "cells",
            "Spreadsheet::PropertySheet",
            "Cells",
        ),
    ] {
        let xml = format!("<Document SchemaVersion='4' FileVersion='1'><Objects Count='1'><Object name='A' type='{kind}'/></Objects><ObjectData Count='1'><Object name='A'><Properties Count='1'><Property name='{name}' type='{property_kind}'><Wrong><{root} count='0'/></Wrong></Property></Properties></Object></ObjectData></Document>");
        let decoded = crate::FcstdCodec
            .decode(
                &mut Cursor::new(crate::test_support::test_archive::archive(&xml)),
                &DecodeOptions::default(),
            )
            .expect("actual failed source producer");
        let namespace = decoded.ir().native.namespace("fcstd").unwrap();
        let objects = namespace
            .arena_as::<crate::native::ObjectRecord>("objects")
            .unwrap();
        let properties = namespace
            .arena_as::<crate::native::PropertyRecord>("properties")
            .unwrap();
        let mut projected = decoded.ir().clone();
        projected.model.features.clear();
        projected.model.sketches.clear();
        projected.model.spreadsheets.clear();
        projected.model.parameters.clear();
        let context = cadmpeg_test_support::service_decode_context();
        let reader = crate::property_reader::PropertyReader::typed(&context);
        let (_, _, unformed, _) = super::super::transfer(
            &reader,
            &mut projected,
            &objects,
            &properties,
            &[],
            None,
            None,
        )
        .expect("current typed partial graph works without source Views or failure registry");
        assert!(
            unformed.is_empty(),
            "source-less projection cannot invent a GivenUpHome"
        );
        assert!(projected.model.sketches.is_empty());
        assert!(projected.model.spreadsheets.is_empty());
        assert_eq!(projected.model.features.len(), 1);
        assert!(
            matches!(projected.model.features[0].definition().operation(), cadmpeg_ir::features::FeatureOperation::Native { kind: actual, parameters } if actual.as_str() == kind && parameters.is_empty())
        );
        assert_eq!(projected.model.features[0], decoded.ir().model.features[0]);
    }
}
