//! Tests for the `holes` module.

use std::collections::BTreeMap;

use cadmpeg_ir::features::{FeatureDefinition, FeatureId, HoleKind};
use cadmpeg_ir::geometry::{Surface, SurfaceGeometry};
use cadmpeg_ir::ids::SurfaceId;
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::sketches::{
    SketchEntity, SketchEntityId, SketchGeometry, SketchGeometryDefinition, SketchId,
};

use crate::records::{
    FeatureHistory, FeatureInputGeneratedSurfaceIdentity, FeatureInputLane, FeatureInputName,
};

fn profile_reference_plane_payload(with_component_frame: bool) -> Vec<u8> {
    let mut payload = b"moCompRefPlane_c".to_vec();
    payload.extend([0; 11]);
    payload.extend(2u32.to_le_bytes());
    payload.extend(19u32.to_le_bytes());
    payload.extend([0, 0, 3, 0]);
    payload.extend([0; 27]);
    payload.extend(1.0f64.to_le_bytes());
    payload.extend([
        0x00, 0x00, 0x00, 0x04, 0x00, 0x00, 0x00, 0xf9, 0xff, 0xff, 0xff, 0x00, 0x00, 0x00, 0x00,
        0x65,
    ]);
    payload.extend([0; 4]);
    if with_component_frame {
        let mut component = [0u8; 138];
        component[..4].copy_from_slice(&2u32.to_le_bytes());
        component[14] = 1;
        for (index, value) in [
            0.0f64, 0.0, 1.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
        ]
        .into_iter()
        .enumerate()
        {
            let offset = 15 + index * 8;
            component[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
        }
        component[122..126].copy_from_slice(&4u32.to_le_bytes());
        component[126..130].copy_from_slice(&[0xff; 4]);
        payload.extend(component);
    }
    payload
}

fn model_hole() -> cadmpeg_ir::features::Feature {
    cadmpeg_ir::features::Feature {
        id: FeatureId::mint("synthetic:test:id#hole").expect("identity grammar"),
        ordinal: 0,
        name: Some("Hole".into()),
        suppressed: Some(false),
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        source_properties: BTreeMap::default(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),

        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Hole {
                profile: None,
                profile_filter: None,
                face: None,
                direction: None,
                placements: None,
                shape: cadmpeg_ir::features::HoleShape::new(
                    cadmpeg_ir::features::HoleConstruction::form(HoleKind::Simple),
                    None,
                    Some(cadmpeg_ir::features::PositiveLength::new(4.0).unwrap()),
                )
                .unwrap(),

                extent: None,
                bottom: None,
                taper_angle: None,
                allow_multi_profile_faces: None,
            },
        ),
        native_ref: Some("native-hole".into()),
    }
}

fn native_history() -> FeatureHistory {
    FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::default(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![crate::records::Feature {
            id: "native-hole".into(),
            parent: "history".into(),
            xml_tag: "HoleWizard".into(),
            tree_parent: None,
            source_id: Some("7".into()),
            ordinal: 0,
            name: "Hole".into(),
            kind: "HoleWizard".into(),
            input_class: Some("moHoleWzd_c".into()),
            suppressed: false,
            parameters: BTreeMap::default(),
            dimension_properties: BTreeMap::default(),
            properties: BTreeMap::default(),
            text: None,
            content: Vec::new(),
        }],
    }
}

fn lane() -> FeatureInputLane {
    let identity = |ordinal| FeatureInputGeneratedSurfaceIdentity {
        id: format!("identity-{ordinal}"),
        parent: "lane".into(),
        ordinal,
        offset: u64::from(ordinal),
        type_prefix: [0xc3, 0x80, 0xc5, 0],
        feature_source_id: 7,
        local_identity: 2,
        components: Vec::new(),
    };
    FeatureInputLane {
        id: "lane".into(),
        configuration: None,
        native_payload: Vec::new(),
        classes: Vec::new(),
        names: Vec::new(),
        scalars: Vec::new(),
        relation_bindings: Vec::new(),
        relation_instances: Vec::new(),
        body_selections: Vec::new(),
        edge_selections: Vec::new(),
        surface_selections: Vec::new(),
        generated_surface_identities: vec![identity(0), identity(1)],
        references: Vec::new(),
        sketch_entities: Vec::new(),
    }
}

fn lane_with_position_reference(position_source: u32) -> FeatureInputLane {
    let mut lane = lane();
    lane.native_payload.resize(200, 0);
    lane.names.push(FeatureInputName {
        id: "hole-name".into(),
        parent: "lane".into(),
        ordinal: 0,
        offset: 0,
        value: "Hole".into(),
        object_id: Some(7),
    });
    let trailer = 6 + "Hole".encode_utf16().count() * 2;
    lane.native_payload[trailer..trailer + 8].copy_from_slice(&[0, 0, 0, 0, 0, 0, 0, 0x40]);
    lane.native_payload[trailer + 8..trailer + 12].copy_from_slice(&7u32.to_le_bytes());
    lane.native_payload[trailer + 48..trailer + 50].copy_from_slice(&[0, 0xc0]);
    lane.native_payload[trailer + 50..trailer + 54].copy_from_slice(&position_source.to_le_bytes());
    lane
}

fn cylinder(id: usize, x: f64) -> Surface {
    Surface {
        id: SurfaceId::mint(format!("test:model:entity#surface-{id}")).expect("identity grammar"),
        geometry: SurfaceGeometry::Cylinder(
            cadmpeg_ir::geometry::CylinderSurface::try_new(
                Point3::new(x, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
                2.0,
            )
            .unwrap(),
        ),
        source_object: None,
    }
}

fn profile_line(sketch: &SketchId, ordinal: usize, start: Point2, end: Point2) -> SketchEntity {
    SketchEntity::new(
        SketchEntityId::mint(format!("synthetic:test:id#profile-line-{ordinal}")).unwrap(),
        sketch.clone(),
        SketchGeometry::try_from(SketchGeometryDefinition::Line { start, end }).unwrap(),
    )
}

mod axial_profile;
mod hole_axis;
mod position;
mod writer;
