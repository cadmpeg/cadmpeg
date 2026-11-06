// SPDX-License-Identifier: Apache-2.0
//! Writer-domain synthetic tests.
#![allow(clippy::unwrap_used)]
#![allow(
    clippy::cloned_ref_to_slice_refs,
    clippy::default_trait_access,
    clippy::if_not_else,
    clippy::needless_pass_by_value,
    clippy::range_plus_one,
    clippy::semicolon_if_nothing_returned,
    clippy::trivially_copy_pass_by_ref
)]

use cadmpeg_test_support::EditableDecodeResult;

use cadmpeg_ir::codec::write::target::TargetRequest;
use cadmpeg_ir::codec::write::EncodeInput;
use std::io::Cursor;

use cadmpeg_ir::codec::write::Encoder;
use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::test_support::native_test::{f3d_native, f3d_native_mut, update_f3d_native};
use crate::test_support::protein_test::generated_prism_instance_properties;
use crate::test_support::smbh_geometry_test::synthetic_geometry_smbh;
use crate::test_support::zip_test::{
    f3d_with_smbh_and_instance_properties, f3d_with_smbh_and_protein,
    f3d_with_smbh_and_protein_guids,
};
use crate::F3dCodec;

#[test]
fn source_less_synthesis_writes_body_material_assignment() {
    let source = f3d_with_smbh_and_protein(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut ir = decoded.ir().clone();
    ir.model.appearances[0].physical_token = Some("PrismMaterial-019".into());
    let (written, _) = super::source_less_analytic::synthesize_document(&ir, None);
    assert_eq!(written.ir().model.appearance_bindings.len(), 1);
    assert_eq!(
        written.ir().model.appearance_bindings[0].target,
        ir.model.appearance_bindings[0].target
    );
    assert_eq!(
        written.ir().model.appearance_bindings[0].appearance,
        ir.model.appearance_bindings[0].appearance
    );
    assert_eq!(
        written.ir().model.appearance_bindings[0].channels,
        ir.model.appearance_bindings[0].channels
    );
    assert_eq!(
        written.ir().model.appearances[0].physical_token.as_deref(),
        Some("PrismMaterial-019")
    );
    assert_eq!(
        f3d_native(written.ir()).design_material_assignments.len(),
        1
    );
    assert_eq!(
        f3d_native(written.ir()).design_material_assignments[0]
            .entity_id
            .as_str(),
        "0_985"
    );
}

#[test]
fn source_less_material_assignment_without_protein_preserves_appearance() {
    let source = f3d_with_smbh_and_protein(&synthetic_geometry_smbh());
    let decoded = F3dCodec.decode(&mut Cursor::new(source), &DecodeOptions::default()).unwrap();
    let mut ir = decoded.ir().clone();
    let appearance = &mut ir.model.appearances[0];
    appearance.physical_token = Some("PrismMaterial-019".into());
    appearance.base_color = None;
    appearance.schema = None;
    appearance.library_id = None;
    appearance.category = None;
    appearance.properties.clear();
    appearance.textures.clear();
    let (written, _) = super::source_less_analytic::synthesize_document(&ir, None);
    assert_eq!(written.ir().model.appearance_bindings.len(), 1);
    assert_eq!(written.ir().model.appearances.len(), 1);
    assert_eq!(written.ir().model.appearances[0].physical_token, ir.model.appearances[0].physical_token);
    assert_eq!(written.ir().model.appearances[0].base_color, None);
}

#[test]
fn neutral_body_material_authors_design_registry() {
    use cadmpeg_ir::appearance::{Appearance, AppearanceBinding, AppearanceTarget};
    let mut ir = cadmpeg_ir::examples::unit_cube().unwrap();
    ir.model.bodies[0].visible = Some(true);
    let guid = "11111111-2222-3333-4444-555555555555";
    let appearance_id = format!("f3d:design:appearance#{guid}").try_into().unwrap();
    ir.model.appearances.push(Appearance {
        id: appearance_id,
        native_ref: None,
        name: Some("Prism-001".into()),
        asset_guid: Some(guid.into()),
        library_id: None,
        visual_guid: Some(guid.into()),
        physical_token: Some("PrismMaterial-019".into()),
        schema: Some("GenericSchema".into()),
        category: None,
        base_color: Some(cadmpeg_ir::topology::Color::new(0.2, 0.4, 0.6, 1.0).unwrap()),
        properties: Default::default(),
        textures: Vec::new(),
    });
    ir.model.appearance_bindings.push(AppearanceBinding {
        id: "authored:model:appearance-binding#1".try_into().unwrap(),
        native_ref: None,
        target: AppearanceTarget::Body(ir.model.bodies[0].id.clone()),
        appearance: ir.model.appearances[0].id.clone(),
        source_entity_id: None,
        object_type: None,
        visible: None,
        channels: Default::default(),
    });
    let mut without_guid = ir.clone();
    without_guid.model.appearances[0].asset_guid = None;
    without_guid.model.appearances[0].visual_guid = None;
    let (written_without_guid, _) =
        super::source_less_analytic::synthesize_document(&without_guid, None);
    assert_eq!(written_without_guid.ir().model.appearance_bindings.len(), 1);
    assert_eq!(
        written_without_guid.ir().model.appearances[0]
            .physical_token
            .as_deref(),
        Some("PrismMaterial-019")
    );
    assert_eq!(
        written_without_guid.ir().model.appearances[0].base_color,
        without_guid.model.appearances[0].base_color
    );
    let mut multiple = ir.clone();
    let body_id: cadmpeg_ir::ids::BodyId = "authored:model:body#wire".try_into().unwrap();
    let region_id: cadmpeg_ir::ids::RegionId = "authored:model:region#wire".try_into().unwrap();
    let shell_id: cadmpeg_ir::ids::ShellId = "authored:model:shell#wire".try_into().unwrap();
    let vertex_id: cadmpeg_ir::ids::VertexId = "authored:model:vertex#wire".try_into().unwrap();
    let point_id: cadmpeg_ir::ids::PointId = "authored:model:point#wire".try_into().unwrap();
    multiple.model.bodies.push(cadmpeg_ir::topology::Body {
        native_ref: None,
        id: body_id.clone(),
        kind: cadmpeg_ir::topology::BodyKind::Wire,
        regions: vec![region_id.clone()],
        transform: None,
        name: None,
        color: None,
        visible: Some(true),
    });
    multiple.model.regions.push(cadmpeg_ir::topology::Region {
        native_ref: None,
        id: region_id.clone(),
        body: body_id,
        shells: vec![shell_id.clone()],
    });
    multiple.model.shells.push(
        cadmpeg_ir::topology::Shell::new(
            shell_id,
            region_id,
            vec![],
            vec![],
            vec![vertex_id.clone()],
        )
        .unwrap(),
    );
    multiple.model.vertices.push(cadmpeg_ir::topology::Vertex {
        native_ref: None,
        id: vertex_id,
        point: point_id.clone(),
        tolerance: None,
    });
    multiple.model.points.push(cadmpeg_ir::topology::Point::new(
        point_id,
        multiple.model.points[0].position(),
        None,
    ));
    let (multiple_written, _) = super::source_less_analytic::synthesize_document(&multiple, None);
    assert_eq!(multiple_written.ir().model.bodies.len(), 2);
    assert_eq!(multiple_written.ir().model.appearance_bindings.len(), 1);
    assert_eq!(
        multiple_written.ir().model.appearances[0]
            .physical_token
            .as_deref(),
        Some("PrismMaterial-019")
    );
    let mut second_binding = multiple.model.appearance_bindings[0].clone();
    second_binding.id = "authored:model:appearance-binding#2".try_into().unwrap();
    second_binding.target = AppearanceTarget::Body(multiple.model.bodies[1].id.clone());
    let mut second_appearance = multiple.model.appearances[0].clone();
    let second_guid = "22222222-3333-4444-5555-666666666666";
    second_appearance.id = format!("f3d:design:appearance#{second_guid}")
        .try_into()
        .unwrap();
    second_appearance.asset_guid = Some(second_guid.into());
    second_appearance.visual_guid = Some(second_guid.into());
    second_appearance.physical_token = Some("PrismMaterial-020".into());
    second_appearance.base_color =
        Some(cadmpeg_ir::topology::Color::new(0.8, 0.1, 0.2, 1.0).unwrap());
    second_binding.appearance = second_appearance.id.clone();
    multiple.model.appearances.push(second_appearance);
    multiple.model.appearance_bindings.push(second_binding);
    let (multiple_bound, _) = super::source_less_analytic::synthesize_document(&multiple, None);
    assert_eq!(multiple_bound.ir().model.bodies.len(), 2);
    assert_eq!(multiple_bound.ir().model.appearance_bindings.len(), 2);
    for binding in &multiple_bound.ir().model.appearance_bindings {
        let AppearanceTarget::Body(body) = &binding.target else {
            panic!("body assignment")
        };
        let ordinal = multiple_bound
            .ir()
            .model
            .bodies
            .iter()
            .position(|item| &item.id == body)
            .unwrap();
        let original = multiple
            .model
            .appearance_bindings
            .iter()
            .find(|item| {
                item.target == AppearanceTarget::Body(multiple.model.bodies[ordinal].id.clone())
            })
            .unwrap();
        let expected = multiple
            .model
            .appearances
            .iter()
            .find(|item| item.id == original.appearance)
            .unwrap();
        let actual = multiple_bound
            .ir()
            .model
            .appearances
            .iter()
            .find(|item| item.id == binding.appearance)
            .unwrap();
        assert_eq!(actual.base_color, expected.base_color);
        assert_eq!(actual.physical_token, expected.physical_token);
    }
    assert_eq!(
        f3d_native(multiple_bound.ir())
            .design_material_assignments
            .len(),
        2
    );
    let (written, report) = super::source_less_analytic::synthesize_document(&ir, None);
    assert!(written.source_fidelity().unrepresented().is_empty());
    assert_eq!(written.ir().model.appearance_bindings.len(), 1);
    assert_eq!(
        written.ir().model.appearance_bindings[0].target,
        AppearanceTarget::Body(written.ir().model.bodies[0].id.clone())
    );
    assert_eq!(
        written.ir().model.appearance_bindings[0].appearance,
        ir.model.appearance_bindings[0].appearance
    );
    assert_eq!(
        written.ir().model.appearances[0].physical_token.as_deref(),
        Some("PrismMaterial-019")
    );
    assert!(report
        .losses
        .iter()
        .any(|loss| loss.message.contains("authored:model:appearance-binding#1")));
    let (mut edited, _, fidelity) = written.into_parts();
    let color = cadmpeg_ir::topology::Color::new(0.7, 0.5, 0.3, 1.0).unwrap();
    edited.model.appearances[0].base_color = Some(color);
    let plan = F3dCodec
        .plan(
            EncodeInput::new(&edited, Some(&fidelity)),
            TargetRequest::Inherit,
        )
        .unwrap();
    assert!(matches!(
        plan.report().write_path(),
        cadmpeg_ir::report::export::WritePath::Patched { .. }
    ));
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let patched = F3dCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert_eq!(patched.ir().model.appearances[0].base_color, Some(color));
}

#[test]
fn generated_source_less_writes_unassigned_protein_appearance() {
    use std::collections::BTreeMap;

    use cadmpeg_ir::appearance::Appearance;
    use cadmpeg_ir::ids::AppearanceId;
    use cadmpeg_ir::topology::Color;

    let visual_guid = "11111111-2222-3333-4444-555555555555";
    let appearance_id =
        AppearanceId::mint("generated:test:appearance#0").expect("identity grammar");
    let mut source_less = cadmpeg_ir::examples::unit_cube().expect("unit cube fixture is admitted");
    source_less.model.appearances = vec![Appearance {
        native_ref: None,
        id: appearance_id.clone(),
        name: Some("Prism-Generated".into()),
        asset_guid: Some(visual_guid.into()),
        library_id: None,
        visual_guid: Some(visual_guid.into()),
        physical_token: Some("PrismMaterial-Generated".into()),
        schema: Some("GenericSchema".into()),
        category: Some("Plastic/Generated".into()),
        base_color: Some(Color::new(0.15, 0.35, 0.75, 1.0).expect("valid color")),
        properties: BTreeMap::from([
            (
                cadmpeg_core::nonblank_literal!("reflectivity_at_0deg"),
                cadmpeg_ir::scalar::FiniteReal::new(0.25).expect("finite scalar"),
            ),
            (
                cadmpeg_core::nonblank_literal!("refraction_index"),
                cadmpeg_ir::scalar::FiniteReal::new(1.5).expect("finite scalar"),
            ),
        ]),
        textures: Vec::new(),
    }];
    let mut encoded = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less Protein appearance encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less Protein appearance round trip");
    assert_eq!(round_trip.ir().model.appearances.len(), 1);
    let appearance = &round_trip.ir().model.appearances[0];
    assert_eq!(appearance.name.as_deref(), Some("Prism-Generated"));
    assert_eq!(appearance.visual_guid.as_deref(), Some(visual_guid));
    assert_eq!(appearance.schema.as_deref(), Some("GenericSchema"));
    assert_eq!(appearance.category.as_deref(), Some("Plastic/Generated"));
    assert_eq!(
        appearance.base_color,
        Some(Color::new(0.15, 0.35, 0.75, 1.0).expect("valid color"))
    );
    assert_eq!(
        appearance
            .properties
            .get("reflectivity_at_0deg")
            .map(|value| value.get()),
        Some(0.25)
    );
    assert_eq!(
        appearance
            .properties
            .get("refraction_index")
            .map(|value| value.get()),
        Some(1.5)
    );
    assert!(round_trip.ir().model.appearance_bindings.is_empty());
    assert!(crate::test_support::with_decode_context(|ctx| {
        crate::validate::validate_native(ctx, round_trip.ir())
            .expect("validate the native namespace")
    })
    .is_empty());
    let validation = cadmpeg_ir::validate::validate_neutral(round_trip.ir(), Vec::new())
        .expect("validate the neutral model");
    assert!(
        validation.is_ok(),
        "validation findings: {:?}",
        validation.findings
    );
}

#[test]
fn generated_source_less_reports_unsupported_material_assignment() {
    use crate::records::references::DesignMaterialAssignment;

    let mut source_less = cadmpeg_ir::examples::unit_cube().expect("unit cube fixture is admitted");
    f3d_native_mut(&mut source_less).design_material_assignments = vec![DesignMaterialAssignment {
        body_binding_id: "f3d:design:design-body-binding#25"
            .to_owned()
            .try_into()
            .expect("typed binding identity"),
        id: "f3d:generated:material-assignment#0".into(),
        asm_body_key: 42,
        asm_body_key_offset: None,

        entity_suffix_offset: None,
        entity_id: crate::records::identity::DesignEntityId::try_from("0_985".to_owned())
            .expect("valid entity ID"),
        entity_id_offset: None,
        visual_guid: crate::records::references::DesignVisualToken::try_from(
            "11111111-2222-3333-4444-555555555555".to_owned(),
        )
        .unwrap(),
        visual_guid_offset: None,
        physical_token: Some(crate::records::identity::RecordedValue {
            value: "PrismMaterial-Generated".into(),
            offset: None,
        }),
        visual_preset: None,
    }];

    let (output, report) = super::source_less_analytic::synthesize_document(&source_less, None);
    super::source_less_analytic::assert_native_omission(&report);
    assert!(crate::test_support::native_test::f3d_native(output.ir())
        .design_material_assignments
        .is_empty());
    assert_eq!(
        output.ir().model.bodies.len(),
        source_less.model.bodies.len()
    );
}

#[test]
fn generated_source_less_regenerates_duplicate_visibility_layout_hints() {
    let mut source_less = cadmpeg_ir::examples::unit_cube().expect("unit cube fixture is admitted");
    source_less.model.bodies[0].visible = Some(false);
    let row = cadmpeg_asm::brep::records::identity::NativeRowId::from(
        "f3d:asm:body-native-key#1".to_string(),
    );
    source_less.model.bodies[0].native_ref = Some(row.to_string());
    f3d_native_mut(&mut source_less).body_visibilities = [985, 986]
        .into_iter()
        .enumerate()
        .map(|(ordinal, entity_suffix)| {
            crate::records::bodies::BodyVisibility::try_from(
                crate::records::bodies::BodyVisibilityWire {
                    id: format!("f3d:generated-{ordinal}:body-visibility#42"),
                    body: row.clone(),
                    stream: "generated/Design1/BulkStream.dat".into(),
                    byte_offset: 0,
                    asm_body_key_offset: 0,
                    asm_body_key: 42,
                    entity_suffix,
                    visible: false,
                },
            )
            .unwrap()
        })
        .collect();

    let (output, report) = super::source_less_analytic::synthesize_document(&source_less, None);
    assert_eq!(output.ir().model.bodies.len(), 1);
    assert_eq!(output.ir().model.faces.len(), source_less.model.faces.len());
    assert_eq!(output.ir().model.bodies[0].visible, Some(false));
    assert_eq!(f3d_native(output.ir()).body_visibilities.len(), 1);
    super::source_less_analytic::assert_native_omission(&report);
}

#[test]
fn generated_f3d_neutral_appearance_wins_over_assignment_divergence() {
    let source = f3d_with_smbh_and_protein(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated material decode");
    let (mut edited, _, fidelity) = decoded.into_parts();
    update_f3d_native(&mut edited, |native| {
        native.design_material_assignments[0]
            .physical_token
            .as_mut()
            .expect("material field")
            .value = "PrismMaterial-019".into();
    });

    let (output, report) =
        super::source_less_analytic::synthesize_document(&edited, Some(&fidelity));
    super::source_less_analytic::assert_native_omission(&report);
    let native = f3d_native(output.ir());
    assert_eq!(native.design_material_assignments.len(), 1);
    assert_eq!(
        native.design_material_assignments[0]
            .physical_token
            .as_ref()
            .map(|token| token.value.as_str()),
        edited.model.appearances[0].physical_token.as_deref()
    );
    assert_eq!(output.ir().model.appearance_bindings.len(), 1);
    assert_eq!(output.ir().model.bodies.len(), edited.model.bodies.len());
    assert_eq!(
        output.ir().model.appearances[0].base_color,
        edited.model.appearances[0].base_color
    );
}

#[test]
fn generated_f3d_synthesizes_partial_material_assignment_identity_edit() {
    let source = f3d_with_smbh_and_protein(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated material decode");
    let (mut edited, _, fidelity) = decoded.into_parts();
    update_f3d_native(&mut edited, |native| {
        let assignment = &mut native.design_material_assignments[0];
        assignment.entity_id =
            crate::records::identity::DesignEntityId::try_from("0_986".to_owned())
                .expect("valid entity ID");
    });

    let (output, report) =
        super::source_less_analytic::synthesize_document(&edited, Some(&fidelity));
    super::source_less_analytic::assert_native_omission(&report);
    assert!(crate::test_support::native_test::f3d_native(output.ir())
        .design_material_assignments
        .is_empty());
    assert_eq!(output.ir().model.bodies.len(), edited.model.bodies.len());
    assert_eq!(
        output.ir().model.appearances[0].base_color,
        edited.model.appearances[0].base_color
    );
}

#[test]
fn generated_f3d_reports_unencodable_protein_properties() {
    let source = f3d_with_smbh_and_protein(&synthetic_geometry_smbh());
    let decoded = EditableDecodeResult::from(
        F3dCodec
            .decode(&mut Cursor::new(&source), &DecodeOptions::default())
            .expect("generated Protein decode"),
    );

    let mut invalid = decoded.ir().clone();
    invalid.model.appearances[0].properties.insert(
        cadmpeg_core::nonblank_literal!("refraction_index"),
        cadmpeg_ir::scalar::FiniteReal::HALF,
    );
    let (output, report) =
        super::source_less_analytic::synthesize_document(&invalid, Some(decoded.source_fidelity()));
    assert_eq!(output.ir().model.bodies.len(), invalid.model.bodies.len());
    assert_eq!(
        output.ir().model.appearances[0].base_color,
        invalid.model.appearances[0].base_color
    );
    assert!(!output.ir().model.appearances[0]
        .properties
        .contains_key("refraction_index"));
    assert!(report.losses.iter().any(
        |loss| loss.code == crate::loss::F3dLossCode::WriterAppearancePropertiesOmitted.kind()
    ));

    let (mut structural, _, fidelity) = decoded.into_parts();
    structural.model.appearances[0].properties.insert(
        cadmpeg_core::nonblank_literal!("unserialized_property"),
        cadmpeg_ir::scalar::FiniteReal::HALF,
    );
    let (output, report) =
        super::source_less_analytic::synthesize_document(&structural, Some(&fidelity));
    assert_eq!(
        output.ir().model.bodies.len(),
        structural.model.bodies.len()
    );
    assert_eq!(
        output.ir().model.appearances[0].base_color,
        structural.model.appearances[0].base_color
    );
    assert!(!output.ir().model.appearances[0]
        .properties
        .contains_key("unserialized_property"));
    assert!(report.losses.iter().any(
        |loss| loss.code == crate::loss::F3dLossCode::WriterAppearancePropertiesOmitted.kind()
    ));
}

#[test]
fn generated_f3d_routes_appearance_edits_across_multiple_protein_assets() {
    let source = f3d_with_smbh_and_protein_guids(
        &synthetic_geometry_smbh(),
        &[
            "11111111-2222-3333-4444-555555555555",
            "99999999-2222-3333-4444-555555555555",
        ],
    );
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated multi-Protein decode");
    assert_eq!(decoded.ir().model.appearances.len(), 2);
    let (mut edited, _, fidelity) = decoded.into_parts();
    edited.model.appearances[0].base_color =
        Some(cadmpeg_ir::topology::Color::new(0.2, 0.3, 0.4, 1.0).expect("valid color"));
    edited.model.appearances[1].base_color =
        Some(cadmpeg_ir::topology::Color::new(0.6, 0.7, 0.8, 1.0).expect("valid color"));

    let mut regenerated = Vec::new();
    crate::test_support::plan_inherited_write(&edited, &fidelity, &mut regenerated)
        .expect("multi-Protein appearance regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated multi-Protein decode");
    assert_eq!(round_trip.ir().model.appearances, edited.model.appearances);
    let (_, report) = super::source_less_analytic::synthesize_document(&edited, None);
    assert!(report.losses.iter().any(|loss| {
        loss.code.local_code() == "record.residual-unavailable"
            && loss.message.contains("physical asset identity")
    }));
}

#[test]
fn generated_f3d_rewrites_prism_scalar_properties() {
    let source = f3d_with_smbh_and_instance_properties(
        &synthetic_geometry_smbh(),
        &[
            generated_prism_instance_properties(
                "PrismOpaqueSchema",
                "11111111-2222-3333-4444-555555555555",
            ),
            generated_prism_instance_properties(
                "PrismTransparentSchema",
                "99999999-2222-3333-4444-555555555555",
            ),
        ],
    );
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated Prism decode");
    let (mut edited, _, fidelity) = decoded.into_parts();
    let opaque = edited
        .model
        .appearances
        .iter_mut()
        .find(|appearance| appearance.schema.as_deref() == Some("PrismOpaqueSchema"))
        .expect("opaque appearance");
    opaque.properties.insert(
        cadmpeg_core::nonblank_literal!("surface_roughness"),
        cadmpeg_ir::scalar::FiniteReal::new(0.75).expect("finite scalar"),
    );
    let transparent = edited
        .model
        .appearances
        .iter_mut()
        .find(|appearance| appearance.schema.as_deref() == Some("PrismTransparentSchema"))
        .expect("transparent appearance");
    transparent.properties.insert(
        cadmpeg_core::nonblank_literal!("refraction_index"),
        cadmpeg_ir::scalar::FiniteReal::new(2.25).expect("finite scalar"),
    );

    let mut regenerated = Vec::new();
    crate::test_support::plan_inherited_write(&edited, &fidelity, &mut regenerated)
        .expect("Prism scalar regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated Prism decode");
    assert!(round_trip.ir().model.appearances.iter().any(|appearance| {
        appearance.schema.as_deref() == Some("PrismOpaqueSchema")
            && appearance
                .properties
                .get("surface_roughness")
                .map(|value| value.get())
                == Some(0.75)
    }));
    assert!(round_trip.ir().model.appearances.iter().any(|appearance| {
        appearance.schema.as_deref() == Some("PrismTransparentSchema")
            && appearance
                .properties
                .get("refraction_index")
                .map(|value| value.get())
                == Some(2.25)
    }));
}

#[test]
fn synthesized_appearance_properties_report_each_unwritten_key() {
    use cadmpeg_ir::{appearance::Appearance, scalar::FiniteReal};
    for (schema, supported) in [
        (
            "GenericSchema",
            vec!["reflectivity_at_0deg", "refraction_index"],
        ),
        ("PrismOpaqueSchema", vec!["surface_roughness"]),
        ("PrismMetalSchema", vec!["surface_roughness"]),
        ("PrismTransparentSchema", vec!["refraction_index"]),
        (
            "PhysMatSchema",
            vec!["reflectivity_at_0deg", "refraction_index"],
        ),
    ] {
        let mut ir = cadmpeg_ir::examples::unit_cube().unwrap();
        let properties = [
            "reflectivity_at_0deg",
            "refraction_index",
            "surface_roughness",
            "extra_property",
        ]
        .into_iter()
        .map(|key| {
            (
                cadmpeg_core::text::NonBlankString::new(key).unwrap(),
                FiniteReal::ONE,
            )
        })
        .collect();
        ir.model.appearances.push(Appearance {
            native_ref: None,
            id: "test:model:appearance#properties".try_into().unwrap(),
            name: Some("test".into()),
            asset_guid: Some("11111111-2222-3333-4444-555555555555".into()),
            visual_guid: None,
            physical_token: None,
            library_id: None,
            schema: Some(schema.into()),
            category: None,
            base_color: Some(cadmpeg_ir::topology::Color::new(0.2, 0.3, 0.4, 1.0).unwrap()),
            properties,
            textures: Vec::new(),
        });
        let plan = F3dCodec
            .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
            .unwrap();
        let losses = plan
            .report()
            .losses
            .iter()
            .filter(|loss| {
                loss.code == crate::loss::F3dLossCode::WriterAppearancePropertiesOmitted.kind()
            })
            .collect::<Vec<_>>();
        assert_eq!(
            losses.len(),
            4 - supported.len() + usize::from(schema == "PhysMatSchema"),
            "each unwritable property needs its own loss"
        );
        for key in [
            "reflectivity_at_0deg",
            "refraction_index",
            "surface_roughness",
            "extra_property",
        ] {
            assert_eq!(
                losses.iter().any(|loss| loss.message.contains(key)),
                !supported.contains(&key),
                "property {key} under schema {schema}"
            );
        }
        let mut bytes = Vec::new();
        plan.write_to(&mut bytes).unwrap();
        let output = F3dCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .unwrap();
        assert_eq!(output.ir().model.bodies.len(), ir.model.bodies.len());
        assert_eq!(
            output.ir().model.appearances[0].base_color,
            ir.model.appearances[0].base_color
        );
        assert_eq!(
            output.ir().model.appearances[0]
                .properties
                .keys()
                .map(cadmpeg_core::text::NonBlankString::as_str)
                .collect::<std::collections::BTreeSet<_>>(),
            supported.iter().copied().collect()
        );
        ir.model.appearances[0]
            .properties
            .retain(|key, _| supported.contains(&key.as_str()));
        let plan = F3dCodec
            .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
            .unwrap();
        let reductions = plan
            .report()
            .losses
            .iter()
            .filter(|loss| {
                loss.code == crate::loss::F3dLossCode::WriterAppearancePropertiesOmitted.kind()
            })
            .collect::<Vec<_>>();
        if schema == "PhysMatSchema" {
            assert_eq!(
                reductions.len(),
                1,
                "the physical-to-visual schema reduction remains explicit"
            );
            assert!(reductions[0]
                .message
                .to_ascii_lowercase()
                .contains("schema"));
        } else {
            assert!(
                reductions.is_empty(),
                "supported property sets have no property omission"
            );
        }
    }
}
