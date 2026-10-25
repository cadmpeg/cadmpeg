// SPDX-License-Identifier: Apache-2.0
//! Unavailable declaration lookups retain their original, source-bound failure home.

use cadmpeg_core::decode::{u64_from_index, DecodeContext, SourceExtentProof};
use cadmpeg_core::CodecError;
use cadmpeg_ir::source_fidelity::parts::{DecodeParts, PartOutput, PartState};

pub(super) struct UnavailableDeclarationScope {
    source: SourceExtentProof,
    home: String,
    identity: Option<String>,
}

pub(super) struct UnavailableBodyScope {
    failure: UnavailableDeclarationScope,
    owners: Vec<String>,
}

impl UnavailableBodyScope {
    pub(super) fn from_given_up<'id>(
        ctx: &DecodeContext<'_>,
        source: &crate::xml_framing::XmlSource<'_>,
        index: usize,
        level: &str,
        owners: impl IntoIterator<Item = &'id str>,
        output: &PartOutput,
    ) -> Result<Self, CodecError> {
        let failure =
            UnavailableDeclarationScope::from_given_up(ctx, source, index, level, None, output)?;
        let mut ids = Vec::new();
        for id in owners {
            reserve(ctx, &mut ids)?;
            ctx.retain_bytes(
                u64_from_index(id.len()),
                "FreeCAD unavailable body source owner",
            )?;
            ids.push(id.to_owned());
        }
        Ok(Self {
            failure,
            owners: ids,
        })
    }

    pub(super) fn bind(
        &self,
        ctx: &DecodeContext<'_>,
        registry: &cadmpeg_ir::source_fidelity::parts::FinalizedFailureRegistry,
        homes: &mut std::collections::BTreeMap<
            String,
            cadmpeg_ir::source_fidelity::parts::GivenUpHome,
        >,
    ) -> Result<(), CodecError> {
        ctx.require_source_authority(&self.failure.source)?;
        let home = registry.home(&self.failure.home).ok_or_else(|| {
            CodecError::malformed("unavailable body has no original finalized failure home")
        })?;
        for id in &self.owners {
            if !homes.contains_key(id) {
                let bytes = id
                    .len()
                    .checked_add(size_of::<(
                        String,
                        cadmpeg_ir::source_fidelity::parts::GivenUpHome,
                    )>())
                    .ok_or_else(|| {
                        CodecError::malformed("unavailable body home storage overflows")
                    })?;
                ctx.retain_bytes(
                    u64_from_index(bytes),
                    "FreeCAD unavailable body source home index",
                )?;
                homes.insert(id.clone(), home.clone());
            }
        }
        Ok(())
    }
}

impl UnavailableDeclarationScope {
    pub(super) fn from_given_up(
        ctx: &DecodeContext<'_>,
        source: &crate::xml_framing::XmlSource<'_>,
        index: usize,
        level: &str,
        identity: Option<String>,
        output: &PartOutput,
    ) -> Result<Self, CodecError> {
        let proof = ctx.source_extent_proof(source.extent(index)?)?;
        let home = output
            .fidelity()
            .parts()
            .iter()
            .find(|(_, part)| {
                part.level().as_str() == level
                    && part.state() == &(PartState::GivenUp {})
                    && part.stream() == proof.stream()
                    && part.offset() == proof.range().start
                    && part.byte_len() == proof.range().end - proof.range().start
            })
            .map(|(id, _)| id.as_str())
            .ok_or_else(|| {
                CodecError::malformed("unavailable declaration has no actual failure home")
            })?;
        let bytes = home
            .len()
            .checked_add(identity.as_ref().map_or(0, String::len))
            .ok_or_else(|| {
                CodecError::malformed("declaration failure identity storage overflows")
            })?;
        ctx.retain_bytes(
            u64_from_index(bytes),
            "FreeCAD declaration failure authority",
        )?;
        Ok(Self {
            source: proof,
            home: home.to_owned(),
            identity,
        })
    }

    pub(super) fn sponsor(
        &self,
        ctx: &DecodeContext<'_>,
        output: &mut PartOutput,
        identities: impl IntoIterator<Item = String>,
    ) -> Result<(), CodecError> {
        ctx.require_source_authority(&self.source)?;
        output.drop_with_given_up(ctx, &self.home, identities)
    }
}

pub(super) fn unavailable<'a>(
    scopes: &'a [UnavailableDeclarationScope],
    id: &str,
) -> Option<&'a UnavailableDeclarationScope> {
    scopes.iter().find(|scope| {
        scope
            .identity
            .as_deref()
            .is_none_or(|identity| identity == id)
    })
}

pub(super) fn sponsor(
    ctx: &DecodeContext<'_>,
    scopes: &[UnavailableDeclarationScope],
    output: &mut PartOutput,
    unformed: Vec<Vec<String>>,
    declared: &mut std::collections::BTreeSet<String>,
) -> Result<(), CodecError> {
    for identities in unformed {
        let id = identities.first().ok_or_else(|| {
            CodecError::malformed("unavailable declaration lookup has no actual reference identity")
        })?;
        let scope = unavailable(scopes, id).ok_or_else(|| {
            CodecError::malformed("unavailable declaration lookup lost its source authority")
        })?;
        declared.insert(id.clone());
        scope.sponsor(ctx, output, identities)?;
    }
    Ok(())
}

pub(super) fn reserve<T>(ctx: &DecodeContext<'_>, values: &mut Vec<T>) -> Result<(), CodecError> {
    if values.len() < values.capacity() {
        return Ok(());
    }
    let bytes = u64_from_index(size_of::<T>());
    ctx.retain_bytes(bytes, "FreeCAD declaration source authority slots")?;
    values.try_reserve_exact(1).map_err(|_| {
        cadmpeg_core::decode::ResourceLimit::allocation_failed(
            cadmpeg_core::decode::ResourceDimension::RetainedBytes,
            ctx.policy().limits.max_retained_bytes,
            bytes,
            "FreeCAD declaration source authority slots",
        )
    })?;
    Ok(())
}

pub(super) fn object_identities(name: &str) -> Vec<String> {
    vec![
        crate::native::native_id("object", name),
        crate::native::native_id("application", name),
    ]
}

pub(super) fn decode_unavailable_dependencies<'source>(
    ctx: &DecodeContext<'source>,
    source: &crate::xml_framing::XmlSource<'source>,
    index: usize,
    output: &mut PartOutput,
) -> Result<(), CodecError> {
    source.validate(index)?;
    for child in source
        .children(index)
        .filter(|index| source.name(*index).is_ok_and(|name| name == "Dep"))
    {
        ctx.decode_part(
            output,
            source.extent(child)?,
            source.spec(child, "freecad.object_dep")?,
            |_stage| {
                source.validate(child)?;
                source
                    .peek_attribute(ctx, child, "Name")?
                    .filter(|name| !name.trim().is_empty())
                    .ok_or_else(|| CodecError::malformed("Dep has no Name"))?;
                Ok(())
            },
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
