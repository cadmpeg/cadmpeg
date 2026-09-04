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

use cadmpeg_ir::codec::write::EncodeInput;
use cadmpeg_ir::codec::write::TargetRequest;
use std::io::Cursor;

use cadmpeg_ir::codec::write::Encoder;
use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::test_support::*;
use crate::F3dCodec;

#[test]
fn generated_source_less_writes_unassigned_protein_appearance() {
    use std::collections::BTreeMap;

    use cadmpeg_ir::appearance::Appearance;
    use cadmpeg_ir::ids::AppearanceId;
    use cadmpeg_ir::topology::Color;

    let visual_guid = "11111111-2222-3333-4444-555555555555";
    let appearance_id = AppearanceId::mint("generated:appearance#0").expect("identity grammar");
    let mut source_less = cadmpeg_ir::examples::unit_cube();
    source_less.model.appearances = vec![Appearance {
        id: appearance_id.clone(),
        name: Some("Prism-Generated".into()),
        asset_guid: Some(visual_guid.into()),
        library_id: None,
        visual_guid: Some(visual_guid.into()),
        physical_token: Some("PrismMaterial-Generated".into()),
        schema: Some("GenericSchema".into()),
        category: Some("Plastic/Generated".into()),
        base_color: Some(Color {
            r: 0.15,
            g: 0.35,
            b: 0.75,
            a: 1.0,
        }),
        properties: BTreeMap::from([
            ("reflectivity_at_0deg".into(), 0.25),
            ("refraction_index".into(), 1.5),
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
        Some(Color {
            r: 0.15,
            g: 0.35,
            b: 0.75,
            a: 1.0,
        })
    );
    assert_eq!(
        appearance.properties.get("reflectivity_at_0deg"),
        Some(&0.25)
    );
    assert_eq!(appearance.properties.get("refraction_index"), Some(&1.5));
    assert!(round_trip.ir().model.appearance_bindings.is_empty());
    assert!(crate::validate::validate_native(round_trip.ir()).is_empty());
    let validation = cadmpeg_ir::validate::validate_neutral(round_trip.ir(), Vec::new());
    assert!(
        validation.is_ok(),
        "validation findings: {:?}",
        validation.findings
    );
}

#[test]
fn generated_source_less_rejects_material_assignment_without_presentation_graph() {
    use crate::records::DesignMaterialAssignment;

    let mut source_less = cadmpeg_ir::examples::unit_cube();
    f3d_native_mut(&mut source_less).design_material_assignments = vec![DesignMaterialAssignment {
        id: "generated:material-assignment#0".into(),
        asm_body_key: 42,
        asm_body_key_offset: 0,
        entity_suffix: 985,
        entity_suffix_offset: 0,
        entity_id: "0_985".into(),
        entity_id_offset: 0,
        visual_guid: "11111111-2222-3333-4444-555555555555".into(),
        visual_guid_offset: 0,
        physical_token: Some("PrismMaterial-Generated".into()),
        physical_token_offset: None,
        visual_preset: None,
        visual_preset_offset: None,
    }];

    let error = F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut Vec::new()))
        .expect_err("an incomplete generated presentation graph must be refused");
    assert!(error
        .to_string()
        .contains("requires a typed body-presentation B-rep and scene graph"));
}

#[test]
fn generated_source_less_rejects_collapsed_visibility_body_bindings() {
    let mut source_less = cadmpeg_ir::examples::unit_cube();
    source_less.model.bodies[0].visible = Some(false);
    let body = source_less.model.bodies[0].id.clone();
    f3d_native_mut(&mut source_less).body_visibilities = [985, 986]
        .into_iter()
        .enumerate()
        .map(|(ordinal, entity_suffix)| crate::records::BodyVisibility {
            id: format!("generated:body-visibility#{ordinal}"),
            body: body.clone(),
            stream: "generated/Design1/BulkStream.dat".into(),
            byte_offset: 0,
            asm_body_key_offset: 0,
            asm_body_key: 42,
            entity_suffix,
            visible: false,
        })
        .collect();

    let error = F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut Vec::new()))
        .expect_err("conflicting body-map rows must not collapse");
    assert!(error
        .to_string()
        .contains("conflicts with the body-map key/suffix bijection"));
}

#[test]
fn generated_f3d_rejects_material_assignment_divergence() {
    let source = f3d_with_smbh_and_protein(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated material decode");
    let (mut edited, _, fidelity) = decoded.into_parts();
    update_f3d_native(&mut edited, |native| {
        native.design_material_assignments[0].physical_token = Some("PrismMaterial-019".into());
    });

    let error = crate::test_support::plan_inherited_write(&edited, &fidelity, &mut Vec::new())
        .expect_err("divergent assignment and appearance must fail");
    assert!(matches!(error, cadmpeg_core::CodecError::NotImplemented(_)));
}

#[test]
fn generated_f3d_rejects_partial_material_assignment_identity_edit() {
    let source = f3d_with_smbh_and_protein(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated material decode");
    let (mut edited, _, fidelity) = decoded.into_parts();
    update_f3d_native(&mut edited, |native| {
        let assignment = &mut native.design_material_assignments[0];
        assignment.entity_id = "0_986".into();
        assignment.entity_suffix = 986;
    });

    let error = crate::test_support::plan_inherited_write(&edited, &fidelity, &mut Vec::new())
        .expect_err("a partial presentation-graph identity edit must fail");
    assert!(error.to_string().contains(
        "requires synchronized body-presentation, browser-node, B-rep, and scene graphs"
    ));
}

#[test]
fn generated_f3d_rejects_invalid_or_structural_protein_property_edits() {
    let source = f3d_with_smbh_and_protein(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated Protein decode");

    let mut invalid = decoded.ir().clone();
    invalid.model.appearances[0]
        .properties
        .insert("refraction_index".into(), 0.5);
    let error = crate::test_support::plan_inherited_write(
        &invalid,
        decoded.source_fidelity(),
        &mut Vec::new(),
    )
    .expect_err("out-of-range refraction must be refused");
    assert!(
        matches!(error, cadmpeg_core::CodecError::Malformed(message) if message.contains("refraction_index"))
    );

    let (mut structural, _, fidelity) = decoded.into_parts();
    structural.model.appearances[0]
        .properties
        .insert("unserialized_property".into(), 0.5);
    let error = crate::test_support::plan_inherited_write(&structural, &fidelity, &mut Vec::new())
        .expect_err("new Protein property must be refused");
    assert!(
        matches!(error, cadmpeg_core::CodecError::NotImplemented(message) if message.contains("unchanged property set"))
    );
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
    edited.model.appearances[0].base_color = Some(cadmpeg_ir::topology::Color {
        r: 0.2,
        g: 0.3,
        b: 0.4,
        a: 1.0,
    });
    edited.model.appearances[1].base_color = Some(cadmpeg_ir::topology::Color {
        r: 0.6,
        g: 0.7,
        b: 0.8,
        a: 1.0,
    });

    let mut regenerated = Vec::new();
    crate::test_support::plan_inherited_write(&edited, &fidelity, &mut regenerated)
        .expect("multi-Protein appearance regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated multi-Protein decode");
    assert_eq!(round_trip.ir().model.appearances, edited.model.appearances);
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
    opaque.properties.insert("surface_roughness".into(), 0.75);
    let transparent = edited
        .model
        .appearances
        .iter_mut()
        .find(|appearance| appearance.schema.as_deref() == Some("PrismTransparentSchema"))
        .expect("transparent appearance");
    transparent
        .properties
        .insert("refraction_index".into(), 2.25);

    let mut regenerated = Vec::new();
    crate::test_support::plan_inherited_write(&edited, &fidelity, &mut regenerated)
        .expect("Prism scalar regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated Prism decode");
    assert!(round_trip.ir().model.appearances.iter().any(|appearance| {
        appearance.schema.as_deref() == Some("PrismOpaqueSchema")
            && appearance.properties.get("surface_roughness") == Some(&0.75)
    }));
    assert!(round_trip.ir().model.appearances.iter().any(|appearance| {
        appearance.schema.as_deref() == Some("PrismTransparentSchema")
            && appearance.properties.get("refraction_index") == Some(&2.25)
    }));
}
