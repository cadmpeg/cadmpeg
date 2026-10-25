// SPDX-License-Identifier: Apache-2.0
//! Small runtime-selected side entries shared by both XML property carriers.

use cadmpeg_test_support::framing::freecad::binary::BinaryKind;

pub(super) struct Fixture {
    pub(super) runtime: &'static str,
    pub(super) tag: &'static str,
    pub(super) version: Option<u32>,
    pub(super) kind: BinaryKind,
    pub(super) bytes: Vec<u8>,
}

fn counted(rows: impl IntoIterator<Item = Vec<u8>>) -> Vec<u8> {
    let rows: Vec<_> = rows.into_iter().collect();
    let mut bytes = u32::try_from(rows.len()).unwrap().to_le_bytes().to_vec();
    for row in rows {
        bytes.extend(row);
    }
    bytes
}

fn reals(values: &[f64], single: bool) -> Vec<u8> {
    let mut bytes = Vec::new();
    for value in values {
        if single {
            let text = value.to_string();
            bytes.extend(text.parse::<f32>().unwrap().to_le_bytes());
        } else {
            bytes.extend(value.to_le_bytes());
        }
    }
    bytes
}

pub(super) fn all() -> Vec<Fixture> {
    let mut fixtures = vec![
        Fixture {
            runtime: "App::PropertyColorList",
            tag: "ColorList",
            version: None,
            kind: BinaryKind::Color,
            bytes: counted(
                [0x11223344_u32, 0x55667788, 0x99aabbcc].map(|v| v.to_le_bytes().to_vec()),
            ),
        },
        Fixture {
            runtime: "App::PropertyFloatList",
            tag: "FloatList",
            version: None,
            kind: BinaryKind::Float,
            bytes: counted([1.0_f64, 2.0, 3.0].map(|v| v.to_le_bytes().to_vec())),
        },
    ];
    for single in [false, true] {
        fixtures.push(Fixture {
            runtime: "App::PropertyVectorList",
            tag: "VectorList",
            version: None,
            kind: BinaryKind::Vector,
            bytes: counted((0..3).map(|_| reals(&[1.0, 2.0, 3.0], single))),
        });
        fixtures.push(Fixture {
            runtime: "App::PropertyPlacementList",
            tag: "PlacementList",
            version: None,
            kind: BinaryKind::Placement,
            bytes: counted((0..3).map(|_| reals(&[1.0, 2.0, 3.0, 0.0, 0.0, 0.0, 1.0], single))),
        });
    }
    fixtures.push(Fixture {
        runtime: "Part::PropertyFilletEdges",
        tag: "FilletEdges",
        version: None,
        kind: BinaryKind::Fillet,
        bytes: counted((1_i32..=3).map(|edge| {
            let mut b = edge.to_le_bytes().to_vec();
            b.extend(reals(&[2.0, 3.0], false));
            b
        })),
    });
    for (version, provisional) in [
        (0, false),
        (0, true),
        (1, false),
        (1, true),
        (2, false),
        (3, false),
    ] {
        let rows = (0..3).map(|_| {
            let mut row = Vec::new();
            for color in [0x11223344_u32, 0x55667788, 0x99aabbcc, 0xddeeff00] {
                row.extend(color.to_le_bytes());
            }
            row.extend(0.5_f32.to_le_bytes());
            row.extend(0.25_f32.to_le_bytes());
            row
        });
        let mut bytes = counted(rows);
        if version == 3 {
            for _ in 0..3 {
                for text in ["image", "path", "uuid"] {
                    bytes.extend(u32::try_from(text.len()).unwrap().to_le_bytes());
                    bytes.extend(text.as_bytes());
                }
            }
        }
        if provisional {
            let mut provisional = (-1_i32).to_le_bytes().to_vec();
            provisional.extend(bytes);
            bytes = provisional;
        }
        fixtures.push(Fixture {
            runtime: "App::PropertyMaterialList",
            tag: "MaterialList",
            version: Some(version),
            kind: BinaryKind::Material { version },
            bytes,
        });
    }
    for (runtime, tag, components) in [
        ("Points::PropertyGreyValueList", "FloatList", 1),
        ("Inspection::PropertyDistanceList", "FloatList", 1),
        ("Points::PropertyNormalList", "VectorList", 3),
        ("Mesh::PropertyNormalList", "VectorList", 3),
        ("Points::PropertyCurvatureList", "CurvatureList", 8),
        ("Mesh::PropertyCurvatureList", "CurvatureList", 8),
    ] {
        fixtures.push(Fixture {
            runtime,
            tag,
            version: None,
            kind: BinaryKind::F32 { components },
            bytes: counted((0..3).map(|_| {
                (0..components)
                    .flat_map(|_| 1.0_f32.to_le_bytes())
                    .collect()
            })),
        });
    }
    let mut material = 1_u32.to_le_bytes().to_vec();
    for index in 0..6 {
        material.extend(3_u32.to_le_bytes());
        for _ in 0..3 {
            material.extend(if index < 4 {
                0x11223344_u32.to_le_bytes()
            } else {
                0.5_f32.to_le_bytes()
            });
        }
    }
    fixtures.push(Fixture {
        runtime: "Mesh::PropertyMaterial",
        tag: "Material",
        version: None,
        kind: BinaryKind::MeshMaterial,
        bytes: material,
    });
    fixtures
}

pub(super) fn archive(fixtures: &[Fixture]) -> Vec<u8> {
    archive_cut(fixtures, None)
}

pub(super) fn archive_cut(fixtures: &[Fixture], cut: Option<(&str, usize)>) -> Vec<u8> {
    let mut entries = Vec::new();
    let mut app = String::new();
    let mut gui = String::new();
    for (index, fixture) in fixtures.iter().enumerate() {
        for prefix in ["app", "gui", "app-copy"] {
            let properties = if prefix == "gui" { &mut gui } else { &mut app };
            let file = format!("{prefix}-{index}.bin");
            let version = fixture
                .version
                .map_or_else(String::new, |v| format!(" version='{v}'"));
            properties.push_str(&format!(
                "<Property name='{prefix}P{index}' type='{}'><{} file='{file}'{version}/></Property>",
                fixture.runtime, fixture.tag
            ));
            let mut bytes = fixture.bytes.clone();
            if let Some((member, end)) = cut.filter(|(member, _)| *member == file) {
                assert_eq!(member, file);
                bytes.truncate(end);
            }
            entries.push((file, bytes));
        }
    }
    let count = fixtures.len();
    let app_count = count * 2;
    let document=format!("<Document SchemaVersion='4' FileVersion='1'><Objects Count='1'><Object type='App::FeaturePython' name='A' id='1'/></Objects><ObjectData Count='1'><Object name='A'><Properties Count='{app_count}'>{app}</Properties></Object></ObjectData></Document>");
    let gui=format!("<Document SchemaVersion='1'><ViewProviderData Count='1'><ViewProvider name='A'><Properties Count='{count}'>{gui}</Properties></ViewProvider></ViewProviderData></Document>");
    entries.push(("Document.xml".into(), document.into_bytes()));
    entries.push(("GuiDocument.xml".into(), gui.into_bytes()));
    crate::test_support::test_archive::archive_entries(
        &entries
            .iter()
            .map(|(name, bytes)| (name.as_str(), bytes.as_slice()))
            .collect::<Vec<_>>(),
    )
}
