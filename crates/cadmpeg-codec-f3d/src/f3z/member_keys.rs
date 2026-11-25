// SPDX-License-Identifier: Apache-2.0
//! Definition keys of archive members
//! ([spec §1.5](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#15-multi-document-archives-f3z)).
//!
//! The merge qualifies the records of each member by the scope of the
//! placement it reads the member at. This pass gives each member the key
//! that its archive path states, so that the key of a member does not
//! change with the order or the number of its placements.

use std::collections::{BTreeMap, HashMap, HashSet};

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::annotations::{AnnotationBuilder, StreamHandle};
use cadmpeg_ir::document::{EntityRewrite, Model};
use cadmpeg_ir::ids::{IdentityKey, ProductDefinitionId, UnknownId};
use cadmpeg_ir::products::Storage;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::schema::rewrite::typed::{IdentityMap, TextReplacements};
use cadmpeg_ir::{SourceFidelity, StreamName};

use super::merge::InstanceScopes;
use crate::ids::{member_definition_id, member_definition_key, member_key, SCHEME_PREFIX};

/// The definition key of each placement scope of a member read from a file.
pub(super) struct MemberKeys {
    keys: HashMap<String, String>,
    /// The scopes the merge read a member at.
    read: HashSet<String>,
}

impl MemberKeys {
    /// Reads the members from the merged model, before `own_once` renames
    /// the placing occurrences that `instance_scopes` keys.
    pub(super) fn new(model: &Model, instance_scopes: &InstanceScopes) -> Self {
        let definitions = model
            .product_definitions
            .iter()
            .map(|definition| (definition.id.as_str(), definition))
            .collect::<HashMap<_, _>>();
        let prototypes = model
            .occurrences
            .iter()
            .map(|occurrence| (occurrence.id.as_str(), occurrence.prototype.as_str()))
            .collect::<HashMap<_, _>>();
        let mut keys = HashMap::new();
        let mut read = HashSet::new();
        for (placing, scope) in instance_scopes {
            let Some(scope) = scope.strip_prefix(SCHEME_PREFIX) else {
                continue;
            };
            let Some(definition) = prototypes
                .get(placing.as_str())
                .and_then(|prototype| definitions.get(prototype))
            else {
                continue;
            };
            let Storage::File {
                document,
                source: Some(_),
            } = &definition.storage
            else {
                continue;
            };
            let (Some(path), Some(read_at)) = (
                &document.path,
                member_definition_key(definition.id.as_str()),
            ) else {
                continue;
            };
            let key = IdentityKey::encode_segment(path.as_str())
                .as_str()
                .to_owned();
            keys.insert(scope.to_owned(), key.clone());
            keys.insert(read_at.to_owned(), key);
            read.insert(read_at.to_owned());
        }
        Self { keys, read }
    }

    /// `id` with the key of the member it was read from in place of the
    /// placement scope that qualifies it. A member's `File` definition takes
    /// the member's key, and the key of the member that places it as its
    /// qualifier.
    fn map(&self, id: &str) -> Option<String> {
        if let Some(read_at) = member_definition_key(id) {
            let key = self.keys.get(read_at)?;
            let placing = member_key(id)
                .map(|placing| self.keys.get(placing).map_or(placing, String::as_str));
            return member_definition_id(placing, key)
                .ok()
                .map(|id| id.as_str().to_owned());
        }
        let scope = member_key(id)?;
        let key = self.keys.get(scope)?;
        let alone = &id[SCHEME_PREFIX.len() + scope.len() + 1..];
        Some(format!("{SCHEME_PREFIX}{key}@{alone}"))
    }

    /// `text` with the key of each member in place of each placement scope
    /// that qualifies an id it names.
    fn rekey_text(&self, text: &str) -> Option<String> {
        let mut out = String::new();
        let mut last = 0;
        for (at, _) in text.match_indices(SCHEME_PREFIX) {
            let start = at + SCHEME_PREFIX.len();
            let rest = &text[start..];
            let end = rest
                .find(|c: char| c == '@' || c == ':' || c.is_whitespace())
                .unwrap_or(rest.len());
            if !rest[end..].starts_with('@') {
                continue;
            }
            let Some(key) = self.keys.get(&rest[..end]) else {
                continue;
            };
            out.push_str(&text[last..start]);
            out.push_str(key);
            last = start + end;
        }
        (last != 0).then(|| {
            out.push_str(&text[last..]);
            out
        })
    }

    /// Gives every record read from a member and every `File` definition of
    /// a member its key, also where a loss names the record.
    ///
    /// An occurrence that a later placement of a member holds and whose id
    /// the member's own records already state takes the key suffix
    /// `/instance-<n>`.
    pub(super) fn apply(
        &self,
        ctx: &DecodeContext<'_>,
        ir: &mut cadmpeg_ir::CadIr,
        fidelity: &mut SourceFidelity,
        losses: &mut [LossNote],
    ) -> Result<(), CodecError> {
        if self.keys.is_empty() {
            return Ok(());
        }
        for loss in losses.iter_mut() {
            if let Some(message) = self.rekey_text(&loss.message) {
                loss.message = message;
            }
        }
        let mut taken = HashSet::new();
        let mut mapped = Vec::new();
        for occurrence in &ir.model.occurrences {
            let id = occurrence.id.as_str();
            match self.map(id) {
                Some(target) => {
                    let later = member_key(id).is_none_or(|scope| !self.read.contains(scope));
                    mapped.push((later, id, target));
                }
                None => {
                    taken.insert(id.to_owned());
                }
            }
        }
        mapped.sort_unstable();
        let mut occurrences = HashMap::with_capacity(mapped.len());
        for (_, id, target) in mapped {
            let mut free = target.clone();
            let mut instance = 0usize;
            while !taken.insert(free.clone()) {
                instance += 1;
                free = format!("{target}/instance-{instance}");
            }
            occurrences.insert(id.to_owned(), free);
        }
        let map = |id: &str| occurrences.get(id).cloned().or_else(|| self.map(id));

        super::merge::rescope_partial_pattern_sources(&mut ir.model, &map);
        let mut native_targets = BTreeMap::new();
        if ir.native.namespace("f3d").is_some() {
            let namespace = ir.native.namespace_mut(cadmpeg_ir::codec_id!("f3d"));
            for (arena, records) in namespace.arenas_mut() {
                for record in records {
                    let Some(target) = map(record.id()) else {
                        continue;
                    };
                    native_targets.insert(record.id().to_owned(), target);
                    *record = super::merge::rescope_record(ctx, record, arena.as_str(), &map)?;
                }
            }
        }
        let native_text = TextReplacements::build(native_targets.iter())?;
        let mut rewrite = Rekey {
            native_text: &native_text,
            ctx,
            map: &map,
        };
        let geometry = std::mem::take(&mut ir.geometry);
        ir.geometry.extend_rewritten(geometry, &mut rewrite)?;
        let model = std::mem::take(&mut ir.model);
        ir.model.extend_rewritten(model, &mut rewrite)?;
        crate::scene::rename_bound(ctx, ir, &occurrences)?;

        let files = ir
            .model
            .product_definitions
            .iter()
            .filter(|definition| {
                matches!(
                    definition.storage,
                    Storage::File {
                        source: Some(_),
                        ..
                    }
                )
            })
            .map(|definition| definition.id.clone())
            .collect::<HashSet<_>>();
        self.apply_fidelity(fidelity, &map, &files)
    }

    fn apply_fidelity(
        &self,
        fidelity: &mut SourceFidelity,
        map: &dyn Fn(&str) -> Option<String>,
        files: &HashSet<ProductDefinitionId>,
    ) -> Result<(), CodecError> {
        let mut annotations = std::mem::take(&mut fidelity.annotations);
        annotations
            .map_ids(|id| Ok(map(id).unwrap_or_else(|| id.to_owned())))?
            .map_err(CodecError::from)?;
        let provenance = std::mem::take(&mut annotations.provenance);
        let mut builder = AnnotationBuilder::resume(annotations);
        for (id, provenance) in provenance {
            let stream = self
                .map(provenance.stream())
                .unwrap_or_else(|| provenance.stream().to_owned());
            let stream =
                StreamHandle::new(StreamName::try_from(stream).map_err(CodecError::malformed)?);
            builder.note(&id, &stream, provenance.offset, provenance.tag.as_deref());
        }
        fidelity.annotations = builder.build();

        let moved = fidelity
            .retained_records()
            .iter()
            .filter_map(|(id, record)| {
                let target = map(id.as_str());
                let stream = self.map(record.stream());
                (target.is_some() || stream.is_some()).then(|| (id.clone(), target, stream))
            })
            .collect::<Vec<_>>();
        let mut records = Vec::with_capacity(moved.len());
        for (id, target, stream) in moved {
            let Some(mut record) = fidelity.remove_retained_record(id.as_str()) else {
                continue;
            };
            if let Some(stream) = stream {
                record =
                    record.with_owner(StreamName::try_from(stream).map_err(CodecError::malformed)?);
            }
            let id = match target {
                Some(target) => UnknownId::mint(target).map_err(|error| {
                    CodecError::malformed(format_args!("F3Z retained record {id}: {error}"))
                })?,
                None => id,
            };
            records.push((id, record));
        }
        for (id, record) in records {
            fidelity.insert_retained_record(id, record)?;
        }

        for (definition, digest) in std::mem::take(&mut fidelity.file_digests) {
            let definition = match map(definition.as_str()) {
                Some(target) => ProductDefinitionId::mint(target).map_err(CodecError::malformed)?,
                None => definition,
            };
            if files.contains(&definition) {
                fidelity.file_digests.insert(definition, digest);
            }
        }
        Ok(())
    }
}

/// Rewrites every identity of one entity that a member's scope holds.
struct Rekey<'r, 'a> {
    native_text: &'r TextReplacements<'r>,
    ctx: &'r DecodeContext<'a>,
    map: &'r dyn Fn(&str) -> Option<String>,
}

impl EntityRewrite for Rekey<'_, '_> {
    type Error = CodecError;

    fn rewrite<T: cadmpeg_ir::schema::rewrite::typed::RewriteIdentities>(
        &mut self,
        entity: T,
    ) -> Result<T, CodecError> {
        let mut map =
            IdentityMap::new(|id: &str| Ok((self.map)(id).unwrap_or_else(|| id.to_owned())))
                .with_text_replacements(self.native_text);
        let rewritten = entity.rewrite_identities(self.ctx, &mut map);
        map.finish()?;
        rewritten
    }
}

#[cfg(test)]
mod tests;
