// SPDX-License-Identifier: Apache-2.0
//! Sweep, loft, boundary and rib history output.

use super::*;

#[test]
fn semantic_writer_round_trips_typed_sweep() {
    use cadmpeg_ir::{
        features::{FeatureDefinition, FeatureOperation, PathRef, PlanarProfileRef},
        scalar::Angle,
    };

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords>
            <Sketch Name="ProfileA" Type="Sketch" id="21"/>
            <Sketch Name="Path" Type="Sketch" id="22"/>
            <Sketch Name="ProfileB" Type="Sketch" id="23"/>
            <Sweep Name="Pipe" Type="Sweep" id="24" Profile="21" Path="22" Operation="NewBody"><Dimension Name="Scale">1.5</Dimension><Dimension Name="Twist">90deg</Dimension></Sweep>
        </Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    let profile_a = decoded.ir().model.features[0].id.clone();
    let path = decoded.ir().model.features[1].native_ref.clone().unwrap();
    let profile_b = decoded.ir().model.features[2].id.clone();
    assert!(matches!(
        decoded.ir().model.features[3].evaluation.definition(), FeatureDefinition::Operation(FeatureOperation::Sweep {
            shape,
            path: Some(PathRef::Native(path_ref)),
            twist: Some(twist),
            scale: Some(scale),
            ..
        }) if matches!(
            (shape.referenced_profile(), shape.mode(),),
            (
                Some(PlanarProfileRef::Feature(profile)),
                cadmpeg_ir::features::SweepMode::Solid {
                    op: cadmpeg_ir::features::SolidSweepOperation::NewBody
                },
            )
                if decoded.ir().dimension_value(scale).unwrap().get() == 1.5 && profile == &profile_a
            && path_ref == &path
            && (decoded.ir().dimension_value(twist).unwrap().get() - std::f64::consts::FRAC_PI_2).abs() < 1.0e-12)));

    {
        let mut ir_edit = decoded.ir_mut();
        let owner = ir_edit.model.features[3].id.clone();
        ir_edit
            .set_dimension(&owner, "/twist", Angle::new(std::f64::consts::PI).unwrap())
            .unwrap();
        ir_edit
            .set_dimension(
                &owner,
                "/scale",
                cadmpeg_ir::scalar::PositiveReal::new(2.0).unwrap(),
            )
            .unwrap();
        ir_edit.model.features[3].evaluation.edit(|definition, _| {
            let FeatureDefinition::Operation(FeatureOperation::Sweep { shape, .. }) = definition
            else {
                panic!("typed sweep");
            };
            shape.set_referenced_profile(cadmpeg_ir::features::PlanarProfileRef::Feature(
                profile_b.clone(),
            ));
            let (section, sections) =
                std::mem::replace(shape, cadmpeg_ir::features::SweepShape::unresolved(None))
                    .into_sections();
            *shape = cadmpeg_ir::features::SweepShape::solid_sections(
                cadmpeg_ir::features::SolidSweepOperation::Join,
                section,
                sections,
            );
        });
        ir_edit.model.features[3]
            .dependencies
            .retain(|dependency| dependency != &profile_a);
        let mut dependencies = ir_edit.model.features[3].dependencies.to_vec();
        dependencies.insert(0, profile_b);
        ir_edit.model.features[3].dependencies =
            cadmpeg_ir::features::DistinctMembers::try_from(dependencies).unwrap();
    }

    let mut inconsistent = decoded.ir().clone();
    let mut dependencies = inconsistent.model.features[3].dependencies.to_vec();
    dependencies.remove(0);
    inconsistent.model.features[3].dependencies =
        cadmpeg_ir::features::DistinctMembers::try_from(dependencies).unwrap();
    let error = crate::test_support::plan_inherited_write(
        &inconsistent,
        decoded.source_fidelity(),
        &mut Vec::new(),
    )
    .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("profile feature is absent from the consumer's dependencies"),
        "{error}"
    );

    let mut encoded = Vec::new();
    crate::test_support::serialize_edited_history(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut encoded,
    )
    .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let feature = &sldprt_native(regenerated.ir()).feature_histories[0].features[3];
    assert_eq!(feature.properties["Profile"], "23");
    assert_eq!(feature.properties["Path"], "22");
    assert_eq!(feature.properties["Operation"], "Join");
    assert_eq!(feature.parameters["Scale"], "2");
    assert_eq!(
        feature.parameters["Twist"],
        format!("{}rad", std::f64::consts::PI)
    );
}

#[test]
fn semantic_writer_round_trips_sparse_surface_sweep() {
    use cadmpeg_ir::{
        features::{FeatureDefinition, FeatureOperation},
        scalar::Angle,
    };

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Feature Name="Surface-Sweep1" Type="Surface-Sweep" id="137"/></Keywords>"#,
    ));
    source.extend(make_block(
        0x42,
        "Contents/Config-0-ResolvedFeatures",
        &resolved_feature_classes_with_ids(&[("moSweep_c", "Surface-Sweep1", 137)]),
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    assert!(matches!(
        decoded.ir().model.features[0].evaluation.definition(), FeatureDefinition::Operation(FeatureOperation::Sweep {
            shape,
            path: None,
            twist: None,
            scale: None,
            ..
        }) if shape.section_is_unresolved()
            && matches!((shape.mode(),), (cadmpeg_ir::features::SweepMode::Surface {},))));

    {
        let mut ir_edit = decoded.ir_mut();
        ir_edit.model.features[0].evaluation.edit(|definition, _| {
            let FeatureDefinition::Operation(FeatureOperation::Sweep { twist, .. }) = definition
            else {
                panic!("surface sweep");
            };
            *twist = Some(cadmpeg_ir::Dimension::Literal(Angle::new(0.5).unwrap()));
        });
    }

    let mut encoded = Vec::new();
    crate::test_support::serialize_edited_history(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut encoded,
    )
    .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let native = &sldprt_native(regenerated.ir()).feature_histories[0].features[0];
    assert_eq!(native.kind, "Surface-Sweep");
    assert_eq!(native.parameters["Twist"], "0.5rad");
    assert!(!native.properties.contains_key("Profile"));
    assert!(!native.properties.contains_key("Path"));
    assert!(!native.properties.contains_key("Operation"));
    assert!(matches!(
        regenerated.ir().model.features[0].evaluation.definition(), FeatureDefinition::Operation(FeatureOperation::Sweep {
            shape,
            path: None,
            twist: Some(actual_twist),
            scale: None,
            ..
        }) if shape.section_is_unresolved()
            && matches!((shape.mode(),), (cadmpeg_ir::features::SweepMode::Surface {},) if regenerated.ir().dimension_value(actual_twist).unwrap().get() == 0.5)));
}

#[test]
fn semantic_writer_retains_native_solid_sweep_with_unresolved_operation() {
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation, SweepMode};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Feature Name="Operacion1" Type="Personalizado" id="137"/></Keywords>"#,
    ));
    source.extend(make_block(
        0x42,
        "Contents/Config-0-ResolvedFeatures",
        &resolved_feature_classes_with_ids(&[("moSweep_c", "Operacion1", 137)]),
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    assert!(matches!(
        decoded.ir().model.features[0].evaluation.definition(), FeatureDefinition::Operation(FeatureOperation::Sweep {
            shape,
            path: None,
            twist: None,
            scale: None,
            ..
        }) if shape.section_is_unresolved()
            && matches!((shape.mode(),), (SweepMode::Unresolved {},))));
    decoded.ir_mut().model.features[0].name = Some("Renamed sweep".into());

    let mut encoded = Vec::new();
    crate::test_support::plan_inherited_write(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut encoded,
    )
    .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        regenerated.ir().model.features[0].evaluation.definition(), FeatureDefinition::Operation(FeatureOperation::Sweep {
            shape,
            ..
        }) if matches!((shape.mode(),), (SweepMode::Unresolved {},))));
}

#[test]
fn semantic_writer_round_trips_typed_loft() {
    use cadmpeg_ir::features::{
        BooleanOp, FeatureDefinition, FeatureOperation, PathRef, PlanarProfileRef, ProfileRef,
    };

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords>
            <Sketch Name="SectionA" Type="Sketch" id="31"/>
            <Sketch Name="SectionB" Type="Sketch" id="32"/>
            <Sketch Name="SectionC" Type="Sketch" id="33"/>
            <Sketch Name="GuideA" Type="Sketch" id="34"/>
            <Sketch Name="GuideB" Type="Sketch" id="36"/>
            <Loft Name="Transition" Type="Loft" id="35" Profiles="31,32,33" Guides="34" Operation="NewBody" Closed="false"/>
        </Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    let native_refs = decoded.ir().model.features[..5]
        .iter()
        .map(|feature| feature.native_ref.clone().unwrap())
        .collect::<Vec<_>>();
    let feature_refs = decoded.ir().model.features[..5]
        .iter()
        .map(|feature| feature.id.clone())
        .collect::<Vec<_>>();
    assert!(matches!(
        decoded.ir().model.features[5].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Loft {
            sections,
            guidance: cadmpeg_ir::features::LoftGuidance::Guides(guides),
            op: BooleanOp::NewBody,
            closed: false,
            ..
        }) if sections == &vec![
            cadmpeg_ir::features::LoftSection::Profile(ProfileRef::Planar(PlanarProfileRef::Feature(feature_refs[0].clone()))),
            cadmpeg_ir::features::LoftSection::Profile(ProfileRef::Planar(PlanarProfileRef::Feature(feature_refs[1].clone()))),
            cadmpeg_ir::features::LoftSection::Profile(ProfileRef::Planar(PlanarProfileRef::Feature(feature_refs[2].clone()))),
        ] && guides == &vec![PathRef::Native(native_refs[3].clone())]
    ));

    {
        let mut ir_edit = decoded.ir_mut();
        let updated_ir_edit_evaluation = &mut ir_edit.model.features[5].evaluation;
        let mut updated_ir_edit_definition = updated_ir_edit_evaluation.definition().clone();
        let FeatureDefinition::Operation(FeatureOperation::Loft {
            sections,
            guidance,
            op,
            closed,
            ..
        }) = &mut updated_ir_edit_definition
        else {
            panic!("typed loft");
        };
        sections.swap(0, 2);
        *guidance = cadmpeg_ir::features::LoftGuidance::Guides(vec![PathRef::Native(
            native_refs[4].clone(),
        )]);
        *op = BooleanOp::Join;
        *closed = true;
        updated_ir_edit_evaluation.set_definition(updated_ir_edit_definition);
        ir_edit.model.features[5].dependencies =
            cadmpeg_ir::features::DistinctMembers::try_from(vec![
                feature_refs[2].clone(),
                feature_refs[1].clone(),
                feature_refs[0].clone(),
            ])
            .unwrap();
    }

    let mut encoded = Vec::new();
    crate::test_support::serialize_edited_history(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut encoded,
    )
    .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let feature = &sldprt_native(regenerated.ir()).feature_histories[0].features[5];
    assert_eq!(feature.properties["Profiles"], "33,32,31");
    assert_eq!(feature.properties["Guides"], "36");
    assert_eq!(feature.properties["Operation"], "Join");
    assert_eq!(feature.properties["Closed"], "true");
}

#[test]
fn semantic_writer_retains_unresolved_native_loft_construction() {
    use cadmpeg_ir::features::{BooleanOp, FeatureDefinition, FeatureOperation};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Loft Name="Unknown loft" Type="Custom" id="151"/></Keywords>"#,
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    assert!(matches!(
        decoded.ir().model.features[0].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Loft {
            ref sections,
            guidance: cadmpeg_ir::features::LoftGuidance::Guides(ref guides),
            op: BooleanOp::Unresolved,
            closed: false,
            ..
        }) if sections.is_empty() && guides.is_empty()
    ));
    decoded.ir_mut().model.features[0].name = Some("Renamed loft".into());

    let mut encoded = Vec::new();
    crate::test_support::plan_inherited_write(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut encoded,
    )
    .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let native = &sldprt_native(regenerated.ir()).feature_histories[0].features[0];
    assert!(!native.properties.contains_key("Profiles"));
    assert!(!native.properties.contains_key("Operation"));
    assert!(!native.properties.contains_key("Closed"));
    assert!(matches!(
        regenerated.ir().model.features[0].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Loft {
            op: BooleanOp::Unresolved,
            ..
        })
    ));
}

#[test]
fn semantic_writer_round_trips_boundary_boss_as_loft() {
    use cadmpeg_ir::features::{
        BooleanOp, FeatureDefinition, FeatureOperation, PlanarProfileRef, ProfileRef,
    };

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords>
            <Sketch Name="SectionA" Type="Sketch" id="41"/>
            <Sketch Name="SectionB" Type="Sketch" id="42"/>
            <Boundary Name="Blend" Type="BoundaryBoss" id="43" Profiles="41,42"/>
            <Boundary Name="Pocket" Type="BoundaryCut" id="44" Profiles="41,42"/>
        </Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    let refs = decoded.ir().model.features[..2]
        .iter()
        .map(|feature| feature.id.clone())
        .collect::<Vec<_>>();
    assert!(matches!(
        decoded.ir().model.features[2].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Loft {
            sections,
            guidance: cadmpeg_ir::features::LoftGuidance::Guides(guides),
            op: BooleanOp::Join,
            closed: false,
            ..
        }) if sections == &vec![
            cadmpeg_ir::features::LoftSection::Profile(ProfileRef::Planar(PlanarProfileRef::Feature(refs[0].clone()))),
            cadmpeg_ir::features::LoftSection::Profile(ProfileRef::Planar(PlanarProfileRef::Feature(refs[1].clone()))),
        ] && guides.is_empty()
    ));
    assert!(matches!(
        decoded.ir().model.features[3].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Loft {
            op: BooleanOp::Cut,
            closed: false,
            ..
        })
    ));

    {
        let mut ir_edit = decoded.ir_mut();
        let updated_ir_edit_evaluation = &mut ir_edit.model.features[2].evaluation;
        let mut updated_ir_edit_definition = updated_ir_edit_evaluation.definition().clone();
        let FeatureDefinition::Operation(FeatureOperation::Loft {
            sections, closed, ..
        }) = &mut updated_ir_edit_definition
        else {
            panic!("typed boundary loft");
        };
        sections.reverse();
        *closed = true;
        updated_ir_edit_evaluation.set_definition(updated_ir_edit_definition);
        let mut dependencies = ir_edit.model.features[2].dependencies.to_vec();
        dependencies.reverse();
        ir_edit.model.features[2].dependencies =
            cadmpeg_ir::features::DistinctMembers::try_from(dependencies).unwrap();
    }

    let mut encoded = Vec::new();
    crate::test_support::serialize_edited_history(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut encoded,
    )
    .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let feature = &sldprt_native(regenerated.ir()).feature_histories[0].features[2];
    assert_eq!(feature.xml_tag, "Boundary");
    assert_eq!(feature.kind, "BoundaryBoss");
    assert_eq!(feature.properties["Profiles"], "42,41");
    assert_eq!(feature.properties["Operation"], "Join");
    assert_eq!(feature.properties["Closed"], "true");
    assert!(matches!(
        regenerated.ir().model.features[3].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Loft {
            op: BooleanOp::Cut,
            ..
        })
    ));
}

#[test]
fn semantic_writer_retains_partial_native_rib_construction() {
    use cadmpeg_ir::features::{BooleanOp, FeatureDefinition, FeatureOperation, RibDraft};
    use cadmpeg_ir::math::Vector3;

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Rib Name="Unknown web" Type="Rib" id="42" Direction="0,1,0"><Dimension Name="Thickness">NaNmm</Dimension><Dimension Name="Draft">NaNrad</Dimension></Rib></Keywords>"#,
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    assert!(matches!(
       decoded.ir().model.features[0].evaluation.definition(),
       FeatureDefinition::Operation(FeatureOperation::Rib {
           construction: cadmpeg_ir::features::RibConstruction {
               profile: None,
               direction: Some(
                   geometry_1
               ),
               thickness: None,
               side: None,
               draft: RibDraft::Unresolved,
           },
           op: BooleanOp::Unresolved,
       })
    if matches!(geometry_1.get(), Vector3 {
                       x: 0.0,
                       y: 1.0,
                       z: 0.0
                   })));
    let mut detached = decoded.ir().clone();
    detached.model.features[0].native_ref = None;
    crate::writer::tests::parameters_extrude::assert_solved_feature_reduction(
        &detached,
        decoded.source_fidelity(),
        0,
        "unresolved rib construction",
    );

    decoded.ir_mut().model.features[0].name = Some("Renamed web".into());
    let mut encoded = Vec::new();
    crate::test_support::plan_inherited_write(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut encoded,
    )
    .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let native = &sldprt_native(regenerated.ir()).feature_histories[0].features[0];
    assert_eq!(native.name, "Renamed web");
    assert_eq!(native.properties["Direction"], "0,1,0");
    assert!(!native.properties.contains_key("Profile"));
    assert!(!native.properties.contains_key("BothSides"));
    assert!(!native.properties.contains_key("Operation"));
    assert_eq!(native.parameters["Thickness"], "NaNmm");
    assert_eq!(native.parameters["Draft"], "NaNrad");
}

#[test]
fn semantic_writer_round_trips_typed_rib() {
    use cadmpeg_ir::features::{BooleanOp, FeatureDefinition, FeatureOperation, RibDraft, RibSide};
    use cadmpeg_ir::math::Vector3;

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Sketch Name="RibProfile" Type="Sketch" id="41"/><Rib Name="Web" Type="Rib" id="42" Profile="41" Direction="0,1,0" BothSides="false" Operation="Join"><Dimension Name="Thickness">2mm</Dimension><Dimension Name="Draft">5deg</Dimension></Rib></Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    let profile_ref = decoded.ir().model.features[0].id.clone();
    assert!(matches!(
    decoded.ir().model.features[1].evaluation.definition(), FeatureDefinition::Operation(FeatureOperation::Rib {
        construction: cadmpeg_ir::features::RibConstruction {
            profile,
            direction: Some(geometry_1),
            thickness: Some(actual_thickness),
            side: Some(RibSide::OneSided),
            draft: RibDraft::Angle(value),
        },
        op: BooleanOp::Join,
    }) if matches!(
        (profile.as_ref(),),
        (Some(cadmpeg_ir::features::PlanarProfileRef::Feature(profile)),)
            if (
                (profile == &profile_ref && (decoded.ir().dimension_value(value).unwrap().get() - 5f64.to_radians()).abs() < 1.0e-12)
                && decoded.ir().dimension_value(actual_thickness).unwrap().get() == 2.0
            )
            && matches!(geometry_1.get(), Vector3 { x: 0.0, y: 1.0, z: 0.0 })
    )));

    let moved = {
        let owner = &decoded.ir().model.features[1].id;
        decoded
            .ir()
            .model
            .parameters
            .iter()
            .filter(|parameter| {
                parameter.owner.as_ref() == Some(owner)
                    && ["Draft", "Thickness"].contains(&parameter.name.as_str())
            })
            .map(|parameter| {
                (
                    parameter.id.to_string(),
                    parameter.name.clone(),
                    parameter.expression.clone(),
                )
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(moved.len(), 2);
    {
        let mut ir_edit = decoded.ir_mut();
        ir_edit.model.features[1].evaluation.edit(|definition, _| {
            let FeatureDefinition::Operation(FeatureOperation::Rib { construction, op }) =
                definition
            else {
                panic!("typed rib");
            };
            construction.direction = Some(
                cadmpeg_ir::features::FeatureDirection3::new(Vector3::new(1.0, 0.0, 0.0)).unwrap(),
            );
            construction.thickness = Some(cadmpeg_ir::Dimension::Literal(
                cadmpeg_ir::scalar::PositiveLength::new(3.0).unwrap(),
            ));
            construction.side = Some(RibSide::Centered);
            construction.draft = RibDraft::None;
            *op = BooleanOp::NewBody;
        });
    }

    let mut encoded = Vec::new();
    let report = SldprtCodec
        .plan(
            cadmpeg_ir::codec::write::EncodeInput::new(
                decoded.ir(),
                Some(decoded.source_fidelity()),
            ),
            cadmpeg_ir::codec::write::target::TargetRequest::Inherit,
        )
        .unwrap()
        .write_to(&mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    for (parameter, name, expression) in &moved {
        crate::writer::tests::assert_moved_to_global(
            &report,
            regenerated.ir(),
            parameter,
            name,
            expression,
        );
    }
    let feature = &sldprt_native(regenerated.ir()).feature_histories[0].features[1];
    assert_eq!(feature.properties["Profile"], "41");
    assert_eq!(feature.properties["Direction"], "1,0,0");
    assert_eq!(feature.properties["BothSides"], "true");
    assert_eq!(feature.properties["Operation"], "NewBody");
    assert_eq!(feature.parameters["Thickness"], "3mm");
    assert!(!feature.parameters.contains_key("Draft"));
}
