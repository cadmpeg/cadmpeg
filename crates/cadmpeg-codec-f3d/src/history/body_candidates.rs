// SPDX-License-Identifier: Apache-2.0
//! Resolve live and external body recipes with pass-local native identity indexes.
use crate::records::recipes::{ConstructionRecipe, ConstructionRecipeKind};
use crate::records::sketch_links::PersistentDesignLink;
use crate::records::topology::body_recipe::DesignBodyRecipeOperand;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::ids::{BodyId, FaceId};
use cadmpeg_ir::topology::{Body, Region, Shell};
use std::collections::{BTreeSet, HashMap};

struct ExternalIndex<'a> {
    body_by_face: HashMap<&'a FaceId, &'a BodyId>,
    metadata: HashMap<&'a BodyId, &'a Body>,
}
struct LinkIndex<'a> {
    recipes: HashMap<&'a str, Option<&'a ConstructionRecipe>>,
    selected: HashMap<(&'a str, i64), Option<&'a BodyId>>,
}

pub(super) struct BodyCandidates<'a> {
    bodies: &'a [Body],
    regions: &'a [Region],
    shells: &'a [Shell],
    recipes: &'a [ConstructionRecipe],
    links: &'a [PersistentDesignLink],
    external: Option<ExternalIndex<'a>>,
    linked: Option<LinkIndex<'a>>,
}
impl<'a> BodyCandidates<'a> {
    pub(super) fn new(
        bodies: &'a [Body],
        regions: &'a [Region],
        shells: &'a [Shell],
        recipes: &'a [ConstructionRecipe],
        links: &'a [PersistentDesignLink],
    ) -> Self {
        Self {
            bodies,
            regions,
            shells,
            recipes,
            links,
            external: None,
            linked: None,
        }
    }
    fn external_index(&mut self) -> &ExternalIndex<'a> {
        let (bodies, regions, shells) = (self.bodies, self.regions, self.shells);
        self.external.get_or_insert_with(|| {
            let mut body_by_region = HashMap::new();
            for region in regions {
                body_by_region.insert(&region.id, &region.body);
            }
            let mut body_by_face = HashMap::new();
            for shell in shells {
                let Some(body) = body_by_region.get(&shell.region).copied() else {
                    continue;
                };
                for face in shell.faces() {
                    body_by_face.insert(face, body);
                }
            }
            let mut metadata = HashMap::new();
            for body in bodies {
                metadata.insert(&body.id, body);
            }
            ExternalIndex {
                body_by_face,
                metadata,
            }
        })
    }
    pub(super) fn face_body_candidates(
        &mut self,
        operand: &DesignBodyRecipeOperand,
        selected: &BodyId,
    ) -> (bool, bool) {
        let index = self.external_index();
        let mut any = false;
        let mut contains = false;
        for face in operand
            .references()
            .iter()
            .flat_map(|reference| &reference.candidate_faces)
        {
            if let Some(body) = index
                .body_by_face
                .get(face)
                .filter(|body| index.metadata.contains_key(*body))
            {
                any = true;
                contains |= *body == selected;
            }
        }
        (any, contains)
    }
    pub(super) fn linked(
        &mut self,
        ctx: &DecodeContext<'_>,
        operand: &DesignBodyRecipeOperand,
    ) -> Result<Option<BodyId>, CodecError> {
        if self.recipes.is_empty() || self.links.is_empty() {
            return Ok(None);
        }
        let Some(stream) = crate::ids::native_stream(&operand.id) else {
            return Ok(None);
        };
        if self.linked.is_none() {
            let mut recipes = HashMap::new();
            for recipe in self.recipes {
                if recipe.kind != ConstructionRecipeKind::Body {
                    continue;
                }

                recipes
                    .entry(recipe.id.as_str())
                    .and_modify(|row| *row = None)
                    .or_insert(Some(recipe));
            }
            let live = self
                .bodies
                .iter()
                .map(|body| &body.id)
                .collect::<std::collections::HashSet<_>>();
            let mut latest = HashMap::<&BodyId, &PersistentDesignLink>::new();
            for link in self.links {
                let cadmpeg_ir::attributes::AttributeTarget::Body(body) = &link.target else {
                    continue;
                };

                latest
                    .entry(body)
                    .and_modify(|previous| {
                        if link.ordinal > previous.ordinal {
                            *previous = link;
                        }
                    })
                    .or_insert(link);
            }
            let mut selected = HashMap::new();
            for (body, link) in latest {
                if !live.contains(body) {
                    continue;
                }
                let key = (link.design_id.as_str(), link.design_reference);

                selected
                    .entry(key)
                    .and_modify(|previous| {
                        if *previous != Some(body) {
                            *previous = None;
                        }
                    })
                    .or_insert(Some(body));
            }
            self.linked = Some(LinkIndex { recipes, selected });
        }
        let Some(index) = self.linked.as_ref() else {
            return Ok(None);
        };

        let Some(recipe) = index
            .recipes
            .get(operand.recipe_id.as_str())
            .copied()
            .flatten()
        else {
            return Ok(None);
        };
        if crate::ids::native_stream(&recipe.id) != Some(stream) {
            return Ok(None);
        }
        let Some(design) = recipe.design.as_ref() else {
            return Ok(None);
        };
        let Some(selector) = design.selector else {
            return Ok(None);
        };

        let Some(body) = index
            .selected
            .get(&(design.id.value.as_str(), i64::from(selector.value)))
            .copied()
            .flatten()
        else {
            return Ok(None);
        };
        Ok(Some(body.try_clone_for_decode(
            ctx,
            "copy F3D persistent body link identity",
        )?))
    }
    pub(super) fn direct(
        &mut self,
        ctx: &DecodeContext<'_>,
        operand: &DesignBodyRecipeOperand,
    ) -> Result<Option<BodyId>, CodecError> {
        if let Some(body) = self.linked(ctx, operand)? {
            let (any, contains) = self.face_body_candidates(operand, &body);
            return Ok((!any || contains).then_some(body));
        }
        self.external(ctx, operand, None)
    }
    pub(super) fn external(
        &mut self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        operand: &crate::records::topology::body_recipe::DesignBodyRecipeOperand,
        current_history_source: Option<&str>,
    ) -> Result<Option<cadmpeg_ir::ids::BodyId>, cadmpeg_core::CodecError> {
        let index = self.external_index();
        let body_by_face = &index.body_by_face;
        let body_metadata = &index.metadata;
        let current_prefix = current_history_source.map(|source| format!("f3d:brep/{source}/"));
        let mut candidates: Option<BTreeSet<cadmpeg_ir::ids::BodyId>> = None;
        for reference in operand.references() {
            let mut reference_candidates = BTreeSet::new();
            for face in &reference.candidate_faces {
                let Some(body) = body_by_face.get(face).copied() else {
                    continue;
                };
                if current_prefix
                    .as_ref()
                    .is_some_and(|prefix| body.as_str().starts_with(prefix))
                {
                    continue;
                }
                if !reference_candidates.contains(body) {
                    let id = body.try_clone_for_decode(ctx, "copy F3D external body candidate")?;
                    reference_candidates.insert(id);
                }
            }
            if let Some(candidates) = &mut candidates {
                if reference_candidates.is_empty() {
                    return Ok(None);
                }
                candidates.retain(|body| reference_candidates.contains(body));
            } else {
                candidates = Some(reference_candidates);
            }
        }
        let Some(mut candidates) = candidates else {
            return Ok(None);
        };
        let mut displayed = BTreeSet::new();
        for body in candidates.iter().filter(|body| {
            body_metadata
                .get(body)
                .is_some_and(|body| body.visible == Some(true))
        }) {
            let id = body.try_clone_for_decode(ctx, "copy F3D displayed external body")?;
            displayed.insert(id);
        }
        if !displayed.is_empty() {
            candidates = displayed;
        }
        if candidates.len() == 1 {
            Ok(candidates.into_iter().next())
        } else {
            Ok(None)
        }
    }
}
