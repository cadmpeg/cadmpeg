// SPDX-License-Identifier: Apache-2.0
//! Merges the decode of a component file into the decode of its assembly.
//!
//! A component file has a definition key, `m<n>` for the n-th file an
//! assembly merges and `<placing key>/m<n>` for a file that only a
//! subassembly places, so two files never share a key. Its root definition
//! becomes the `File` definition `alibre:xref:product-definition#<key>`, and
//! every other identity and stream name of the component takes `<key>@` at
//! the head of its scope (CADIR §2.2), which keeps two files that each hold
//! `alibre:brep:entity#0` apart. A component that an assembly places twice
//! is decoded and merged once.

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

/// The kind of a component file's `File` definition, after its scope.
const FILE_KIND: &str = ":product-definition#";
/// The scope and kind of a component file's `File` definition.
const FILE_DEFINITION: &str = "xref:product-definition#";

/// How the identities of one component file move into its assembly.
#[derive(Clone, Copy)]
struct Keyed<'k> {
    /// The definition key of the component file.
    key: &'k str,
    /// The id of the component's root definition, which becomes its `File`
    /// definition.
    root: Option<&'k str>,
}

impl Keyed<'_> {
    /// The id or stream name of `text` in the assembly; `None` for text that
    /// is not an Alibre identity.
    fn rescope(self, text: &str) -> Option<String> {
        let rest = text.strip_prefix(FORMAT)?.strip_prefix(':')?;
        let key = self.key;
        if self.root == Some(text) {
            return Some(format!("{FORMAT}:{FILE_DEFINITION}{key}"));
        }
        let scope_end = rest.find(':').unwrap_or(rest.len());
        Some(match rest[..scope_end].split_once('@') {
            // A record of a file that this component places: the key of that
            // file grows by this component's key, and so does the key of the
            // file's own `File` definition.
            Some((qualifier, scope)) => {
                let tail = &rest[scope_end..];
                let tail = match tail.strip_prefix(FILE_KIND) {
                    Some(nested) if scope == "xref" => {
                        format!("{}{key}/{nested}", FILE_KIND)
                    }
                    _ => tail.to_owned(),
                };
                format!("{FORMAT}:{key}/{qualifier}@{scope}{tail}")
            }
            None => match rest.strip_prefix(FILE_DEFINITION) {
                // The `File` definition of a file that this component places
                // is a record of this component.
                Some(nested) => format!("{FORMAT}:{key}@{FILE_DEFINITION}{key}/{nested}"),
                None => format!("{FORMAT}:{key}@{rest}"),
            },
        })
    }

    /// A stream name of the component in the assembly.
    fn stream(self, name: &str) -> String {
        match self.rescope(name) {
            Some(scoped) => scoped,
            None => format!("{FORMAT}:{}@{name}", self.key),
        }
    }
}

/// Moves every Alibre identity of an entity into one scope.
struct Scope<'r, 'a> {
    ctx: &'r DecodeContext<'a>,
    keyed: Keyed<'r>,
}

impl EntityRewrite for Scope<'_, '_> {
    type Error = CodecError;

    fn rewrite<T: RewriteIdentities>(&mut self, entity: T) -> Result<T, CodecError> {
        let mut map = IdentityMap::new(|id: &str| {
            Ok(self.keyed.rescope(id).unwrap_or_else(|| id.to_owned()))
        });
        let rewritten = entity.rewrite_identities(self.ctx, &mut map);
        map.finish()?;
        rewritten
    }
}

/// Rewrites every text of a JSON value that is an Alibre identity.
fn rescope_value(value: &mut Value, keyed: Keyed<'_>) {
    match value {
        Value::String(text) => {
            if let Some(scoped) = keyed.rescope(text) {
                *text = scoped;
            }
        }
        Value::Array(items) => items.iter_mut().for_each(|item| rescope_value(item, keyed)),
        Value::Object(members) => members
            .values_mut()
            .for_each(|member| rescope_value(member, keyed)),
        _ => {}
    }
}

fn rescope_record(record: &NativeRecord, keyed: Keyed<'_>) -> Result<NativeRecord, CodecError> {
    let mut fields = record.fields().map_err(CodecError::from)?;
    for value in fields.values_mut() {
        rescope_value(value, keyed);
    }
    let id = keyed
        .rescope(record.id())
        .unwrap_or_else(|| record.id().to_owned());
    NativeRecord::new(Identity::new(id).map_err(CodecError::malformed)?, fields)
        .map_err(CodecError::from)
}

fn rescope_fidelity(
    source: SourceFidelity,
    keyed: Keyed<'_>,
    decode_digest: Sha256Digest,
) -> Result<SourceFidelity, CodecError> {
    let (mut annotations, records) = source.into_parts();
    annotations
        .map_ids(|id| Ok(keyed.rescope(id).unwrap_or_else(|| id.to_owned())))?
        .map_err(CodecError::from)?;
    // A stream name keeps the file it belongs to: `alibre:SAB_DATA` of the
    // component keyed `m0` reads `alibre:m0@SAB_DATA`, and of a file that
    // the component keyed `m1` places, `alibre:m1/m0@SAB_DATA`.
    let scoped_stream = |name: &str| keyed.stream(name);
    let provenance = std::mem::take(&mut annotations.provenance);
    let mut builder = AnnotationBuilder::resume(annotations);
    for (id, provenance) in provenance {
        let stream = cadmpeg_ir::StreamName::try_from(scoped_stream(provenance.stream()))
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
        let scoped = keyed.rescope(id.as_str()).unwrap_or_else(|| id.to_string());
        let id = UnknownId::mint(scoped).map_err(|error| {
            CodecError::malformed(format_args!("Alibre retained record {id}: {error}"))
        })?;
        let stream = cadmpeg_ir::StreamName::try_from(scoped_stream(record.stream()))
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
    /// The document tolerances of the component when they differ from the
    /// tolerances of the assembly, which the assembly keeps.
    pub(crate) tolerances_left_out: Option<cadmpeg_ir::units::Tolerances>,
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
    key: &str,
    document: ExternalDocument,
) -> Result<Merged, CodecError> {
    let cadmpeg_ir::codec::Decoded {
        ir: mut child_ir,
        body,
        source_fidelity,
    } = child;
    let design_left_out = child_ir.design.entity_count() > 0 || child_ir.process.entity_count() > 0;
    let tolerances_left_out =
        (child_ir.tolerances != parent.tolerances).then_some(child_ir.tolerances);
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
    let root_id = definition.as_str().to_owned();
    let keyed = Keyed {
        key,
        root: Some(&root_id),
    };
    let mut rewrite = Scope { ctx, keyed };
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
                arena.push(rescope_record(record, keyed)?);
            }
        }
    }
    let decode_digest = fidelity.decode_digest.clone();
    fidelity.append(rescope_fidelity(source_fidelity, keyed, decode_digest)?)?;
    let definition = keyed.rescope(definition.as_str()).ok_or_else(|| {
        CodecError::malformed("an Alibre root definition is not an Alibre identity")
    })?;
    Ok(Merged {
        definition: cadmpeg_ir::ids::ProductDefinitionId::mint(definition)
            .map_err(CodecError::malformed)?,
        geometry_transferred: body.transfer.geometry_transferred(),
        losses: body.losses,
        design_left_out,
        tolerances_left_out,
    })
}

#[cfg(test)]
mod tests {
    use super::Keyed;

    const ROOT: &str = "alibre:model:product-definition#root";

    fn keyed(key: &str) -> Keyed<'_> {
        Keyed {
            key,
            root: Some(ROOT),
        }
    }

    #[test]
    fn a_record_of_a_component_takes_its_key_at_the_head_of_its_scope() {
        assert_eq!(
            keyed("m0").rescope("alibre:brep:entity#0").as_deref(),
            Some("alibre:m0@brep:entity#0")
        );
        assert_eq!(keyed("m0").stream("alibre:SAB_DATA"), "alibre:m0@SAB_DATA");
        assert_eq!(keyed("m0").stream("SAB_DATA"), "alibre:m0@SAB_DATA");
    }

    #[test]
    fn the_root_definition_becomes_the_file_definition_of_its_key() {
        assert_eq!(
            keyed("m1").rescope(ROOT).as_deref(),
            Some("alibre:xref:product-definition#m1")
        );
    }

    #[test]
    fn a_file_that_a_subassembly_places_is_keyed_below_it() {
        // In the subassembly's own decode, the file it places is `m0`.
        assert_eq!(
            keyed("m1").rescope("alibre:m0@brep:entity#0").as_deref(),
            Some("alibre:m1/m0@brep:entity#0")
        );
        assert_eq!(
            keyed("m1")
                .rescope("alibre:xref:product-definition#m0")
                .as_deref(),
            Some("alibre:m1@xref:product-definition#m1/m0")
        );
        assert_eq!(
            keyed("m2")
                .rescope("alibre:m1@xref:product-definition#m1/m0")
                .as_deref(),
            Some("alibre:m2/m1@xref:product-definition#m2/m1/m0")
        );
        assert_eq!(
            keyed("m1").stream("alibre:m0@SAB_DATA"),
            "alibre:m1/m0@SAB_DATA"
        );
    }

    #[test]
    fn only_alibre_identities_move() {
        assert_eq!(keyed("m0").rescope("alibrex:brep#0"), None);
        assert_eq!(keyed("m0").rescope("acis:save-format"), None);
    }
}
