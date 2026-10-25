// SPDX-License-Identifier: Apache-2.0
//! Accepted interpretations prevent aliases from replaying a physical part tree.

use super::PayloadBinding;
use crate::native::entry_contents::EntryGrammar;
use cadmpeg_core::decode::{u64_from_index, DecodeContext, SourceExtentProof, View};
use cadmpeg_core::CodecError;

pub(crate) struct EntryInterpretation<'source> {
    entry: String,
    grammar: EntryGrammar,
    source: View<'source>,
    proof: SourceExtentProof,
    pub(crate) binding: Option<PayloadBinding<'source>>,
    pub(crate) given_up: Option<String>,
    material_strings: Vec<[Option<View<'source>>; 3]>,
}

#[derive(Default)]
pub(crate) struct EntryInterpretations<'source> {
    claims: Vec<MemoEntry<'source>>,
    presentation_source: Option<std::rc::Rc<crate::xml_framing::XmlSource<'source>>>,
}
enum MemoEntry<'source> {
    Presentation {
        id: String,
        control: crate::gui::source::presentation::Control<'source>,
    },
    Appearance {
        property: String,
        control: super::appearance::Control<'source>,
    },
    Interpretation(EntryInterpretation<'source>),
    MaterialName {
        property: String,
        name: crate::xml_framing::XmlAttributeSource<'source>,
    },
}
impl<'source> EntryInterpretations<'source> {
    pub(crate) fn set_presentation_source(
        &mut self,
        source: std::rc::Rc<crate::xml_framing::XmlSource<'source>>,
    ) {
        self.presentation_source = Some(source);
    }
    pub(crate) fn presentation_source(
        &self,
    ) -> Result<std::rc::Rc<crate::xml_framing::XmlSource<'source>>, CodecError> {
        self.presentation_source
            .as_ref()
            .map(std::rc::Rc::clone)
            .ok_or_else(|| CodecError::malformed("GUI wrapper has no original scanner supplier"))
    }
    pub(crate) fn presentation_control(
        &self,
        id: &str,
    ) -> Option<&crate::gui::source::presentation::Control<'source>> {
        self.claims.iter().find_map(|claim| match claim {
            MemoEntry::Presentation {
                id: actual,
                control,
            } if actual == id => Some(control),
            _ => None,
        })
    }
    pub(crate) fn bind_presentation(
        &mut self,
        ctx: &DecodeContext<'_>,
        id: &str,
        control: crate::gui::source::presentation::Control<'source>,
    ) -> Result<(), CodecError> {
        let bytes = id
            .len()
            .checked_add(size_of::<MemoEntry<'source>>())
            .ok_or_else(|| CodecError::malformed("presentation supplier storage overflows"))?;
        self.admit_next(ctx, bytes, "GUI presentation source suppliers")?;
        self.claims.try_reserve_exact(1).map_err(|_| {
            cadmpeg_core::decode::ResourceLimit::allocation_failed(
                cadmpeg_core::decode::ResourceDimension::RetainedBytes,
                ctx.policy().limits.max_retained_bytes,
                u64_from_index(bytes),
                "GUI presentation source suppliers",
            )
        })?;
        self.claims.push(MemoEntry::Presentation {
            id: id.to_owned(),
            control,
        });
        Ok(())
    }
    fn admit_next(
        &self,
        ctx: &DecodeContext<'_>,
        additional: usize,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        let bytes = self
            .claims
            .len()
            .checked_mul(size_of::<MemoEntry<'source>>())
            .and_then(|bytes| bytes.checked_add(additional))
            .ok_or_else(|| {
                CodecError::malformed("entry interpretation working storage overflows")
            })?;
        super::working(ctx, u64_from_index(bytes), operation)
    }
    pub(crate) fn bind_appearance(
        &mut self,
        ctx: &DecodeContext<'_>,
        property: &str,
        control: super::appearance::Control<'source>,
    ) -> Result<(), CodecError> {
        let bytes = property
            .len()
            .checked_add(size_of::<MemoEntry<'source>>())
            .ok_or_else(|| CodecError::malformed("appearance supplier storage overflows"))?;
        self.admit_next(ctx, bytes, "appearance XML source suppliers")?;
        self.claims.try_reserve_exact(1).map_err(|_| {
            cadmpeg_core::decode::ResourceLimit::allocation_failed(
                cadmpeg_core::decode::ResourceDimension::RetainedBytes,
                ctx.policy().limits.max_retained_bytes,
                u64_from_index(bytes),
                "appearance XML source suppliers",
            )
        })?;
        self.claims.push(MemoEntry::Appearance {
            property: property.to_owned(),
            control,
        });
        Ok(())
    }
    pub(crate) fn appearance_control(
        &self,
        property: &str,
    ) -> Option<&super::appearance::Control<'source>> {
        self.claims.iter().find_map(|claim| match claim {
            MemoEntry::Appearance {
                property: actual,
                control,
            } if actual == property => Some(control),
            _ => None,
        })
    }
    pub(crate) fn source(
        &self,
        ctx: &DecodeContext<'_>,
        entry: &str,
        grammar: EntryGrammar,
    ) -> Result<Option<View<'source>>, CodecError> {
        let Some(claim) = self.claims.iter().find_map(|claim| match claim {
            MemoEntry::Interpretation(claim)
                if claim.entry == entry && claim.grammar == grammar =>
            {
                Some(claim)
            }
            _ => None,
        }) else {
            return Ok(None);
        };
        ctx.require_source_authority(&claim.proof)?;
        ctx.require_source_ancestry(&claim.proof, claim.source)?;
        Ok(Some(claim.source))
    }
    pub(crate) fn checkpoint(&self) -> usize {
        self.claims.len()
    }
    pub(crate) fn rollback(&mut self, checkpoint: usize) {
        self.claims.truncate(checkpoint);
    }
    pub(crate) fn find(
        &self,
        ctx: &DecodeContext<'_>,
        entry: &str,
        grammar: EntryGrammar,
        source: View<'source>,
    ) -> Result<Option<&EntryInterpretation<'source>>, CodecError> {
        let Some(claim) = self
            .claims
            .iter()
            .filter_map(|claim| match claim {
                MemoEntry::Interpretation(claim) => Some(claim),
                _ => None,
            })
            .find(|claim| claim.entry == entry && claim.grammar == grammar)
        else {
            return Ok(None);
        };
        ctx.require_source_authority(&claim.proof)?;
        ctx.require_source_ancestry(&claim.proof, source)?;
        if source.start() != claim.source.start() || source.end() != claim.source.end() {
            return Err(CodecError::malformed(
                "entry alias changes the actual source extent",
            ));
        }
        Ok(Some(claim))
    }
    pub(crate) fn insert(
        &mut self,
        ctx: &DecodeContext<'_>,
        entry: &str,
        grammar: EntryGrammar,
        source: View<'source>,
        binding: Option<PayloadBinding<'source>>,
        given_up: Option<String>,
    ) -> Result<(), CodecError> {
        let bytes = size_of::<MemoEntry<'source>>()
            .checked_add(entry.len())
            .and_then(|bytes| bytes.checked_add(given_up.as_ref().map_or(0, String::len)))
            .ok_or_else(|| CodecError::malformed("entry interpretation memo storage overflows"))?;
        self.admit_next(ctx, bytes, "entry interpretation source memo")?;
        self.claims.try_reserve_exact(1).map_err(|_| {
            cadmpeg_core::decode::ResourceLimit::allocation_failed(
                cadmpeg_core::decode::ResourceDimension::RetainedBytes,
                ctx.policy().limits.max_retained_bytes,
                u64_from_index(bytes),
                "entry interpretation source memo",
            )
        })?;
        self.claims
            .push(MemoEntry::Interpretation(EntryInterpretation {
                entry: entry.to_owned(),
                grammar,
                source,
                proof: ctx.source_extent_proof(source)?,
                binding,
                given_up,
                material_strings: Vec::new(),
            }));
        Ok(())
    }
    pub(crate) fn bind_material_strings(
        &mut self,
        entry: &str,
        grammar: EntryGrammar,
        strings: Vec<[Option<View<'source>>; 3]>,
    ) -> Result<(), CodecError> {
        let claim = self
            .claims
            .iter_mut()
            .rev()
            .find_map(|claim| match claim {
                MemoEntry::Interpretation(claim)
                    if claim.entry == entry && claim.grammar == grammar =>
                {
                    Some(claim)
                }
                _ => None,
            })
            .ok_or_else(|| {
                CodecError::malformed("material sources have no accepted interpretation")
            })?;
        claim.material_strings = strings;
        Ok(())
    }
    pub(crate) fn bind_material_name(
        &mut self,
        ctx: &DecodeContext<'_>,
        property: &str,
        name: crate::xml_framing::XmlAttributeSource<'source>,
    ) -> Result<(), CodecError> {
        let bytes = property
            .len()
            .checked_add(size_of::<MemoEntry<'source>>())
            .ok_or_else(|| CodecError::malformed("material name source storage overflows"))?;
        self.admit_next(ctx, bytes, "material property name supplier")?;
        self.claims.try_reserve_exact(1).map_err(|_| {
            cadmpeg_core::decode::ResourceLimit::allocation_failed(
                cadmpeg_core::decode::ResourceDimension::RetainedBytes,
                ctx.policy().limits.max_retained_bytes,
                u64_from_index(bytes),
                "material property name supplier",
            )
        })?;
        self.claims.push(MemoEntry::MaterialName {
            property: property.to_owned(),
            name,
        });
        Ok(())
    }
    pub(crate) fn material_name(
        &self,
        property: &str,
    ) -> Option<&crate::xml_framing::XmlAttributeSource<'source>> {
        self.claims.iter().find_map(|claim| match claim {
            MemoEntry::MaterialName {
                property: actual,
                name,
            } if actual == property => Some(name),
            _ => None,
        })
    }
    pub(crate) fn material_string(
        &self,
        ctx: &DecodeContext<'_>,
        entry: &str,
        version: u32,
        index: usize,
        field: usize,
    ) -> Result<Option<View<'source>>, CodecError> {
        let Some(claim) = self.claims.iter().find_map(|claim| match claim {
            MemoEntry::Interpretation(claim)
                if claim.entry == entry && claim.grammar == EntryGrammar::Material(version) =>
            {
                Some(claim)
            }
            _ => None,
        }) else {
            return Ok(None);
        };
        let source = claim
            .material_strings
            .get(index)
            .and_then(|strings| strings.get(field))
            .copied()
            .flatten();
        if let Some(source) = source {
            ctx.require_source_authority(&claim.proof)?;
            ctx.require_source_ancestry(&claim.proof, source)?;
        }
        Ok(source)
    }
}
