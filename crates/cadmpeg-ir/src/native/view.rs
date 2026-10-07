// SPDX-License-Identifier: Apache-2.0
//! Borrowed product identities and links during staged source admission.

use super::NativeRecord;
use crate::document::CadIr;
use crate::unknown::UnknownRecord;

#[derive(Clone, Copy)]
pub(crate) enum NativeEntity<'a> {
    Product(&'a NativeRecord),
    Source(&'a UnknownRecord),
}

impl<'a> NativeEntity<'a> {
    pub(crate) fn id(self) -> &'a str {
        match self {
            Self::Product(record) => record.id(),
            Self::Source(record) => record.id().as_str(),
        }
    }

    pub(crate) fn links(
        self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<impl Iterator<Item = &'a str>, cadmpeg_core::CodecError> {
        let product = match self {
            Self::Product(record) => ctx
                .find_map(
                    record.fields(),
                    |(key, value)| {
                        let order = crate::ids::comparison::compare(
                            ctx,
                            key,
                            "links",
                            "native link field comparison",
                        )?;
                        Ok(match order {
                            std::cmp::Ordering::Less => None,
                            std::cmp::Ordering::Equal => Some(Some(value)),
                            std::cmp::Ordering::Greater => Some(None),
                        })
                    },
                    "native link field lookup",
                )?
                .flatten()
                .and_then(serde_json::Value::as_array),
            Self::Source(_) => None,
        };
        let source = match self {
            Self::Source(record) => record.links(),
            Self::Product(_) => &[],
        };
        Ok(ctx
            .admit_iter(
                product.map_or(&[][..], Vec::as_slice),
                "native outgoing link scan",
            )?
            .filter_map(serde_json::Value::as_str)
            .chain(
                ctx.admit_iter(source, "native outgoing link scan")?
                    .map(String::as_str),
            ))
    }
}

#[derive(Clone, Copy)]
pub(crate) enum NativeArena<'a> {
    Product(&'a [NativeRecord]),
    Source(&'a [UnknownRecord], &'a [usize]),
}

impl<'a> NativeArena<'a> {
    pub(crate) fn records<S: crate::index::IndexStorage>(
        self,
        storage: &S,
        operation: &'static str,
    ) -> Result<impl Iterator<Item = NativeEntity<'a>>, S::Error> {
        let (products, sources, order): (&[NativeRecord], &[UnknownRecord], &[usize]) = match self {
            Self::Product(records) => (records, &[], &[]),
            Self::Source(records, order) => (&[], records, order),
        };
        Ok(storage
            .admit_iter(products, operation)?
            .map(NativeEntity::Product)
            .chain(
                storage
                    .admit_iter(order, operation)?
                    .map(move |index| NativeEntity::Source(&sources[*index])),
            ))
    }

    pub(crate) fn len(self) -> usize {
        match self {
            Self::Product(records) => records.len(),
            Self::Source(_, order) => order.len(),
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct NativeView<'a> {
    pub(crate) ir: &'a CadIr,
    pub(crate) unknowns: Option<(&'a str, &'a [UnknownRecord], &'a [usize])>,
}

impl<'a> NativeView<'a> {
    pub(crate) fn new(
        ir: &'a CadIr,
        unknowns: Option<(&'a str, &'a [UnknownRecord], &'a [usize])>,
    ) -> Self {
        Self { ir, unknowns }
    }

    /// Visit native arenas in map order, replacing one unknown arena by source facts.
    pub(crate) fn visit<S: crate::index::IndexStorage, E: From<S::Error>>(
        self,
        storage: &S,
        operation: &'static str,
        mut visit: impl FnMut(&'a str, &'a str, NativeArena<'a>) -> Result<(), E>,
    ) -> Result<(), E> {
        let mut pending = self.unknowns;
        for (format, namespace) in storage.admit_iter(&self.ir.native.0, operation)? {
            let order = if let Some((replacement, _, _)) = pending {
                Some(storage.compare(replacement, format.as_str(), operation)?)
            } else {
                None
            };
            if order == Some(std::cmp::Ordering::Less) {
                if let Some((replacement, records, indices)) = pending.take() {
                    visit(
                        replacement,
                        "unknowns",
                        NativeArena::Source(records, indices),
                    )?;
                }
            }
            let replacing = order == Some(std::cmp::Ordering::Equal);
            for (arena, records) in storage.admit_iter(namespace.arenas(), operation)? {
                if replacing && pending.is_some() {
                    let order = storage.compare("unknowns", arena.as_str(), operation)?;
                    if order != std::cmp::Ordering::Greater {
                        if let Some((replacement, sources, indices)) = pending.take() {
                            visit(
                                replacement,
                                "unknowns",
                                NativeArena::Source(sources, indices),
                            )?;
                        }
                    }
                    if order == std::cmp::Ordering::Equal {
                        continue;
                    }
                }
                visit(format, arena, NativeArena::Product(records))?;
            }
            if replacing {
                if let Some((replacement, sources, indices)) = pending.take() {
                    visit(
                        replacement,
                        "unknowns",
                        NativeArena::Source(sources, indices),
                    )?;
                }
            }
        }
        if let Some((format, records, order)) = pending {
            visit(format, "unknowns", NativeArena::Source(records, order))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{NativeArena, NativeEntity, NativeView};
    use crate::document::CadIr;
    use crate::index::PublicStorage;
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    #[test]
    fn native_view_replacement_preserves_order_and_admits_before_visiting() {
        let mut ir = CadIr::empty();
        for format in ["b", "m", "z"] {
            for arena in ["a", "unknowns", "z"] {
                ir.native
                    .namespace_mut(format)
                    .arenas_mut()
                    .insert(arena.into(), Vec::new());
            }
        }
        for replacement in ["a", "b", "l", "m", "y", "z", "zz"] {
            let view = NativeView::new(&ir, Some((replacement, &[], &[])));
            let mut expected = Vec::new();
            view.visit(
                &PublicStorage,
                "native view fixture",
                |format, arena, records| {
                    expected.push((format, arena, matches!(records, NativeArena::Source(_, _))));
                    Ok::<_, std::convert::Infallible>(())
                },
            )
            .unwrap();
            assert!(expected
                .windows(2)
                .all(|pair| (pair[0].0, pair[0].1) < (pair[1].0, pair[1].1)));
            assert_eq!(
                expected
                    .iter()
                    .filter(|row| row.2)
                    .copied()
                    .collect::<Vec<_>>(),
                [(replacement, "unknowns", true)]
            );
            assert_eq!(
                expected.len(),
                if ["b", "m", "z"].contains(&replacement) {
                    9
                } else {
                    10
                }
            );
            let mut policy = DecodePolicy::service();
            policy.limits.max_retained_bytes = 0;
            policy.limits.max_materialized_bytes = 0;
            policy.limits.max_collection_items = 0;
            let arena = DecodeArena::new();
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let mut decoded = Vec::new();
            view.visit(
                &crate::index::DecodeStorage(&ctx),
                "native view fixture",
                |format, arena, records| {
                    decoded.push((format, arena, matches!(records, NativeArena::Source(_, _))));
                    Ok::<_, CodecError>(())
                },
            )
            .unwrap();
            assert_eq!(decoded, expected);
            ctx.finish_session().unwrap();
            cadmpeg_test_support::refusal::resource_limit_at(
                ResourceDimension::WorkUnits,
                "native view fixture",
                |cap| {
                    let arena = DecodeArena::new();
                    let mut policy = policy.clone();
                    policy.limits.max_work_units = cap;
                    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)?;
                    let mut visited = Vec::new();
                    let result = view.visit(
                        &crate::index::DecodeStorage(&ctx),
                        "native view fixture",
                        |format, arena, _| {
                            visited.push((format, arena));
                            Ok::<_, CodecError>(())
                        },
                    );
                    if let Err(CodecError::ResourceLimit(limit)) = result {
                        assert!(visited.is_empty());
                        assert!(
                            matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit)
                        );
                        return Err(limit.into());
                    }
                    result
                },
            );
        }
    }

    #[test]
    fn native_links_admit_arrays_and_keep_only_string_targets() {
        let record = super::NativeRecord::new(
            crate::ids::Identity::new("test:native:record#links").unwrap(),
            serde_json::Map::from_iter([
                ("a".into(), serde_json::Value::Null),
                ("links".into(), serde_json::json!(["first", 7, "last"])),
                ("z".into(), serde_json::Value::Null),
            ]),
        )
        .unwrap();
        let ctx = cadmpeg_test_support::service_decode_context();
        assert_eq!(
            NativeEntity::Product(&record)
                .links(&ctx)
                .unwrap()
                .collect::<Vec<_>>(),
            ["first", "last"]
        );
        for operation in [
            "native link field lookup",
            "native link field comparison",
            "native outgoing link scan",
        ] {
            cadmpeg_test_support::refusal::resource_limit_at(
                ResourceDimension::WorkUnits,
                operation,
                |cap| {
                    let mut policy = DecodePolicy::service();
                    policy.limits.max_work_units = cap;
                    let arena = DecodeArena::new();
                    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)?;
                    let result = NativeEntity::Product(&record).links(&ctx).map(|links| {
                        links.for_each(|_| ());
                    });
                    match result {
                        Err(CodecError::ResourceLimit(limit)) => {
                            assert!(
                                matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit)
                            );
                            Err(limit.into())
                        }
                        Err(error) => Err(error),
                        Ok(()) => Ok(()),
                    }
                },
            );
        }
    }
}
