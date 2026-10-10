// SPDX-License-Identifier: Apache-2.0
//! Key comparisons for annotation updates without changing stored string keys.

use std::borrow::Borrow;
use std::cmp::Ordering;
use std::collections::BTreeMap;

use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use cadmpeg_core::CodecError;

trait IdentityKey {
    fn text(&self) -> &str;
    fn admit(&self, _bytes: usize) -> Result<(), CodecError> {
        Ok(())
    }
}

impl IdentityKey for String {
    fn text(&self) -> &str {
        self
    }
}

impl<'key> Borrow<dyn IdentityKey + 'key> for String {
    fn borrow(&self) -> &(dyn IdentityKey + 'key) {
        self
    }
}

impl PartialEq for dyn IdentityKey + '_ {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}
impl Eq for dyn IdentityKey + '_ {}
impl PartialOrd for dyn IdentityKey + '_ {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for dyn IdentityKey + '_ {
    fn cmp(&self, other: &Self) -> Ordering {
        // Stored keys have no admission policy; the query charges each visited
        // chunk. On refusal the read-only search stops inspecting key bytes.
        // The caller checks the fuse before using the returned map slot.
        if self.admit(1).and_then(|()| other.admit(1)).is_err() {
            return Ordering::Equal;
        }
        for (left, right) in self
            .text()
            .as_bytes()
            .chunks(8)
            .zip(other.text().as_bytes().chunks(8))
        {
            let count = left.len().min(right.len());
            if self.admit(count).and_then(|()| other.admit(count)).is_err() {
                return Ordering::Equal;
            }
            let order = left.cmp(right);
            if order != Ordering::Equal {
                return order;
            }
        }
        self.text().len().cmp(&other.text().len())
    }
}

struct IdentityQuery<'key, 'ctx, 'arena> {
    text: &'key str,
    ctx: &'ctx DecodeContext<'arena>,
    operation: &'static str,
}
impl IdentityKey for IdentityQuery<'_, '_, '_> {
    fn text(&self) -> &str {
        self.text
    }
    fn admit(&self, bytes: usize) -> Result<(), CodecError> {
        self.ctx.charge_work(u64_from_index(bytes), self.operation)
    }
}

pub(super) fn get_mut<'map, V>(
    ctx: &DecodeContext<'_>,
    map: &'map mut BTreeMap<String, V>,
    id: &str,
    operation: &'static str,
) -> Result<Option<&'map mut V>, CodecError> {
    ctx.charge_work(0, operation)?;
    let query = IdentityQuery {
        text: id,
        ctx,
        operation,
    };
    let entry = map.get_mut::<dyn IdentityKey>(&query);
    ctx.charge_work(0, operation)?;
    Ok(entry)
}

pub(super) fn admit_insert<V>(
    ctx: &DecodeContext<'_>,
    entries: usize,
    bytes: usize,
    operation: &'static str,
) -> Result<(), CodecError> {
    // Insertion uses the standard String comparator. Bound that single path;
    // the preceding lookup has already charged the comparisons it performed.
    super::admit_identity_work(ctx, entries, bytes, 1, operation)?;
    ctx.admit_btree_node_storage::<String, V>(entries, operation)?;
    ctx.charge_collection_items(1, operation)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadmpeg_core::decode::{DecodeArena, DecodePolicy};

    #[test]
    fn charged_lookup_has_string_order_and_stops_before_mutation_on_refusal() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 9;
        let ctx = DecodeContext::new(&arena, &policy, false);
        let mut map = BTreeMap::from([("abcdefgh-long-source-identity".to_owned(), 1)]);
        assert_eq!(
            get_mut(&ctx, &mut map, "abcdefgz-other", "lookup").unwrap(),
            None
        );
        assert_eq!(ctx.charge_work_limit(1, "measure").unwrap_err().used, 9);
        let original = map.clone();
        assert!(get_mut(&ctx, &mut map, "abcdefgh-long-source-identity", "lookup").is_err());
        assert_eq!(map, original);

        let ctx = DecodeContext::new(&arena, &DecodePolicy::service(), false);
        let mut map = BTreeMap::from([
            ("a".to_owned(), 1),
            ("abcdefgh".to_owned(), 2),
            ("abcdefgh-more".to_owned(), 3),
            ("z".to_owned(), 4),
        ]);
        for id in ["", "a", "abc", "abcdefgh", "abcdefgh-more", "z", "zz"] {
            let expected = map.get(id).copied();
            assert_eq!(
                get_mut(&ctx, &mut map, id, "lookup").unwrap().copied(),
                expected
            );
        }
    }
    #[test]
    fn repeated_provenance_update_charges_one_search_and_the_owned_key_copy() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 650_000;
        let ctx = DecodeContext::new(&arena, &policy, false);
        let stream =
            super::super::StreamHandle::new(&ctx, crate::stream_name!("source"), "source").unwrap();
        let id = format!("test:model:point#{}", "x".repeat(240));
        let mut builder = super::super::AnnotationBuilder::default();
        for offset in 0..1000 {
            let key = ctx.copy_retained_text(&id, "owned update key").unwrap();
            builder
                .note_owned(&ctx, key, &stream, offset, None)
                .unwrap();
        }
        assert_eq!(builder.annotations().provenance.len(), 1);
        let annotations = builder.build();
        assert_eq!(annotations.provenance[&id].offset, 999);
        ctx.finish_session().unwrap();
    }
}
