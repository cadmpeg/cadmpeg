// SPDX-License-Identifier: Apache-2.0
//! Binary property members regenerated from current typed entry contents.

pub(in crate::writer) mod appearances;
mod auxiliary;
use crate::native::entry_contents::{BinaryPrecision, EntryContents, F32Values};
use crate::native::{EntryRecord, ValueRecord};
use cadmpeg_core::CodecError;
use cadmpeg_ir::native::NativeNamespace;
use cadmpeg_ir::report::loss::LossNote;
use std::collections::{BTreeMap, BTreeSet};

pub(super) fn write(
    namespace: Option<&NativeNamespace>,
    owner: &str,
    kind: &str,
    values: &[ValueRecord],
    entries: &mut BTreeMap<String, Vec<u8>>,
    losses: &mut Vec<LossNote>,
    outcomes: &mut cadmpeg_ir::codec::write::NativeDispositions,
) -> Result<BTreeSet<String>, CodecError> {
    write_impl(
        namespace, owner, kind, values, None, entries, losses, outcomes,
    )
}
pub(super) fn write_gui(
    ir: &cadmpeg_ir::CadIr,
    namespace: Option<&NativeNamespace>,
    property: &crate::native::GuiPropertyRecord,
    entries: &mut BTreeMap<String, Vec<u8>>,
    losses: &mut Vec<LossNote>,
    outcomes: &mut cadmpeg_ir::codec::write::NativeDispositions,
) -> Result<BTreeSet<String>, CodecError> {
    write_impl(
        namespace,
        &property.id,
        &property.type_name,
        &property.values,
        Some((ir, property)),
        entries,
        losses,
        outcomes,
    )
}
fn write_impl(
    namespace: Option<&NativeNamespace>,
    owner: &str,
    kind: &str,
    values: &[ValueRecord],
    appearance: Option<(&cadmpeg_ir::CadIr, &crate::native::GuiPropertyRecord)>,
    entries: &mut BTreeMap<String, Vec<u8>>,
    losses: &mut Vec<LossNote>,
    outcomes: &mut cadmpeg_ir::codec::write::NativeDispositions,
) -> Result<BTreeSet<String>, CodecError> {
    let Some(namespace) = namespace else {
        return Ok(BTreeSet::new());
    };
    let members = namespace.arena_as::<EntryRecord>("entries")?;
    let mut emitted = BTreeSet::new();
    for member in &members {
        if !values.iter().any(|value| {
            value
                .attributes
                .get("file")
                .is_some_and(|file| file == member.name())
        }) {
            continue;
        }
        for content in member
            .contents()
            .iter()
            .filter(|content| crate::gui::payloads::projection::matches(content, kind, values))
        {
            let mut current = content.clone();
            if let Some((ir, property)) = appearance {
                if !appearances::rewrite(ir, property, member, &mut current, losses)? {
                    continue;
                }
            }
            let Some(bytes) = encode(&current, owner, losses)? else {
                continue;
            };
            if entries
                .get(member.name())
                .is_some_and(|previous| previous != &bytes)
            {
                losses.push(cadmpeg_ir::codec::write::ExportLossCode::SourceUndecodedOmitted.note(format!("property {owner} conflicts with another current interpretation of entry {}; its payload is omitted",member.id())));
                continue;
            }
            entries.insert(member.name().to_owned(), bytes);
            emitted.insert(member.name().to_owned());
            super::dispositions::record(
                outcomes,
                "entries",
                member.id(),
                cadmpeg_ir::codec::write::NativeDisposition::Regenerated,
            );
        }
    }
    Ok(emitted)
}
fn count(bytes: &mut Vec<u8>, count: usize) -> Result<(), CodecError> {
    bytes.extend_from_slice(
        &u32::try_from(count)
            .map_err(CodecError::malformed)?
            .to_le_bytes(),
    );
    Ok(())
}
fn omitted(owner: &str, positions: impl Iterator<Item = usize>, losses: &mut Vec<LossNote>) {
    let positions = positions.collect::<Vec<_>>();
    if !positions.is_empty() {
        losses.push(
            cadmpeg_ir::codec::write::ExportLossCode::SourceUndecodedOmitted.note(format!(
                "property {owner} omits undecoded binary positions {positions:?}"
            )),
        );
    }
}
pub(super) fn encode(
    content: &EntryContents,
    owner: &str,
    losses: &mut Vec<LossNote>,
) -> Result<Option<Vec<u8>>, CodecError> {
    let mut bytes = Vec::new();
    macro_rules! scalar_list {
        ($values:expr,$body:expr) => {{
            let values = $values;
            omitted(
                owner,
                values
                    .iter()
                    .enumerate()
                    .filter_map(|(index, value)| value.is_none().then_some(index)),
                losses,
            );
            count(&mut bytes, values.iter().flatten().count())?;
            for value in values.iter().flatten() {
                ($body)(&mut bytes, value)?;
            }
        }};
    }
    match content {
        EntryContents::Color { values } => {
            scalar_list!(values, |bytes: &mut Vec<u8>,
                                  value: &u32|
             -> Result<(), CodecError> {
                bytes.extend_from_slice(&value.to_le_bytes());
                Ok(())
            })
        }
        EntryContents::Float { values } => {
            scalar_list!(values, |bytes: &mut Vec<u8>,
                                  value: &cadmpeg_ir::scalar::FiniteReal|
             -> Result<(), CodecError> {
                bytes.extend_from_slice(&value.get().to_le_bytes());
                Ok(())
            })
        }
        EntryContents::Vector { precision, values } => {
            scalar_list!(values, |bytes: &mut Vec<u8>,
                                  value: &[cadmpeg_ir::scalar::FiniteReal; 3]|
             -> Result<(), CodecError> {
                for value in value {
                    real(bytes, value.get(), *precision)?;
                }
                Ok(())
            })
        }
        EntryContents::Placement { precision, values } => {
            scalar_list!(values, |bytes: &mut Vec<u8>,
                                  value: &[cadmpeg_ir::scalar::FiniteReal; 7]|
             -> Result<(), CodecError> {
                for value in value {
                    real(bytes, value.get(), *precision)?;
                }
                Ok(())
            })
        }
        EntryContents::Fillet { values } => scalar_list!(
            values,
            |bytes: &mut Vec<u8>,
             value: &crate::native::entry_contents::FilletValue|
             -> Result<(), CodecError> {
                bytes.extend_from_slice(&value.edge.to_le_bytes());
                bytes.extend_from_slice(&value.radius1.get().to_le_bytes());
                bytes.extend_from_slice(&value.radius2.get().to_le_bytes());
                Ok(())
            }
        ),
        EntryContents::Material { version, values } => {
            let complete = |value: &crate::native::entry_contents::MaterialSlot| {
                value.fixed.is_some()
                    && (*version != 3
                        || (value.image.is_some()
                            && value.image_path.is_some()
                            && value.uuid.is_some()))
            };
            omitted(
                owner,
                values
                    .iter()
                    .enumerate()
                    .filter_map(|(index, value)| (!complete(value)).then_some(index)),
                losses,
            );
            let held = values
                .iter()
                .filter(|value| complete(value))
                .collect::<Vec<_>>();
            if *version <= 1 && held.len() > i32::MAX as usize {
                bytes.extend_from_slice(&(-1_i32).to_le_bytes());
            }
            count(&mut bytes, held.len())?;
            for value in &held {
                if let Some(value) = &value.fixed {
                    for color in [value.ambient, value.diffuse, value.specular, value.emissive] {
                        bytes.extend_from_slice(&color.to_le_bytes());
                    }
                    bytes.extend_from_slice(&value.shininess.get().to_le_bytes());
                    bytes.extend_from_slice(&value.transparency.get().to_le_bytes());
                }
            }
            if *version == 3 {
                for value in &held {
                    for string in [&value.image, &value.image_path, &value.uuid] {
                        let Some(string) = string else {
                            return Err(CodecError::malformed(
                                "material string completeness changed",
                            ));
                        };
                        count(&mut bytes, string.len())?;
                        bytes.extend_from_slice(string.as_bytes());
                    }
                }
            }
        }
        EntryContents::F32 { values } => match values {
            F32Values::Scalar(values) => {
                scalar_list!(values, |bytes: &mut Vec<u8>,
                                      value: &cadmpeg_ir::scalar::FiniteBinary32|
                 -> Result<(), CodecError> {
                    bytes.extend_from_slice(&value.get().to_le_bytes());
                    Ok(())
                })
            }
            F32Values::Vector(values) => {
                scalar_list!(values, |bytes: &mut Vec<u8>,
                                      value: &[cadmpeg_ir::scalar::FiniteBinary32; 3]|
                 -> Result<(), CodecError> {
                    for value in value {
                        bytes.extend_from_slice(&value.get().to_le_bytes());
                    }
                    Ok(())
                })
            }
            F32Values::Curvature(values) => {
                scalar_list!(values, |bytes: &mut Vec<u8>,
                                      value: &[cadmpeg_ir::scalar::FiniteBinary32; 8]|
                 -> Result<(), CodecError> {
                    for value in value {
                        bytes.extend_from_slice(&value.get().to_le_bytes());
                    }
                    Ok(())
                })
            }
        },
        EntryContents::MeshMaterial { .. } => return auxiliary::encode(content, owner, losses),
        EntryContents::Mesh { .. } | EntryContents::Points { .. } => return Ok(None),
    }
    Ok(Some(bytes))
}
fn real(bytes: &mut Vec<u8>, value: f64, precision: BinaryPrecision) -> Result<(), CodecError> {
    match precision {
        BinaryPrecision::Double => bytes.extend_from_slice(&value.to_le_bytes()),
        BinaryPrecision::Single => {
            let encoded = value
                .to_string()
                .parse::<f32>()
                .map_err(CodecError::malformed)?;
            if !encoded.is_finite() {
                return Err(CodecError::malformed(
                    "single precision list scalar overflows",
                ));
            }
            bytes.extend_from_slice(&encoded.to_le_bytes());
        }
    }
    Ok(())
}
#[cfg(test)]
mod tests;
