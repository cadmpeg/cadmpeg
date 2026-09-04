//! Transfer of byte-proven CATIA display colors to neutral appearance bindings.

use std::collections::{BTreeMap, HashSet};

use cadmpeg_ir::appearance::{Appearance, AppearanceBinding, AppearanceTarget};
use cadmpeg_ir::ids::AppearanceId;
use cadmpeg_ir::topology::Color;
use cadmpeg_ir::CadIr;

use crate::families::standard::fbb::standard_face_colors;
use crate::native::CatiaNative;
use crate::value_block::ValueField;

#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct TransferResult {
    pub(crate) decoded_packets: usize,
    pub(crate) transferred_packets: usize,
    pub(crate) unresolved_packets: usize,
    pub(crate) emitted_assets: usize,
    pub(crate) emitted_bindings: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Packet {
    AllFaces([u8; 4]),
    Body([u8; 4]),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SourcedPacket {
    packet: Packet,
    source_id: String,
}

impl SourcedPacket {
    fn rgba(&self) -> [u8; 4] {
        match self.packet {
            Packet::AllFaces(rgba) | Packet::Body(rgba) => rgba,
        }
    }

    fn is_all_faces(&self) -> bool {
        matches!(self.packet, Packet::AllFaces(_))
    }

    fn is_body(&self) -> bool {
        matches!(self.packet, Packet::Body(_))
    }
}

/// Transfers presentation packets belonging to the selected modeling graph.
///
/// `01 R G B` assigns an opaque color to the complete face population.
/// `03 R G B A` assigns a body color when singular. A population of `03`
/// packets is positional; its authoritative face colors are the ABGR payloads
/// in the standard FBB face rows.
pub(crate) fn transfer(
    ir: &mut CadIr,
    native: &CatiaNative,
    graph_scope: Option<&HashSet<String>>,
    standard_fbb: Option<&[u8]>,
) -> TransferResult {
    let initial_assets = ir.model.appearances.len();
    let initial_bindings = ir.model.appearance_bindings.len();
    let packets = native
        .value_blocks
        .iter()
        .filter(|block| {
            graph_scope.is_none_or(|scope| {
                block
                    .object_graph
                    .as_ref()
                    .is_some_and(|graph| scope.contains(graph))
            })
        })
        .flat_map(|block| {
            block
                .fields
                .iter()
                .enumerate()
                .filter_map(|(ordinal, field)| {
                    let packet = packet(field)?;
                    let ValueField::Inline { offset, .. } = field else {
                        return None;
                    };
                    Some(SourcedPacket {
                        packet,
                        source_id: format!("{}:field#{offset:010}:{ordinal:06}", block.id).into(),
                    })
                })
        })
        .collect::<Vec<_>>();
    let mut result = TransferResult {
        decoded_packets: packets.len(),
        ..TransferResult::default()
    };

    let all_faces = packets
        .iter()
        .filter_map(|packet| match packet.packet {
            Packet::AllFaces(rgba) => Some(rgba),
            Packet::Body(_) => None,
        })
        .collect::<Vec<_>>();

    let body = packets
        .iter()
        .filter_map(|packet| match packet.packet {
            Packet::Body(rgba) => Some(rgba),
            Packet::AllFaces(_) => None,
        })
        .collect::<Vec<_>>();

    // Assets are meaningful independently of topology ownership. Retain every
    // decoded color even when its target incidence remains unresolved.
    for packet in &packets {
        insert_appearance(ir, packet.rgba());
    }

    let positional_colors = standard_fbb
        .and_then(standard_face_colors)
        .filter(|colors| colors.len() == ir.model.faces.len())
        .filter(|colors| match all_faces.as_slice() {
            [] => body.len() > 1 && colors.as_slice() == body.as_slice(),
            [base] => {
                let overrides = colors
                    .iter()
                    .copied()
                    .filter(|rgba| rgba != base)
                    .collect::<Vec<_>>();
                same_color_multiset(&overrides, &body)
            }
            _ => false,
        });
    if let Some(colors) = positional_colors {
        let faces = ir
            .model
            .faces
            .iter()
            .map(|face| face.id.clone())
            .collect::<Vec<_>>();
        for (index, (face, rgba)) in faces.into_iter().zip(colors).enumerate() {
            let appearance = insert_appearance(ir, rgba);
            insert_binding(ir, &appearance, AppearanceTarget::Face(face), index);
        }
        result.transferred_packets += body.len() + all_faces.len();
    } else {
        if all_faces.len() == 1 && !ir.model.faces.is_empty() {
            let appearance = insert_appearance(ir, all_faces[0]);
            let faces = ir
                .model
                .faces
                .iter()
                .map(|face| face.id.clone())
                .collect::<Vec<_>>();
            for (index, face) in faces.into_iter().enumerate() {
                insert_binding(ir, &appearance, AppearanceTarget::Face(face), index);
            }
            result.transferred_packets += 1;
        } else {
            for packet in packets.iter().filter(|packet| packet.is_all_faces()) {
                insert_source_binding(ir, packet);
            }
            result.unresolved_packets += all_faces.len();
        }

        match body.as_slice() {
            [rgba] if all_faces.is_empty() && ir.model.bodies.len() == 1 => {
                let appearance = insert_appearance(ir, *rgba);
                let target = AppearanceTarget::Body(ir.model.bodies[0].id.clone());
                insert_binding(ir, &appearance, target, 0);
                result.transferred_packets += 1;
            }
            values => {
                for packet in packets.iter().filter(|packet| packet.is_body()) {
                    insert_source_binding(ir, packet);
                }
                result.unresolved_packets += values.len();
            }
        }
    }
    result.emitted_assets = ir.model.appearances.len() - initial_assets;
    result.emitted_bindings = ir.model.appearance_bindings.len() - initial_bindings;
    debug_assert_eq!(
        result.decoded_packets,
        result.transferred_packets + result.unresolved_packets
    );
    result
}

fn same_color_multiset(left: &[[u8; 4]], right: &[[u8; 4]]) -> bool {
    fn counts(values: &[[u8; 4]]) -> BTreeMap<[u8; 4], usize> {
        let mut counts = BTreeMap::new();
        for value in values {
            *counts.entry(*value).or_default() += 1;
        }
        counts
    }
    counts(left) == counts(right)
}

fn packet(field: &ValueField) -> Option<Packet> {
    let ValueField::Inline { code, bytes, .. } = field else {
        return None;
    };
    match (*code, bytes.as_slice()) {
        (0xeb, [0x01, r, g, b]) => Some(Packet::AllFaces([*r, *g, *b, 0xff])),
        (0xec, [0x03, r, g, b, a]) => Some(Packet::Body([*r, *g, *b, *a])),
        _ => None,
    }
}

fn insert_appearance(ir: &mut CadIr, rgba: [u8; 4]) -> AppearanceId {
    let id = AppearanceId::mint(format!(
        "catia:appearance:rgba#{:02x}{:02x}{:02x}{:02x}",
        rgba[0], rgba[1], rgba[2], rgba[3]
    ))
    .expect("identity grammar");
    if !ir
        .model
        .appearances
        .iter()
        .any(|appearance| appearance.id == id)
    {
        ir.model.appearances.push(Appearance {
            id: id.clone(),
            name: None,
            library_id: None,
            asset_guid: None,
            visual_guid: None,
            physical_token: None,
            schema: Some("CATIA V5 display color".into()),
            category: None,
            base_color: Some(Color {
                r: f32::from(rgba[0]) / 255.0,
                g: f32::from(rgba[1]) / 255.0,
                b: f32::from(rgba[2]) / 255.0,
                a: f32::from(rgba[3]) / 255.0,
            }),
            properties: BTreeMap::new(),
            textures: Vec::new(),
        });
    }
    id
}

fn insert_binding(
    ir: &mut CadIr,
    appearance: &AppearanceId,
    target: AppearanceTarget,
    index: usize,
) {
    let appearance_key = appearance
        .as_str()
        .rsplit_once('#')
        .map_or(appearance.as_str(), |(_, key)| key);
    insert_binding_record(
        ir,
        appearance,
        target,
        format!("catia:appearance:binding#{index}:{appearance_key}"),
    );
}

fn insert_source_binding(ir: &mut CadIr, packet: &SourcedPacket) {
    let appearance = insert_appearance(ir, packet.rgba());
    let appearance_key = appearance
        .as_str()
        .rsplit_once('#')
        .map_or(appearance.as_str(), |(_, key)| key);
    let source_key = identity_key_fragment(&packet.source_id);
    insert_binding_record(
        ir,
        &appearance,
        AppearanceTarget::Source {
            source_id: packet.source_id.clone(),
        },
        format!("catia:appearance:source-binding#source-{source_key}:{appearance_key}"),
    );
}

/// Encode a source token before embedding it in an entity identity key.
///
/// Source tokens are retained verbatim in [`AppearanceTarget::Source`], but
/// their delimiters are not part of the entity-key grammar. Hex encoding is
/// injective and keeps the binding identity to one reserved `#` separator.
fn identity_key_fragment(value: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut fragment = String::with_capacity(value.len() * 2);
    for byte in value.bytes() {
        fragment.push(HEX[usize::from(byte >> 4)] as char);
        fragment.push(HEX[usize::from(byte & 0x0f)] as char);
    }
    fragment
}

fn insert_binding_record(
    ir: &mut CadIr,
    appearance: &AppearanceId,
    target: AppearanceTarget,
    id: String,
) {
    ir.model.appearance_bindings.push(AppearanceBinding {
        id: id.into(),
        target,
        appearance: appearance.clone(),
        source_entity_id: None,
        object_type: Some("CATIA V5 display property".into()),
        visible: None,
        channels: BTreeMap::new(),
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native::CatiaValueBlock;
    use cadmpeg_ir::ids::{BodyId, FaceId, ShellId, SurfaceId};
    use cadmpeg_ir::topology::{Body, BodyKind, Face, Sense};

    fn model(face_count: usize) -> CadIr {
        let mut ir = CadIr::empty();
        ir.model.bodies.push(Body {
            id: BodyId::mint("body").expect("identity grammar"),
            kind: BodyKind::Solid,
            regions: vec![],
            transform: None,
            name: None,
            color: None,
            visible: None,
        });
        for index in 0..face_count {
            ir.model.faces.push(Face {
                id: FaceId::mint(format!("face-{index}")).expect("identity grammar"),
                shell: ShellId::mint("shell").expect("identity grammar"),
                surface: SurfaceId::mint(format!("surface-{index}")).expect("identity grammar"),
                sense: Sense::Forward,
                loops: vec![].into(),
                name: None,
                color: None,
                tolerance: None,
            });
        }
        ir
    }

    fn native(fields: Vec<ValueField>) -> CatiaNative {
        let mut native = CatiaNative::default();
        native.value_blocks.push(CatiaValueBlock {
            id: "values".into(),
            byte_offset: 0,
            byte_len: 0,
            declared_len: 0,
            object_graph: None,
            catalog: "catalog".into(),
            payload: vec![],
            fields,
            schema_selections: vec![],
        });
        native
    }

    fn inline(code: u8, bytes: &[u8]) -> ValueField {
        ValueField::Inline {
            code,
            bytes: bytes.to_vec(),
            offset: 0,
        }
    }

    fn six_face_brep(rgba: [u8; 4]) -> Vec<u8> {
        (0..6)
            .flat_map(|_| [0xb0, 4, 4, 0xff, rgba[3], rgba[2], rgba[1], rgba[0]])
            .collect()
    }

    #[test]
    fn accepts_only_exact_display_packets() {
        let inline = |code, bytes| ValueField::Inline {
            code,
            bytes,
            offset: 0,
        };
        assert_eq!(
            packet(&inline(0xeb, vec![1, 0xd1, 0x1a, 0x1f])),
            Some(Packet::AllFaces([0xd1, 0x1a, 0x1f, 0xff]))
        );
        assert_eq!(
            packet(&inline(0xec, vec![3, 0xd1, 0x1a, 0x1f, 0x99])),
            Some(Packet::Body([0xd1, 0x1a, 0x1f, 0x99]))
        );
        assert_eq!(packet(&inline(0xec, vec![3, 1, 2, 3])), None);
        assert_eq!(packet(&inline(0xeb, vec![1, 1, 2, 3, 4])), None);
        assert_eq!(packet(&inline(0xec, vec![1, 0xd1, 0x1a, 0x1f])), None);
    }

    #[test]
    fn transfers_unstyled_body_and_all_faces_without_inventing_targets() {
        let mut ir = model(6);
        assert_eq!(
            transfer(&mut ir, &CatiaNative::default(), None, None),
            TransferResult::default()
        );
        assert!(ir.model.appearances.is_empty());

        let mut ir = model(6);
        let result = transfer(
            &mut ir,
            &native(vec![inline(0xec, &[3, 0xd1, 0x1a, 0x1f, 0xff])]),
            None,
            None,
        );
        assert_eq!(
            (
                result.decoded_packets,
                result.transferred_packets,
                result.unresolved_packets,
                result.emitted_assets,
                result.emitted_bindings
            ),
            (1, 1, 0, 1, 1)
        );
        assert!(matches!(
            ir.model.appearance_bindings[0].target,
            AppearanceTarget::Body(_)
        ));
        assert_eq!(
            ir.model.appearance_bindings[0].id.as_str(),
            "catia:appearance:binding#0:d11a1fff"
        );

        let mut ir = model(6);
        let result = transfer(
            &mut ir,
            &native(vec![inline(0xeb, &[1, 0xd1, 0x1a, 0x1f])]),
            None,
            None,
        );
        assert_eq!(
            (
                result.decoded_packets,
                result.transferred_packets,
                result.unresolved_packets,
                result.emitted_assets,
                result.emitted_bindings
            ),
            (1, 1, 0, 1, 6)
        );
        assert!(ir
            .model
            .appearance_bindings
            .iter()
            .all(|binding| matches!(binding.target, AppearanceTarget::Face(_))));
    }

    #[test]
    fn retains_unbound_override_asset_and_reports_its_packet() {
        let mut ir = model(6);
        let result = transfer(
            &mut ir,
            &native(vec![
                inline(0xeb, &[1, 0xd1, 0x1a, 0x1f]),
                inline(0xec, &[3, 0x14, 0x3d, 0xe0, 0xff]),
            ]),
            None,
            None,
        );
        assert_eq!(
            (
                result.decoded_packets,
                result.transferred_packets,
                result.unresolved_packets,
                result.emitted_assets,
                result.emitted_bindings
            ),
            (2, 1, 1, 2, 7)
        );
        assert!(ir
            .model
            .appearances
            .iter()
            .any(|asset| asset.id.as_str().contains("143de0ff")));
        assert_eq!(
            ir.model
                .appearance_bindings
                .iter()
                .filter(|binding| matches!(binding.target, AppearanceTarget::Source { .. }))
                .count(),
            1
        );
        assert!(ir.model.appearance_bindings.iter().any(|binding| {
            binding.appearance.as_str().contains("143de0ff")
                && matches!(
                    &binding.target,
                    AppearanceTarget::Source { source_id }
                        if source_id == "values:field#0000000000:000001"
                )
        }));
        assert!(ir
            .model
            .appearance_bindings
            .iter()
            .filter(|binding| matches!(binding.target, AppearanceTarget::Face(_)))
            .all(|binding| binding.appearance.as_str().contains("d11a1fff")));
    }

    #[test]
    fn source_binding_identity_encodes_nested_source_delimiters() {
        let packet = SourcedPacket {
            packet: Packet::Body([0x14, 0x3d, 0xe0, 0xff]),
            source_id: "catia:outer:value-block#0001:field#0002".into(),
        };
        let mut ir = model(0);
        insert_source_binding(&mut ir, &packet);
        let binding = ir
            .model
            .appearance_bindings
            .first()
            .expect("source binding");
        assert_eq!(binding.id.as_str().matches('#').count(), 1);
        assert!(binding
            .id
            .as_str()
            .starts_with("catia:appearance:source-binding#source-"));
        assert!(matches!(
            &binding.target,
            AppearanceTarget::Source { source_id } if source_id == &packet.source_id
        ));
    }

    #[test]
    fn positional_transparency_requires_matching_fbb_values() {
        let rgba = [0xd1, 0x1a, 0x1f, 0x99];
        let fields = (0..6)
            .map(|_| inline(0xec, &[3, rgba[0], rgba[1], rgba[2], rgba[3]]))
            .collect::<Vec<_>>();
        let mut ir = model(6);
        let result = transfer(
            &mut ir,
            &native(fields.clone()),
            None,
            Some(&six_face_brep(rgba)),
        );
        assert_eq!(
            (
                result.decoded_packets,
                result.transferred_packets,
                result.unresolved_packets,
                result.emitted_assets,
                result.emitted_bindings
            ),
            (6, 6, 0, 1, 6)
        );
        let ids = ir
            .model
            .appearance_bindings
            .iter()
            .map(|binding| &binding.id)
            .collect::<HashSet<_>>();
        assert_eq!(ids.len(), 6);
        assert!(ir.model.appearances[0]
            .base_color
            .is_some_and(|color| color.a == 0.6));

        let mut ir = model(6);
        let result = transfer(
            &mut ir,
            &native(fields),
            None,
            Some(&six_face_brep([0x14, 0x3d, 0xe0, 0xff])),
        );
        assert_eq!(
            (
                result.decoded_packets,
                result.transferred_packets,
                result.unresolved_packets,
                result.emitted_bindings
            ),
            (6, 0, 6, 6)
        );
        assert!(ir
            .model
            .appearance_bindings
            .iter()
            .all(|binding| matches!(binding.target, AppearanceTarget::Source { .. })));
    }

    #[test]
    fn positional_colors_require_standard_face_population_provenance() {
        let rgba = [0xd1, 0x1a, 0x1f, 0x99];
        let fields = (0..6)
            .map(|_| inline(0xec, &[3, rgba[0], rgba[1], rgba[2], rgba[3]]))
            .collect::<Vec<_>>();
        let mut ir = model(6);
        let result = transfer(&mut ir, &native(fields), None, None);
        assert_eq!(
            (
                result.decoded_packets,
                result.transferred_packets,
                result.unresolved_packets,
                result.emitted_bindings,
            ),
            (6, 0, 6, 6)
        );
        assert!(ir
            .model
            .appearance_bindings
            .iter()
            .all(|binding| matches!(binding.target, AppearanceTarget::Source { .. })));
    }

    #[test]
    fn positional_population_supersedes_but_still_accounts_for_all_faces_packet() {
        let rgba = [0xd1, 0x1a, 0x1f, 0x99];
        let mut fields = vec![inline(0xeb, &[1, 0xd1, 0x1a, 0x1f])];
        fields.extend((0..6).map(|_| inline(0xec, &[3, rgba[0], rgba[1], rgba[2], rgba[3]])));
        let mut ir = model(6);
        let result = transfer(
            &mut ir,
            &native(fields.clone()),
            None,
            Some(&six_face_brep(rgba)),
        );
        assert_eq!(
            (
                result.decoded_packets,
                result.transferred_packets,
                result.unresolved_packets,
                result.emitted_assets,
                result.emitted_bindings
            ),
            (7, 7, 0, 2, 6)
        );
        assert!(ir
            .model
            .appearance_bindings
            .iter()
            .all(|binding| binding.appearance.as_str().contains("d11a1f99")));

        let mut ir = model(6);
        let result = transfer(
            &mut ir,
            &native(fields),
            None,
            Some(&six_face_brep([0x14, 0x3d, 0xe0, 0xff])),
        );
        assert_eq!(
            (
                result.decoded_packets,
                result.transferred_packets,
                result.unresolved_packets,
                result.emitted_bindings
            ),
            (7, 1, 6, 12)
        );
        assert_eq!(
            ir.model
                .appearance_bindings
                .iter()
                .filter(|binding| matches!(binding.target, AppearanceTarget::Source { .. }))
                .count(),
            6
        );
        assert!(ir
            .model
            .appearance_bindings
            .iter()
            .filter(|binding| matches!(binding.target, AppearanceTarget::Face(_)))
            .all(|binding| binding.appearance.as_str().contains("d11a1fff")));
    }

    #[test]
    fn base_plus_override_population_uses_effective_fbb_face_colors() {
        let gray = [0x8c, 0x8c, 0x8c, 0xff];
        let blue = [0x0d, 0x26, 0xe6, 0xff];
        for target in 0..6 {
            let mut colors = [gray; 6];
            colors[target] = blue;
            let brep = colors
                .into_iter()
                .flat_map(|rgba| [0xb0, 4, 4, 0xff, rgba[3], rgba[2], rgba[1], rgba[0]])
                .collect::<Vec<_>>();
            let mut ir = model(6);
            let result = transfer(
                &mut ir,
                &native(vec![
                    inline(0xeb, &[1, gray[0], gray[1], gray[2]]),
                    inline(0xec, &[3, blue[0], blue[1], blue[2], blue[3]]),
                ]),
                None,
                Some(&brep),
            );
            assert_eq!(
                (
                    result.decoded_packets,
                    result.transferred_packets,
                    result.unresolved_packets,
                    result.emitted_assets,
                    result.emitted_bindings
                ),
                (2, 2, 0, 2, 6)
            );
            for (index, binding) in ir.model.appearance_bindings.iter().enumerate() {
                let key = if index == target {
                    "0d26e6ff"
                } else {
                    "8c8c8cff"
                };
                assert!(binding.appearance.as_str().contains(key));
            }
        }
    }

    #[test]
    fn base_plus_distinct_overrides_requires_exact_fbb_multiset() {
        let colors = [
            [0xe6, 0x0d, 0x0d, 0xff],
            [0x0d, 0xcc, 0x1a, 0xff],
            [0x0d, 0x26, 0xe6, 0xff],
            [0xf2, 0xcc, 0x0d, 0xff],
            [0xd9, 0x0d, 0xbf, 0xff],
            [0x0d, 0xcc, 0xd9, 0xff],
        ];
        let fields = std::iter::once(inline(0xeb, &[1, colors[0][0], colors[0][1], colors[0][2]]))
            .chain(
                colors[1..]
                    .iter()
                    .map(|rgba| inline(0xec, &[3, rgba[0], rgba[1], rgba[2], rgba[3]])),
            )
            .collect::<Vec<_>>();
        let brep = colors
            .into_iter()
            .flat_map(|rgba| [0xb0, 4, 4, 0xff, rgba[3], rgba[2], rgba[1], rgba[0]])
            .collect::<Vec<_>>();
        let mut ir = model(6);
        let result = transfer(&mut ir, &native(fields.clone()), None, Some(&brep));
        assert_eq!(
            (
                result.decoded_packets,
                result.transferred_packets,
                result.unresolved_packets,
                result.emitted_bindings
            ),
            (6, 6, 0, 6)
        );
        for (binding, rgba) in ir.model.appearance_bindings.iter().zip(colors) {
            assert!(binding.appearance.as_str().contains(&format!(
                "{:02x}{:02x}{:02x}{:02x}",
                rgba[0], rgba[1], rgba[2], rgba[3]
            )));
        }

        let mut mismatched = brep;
        mismatched[7] = 0xff;
        let mut ir = model(6);
        let result = transfer(&mut ir, &native(fields), None, Some(&mismatched));
        assert_eq!(
            (
                result.transferred_packets,
                result.unresolved_packets,
                result.emitted_bindings
            ),
            (1, 5, 11)
        );
        assert!(ir
            .model
            .appearance_bindings
            .iter()
            .filter(|binding| matches!(binding.target, AppearanceTarget::Face(_)))
            .all(|binding| binding.appearance.as_str().contains("e60d0dff")));
        assert_eq!(
            ir.model
                .appearance_bindings
                .iter()
                .filter(|binding| matches!(binding.target, AppearanceTarget::Source { .. }))
                .count(),
            5
        );
    }
}
