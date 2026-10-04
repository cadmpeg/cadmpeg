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

    pub(crate) fn links<'ctx>(
        self,
        ctx: &'ctx cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<
        impl Iterator<Item = Result<&'a str, cadmpeg_core::CodecError>> + 'ctx,
        cadmpeg_core::CodecError,
    >
    where
        'a: 'ctx,
    {
        let product = match self {
            Self::Product(record) => {
                for _ in 0..=record.fields().len() {
                    ctx.charge_work(6, "native link field lookup")?;
                }
                record
                    .fields()
                    .get("links")
                    .and_then(serde_json::Value::as_array)
            }
            Self::Source(_) => None,
        };
        let source = match self {
            Self::Source(record) => record.links(),
            Self::Product(_) => &[],
        };
        Ok(product
            .into_iter()
            .flatten()
            .filter_map(
                move |value| match ctx.charge_work(1, "native outgoing link scan") {
                    Err(error) => Some(Err(error)),
                    Ok(()) => value.as_str().map(Ok),
                },
            )
            .chain(source.iter().map(move |text| {
                ctx.charge_work(1, "native outgoing link scan")?;
                Ok(text.as_str())
            })))
    }
}

#[derive(Clone, Copy)]
pub(crate) enum NativeArena<'a> {
    Product(&'a [NativeRecord]),
    Source(&'a [UnknownRecord], &'a [usize]),
}

impl<'a> NativeArena<'a> {
    pub(crate) fn records(self) -> impl Iterator<Item = NativeEntity<'a>> {
        let (products, sources, order): (&[NativeRecord], &[UnknownRecord], &[usize]) = match self {
            Self::Product(records) => (records, &[], &[]),
            Self::Source(records, order) => (&[], records, order),
        };
        products.iter().map(NativeEntity::Product).chain(
            order
                .iter()
                .map(move |index| NativeEntity::Source(&sources[*index])),
        )
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
    pub(crate) fn visit<E>(
        self,
        mut work: impl FnMut(usize) -> Result<(), E>,
        mut visit: impl FnMut(&'a str, &'a str, NativeArena<'a>) -> Result<(), E>,
    ) -> Result<(), E> {
        let mut pending = self.unknowns;
        for (format, namespace) in &self.ir.native.0 {
            work(1)?;
            if let Some((replacement, records, order)) = pending {
                work(format.len())?;
                work(replacement.len())?;
                if replacement < format.as_str() {
                    visit(replacement, "unknowns", NativeArena::Source(records, order))?;
                    pending = None;
                }
            }
            for (arena, records) in namespace.arenas() {
                work(1)?;
                if let Some((replacement, sources, order)) = pending {
                    work(format.len())?;
                    work(replacement.len())?;
                    work(arena.len())?;
                    work("unknowns".len())?;
                    if replacement == format && "unknowns" <= arena.as_str() {
                        visit(replacement, "unknowns", NativeArena::Source(sources, order))?;
                        pending = None;
                    }
                }
                if let Some((replacement, _, _)) = self.unknowns {
                    work(format.len())?;
                    work(replacement.len())?;
                    work(arena.len())?;
                    work("unknowns".len())?;
                    if replacement == format && arena == "unknowns" {
                        continue;
                    }
                }
                visit(format, arena, NativeArena::Product(records))?;
            }
            if let Some((replacement, sources, order)) = pending {
                work(format.len())?;
                work(replacement.len())?;
                if replacement == format {
                    visit(replacement, "unknowns", NativeArena::Source(sources, order))?;
                    pending = None;
                }
            }
        }
        if let Some((format, records, order)) = pending {
            visit(format, "unknowns", NativeArena::Source(records, order))?;
        }
        Ok(())
    }
}
