use crate::test_support::assembly_test::{f3d_without_brep, f3z_archive, XREF_ROLE};
use crate::test_support::smbh_geometry_test::synthetic_geometry_smbh;
use crate::test_support::zip_test::f3d_with_work_axis;
use cadmpeg_ir::codec::DecodeOptions;
use cadmpeg_ir::design::construction::InputReference;
use cadmpeg_ir::design::names::{Base64Bytes, NativeNameValue};
use cadmpeg_ir::design::selection::{PickTarget, Picks};
use cadmpeg_ir::features::FeatureOperation;
use cadmpeg_test_support::EditableDecodeResult;
use std::io::Cursor;

#[test]
fn work_axis_partial_evidence_and_literal_names_survive_real_f3z_merge() {
    for geometry in [false, true] {
        let smbh = if geometry {
            synthetic_geometry_smbh()
        } else {
            Vec::new()
        };
        let first = f3d_with_work_axis(&smbh, "source", false, &[], false);
        let first = crate::test_support::archive_session::decode(
            &mut Cursor::new(first),
            &DecodeOptions::default(),
        )
        .unwrap();
        let axis = first
            .ir()
            .model
            .features
            .iter()
            .find(|f| {
                matches!(
                    f.evaluation.definition().operation(),
                    FeatureOperation::DatumAxis { .. }
                )
            })
            .expect("literal synthetic carrier produces a datum axis");
        let feature_id = axis.id.as_str();
        let literal = axis.native_ref.as_deref().expect("native scope owner");
        let component = f3d_with_work_axis(&smbh, literal, true, &[], false);
        let alone = EditableDecodeResult::from(
            crate::test_support::archive_session::decode(
                &mut Cursor::new(component.clone()),
                &DecodeOptions::default(),
            )
            .unwrap(),
        );
        let root = f3d_without_brep(
            "assembly-design",
            "root.f3d",
            &[("comp.f3d", XREF_ROLE), ("comp.f3d", XREF_ROLE)],
        );
        let archive = f3z_archive(
            "root.f3d",
            &[
                ("root.f3d", root.as_slice()),
                ("comp.f3d", component.as_slice()),
            ],
        );
        let merged = EditableDecodeResult::from(
            crate::test_support::archive_session::decode(
                &mut Cursor::new(archive),
                &DecodeOptions::default(),
            )
            .unwrap(),
        );
        let axes = merged
            .ir()
            .model
            .features
            .iter()
            .filter(|f| {
                matches!(
                    f.evaluation.definition().operation(),
                    FeatureOperation::DatumAxis { .. }
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(
            axes.len(),
            1,
            "repeated placements share one member definition"
        );
        assert_ne!(axes[0].id.as_str(), feature_id);
        assert_ne!(axes[0].native_ref.as_deref(), Some(literal));
        let parameters = &merged.ir().model.parameters;
        assert_eq!(parameters.len(), 1);
        assert_eq!(parameters[0].name, "Literal");
        assert_eq!(parameters[0].expression, literal);
        assert_eq!(parameters[0].owner.as_ref(), Some(&axes[0].id));

        let FeatureOperation::DatumAxis { axis } = axes[0].evaluation.definition().operation()
        else {
            unreachable!()
        };
        assert!(axis.geometry().is_some());
        let native = crate::native::F3dNative::load(
            merged.ir().native.namespace("f3d").unwrap(),
            cadmpeg_core::decode::ResourceLimits::desktop(),
        )
        .unwrap();
        assert!(native
            .design_parameter_scopes
            .iter()
            .any(|scope| Some(scope.id.as_str()) == axes[0].native_ref.as_deref()));
        assert_eq!(native.design_parameters.len(), 1);
        assert_eq!(native.design_parameters[0].expression(), literal);
        let names = native
            .design_parameter_scopes
            .iter()
            .filter_map(|s| s.work_axis_construction())
            .flat_map(|a| &a.candidates)
            .filter_map(|c| c.prefix.name.value())
            .collect::<Vec<_>>();
        assert_eq!(
            names,
            vec![literal],
            "native literal exactly equal to a source scope identity survives namespacing"
        );
        let unreadable = |result: &EditableDecodeResult| {
            result
                .report()
                .losses
                .iter()
                .filter(|loss| loss.code.to_string() == "decode/record.unreadable")
                .count()
        };
        assert_eq!(unreadable(&alone), 1);
        assert_eq!(unreadable(&merged), 1);
        let stubs = merged.ir().native_unknowns("f3d").unwrap();
        let stub = stubs
            .iter()
            .find(|stub| {
                merged
                    .source_fidelity()
                    .retained_record(stub.id.as_str())
                    .is_some_and(|record| record.data() == Some(f64::NAN.to_le_bytes().as_slice()))
            })
            .expect("the failed scalar has its own retained byte image");
        assert_eq!(stub.links.len(), 1);
        assert!(native
            .design_parameter_scopes
            .iter()
            .any(|scope| scope.id == stub.links[0].as_str()));

        assert_eq!(merged.ir().geometry.bodies.is_empty(), !geometry);
    }
}

#[test]
fn distinct_work_axis_members_with_equal_local_ids_keep_their_owners_images_and_paths() {
    use crate::bytes::reference_fields::ReadField;
    use crate::records::feature::work_axes::{AxisCarrierPropertyValue, DesignWorkAxisSource};
    let seed = f3d_with_work_axis(&[], "seed", false, &[], false);
    let seed = crate::test_support::archive_session::decode(
        &mut Cursor::new(seed),
        &DecodeOptions::default(),
    )
    .unwrap();
    let source = seed
        .ir()
        .model
        .features
        .iter()
        .find(|feature| {
            matches!(
                feature.evaluation.definition().operation(),
                FeatureOperation::DatumAxis { .. }
            )
        })
        .unwrap();
    let local_feature = source.id.as_str();
    let local_native = source.native_ref.as_deref().unwrap();
    let prefix = |ordinal| format!("f3d:role-{XREF_ROLE}/reference-{ordinal}/occurrence-0@");
    let destination = format!(
        "{}{}",
        prefix(0),
        local_feature.strip_prefix("f3d:").unwrap()
    );
    let properties = [(local_feature, 42), (destination.as_str(), 84)];
    let members = [
        f3d_with_work_axis(&[], local_feature, false, &properties, true),
        f3d_with_work_axis(&[], &destination, false, &properties, true),
    ];
    assert_ne!(members[0], members[1]);
    let root = f3d_without_brep(
        "assembly-design",
        "root.f3d",
        &[("first.f3d", XREF_ROLE), ("second.f3d", XREF_ROLE)],
    );
    let archive = f3z_archive(
        "root.f3d",
        &[
            ("root.f3d", &root),
            ("first.f3d", &members[0]),
            ("second.f3d", &members[1]),
        ],
    );
    let merged = EditableDecodeResult::from(
        crate::test_support::archive_session::decode(
            &mut Cursor::new(archive),
            &DecodeOptions::default(),
        )
        .unwrap(),
    );
    let native = crate::native::F3dNative::load(
        merged.ir().native.namespace("f3d").unwrap(),
        cadmpeg_core::decode::ResourceLimits::desktop(),
    )
    .unwrap();
    let axes = merged
        .ir()
        .model
        .features
        .iter()
        .filter(|feature| {
            matches!(
                feature.evaluation.definition().operation(),
                FeatureOperation::DatumAxis { .. }
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(axes.len(), 2);
    assert_eq!(merged.ir().model.parameters.len(), 2);
    for (ordinal, literal) in [(0, local_feature), (1, destination.as_str())] {
        let expected_feature = format!(
            "{}{}",
            prefix(ordinal),
            local_feature.strip_prefix("f3d:").unwrap()
        );
        let expected_native = format!(
            "{}{}",
            prefix(ordinal),
            local_native.strip_prefix("f3d:").unwrap()
        );
        let axis = axes
            .iter()
            .find(|feature| feature.id.as_str() == expected_feature)
            .unwrap();
        assert_eq!(axis.native_ref.as_deref(), Some(expected_native.as_str()));
        let parameters = merged
            .ir()
            .model
            .parameters
            .iter()
            .filter(|parameter| parameter.owner.as_ref() == Some(&axis.id))
            .collect::<Vec<_>>();
        assert_eq!(parameters.len(), 1);
        assert_eq!(parameters[0].expression, literal);
        assert_eq!(parameters[0].name, "Literal");
        let source_parameter = native
            .design_parameters
            .iter()
            .find(|parameter| Some(parameter.id.as_str()) == parameters[0].native_ref.as_deref())
            .unwrap();
        assert_eq!(source_parameter.expression(), literal);
        assert_eq!(source_parameter.record_index, 1701);
        let scope = native
            .design_parameter_scopes
            .iter()
            .find(|scope| scope.id == expected_native)
            .unwrap();
        assert_eq!(
            scope
                .work_axis_construction()
                .unwrap()
                .scope_references
                .iter()
                .map(|reference| reference.local().unwrap().0)
                .collect::<Vec<_>>(),
            [1500, 1700]
        );
        let candidate = &scope.work_axis_construction().unwrap().candidates[0];
        assert!(candidate.endpoints.is_empty());
        let FeatureOperation::DatumAxis { axis: neutral } =
            axis.evaluation.definition().operation()
        else {
            panic!("axis")
        };
        assert!(neutral.geometry().is_some());
        let [input] = neutral.inputs() else {
            panic!("the cross-document input reference is one input")
        };
        let Some(InputReference::Picks(selection)) = &input.reference else {
            panic!("a selection")
        };
        let Picks::Named(picks) = &selection.picks else {
            panic!("named picks")
        };
        let [pick] = picks.as_slice() else {
            panic!("one pick")
        };
        assert!(matches!(pick.target, PickTarget::Native { within: None }));
        assert!(pick.witness.is_none());
        assert_eq!(candidate.prefix.name, ReadField::Value(literal.to_owned()));
        assert_eq!(candidate.prefix.properties.len(), 2);
        for (property, (key, value)) in candidate.prefix.properties.iter().zip(properties) {
            assert_eq!(property.key, ReadField::Value(key.to_owned()));
            assert_eq!(
                property.value,
                ReadField::Value(AxisCarrierPropertyValue::Unsigned(value))
            );
        }
        let Some(DesignWorkAxisSource::PointDirection { inputs, .. }) = &candidate.source else {
            panic!("point direction")
        };
        assert_eq!(inputs.references.len(), 1);
        let reference = &inputs.references[0];
        assert!(reference.complete);
        assert!(reference.local().is_none());
        assert_eq!(reference.present, ReadField::Value(1));
        assert_eq!(reference.cross_document, ReadField::Value(1));
        assert_eq!(reference.different_segment, ReadField::Absent);
        assert_eq!(reference.same_document, ReadField::Value(0));
        assert_eq!(reference.target, ReadField::Value(1600));
        assert_eq!(
            reference.inline_type_guid,
            ReadField::Value("69EE2FA7-BCC7-449E-9CA9-976CEFDFED44".into())
        );
        assert_eq!(reference.target_type_guid, reference.inline_type_guid);
        assert_eq!(reference.segment, ReadField::Value(7));
        assert_eq!(
            reference.asset_guid,
            ReadField::Value("11111111-2222-3333-4444-555555555555".into())
        );
        assert_eq!(reference.link_name, ReadField::Value(literal.to_owned()));
        assert_eq!(reference.versioned, ReadField::Value(1));
        assert_eq!(reference.version_key_guid, reference.asset_guid);
        assert_eq!(reference.version_urn, ReadField::Value(literal.to_owned()));
        assert_eq!(
            pick.native.as_ref().map(|name| &name.value),
            Some(&NativeNameValue::Bytes(Base64Bytes(
                reference.encoded().unwrap()
            )))
        );
        let image = format!("{}file:source-image#0", prefix(ordinal));
        assert_eq!(
            merged
                .source_fidelity()
                .retained_record(&image)
                .unwrap()
                .data(),
            Some(members[ordinal].as_slice())
        );
        for feature in merged
            .ir()
            .model
            .features
            .iter()
            .filter(|feature| feature.id.as_str().starts_with(&prefix(ordinal)))
        {
            if let FeatureOperation::Sketch { sketch } = feature.evaluation.definition().operation()
            {
                let Some(sketch) = sketch.id() else { continue };
                assert!(sketch.as_str().starts_with(&prefix(ordinal)));
                assert!(merged
                    .ir()
                    .model
                    .sketches
                    .iter()
                    .any(|record| &record.id == sketch));
            }
        }
    }
}
