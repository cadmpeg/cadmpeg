// SPDX-License-Identifier: Apache-2.0
//! Actual XML carrier names select independent binary framing, never codec output.

use super::*;
use cadmpeg_test_support::framing::{FrameParent, PartTable};

#[derive(Clone)]
pub(super) struct Carrier {
    pub(super) member: String,
    pub(super) kind: BinaryKind,
}

#[derive(Debug)]
pub(super) struct Boundary {
    pub(super) carrier: usize,
    pub(super) level: String,
    pub(super) offset: u64,
    pub(super) length: u64,
    pub(super) framing: Vec<std::ops::Range<u64>>,
}

fn kind(runtime: &str, value: roxmltree::Node<'_, '_>) -> Option<BinaryKind> {
    let (tag, kind) = match runtime {
        "App::PropertyColorList" => ("ColorList", BinaryKind::Color),
        "App::PropertyFloatList" => ("FloatList", BinaryKind::Float),
        "App::PropertyVectorList" => ("VectorList", BinaryKind::Vector),
        "App::PropertyPlacementList" => ("PlacementList", BinaryKind::Placement),
        "Part::PropertyFilletEdges" => ("FilletEdges", BinaryKind::Fillet),
        "App::PropertyMaterialList" => (
            "MaterialList",
            BinaryKind::Material {
                version: value
                    .attribute("version")
                    .map(|text| text.parse().unwrap())
                    .unwrap_or(0),
            },
        ),
        "Points::PropertyGreyValueList" | "Inspection::PropertyDistanceList" => {
            ("FloatList", BinaryKind::F32 { components: 1 })
        }
        "Points::PropertyNormalList" | "Mesh::PropertyNormalList" => {
            ("VectorList", BinaryKind::F32 { components: 3 })
        }
        "Points::PropertyCurvatureList" | "Mesh::PropertyCurvatureList" => {
            ("CurvatureList", BinaryKind::F32 { components: 8 })
        }
        "Mesh::PropertyMaterial" => ("Material", BinaryKind::MeshMaterial),
        "Mesh::PropertyMeshKernel" => ("Mesh", BinaryKind::Mesh),
        "Points::PropertyPointKernel" => ("Points", BinaryKind::Points),
        _ => return None,
    };
    assert_eq!(
        value.tag_name().name(),
        tag,
        "actual registered binary carrier"
    );
    Some(kind)
}

pub(super) fn inventory(bytes: &[u8], decoded: &DecodeResult) -> (Vec<Carrier>, Vec<Boundary>) {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::service()).unwrap();
    let root = ctx.name_stream(root, "file").unwrap();
    let archive = cadmpeg_container::ArchiveSnapshot::new(&ctx, root).unwrap();
    let mut frames = Vec::new();
    let entries = zip::frame(&ctx, root, &mut frames).unwrap();
    let mut carriers = Vec::new();
    let mut controls = Vec::new();
    for name in ["Document.xml", "GuiDocument.xml"] {
        if !entries
            .iter()
            .any(|entry| entry.name().window() == name.as_bytes())
        {
            continue;
        }
        let xml = archive.open(&ctx, name).unwrap();
        let document =
            roxmltree::Document::parse(std::str::from_utf8(xml.window()).unwrap()).unwrap();
        for property in document.descendants().filter(|node| {
            node.is_element() && matches!(node.tag_name().name(), "Property" | "_Property")
        }) {
            let Some(runtime) = property.attribute("type") else {
                continue;
            };
            for value in property.children().filter(roxmltree::Node::is_element) {
                let Some(kind) = kind(runtime, value) else {
                    continue;
                };
                let Some(member) = value.attribute("file").filter(|name| !name.is_empty()) else {
                    continue;
                };
                assert!(
                    entries
                        .iter()
                        .any(|entry| entry.name().window() == member.as_bytes()),
                    "real carrier entry {member}"
                );
                carriers.push(Carrier {
                    member: member.into(),
                    kind,
                });
                let mut dispatch = Vec::new();
                for attribute in [
                    property.attribute_node("type"),
                    value.attribute_node("file"),
                    value.attribute_node("version"),
                ]
                .into_iter()
                .flatten()
                {
                    for range in [attribute.range_qname(), attribute.range_value()] {
                        if !range.is_empty() {
                            dispatch.push(
                                xml.child(xml.start() + range.start, xml.start() + range.end)
                                    .unwrap(),
                            );
                        }
                    }
                }
                let tag_start = value.range().start + 1;
                let tag_end = xml.window()[tag_start..]
                    .iter()
                    .position(|byte| matches!(byte, b' ' | b'\t' | b'\r' | b'\n' | b'/' | b'>'))
                    .unwrap()
                    + tag_start;
                dispatch.push(
                    xml.child(xml.start() + tag_start, xml.start() + tag_end)
                        .unwrap(),
                );
                controls.push(dispatch);
            }
        }
    }
    assert_eq!(
        carriers
            .iter()
            .map(|carrier| &carrier.member)
            .collect::<BTreeSet<_>>()
            .len(),
        carriers.len(),
        "public binary entries have one actual XML owner"
    );
    let mut boundaries = Vec::new();
    let mut matched = BTreeSet::new();
    let mut levels = BTreeSet::new();
    for (carrier_index, carrier) in carriers.iter().enumerate() {
        let entry = entries
            .iter()
            .find(|entry| entry.name().window() == carrier.member.as_bytes())
            .unwrap();
        let member = archive.open(&ctx, &carrier.member).unwrap();
        let start = frames.len();
        frame_binary_member(&ctx, member, carrier.kind, entry.frame_index(), &mut frames).unwrap();
        let entry_frame = frames
            .remove(start)
            .with_external_dispatch(&ctx, controls[carrier_index].clone())
            .unwrap();
        frames.insert(start, entry_frame);
        for frame in &frames[start..] {
            let (id, actual) = decoded
                .source_fidelity()
                .parts()
                .iter()
                .find(|(_, part)| {
                    part.level() == frame.level()
                        && part.stream() == frame.capture().stream()
                        && part.offset() == frame.capture().offset()
                        && part.byte_len() == frame.capture().byte_len()
                })
                .unwrap_or_else(|| panic!("missing independent binary frame {frame:?}"));
            assert!(
                matched.insert(id.clone()),
                "duplicate source frame {frame:?}"
            );
            levels.insert(frame.level().as_str().to_owned());
            assert_eq!(actual.framing_bytes(), frame.framing(), "{frame:?}");
            let FrameParent::Part(parent) = frame.parent() else {
                panic!("binary part has no physical parent");
            };
            let expected = &frames[parent];
            let parent = &decoded.source_fidelity().parts()[actual.parent().unwrap()];
            assert_eq!(
                (
                    parent.level(),
                    parent.stream(),
                    parent.offset(),
                    parent.byte_len()
                ),
                (
                    expected.level(),
                    expected.capture().stream(),
                    expected.capture().offset(),
                    expected.capture().byte_len()
                )
            );
            boundaries.push(Boundary {
                carrier: carrier_index,
                level: frame.level().as_str().into(),
                offset: frame.capture().offset(),
                length: frame.capture().byte_len(),
                framing: frame.framing().to_vec(),
            });
        }
    }
    PartTable::load().unwrap().validate(&ctx, &frames).unwrap();
    for (id, part) in decoded
        .source_fidelity()
        .parts()
        .iter()
        .filter(|(_, part)| levels.contains(part.level().as_str()))
    {
        assert!(matched.contains(id), "decoder-only binary part {part:?}");
    }
    (carriers, boundaries)
}

pub(super) fn member(bytes: &[u8], name: &str) -> Vec<u8> {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::service()).unwrap();
    cadmpeg_container::ArchiveSnapshot::new(&ctx, root)
        .unwrap()
        .open(&ctx, name)
        .unwrap()
        .window()
        .to_vec()
}

pub(super) fn mutate(bytes: &[u8], carrier: &Carrier, target: &Boundary, fill: u8) -> Vec<u8> {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::service()).unwrap();
    let mut raw = member(bytes, &carrier.member);
    for at in target.offset..target.offset + target.length {
        if !target.framing.iter().any(|range| range.contains(&at)) {
            raw[usize::try_from(at).unwrap()] = fill;
        }
    }
    zip::rebuild_member(&ctx, root, carrier.member.as_bytes(), &raw).unwrap()
}
