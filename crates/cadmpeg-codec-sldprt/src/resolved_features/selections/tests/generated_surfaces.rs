//! Generated surface identity path admission.

use super::super::generated_surface_identities;
use crate::records::{FeatureInputClass, FeatureInputLane};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

#[test]
fn generated_surface_identity_path_keeps_absent_source_component() {
    let class_name = "moWzdHoleSurfIdRep_c";
    let prefix = [0xc3, 0x80, 0xc5, 0x00];
    let mut payload = [0xff, 0xff, 0x01, 0x00].to_vec();
    payload.extend(u16::try_from(class_name.len()).unwrap().to_le_bytes());
    payload.extend(class_name.as_bytes());
    payload.extend([0, 0]);
    let offset = payload.len();
    payload.extend(prefix);
    payload.extend(89u32.to_le_bytes());
    payload.extend(1u32.to_le_bytes());
    payload.extend(0x85b5u16.to_le_bytes());
    payload.extend([0, 0]);
    payload.extend(prefix);
    payload.extend(u32::MAX.to_le_bytes());
    payload.extend(2u32.to_le_bytes());
    payload.extend(3u32.to_le_bytes());
    let lane = FeatureInputLane {
        id: "lane".into(),
        configuration: None,
        native_payload: payload,
        classes: vec![FeatureInputClass {
            id: "class".into(), parent: "lane".into(), ordinal: 0,
            offset: 0, name: class_name.into(),
        }],
        names: Vec::new(), scalars: Vec::new(),
        relation_bindings: Vec::new(), relation_instances: Vec::new(),
        body_selections: Vec::new(), edge_selections: Vec::new(),
        surface_selections: Vec::new(), generated_surface_identities: Vec::new(),
        references: Vec::new(), sketch_entities: Vec::new(),
    };
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    let identities = generated_surface_identities(&ctx, &lane).unwrap();
    assert_eq!(identities.len(), 1);
    let identity = &identities[0];
    assert_eq!(identity.offset, u64::try_from(offset).unwrap());
    assert_eq!(identity.feature_source_id.value(), 89);
    assert_eq!(identity.local_identity, 0x85b5);
    assert_eq!(identity.components.len(), 2);
    assert_eq!(identity.components[0].instance, None);
    assert_eq!(identity.components[0].local_id, None);
    assert_eq!(identity.components[1].instance, Some(0x85b5));
    assert_eq!(&identity.components[1].type_signature[4..8], &u32::MAX.to_le_bytes());
    assert_eq!(identity.components[1].local_id, Some(3));
}
