// SPDX-License-Identifier: Apache-2.0
//! Mesh material arrays are regenerated from their owning entry interpretation.
use super::*;
pub(super) fn encode(
    content: &EntryContents,
    owner: &str,
    losses: &mut Vec<LossNote>,
) -> Result<Option<Vec<u8>>, CodecError> {
    let EntryContents::MeshMaterial {
        binding,
        ambient,
        diffuse,
        specular,
        emissive,
        shininess,
        transparency,
    } = content
    else {
        return Err(CodecError::malformed("non-mesh material content"));
    };
    let (
        Some(binding),
        Some(ambient),
        Some(diffuse),
        Some(specular),
        Some(emissive),
        Some(shininess),
        Some(transparency),
    ) = (
        binding,
        ambient,
        diffuse,
        specular,
        emissive,
        shininess,
        transparency,
    )
    else {
        losses.push(
            cadmpeg_ir::codec::write::ExportLossCode::SourceUndecodedOmitted.note(format!(
                "property {owner} has unknown mesh material array arity or binding and is omitted"
            )),
        );
        return Ok(None);
    };
    let mut bytes = binding.to_le_bytes().to_vec();
    for values in [ambient, diffuse, specular, emissive] {
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
            bytes.extend_from_slice(&value.to_le_bytes());
        }
    }
    for values in [shininess, transparency] {
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
            bytes.extend_from_slice(&value.get().to_le_bytes());
        }
    }
    Ok(Some(bytes))
}
