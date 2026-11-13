use super::MeshProjection;
use crate::container::ContainerScan;
use crate::layout::indexed_design_record_header as header;
use crate::layout::paramesh_flagged_feature_scope_prefix as flagged;
use crate::loss::F3dLossCode;
use crate::records::feature::scope::DesignParameterScope;
use cadmpeg_core::decode::{DecodeContext, ParseError, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::mesh::MeshSelection;
use cadmpeg_ir::features::{
    Feature, FeatureDefinition, FeatureOperation, NativeSelections, SelectionReference,
};
use cadmpeg_ir::report::loss::LossNote;

#[derive(Debug)]
enum ImportError {
    Parse(ParseError),
    Framing,
    Codec(CodecError),
}

impl From<ParseError> for ImportError {
    fn from(error: ParseError) -> Self {
        Self::Parse(error)
    }
}

impl std::fmt::Display for ImportError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Parse(error) => write!(formatter, "{error:?}"),
            Self::Framing => formatter.write_str("invalid mesh body-reference prefix"),
            Self::Codec(error) => write!(formatter, "{error}"),
        }
    }
}

struct ImportBodies {
    prefix_word: Option<u32>,
    bodies: Vec<u32>,
}

fn read_bodies(ctx: &DecodeContext<'_>, mut view: View<'_>) -> Result<ImportBodies, ImportError> {
    view.req_take(header::LEN)?;
    if view
        .req_take(flagged::FLAG - flagged::ZERO_RUN_9)?
        .iter()
        .any(|byte| *byte != 0)
    {
        return Err(ImportError::Framing);
    }
    let prefix_word = match view.req_u8()? {
        0 => None,
        1 => Some(view.req_u32_le()?),
        _ => return Err(ImportError::Framing),
    };
    let declared = view.req_u32_le()?;
    let count = view
        .counted(u64::from(declared), header::LEN)
        .ok_or(ImportError::Framing)?;
    let mut bodies = ctx
        .alloc_filled(count.get(), 0, "F3D mesh import body references")
        .map_err(ImportError::Codec)?;
    for body in &mut bodies {
        if view.req_u8()? != 1 {
            return Err(ImportError::Framing);
        }
        let target = view.req_u64_le()?;
        let tail = view.req_take(2)?;
        if target == 0 || tail.iter().any(|byte| *byte != 0) {
            return Err(ImportError::Framing);
        }
        *body = u32::try_from(target).map_err(|_| ImportError::Framing)?;
    }
    Ok(ImportBodies {
        prefix_word,
        bodies,
    })
}

fn scope_view<'a>(scan: &ContainerScan<'a>, scope: &DesignParameterScope) -> Option<View<'a>> {
    let stream = crate::ids::native_stream(&scope.id)?.strip_prefix("f3d:")?;
    let entry = crate::ids::decode_identity_key_component(stream)?;
    let view = scan.entry_view(&entry)?;
    let start = view
        .start()
        .checked_add(usize::try_from(scope.byte_offset()).ok()?)?;
    let end = start.checked_add(usize::try_from(scope.frame_length()).ok()?)?;
    view.child(start, end)
}

fn native_source(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan<'_>,
    feature: &mut Feature,
    scope: Option<&DesignParameterScope>,
    losses: &mut Vec<LossNote>,
) -> Result<MeshSelection, CodecError> {
    let Some((source, scope)) = feature
        .native_ref
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .zip(scope)
    else {
        losses.push(F3dLossCode::FeatureMeshImportSourceUnparsed.note(format!(
            "Mesh import {} retains its originating feature identity because its native scope source is unavailable.", feature.id
        )));
        return Ok(MeshSelection::Unresolved {
            feature: feature.id.clone(),
        });
    };
    let source = source.to_owned();
    let parsed = scope_view(scan, scope).map(|view| read_bodies(ctx, view));
    if let Some(Ok(ImportBodies {
        prefix_word: Some(word),
        ..
    })) = &parsed
    {
        feature.source_properties.insert(
            cadmpeg_core::nonblank_literal!("mesh-scope-prefix-word"),
            word.to_string(),
        );
        losses.push(F3dLossCode::MeshScopePrefixUninterpreted.note(format!(
            "Mesh import {source} retains prefix word {word} in its source properties."
        )));
    }
    let references = match parsed {
        Some(Ok(import)) if !import.bodies.is_empty() => {
            let mut references = ctx.alloc_filled(
                import.bodies.len(),
                String::new(),
                "F3D native mesh import selections",
            )?;
            for (ordinal, (body, reference)) in
                import.bodies.iter().zip(&mut references).enumerate()
            {
                *reference = body.to_string();
                feature.source_properties.insert(
                    cadmpeg_core::nonblank_literal!("mesh-body-reference:{}", ordinal),
                    body.to_string(),
                );
            }
            losses.push(F3dLossCode::FeatureMeshImportTessellationsUnresolved.note(format!(
                "Mesh import {source} retains decoded body records {:?}; their tessellations remain unresolved.", import.bodies
            )));
            Some(references)
        }
        Some(Ok(_)) => {
            feature.source_properties.insert(
                cadmpeg_core::nonblank_literal!("mesh-body-count"),
                "0".into(),
            );
            losses.push(F3dLossCode::FeatureMeshImportEmptySourceAssumed.note(format!(
                "Mesh import {source} stores an empty body-reference run; its scope identifies the unresolved import source."
            )));
            None
        }
        Some(Err(ImportError::Codec(error))) => return Err(error),
        Some(Err(error)) => {
            losses.push(F3dLossCode::FeatureMeshImportSourceUnparsed.note(format!(
                "Mesh import {source} retains its native scope source because its body-reference prefix is unresolved: {error}."
            )));
            None
        }
        None => {
            losses.push(F3dLossCode::FeatureMeshImportSourceUnparsed.note(format!(
                "Mesh import {source} retains its native source because its scope frame is unavailable."
            )));
            None
        }
    };
    let members = match references.map(NativeSelections::try_from).transpose() {
        Ok(members) => members,
        Err(error) => {
            losses.push(F3dLossCode::FeatureMeshImportSourceUnparsed.note(format!(
                "Mesh import {source} keeps its decoded body-reference properties and native scope source because its selection is invalid: {error}."
            )));
            None
        }
    };
    let native = SelectionReference::try_from(source).map_err(CodecError::malformed)?;
    Ok(MeshSelection::Native { native, members })
}

pub(super) fn bind_features(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan<'_>,
    features: &mut [Feature],
    scopes: &[DesignParameterScope],
    projection: &MeshProjection,
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    for feature in features {
        if feature.source_tag.as_deref() != Some("Base Mesh Feature") {
            continue;
        }
        let scope = feature
            .native_ref
            .as_deref()
            .and_then(|reference| scopes.iter().find(|scope| scope.id == reference));
        let tessellations = scope
            .and_then(|scope| {
                let stream =
                    crate::ids::native_stream(&scope.id).unwrap_or(crate::ids::DEFAULT_STREAM);
                projection
                    .tessellations_by_scope
                    .get(&(stream.to_owned(), scope.record_index))
            })
            .filter(|values| !values.is_empty());
        let tessellations = match tessellations {
            Some(values) => {
                let first = cadmpeg_ir::tessellation::TessellationId::mint(values[0].clone())
                    .map_err(CodecError::malformed)?;
                let mut identities =
                    ctx.alloc_filled(values.len(), first, "F3D mesh import tessellations")?;
                for (value, identity) in values.iter().zip(&mut identities) {
                    *identity = cadmpeg_ir::tessellation::TessellationId::mint(value.clone())
                        .map_err(CodecError::malformed)?;
                }
                MeshSelection::Tessellations(identities.try_into().map_err(CodecError::malformed)?)
            }
            None => native_source(ctx, scan, feature, scope, losses)?,
        };
        feature
            .evaluation
            .set_definition(FeatureDefinition::Operation(FeatureOperation::MeshImport {
                tessellations,
            }));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
