// SPDX-License-Identifier: Apache-2.0
//! Independent source-field expectations retain known arity and sparse row positions.

use crate::native::entry_contents::{
    BinaryMaterial, BinaryPrecision, EntryContents, F32Values, FilletValue, MaterialSlot,
};
use cadmpeg_ir::attributes::{AttributeNumericList, AttributePlacement, AttributeValue};
use cadmpeg_ir::scalar::{FiniteBinary32, FiniteReal};

fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        bytes.get(at..at.checked_add(4)?)?.try_into().ok()?,
    ))
}
fn f32_at(bytes: &[u8], at: usize) -> Option<FiniteBinary32> {
    FiniteBinary32::new(f32::from_bits(u32_at(bytes, at)?))
}
fn f64_at(bytes: &[u8], at: usize) -> Option<FiniteReal> {
    FiniteReal::new(f64::from_le_bytes(
        bytes.get(at..at.checked_add(8)?)?.try_into().ok()?,
    ))
}
fn real_row<const N: usize>(
    bytes: &[u8],
    at: usize,
    precision: BinaryPrecision,
) -> Option<[FiniteReal; N]> {
    let mut values = [FiniteReal::ZERO; N];
    for (index, value) in values.iter_mut().enumerate() {
        *value = match precision {
            BinaryPrecision::Single => {
                FiniteReal::new(f64::from(f32_at(bytes, at + index * 4)?.get()))?
            }
            BinaryPrecision::Double => f64_at(bytes, at + index * 8)?,
        };
    }
    Some(values)
}
fn f32_row<const N: usize>(bytes: &[u8], at: usize) -> Option<[FiniteBinary32; N]> {
    (0..N)
        .map(|index| f32_at(bytes, at + index * 4))
        .collect::<Option<Vec<_>>>()?
        .try_into()
        .ok()
}
fn count(bytes: &[u8], width: usize, original: usize) -> Option<usize> {
    let count = usize::try_from(u32_at(bytes, 0)?).ok()?;
    let end = 4_usize.checked_add(count.checked_mul(width)?)?;
    if end < bytes.len() || (count != original && end > bytes.len()) {
        return None;
    }
    Some(count)
}
fn width(precision: BinaryPrecision) -> usize {
    if precision == BinaryPrecision::Single {
        4
    } else {
        8
    }
}

pub(super) fn contents(original: &EntryContents, bytes: &[u8]) -> Option<EntryContents> {
    Some(match original {
        EntryContents::Color { values } => {
            let count = count(bytes, 4, values.len())?;
            EntryContents::Color {
                values: (0..count)
                    .map(|index| u32_at(bytes, 4 + index * 4))
                    .collect(),
            }
        }
        EntryContents::Float { values } => {
            let count = count(bytes, 8, values.len())?;
            EntryContents::Float {
                values: (0..count)
                    .map(|index| f64_at(bytes, 4 + index * 8))
                    .collect(),
            }
        }
        EntryContents::Vector { values, .. } => {
            let precision = if u32_at(bytes, 0)
                .and_then(|count| usize::try_from(count).ok())
                .and_then(|count| count.checked_mul(12)?.checked_add(4))
                == Some(bytes.len())
            {
                BinaryPrecision::Single
            } else {
                BinaryPrecision::Double
            };
            let stride = 3 * width(precision);
            let count = count(bytes, stride, values.len())?;
            EntryContents::Vector {
                precision,
                values: (0..count)
                    .map(|index| real_row(bytes, 4 + index * stride, precision))
                    .collect(),
            }
        }
        EntryContents::Placement { values, .. } => {
            let precision = if u32_at(bytes, 0)
                .and_then(|count| usize::try_from(count).ok())
                .and_then(|count| count.checked_mul(28)?.checked_add(4))
                == Some(bytes.len())
            {
                BinaryPrecision::Single
            } else {
                BinaryPrecision::Double
            };
            let stride = 7 * width(precision);
            let count = count(bytes, stride, values.len())?;
            EntryContents::Placement {
                precision,
                values: (0..count)
                    .map(|index| {
                        real_row::<7>(bytes, 4 + index * stride, precision)
                            .filter(|values| values[3..].iter().any(|value| value.get() != 0.))
                    })
                    .collect(),
            }
        }
        EntryContents::Fillet { values } => {
            let count = count(bytes, 20, values.len())?;
            EntryContents::Fillet {
                values: (0..count)
                    .map(|index| {
                        let at = 4 + index * 20;
                        Some(FilletValue {
                            edge: i32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?),
                            radius1: f64_at(bytes, at + 4)?,
                            radius2: f64_at(bytes, at + 12)?,
                        })
                    })
                    .collect(),
            }
        }
        EntryContents::F32 { values } => EntryContents::F32 {
            values: match values {
                F32Values::Scalar(values) => {
                    let count = count(bytes, 4, values.len())?;
                    F32Values::Scalar(
                        (0..count)
                            .map(|index| f32_at(bytes, 4 + index * 4))
                            .collect(),
                    )
                }
                F32Values::Vector(values) => {
                    let count = count(bytes, 12, values.len())?;
                    F32Values::Vector(
                        (0..count)
                            .map(|index| f32_row(bytes, 4 + index * 12))
                            .collect(),
                    )
                }
                F32Values::Curvature(values) => {
                    let count = count(bytes, 32, values.len())?;
                    F32Values::Curvature(
                        (0..count)
                            .map(|index| f32_row(bytes, 4 + index * 32))
                            .collect(),
                    )
                }
            },
        },
        EntryContents::Material { version, values } => {
            let start = if *version <= 1 && u32_at(bytes, 0) == Some(u32::MAX) {
                4
            } else {
                0
            };
            let count = usize::try_from(u32_at(bytes, start)?).ok()?;
            if count != values.len() {
                return None;
            }
            let mut values = (0..count)
                .map(|index| {
                    let at = start + 4 + index * 24;
                    MaterialSlot {
                        fixed: (|| {
                            Some(BinaryMaterial {
                                ambient: u32_at(bytes, at)?,
                                diffuse: u32_at(bytes, at + 4)?,
                                specular: u32_at(bytes, at + 8)?,
                                emissive: u32_at(bytes, at + 12)?,
                                shininess: f32_at(bytes, at + 16)?,
                                transparency: f32_at(bytes, at + 20)?,
                            })
                        })(),
                        image: None,
                        image_path: None,
                        uuid: None,
                    }
                })
                .collect::<Vec<_>>();
            let mut at = start + 4 + count * 24;
            if *version == 3 {
                'strings: for value in &mut values {
                    for slot in [&mut value.image, &mut value.image_path, &mut value.uuid] {
                        let Some(length) =
                            u32_at(bytes, at).and_then(|count| usize::try_from(count).ok())
                        else {
                            // A partial length word has no successor boundary.
                            // The fixed region and earlier complete strings stay.
                            at = bytes.len();
                            break 'strings;
                        };
                        let start = at.checked_add(4)?;
                        let end = start.checked_add(length)?;
                        if end > bytes.len() {
                            at = bytes.len();
                            break 'strings;
                        }
                        *slot = bytes
                            .get(start..end)
                            .and_then(|bytes| std::str::from_utf8(bytes).ok())
                            .filter(|value| !value.contains('\0'))
                            .map(str::to_owned);
                        at = end;
                    }
                }
            }
            if at < bytes.len() {
                return None;
            }
            EntryContents::Material {
                version: *version,
                values,
            }
        }
        EntryContents::MeshMaterial {
            ambient,
            diffuse,
            specular,
            emissive,
            shininess,
            transparency,
            ..
        } => {
            let binding = u32_at(bytes, 0).filter(|value| *value <= 2);
            let mut at = 4;
            let mut colors = Vec::new();
            for original in [ambient, diffuse, specular, emissive] {
                colors.push(mesh_array(
                    bytes,
                    &mut at,
                    original.as_ref().map(Vec::len),
                    |bytes, at| u32_at(bytes, at),
                ));
            }
            let shininess = mesh_array(bytes, &mut at, shininess.as_ref().map(Vec::len), f32_at);
            let transparency =
                mesh_array(bytes, &mut at, transparency.as_ref().map(Vec::len), f32_at);
            EntryContents::MeshMaterial {
                binding,
                ambient: colors.remove(0),
                diffuse: colors.remove(0),
                specular: colors.remove(0),
                emissive: colors.remove(0),
                shininess,
                transparency,
            }
        }
        EntryContents::Mesh { .. } | EntryContents::Points { .. } => return None,
    })
}
fn mesh_array<T>(
    bytes: &[u8],
    at: &mut usize,
    original: Option<usize>,
    read: impl Fn(&[u8], usize) -> Option<T>,
) -> Option<Vec<Option<T>>> {
    let count = usize::try_from(u32_at(bytes, *at)?).ok()?;
    let start = at.checked_add(4)?;
    let end = start.checked_add(count.checked_mul(4)?)?;
    if end > bytes.len() && Some(count) != original {
        *at = bytes.len();
        return None;
    }
    let values = (0..count)
        .map(|index| read(bytes, start + index * 4))
        .collect();
    *at = end;
    Some(values)
}

pub(super) fn expected(
    before: &cadmpeg_ir::codec::DecodeResult,
    stream: &str,
    member: &[u8],
) -> cadmpeg_ir::CadIr {
    let mut ir = before.ir().clone();
    let namespace = ir.native.namespace_mut("fcstd");
    let mut entries = namespace
        .arena_as::<crate::native::EntryRecord>("entries")
        .unwrap();
    for entry in &mut entries {
        let Some(layout) = before.source_fidelity().record_layout().get(entry.id()) else {
            continue;
        };
        // Entry producers have ZIP-header primaries and member continuations.
        let belongs = layout.reads().contains_key(stream);
        if layout.stream() != stream && !belongs {
            continue;
        }
        entry.contents = entry
            .contents
            .iter()
            .filter_map(|content| contents(content, member))
            .collect();
    }
    namespace
        .set_arena(
            &cadmpeg_test_support::service_decode_context(),
            "entries",
            &entries,
        )
        .unwrap();
    project_numeric(&mut ir, &entries);
    ir
}

fn project_numeric(ir: &mut cadmpeg_ir::CadIr, entries: &[crate::native::EntryRecord]) {
    let properties = ir
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<crate::native::PropertyRecord>("properties")
        .unwrap();
    for attribute in &mut ir.model.attributes {
        let [AttributeValue::NumericList(list)] = attribute.values.as_mut_slice() else {
            continue;
        };
        let Some(property) = properties
            .iter()
            .find(|property| Some(property.id.as_str()) == attribute.native_ref.as_deref())
        else {
            continue;
        };
        let content = entries
            .iter()
            .filter(|entry| {
                property
                    .side_entries()
                    .iter()
                    .any(|name| name == entry.name())
            })
            .flat_map(crate::native::EntryRecord::contents)
            .next();
        *list = match (&*list, content) {
            (AttributeNumericList::Float(_), Some(EntryContents::Float { values })) => {
                AttributeNumericList::Float(Some(values.clone()))
            }
            (AttributeNumericList::Color(_), Some(EntryContents::Color { values })) => {
                AttributeNumericList::Color(Some(values.clone()))
            }
            (AttributeNumericList::Vector(_), Some(EntryContents::Vector { values, .. })) => {
                AttributeNumericList::Vector(Some(
                    values.iter().map(|value| value.map(Into::into)).collect(),
                ))
            }
            (AttributeNumericList::Placement(_), Some(EntryContents::Placement { values, .. })) => {
                AttributeNumericList::Placement(Some(
                    values
                        .iter()
                        .map(|value| {
                            value.map(|value| AttributePlacement {
                                position: [value[0], value[1], value[2]].into(),
                                quaternion: [value[3], value[4], value[5], value[6]].into(),
                            })
                        })
                        .collect(),
                ))
            }
            (
                AttributeNumericList::Float(_),
                Some(EntryContents::F32 {
                    values: F32Values::Scalar(values),
                }),
            ) => AttributeNumericList::Float(Some(
                values
                    .iter()
                    .map(|value| value.and_then(|value| FiniteReal::new(f64::from(value.get()))))
                    .collect(),
            )),
            (
                AttributeNumericList::Vector(_),
                Some(EntryContents::F32 {
                    values: F32Values::Vector(values),
                }),
            ) => AttributeNumericList::Vector(Some(
                values
                    .iter()
                    .map(|value| {
                        value.map(|value| {
                            value
                                .map(|value| FiniteReal::new(f64::from(value.get())).unwrap())
                                .into()
                        })
                    })
                    .collect(),
            )),
            (
                AttributeNumericList::Curvature(_),
                Some(EntryContents::F32 {
                    values: F32Values::Curvature(values),
                }),
            ) => AttributeNumericList::Curvature(Some(
                values
                    .iter()
                    .map(|value| {
                        value.map(|value| {
                            value
                                .map(|value| FiniteReal::new(f64::from(value.get())).unwrap())
                                .into()
                        })
                    })
                    .collect(),
            )),
            (AttributeNumericList::Float(_), _) => AttributeNumericList::Float(None),
            (AttributeNumericList::Vector(_), _) => AttributeNumericList::Vector(None),
            (AttributeNumericList::Placement(_), _) => AttributeNumericList::Placement(None),
            (AttributeNumericList::Color(_), _) => AttributeNumericList::Color(None),
            (AttributeNumericList::Curvature(_), _) => AttributeNumericList::Curvature(None),
        };
    }
}
