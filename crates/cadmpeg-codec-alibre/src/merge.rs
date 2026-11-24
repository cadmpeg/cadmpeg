// SPDX-License-Identifier: Apache-2.0
//! Merges the decode of a component file into the decode of its assembly.
//!
//! Every identity of the component moves under `alibre:xref/<scope>/`, where
//! the scope is one component file of the assembly. The scope keeps two files
//! that each hold `alibre:brep:entity#0` apart, and a component that an
//! assembly places twice is decoded and merged once.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::annotations::{AnnotationBuilder, StreamHandle};
use cadmpeg_ir::document::EntityRewrite;
use cadmpeg_ir::hash::digest::Sha256Digest;
use cadmpeg_ir::ids::{Identity, UnknownId};
use cadmpeg_ir::products::{ExternalDocument, ProductDefinitionKind, Storage};
use cadmpeg_ir::schema::rewrite::typed::{IdentityMap, RewriteIdentities};
use cadmpeg_ir::{CadIr, NativeRecord, SourceFidelity};
use serde_json::Value;

use crate::FORMAT;

/// The identity of `text` inside `scope`; `None` for text that is not an
/// Alibre identity.
fn rescope(text: &str, scope: &str) -> Option<String> {
    text.strip_prefix(FORMAT)?
        .strip_prefix(':')
        .map(|rest| format!("{FORMAT}:xref/{scope}/{rest}"))
}

/// Moves every Alibre identity of an entity into one scope.
struct Scope<'r, 'a> {
    ctx: &'r DecodeContext<'a>,
    scope: &'r str,
}

impl EntityRewrite for Scope<'_, '_> {
    type Error = CodecError;

    fn rewrite<T: RewriteIdentities>(&mut self, entity: T) -> Result<T, CodecError> {
        let mut map = IdentityMap::new(|id: &str| {
            Ok(rescope(id, self.scope).unwrap_or_else(|| id.to_owned()))
        });
        let rewritten = entity.rewrite_identities(self.ctx, &mut map);
        map.finish()?;
        rewritten
    }
}

/// Rewrites every text of a JSON value that is an Alibre identity.
fn rescope_value(value: &mut Value, scope: &str) {
    match value {
        Value::String(text) => {
            if let Some(scoped) = rescope(text, scope) {
                *text = scoped;
            }
        }
        Value::Array(items) => items.iter_mut().for_each(|item| rescope_value(item, scope)),
        Value::Object(members) => members
            .values_mut()
            .for_each(|member| rescope_value(member, scope)),
        _ => {}
    }
}

fn rescope_record(record: &NativeRecord, scope: &str) -> Result<NativeRecord, CodecError> {
    let mut fields = record.fields().map_err(CodecError::from)?;
    for value in fields.values_mut() {
        rescope_value(value, scope);
    }
    let id = rescope(record.id(), scope).unwrap_or_else(|| record.id().to_owned());
    NativeRecord::new(Identity::new(id).map_err(CodecError::malformed)?, fields)
        .map_err(CodecError::from)
}

fn rescope_fidelity(
    source: SourceFidelity,
    scope: &str,
    decode_digest: Sha256Digest,
) -> Result<SourceFidelity, CodecError> {
    let (mut annotations, records) = source.into_parts();
    annotations
        .map_ids(|id| Ok(rescope(id, scope).unwrap_or_else(|| id.to_owned())))?
        .map_err(CodecError::from)?;
    // The scope is one stream component, so a stream name keeps the file it
    // belongs to.
    let owner = format!("{FORMAT}:xref/{scope}/");
    let provenance = std::mem::take(&mut annotations.provenance);
    let mut builder = AnnotationBuilder::resume(annotations);
    for (id, provenance) in provenance {
        let stream = cadmpeg_ir::StreamName::try_from(format!("{owner}{}", provenance.stream()))
            .map_err(CodecError::malformed)?;
        builder.note(
            &id,
            &StreamHandle::new(stream),
            provenance.offset,
            provenance.tag.as_deref(),
        );
    }
    let mut rescoped = SourceFidelity::with_annotations(builder.build(), decode_digest);
    for (id, record) in records {
        let scoped = rescope(id.as_str(), scope).unwrap_or_else(|| id.to_string());
        let id = UnknownId::mint(scoped).map_err(|error| {
            CodecError::malformed(format_args!("Alibre retained record {id}: {error}"))
        })?;
        let stream = cadmpeg_ir::StreamName::try_from(format!("{owner}{}", record.stream()))
            .map_err(CodecError::malformed)?;
        rescoped.insert_retained_record(id, record.with_owner(stream))?;
    }
    Ok(rescoped)
}

/// What a merged component leaves for the assembly to place.
pub(crate) struct Merged {
    /// The product definition the component became.
    pub(crate) definition: cadmpeg_ir::ids::ProductDefinitionId,
    /// Whether the component carries decoded geometry.
    pub(crate) geometry_transferred: bool,
    /// The losses of the component decode, stated for the assembly.
    pub(crate) losses: Vec<cadmpeg_ir::report::loss::LossNote>,
    /// Whether the component holds design records that the merge leaves out.
    pub(crate) design_left_out: bool,
}

/// Merges a decoded component into `parent` under `scope`.
///
/// The component's root definition becomes the definition that the
/// assembly's occurrences place; `document` names the file it came from.
pub(crate) fn merge_component(
    ctx: &DecodeContext<'_>,
    parent: &mut CadIr,
    fidelity: &mut SourceFidelity,
    child: cadmpeg_ir::codec::Decoded,
    scope: &str,
    document: ExternalDocument,
) -> Result<Merged, CodecError> {
    let cadmpeg_ir::codec::Decoded {
        ir: mut child_ir,
        body,
        source_fidelity,
    } = child;
    let design_left_out = child_ir.design.entity_count() > 0 || child_ir.process.entity_count() > 0;
    let source = child_ir.source.clone();
    let Some(root) = child_ir
        .model
        .product_definitions
        .iter_mut()
        .find(|definition| definition.is_root())
    else {
        return Err(CodecError::malformed(
            "an Alibre component decode states no root definition",
        ));
    };
    let definition = root.id.clone();
    root.storage = Storage::File { document, source };
    if root.kind.is_none() {
        root.kind = Some(ProductDefinitionKind::Part);
    }
    let mut rewrite = Scope { ctx, scope };
    parent
        .geometry
        .extend_rewritten(child_ir.geometry, &mut rewrite)?;
    parent
        .model
        .extend_rewritten(child_ir.model, &mut rewrite)?;
    if let Some(mut namespace) = child_ir.native.0.remove("alibre") {
        let target = parent.native.namespace_mut(cadmpeg_ir::codec_id!("alibre"));
        for (name, records) in std::mem::take(namespace.arenas_mut()) {
            let arena = target.arenas_mut().entry(name).or_default();
            for record in &records {
                ctx.charge_entities(1, "merge Alibre component native record")?;
                arena.push(rescope_record(record, scope)?);
            }
        }
    }
    let decode_digest = fidelity.decode_digest.clone();
    fidelity.append(rescope_fidelity(source_fidelity, scope, decode_digest)?)?;
    let definition = rescope(definition.as_str(), scope).ok_or_else(|| {
        CodecError::malformed("an Alibre root definition is not an Alibre identity")
    })?;
    Ok(Merged {
        definition: cadmpeg_ir::ids::ProductDefinitionId::mint(definition)
            .map_err(CodecError::malformed)?,
        geometry_transferred: body.transfer.geometry_transferred(),
        losses: body.losses,
        design_left_out,
    })
}

#[cfg(test)]
mod tests {
    use super::rescope;

    #[test]
    fn only_alibre_identities_move_into_a_scope() {
        assert_eq!(
            rescope("alibre:brep:entity#0", "m0").as_deref(),
            Some("alibre:xref/m0/brep:entity#0")
        );
        assert_eq!(
            rescope("alibre:xref/m0/brep:entity#0", "m1").as_deref(),
            Some("alibre:xref/m1/xref/m0/brep:entity#0")
        );
        assert_eq!(rescope("alibrex:brep#0", "m0"), None);
        assert_eq!(rescope("acis:save-format", "m0"), None);
    }
}
