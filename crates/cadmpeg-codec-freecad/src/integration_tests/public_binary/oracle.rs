// SPDX-License-Identifier: Apache-2.0
//! Independent fixed-field native expectations from the real member bytes.

use super::*;
use crate::native::entry_contents::{
    BinaryMaterial, BinaryPrecision, EntryContents, F32Values, FilletValue, MaterialSlot, MeshFacet,
};
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::scalar::{FiniteBinary32, FiniteReal};

fn word(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
}
fn real32(bytes: &[u8], at: usize) -> FiniteBinary32 {
    FiniteBinary32::new(f32::from_bits(word(bytes, at))).expect("independent finite f32")
}
fn real64(bytes: &[u8], at: usize) -> FiniteReal {
    FiniteReal::new(f64::from_le_bytes(bytes[at..at + 8].try_into().unwrap()))
        .expect("independent finite f64")
}
fn reals<const N: usize>(bytes: &[u8]) -> [FiniteReal; N] {
    assert!(bytes.len() == N * 4 || bytes.len() == N * 8);
    std::array::from_fn(|index| {
        if bytes.len() == N * 4 {
            FiniteReal::new(f64::from(real32(bytes, index * 4).get())).unwrap()
        } else {
            real64(bytes, index * 8)
        }
    })
}
fn point(bytes: &[u8], at: usize, big: bool) -> FinitePoint3 {
    let values: [f64; 3] = std::array::from_fn(|index| {
        let raw: [u8; 4] = bytes[at + index * 4..at + index * 4 + 4]
            .try_into()
            .unwrap();
        f64::from(if big {
            f32::from_be_bytes(raw)
        } else {
            f32::from_le_bytes(raw)
        })
    });
    FinitePoint3::new(cadmpeg_ir::math::Point3::new(
        values[0], values[1], values[2],
    ))
    .expect("independent finite point")
}

fn payloads(carrier: &source::Carrier, bytes: &[u8]) -> EntryContents {
    if matches!(carrier.kind, BinaryKind::MeshMaterial) {
        let binding = word(bytes, 0);
        assert!(binding <= 2);
        let mut at = 4;
        let mut colors = Vec::new();
        for _ in 0..4 {
            let count = usize::try_from(word(bytes, at)).unwrap();
            at += 4;
            colors.push(Some(
                (0..count)
                    .map(|index| Some(word(bytes, at + index * 4)))
                    .collect(),
            ));
            at += count * 4;
        }
        let mut floats = Vec::new();
        for _ in 0..2 {
            let count = usize::try_from(word(bytes, at)).unwrap();
            at += 4;
            floats.push(Some(
                (0..count)
                    .map(|index| Some(real32(bytes, at + index * 4)))
                    .collect(),
            ));
            at += count * 4;
        }
        assert_eq!(at, bytes.len(), "independent six arrays cover the member");
        let mut colors = colors.into_iter();
        let mut floats = floats.into_iter();
        return EntryContents::MeshMaterial {
            binding: Some(binding),
            ambient: colors.next().unwrap(),
            diffuse: colors.next().unwrap(),
            specular: colors.next().unwrap(),
            emissive: colors.next().unwrap(),
            shininess: floats.next().unwrap(),
            transparency: floats.next().unwrap(),
        };
    }
    let first = word(bytes, 0);
    let provisional =
        matches!(carrier.kind, BinaryKind::Material { version: 0 | 1 }) && first & 0x8000_0000 != 0;
    let start = if provisional { 8 } else { 4 };
    let count = if provisional { word(bytes, 4) } else { first };
    let count = usize::try_from(count).unwrap();
    let width = match carrier.kind {
        BinaryKind::Color => 4,
        BinaryKind::Float => 8,
        BinaryKind::Fillet => 20,
        BinaryKind::Vector => {
            if count != 0 && bytes.len() - start == count * 12 {
                12
            } else {
                24
            }
        }
        BinaryKind::Placement => {
            if count != 0 && bytes.len() - start == count * 28 {
                28
            } else {
                56
            }
        }
        BinaryKind::Material { .. } => 24,
        BinaryKind::F32 { components } => components * 4,
        _ => panic!("public list kind is selected by actual XML"),
    };
    let end = start + count * width;
    assert!(end <= bytes.len());
    let rows =
        || (0..count).map(|order| &bytes[start + order * width..start + (order + 1) * width]);
    let mut result = match carrier.kind {
        BinaryKind::Color => EntryContents::Color {
            values: rows().map(|row| Some(word(row, 0))).collect(),
        },
        BinaryKind::Float => EntryContents::Float {
            values: rows().map(|row| Some(real64(row, 0))).collect(),
        },
        BinaryKind::Vector => EntryContents::Vector {
            precision: if width == 12 {
                BinaryPrecision::Single
            } else {
                BinaryPrecision::Double
            },
            values: rows().map(|row| Some(reals(row))).collect(),
        },
        BinaryKind::Placement => EntryContents::Placement {
            precision: if width == 28 {
                BinaryPrecision::Single
            } else {
                BinaryPrecision::Double
            },
            values: rows()
                .map(|row| {
                    let values = reals::<7>(row);
                    assert!(
                        values[3..].iter().any(|value| value.get() != 0.0),
                        "independent nonzero placement quaternion"
                    );
                    Some(values)
                })
                .collect(),
        },
        BinaryKind::Fillet => EntryContents::Fillet {
            values: rows()
                .map(|row| {
                    Some(FilletValue {
                        edge: i32::from_le_bytes(row[..4].try_into().unwrap()),
                        radius1: real64(row, 4),
                        radius2: real64(row, 12),
                    })
                })
                .collect(),
        },
        BinaryKind::Material { version } => EntryContents::Material {
            version,
            values: rows()
                .map(|row| MaterialSlot {
                    fixed: Some(BinaryMaterial {
                        ambient: word(row, 0),
                        diffuse: word(row, 4),
                        specular: word(row, 8),
                        emissive: word(row, 12),
                        shininess: real32(row, 16),
                        transparency: real32(row, 20),
                    }),
                    image: None,
                    image_path: None,
                    uuid: None,
                })
                .collect(),
        },
        BinaryKind::F32 { components } => EntryContents::F32 {
            values: match components {
                1 => F32Values::Scalar(rows().map(|row| Some(real32(row, 0))).collect()),
                3 => F32Values::Vector(
                    rows()
                        .map(|row| Some(std::array::from_fn(|index| real32(row, index * 4))))
                        .collect(),
                ),
                8 => F32Values::Curvature(
                    rows()
                        .map(|row| Some(std::array::from_fn(|index| real32(row, index * 4))))
                        .collect(),
                ),
                _ => panic!("registered f32 record width"),
            },
        },
        _ => unreachable!(),
    };
    let mut at = end;
    if let EntryContents::Material { version: 3, values } = &mut result {
        for slot in values {
            for field in [&mut slot.image, &mut slot.image_path, &mut slot.uuid] {
                let size = usize::try_from(word(bytes, at)).unwrap();
                at += 4;
                *field = Some(
                    std::str::from_utf8(&bytes[at..at + size])
                        .expect("independent UTF-8 material string")
                        .to_owned(),
                );
                at += size;
            }
        }
    }
    assert_eq!(
        at,
        bytes.len(),
        "independent list covers the complete member"
    );
    result
}

fn kernel(carrier: &source::Carrier, bytes: &[u8]) -> EntryContents {
    if matches!(carrier.kind, BinaryKind::Mesh) {
        let big = word(bytes, 0) == 0xd0c0_b0a0;
        assert!(word(bytes, 0) == 0xa0b0_c0d0 || big);
        let read = |at| {
            let value = word(bytes, at);
            if big {
                value.swap_bytes()
            } else {
                value
            }
        };
        assert_eq!(read(4), 0x0001_0000);
        let point_count = usize::try_from(read(264)).unwrap();
        let facet_count = usize::try_from(read(268)).unwrap();
        let vertices = (0..point_count)
            .map(|index| Some(point(bytes, 272 + index * 12, big)))
            .collect();
        let facets_start = 272 + point_count * 12;
        let facets = (0..facet_count)
            .map(|index| {
                let at = facets_start + index * 24;
                let points = std::array::from_fn(|index| read(at + index * 4));
                assert!(points
                    .iter()
                    .all(|index| usize::try_from(*index).unwrap() < point_count));
                let neighbours = std::array::from_fn(|index| {
                    let value = read(at + 12 + index * 4);
                    if value == u32::MAX {
                        None
                    } else {
                        assert!(usize::try_from(value).unwrap() < facet_count);
                        Some(value)
                    }
                });
                Some(MeshFacet { points, neighbours })
            })
            .collect();
        let bounds_start = facets_start + facet_count * 24;
        assert_eq!(bounds_start + 24, bytes.len());
        let bounds = std::array::from_fn(|index| {
            FiniteBinary32::new(f32::from_bits(read(bounds_start + index * 4))).unwrap()
        });
        EntryContents::Mesh {
            big_endian: big,
            vertices,
            facets,
            bounds: Some(bounds),
        }
    } else {
        assert!(matches!(carrier.kind, BinaryKind::Points));
        let count = usize::try_from(word(bytes, 0)).unwrap();
        assert_eq!(4 + count * 12, bytes.len());
        EntryContents::Points {
            points: (0..count)
                .map(|index| Some(point(bytes, 4 + index * 12, false)))
                .collect(),
        }
    }
}

pub(super) fn expected(
    before: &DecodeResult,
    carrier: &source::Carrier,
    bytes: &[u8],
) -> cadmpeg_ir::CadIr {
    let mut ir = before.ir().clone();
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut entries = ir
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<crate::native::EntryRecord>("entries")
        .unwrap();
    let entry = entries
        .iter_mut()
        .find(|entry| entry.name() == carrier.member)
        .expect("actual source carrier entry");
    let contents = if matches!(carrier.kind, BinaryKind::Mesh | BinaryKind::Points) {
        kernel(carrier, bytes)
    } else {
        payloads(carrier, bytes)
    };
    let grammar = contents.grammar();
    let slot = entry
        .contents
        .iter_mut()
        .find(|prior| prior.grammar() == grammar)
        .expect("baseline has this exact registered typed interpretation");
    *slot = contents;
    let selected = entry.clone();
    ir.native
        .namespace_mut("fcstd")
        .set_arena(&ctx, "entries", &entries)
        .unwrap();
    neutral::update(&mut ir, &selected, carrier.kind).unwrap();
    ir
}
