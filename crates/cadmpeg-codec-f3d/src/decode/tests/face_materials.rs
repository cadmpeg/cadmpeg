// SPDX-License-Identifier: Apache-2.0

use cadmpeg_ir::appearance::Appearance;
use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue, SourceAttribute};
use cadmpeg_ir::topology::Color;

use crate::materials::FaceAppearanceAssignment;

const FACE_GUID: &str = "aaaaaaaa-1111-2222-3333-bbbbbbbbbbbb";
const OTHER_GUID: &str = "cccccccc-1111-2222-3333-dddddddddddd";
const VISUAL: &str = "11111111-2222-3333-4444-555555555555_Post2015";
const REVISION: &str = "11111111-2222-3333-4444-555555555555_Post2015_Post2015";

fn assignment(guid: &str, token: &str) -> FaceAppearanceAssignment {
    FaceAppearanceAssignment {
        face_guid: guid.into(),
        visual_guid: token.to_owned().try_into().expect("visual token"),
        color: Some(Color::new(0.25, 0.5, 0.75, 1.0).expect("normalized color")),
    }
}

fn model() -> cadmpeg_ir::CadIr {
    let mut ir = cadmpeg_ir::examples::unit_cube().expect("synthetic cube");
    for (index, guid) in [FACE_GUID, OTHER_GUID].into_iter().enumerate() {
        ir.model.attributes.push(SourceAttribute {
            id: format!("test:model:attribute#{index}")
                .try_into()
                .expect("identity"),
            target: AttributeTarget::Face(ir.model.faces[index].id.clone()),
            name: "ATTRIB_CUSTOM-attrib".into(),
            values: vec![
                AttributeValue::String("NEUTRON_Material_attrib_def".into()),
                AttributeValue::String(guid.into()),
            ],
        });
    }
    for (index, token) in [VISUAL, REVISION].into_iter().enumerate() {
        ir.model.appearances.push(Appearance {
            id: format!("test:model:appearance#{index}")
                .try_into()
                .expect("identity"),
            name: None,
            asset_guid: None,
            library_id: None,
            visual_guid: Some(token.into()),
            physical_token: None,
            schema: None,
            category: None,
            base_color: None,
            properties: Default::default(),
            textures: Vec::new(),
        });
    }
    ir
}

#[test]
fn distinct_face_material_revisions_report_loss_without_selecting_by_order() {
    for tokens in [[VISUAL, REVISION, VISUAL], [REVISION, VISUAL, REVISION]] {
        let mut ir = model();
        let explicit = Color::new(0.75, 0.5, 0.25, 1.0).expect("normalized color");
        if tokens[0] == REVISION {
            ir.model.faces[0].color = Some(explicit);
        }
        let expected_color = ir.model.faces[0]
            .color
            .or(assignment(FACE_GUID, VISUAL).color);
        let mut assignments = tokens.map(|token| assignment(FACE_GUID, token)).to_vec();
        assignments.push(assignment(OTHER_GUID, VISUAL));
        let losses = super::super::resolve_face_appearance_bindings(&mut ir, &assignments)
            .expect("distinct visual tokens are admitted");
        assert_eq!(losses.len(), 1);
        assert_eq!(
            losses[0].code.local_code(),
            "material.face-appearance-ambiguous"
        );
        assert_eq!(
            losses[0].code,
            crate::loss::F3dLossCode::FaceAppearanceAmbiguous.kind()
        );
        assert_eq!(ir.model.appearances.len(), 2);
        assert_eq!(ir.model.appearance_bindings.len(), 1);
        assert_eq!(
            ir.model.appearance_bindings[0].target,
            cadmpeg_ir::appearance::AppearanceTarget::Face(ir.model.faces[1].id.clone())
        );
        assert_eq!(ir.model.faces[0].color, expected_color);
    }
}

#[test]
fn repeated_complete_face_material_token_binds_once_without_loss() {
    let mut ir = model();
    let assignment = assignment(FACE_GUID, VISUAL);
    let losses =
        super::super::resolve_face_appearance_bindings(&mut ir, &[assignment.clone(), assignment])
            .expect("equal complete tokens identify one candidate");
    assert!(losses.is_empty());
    assert_eq!(ir.model.appearance_bindings.len(), 1);
    assert_eq!(
        ir.model.appearance_bindings[0].appearance,
        ir.model.appearances[0].id
    );
}
