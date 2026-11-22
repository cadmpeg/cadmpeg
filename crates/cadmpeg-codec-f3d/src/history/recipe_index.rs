// SPDX-License-Identifier: Apache-2.0
//! Index persistent recipe selectors once per immutable history snapshot.

use crate::history_records::AsmHistoricalTopology;
use crate::records::topology::body_recipe::AsmHistoricalEntityKind;
use std::collections::{HashMap, HashSet};

pub(super) struct ReferenceLaneFaces {
    pub(super) faces: Vec<cadmpeg_ir::ids::FaceId>,
    pub(super) disjoint_names: bool,
}

/// How the Design references of one persistent name combine into its faces.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum NameReferences {
    /// Every face that one of the name's references joins.
    Union,
    /// The faces that every reference of the name joins; the union when no
    /// face carries all of them.
    Common,
}

/// The faces of one lane with the Design references of each name joined by
/// [`NameReferences::Union`].
pub(super) fn reference_lane_faces(
    lane: &[crate::records::dimensions::DesignRecipeReference],
) -> ReferenceLaneFaces {
    reference_lane_faces_by(lane, NameReferences::Union)
}

pub(super) fn reference_lane_faces_by(
    lane: &[crate::records::dimensions::DesignRecipeReference],
    combine: NameReferences,
) -> ReferenceLaneFaces {
    let mut references: std::collections::BTreeMap<_, Vec<_>> = std::collections::BTreeMap::new();
    for reference in lane {
        references
            .entry(reference.selector_offset)
            .or_default()
            .push(super::effective_faces(reference));
    }
    let mut names = references
        .into_values()
        .map(|references| name_faces(&references, combine))
        .filter(|faces| !faces.is_empty());
    let mut faces = names.next().unwrap_or_default();
    let mut union = faces.clone();
    for name in names {
        faces.retain(|face| name.contains(face));
        for face in name {
            if !union.contains(&face) {
                union.push(face);
            }
        }
    }
    let disjoint_names = faces.is_empty() && !union.is_empty();
    ReferenceLaneFaces {
        faces: if disjoint_names { union } else { faces },
        disjoint_names,
    }
}

/// The faces of one persistent name from the candidate faces of each of its
/// Design references.
fn name_faces(
    references: &[&[cadmpeg_ir::ids::FaceId]],
    combine: NameReferences,
) -> Vec<cadmpeg_ir::ids::FaceId> {
    let mut union: Vec<cadmpeg_ir::ids::FaceId> = Vec::new();
    for face in references.iter().copied().flatten() {
        if !union.contains(face) {
            union.push(face.clone());
        }
    }
    if combine == NameReferences::Union {
        return union;
    }
    let common: Vec<_> = union
        .iter()
        .filter(|face| references.iter().all(|faces| faces.contains(face)))
        .cloned()
        .collect();
    if common.is_empty() {
        union
    } else {
        common
    }
}

type TokenReferences<'a> = HashMap<&'a str, HashMap<u32, HashMap<i64, Vec<usize>>>>;

pub(super) struct RecipeTopologyIndex<'a> {
    pub(super) topology: &'a AsmHistoricalTopology,
    tokens: TokenReferences<'a>,
    faces: Option<HashMap<i64, Vec<usize>>>,
}

impl<'a> RecipeTopologyIndex<'a> {
    fn new(topology: &'a AsmHistoricalTopology) -> Self {
        let live_faces = topology.faces.iter().copied().collect::<HashSet<_>>();
        let live_edges = topology.edges.iter().copied().collect::<HashSet<_>>();
        let mut tokens = TokenReferences::new();
        for (index, tag) in topology.persistent_subentity_tags.iter().enumerate() {
            let live = match tag.entity_kind {
                AsmHistoricalEntityKind::Face => live_faces.contains(&tag.entity_ref),
                AsmHistoricalEntityKind::Edge => live_edges.contains(&tag.entity_ref),
                _ => false,
            };
            if !live || tag.design_references.is_empty() {
                continue;
            }
            let references = tokens
                .entry(tag.token.as_str())
                .or_default()
                .entry(tag.token_integer)
                .or_default();
            for reference in &tag.design_references {
                let rows = references.entry(*reference).or_default();
                // One tag can repeat its Design reference; the old membership query
                // selected that tag once, so retain one row in each reference group.
                if rows.last() != Some(&index) {
                    rows.push(index);
                }
            }
        }
        Self {
            topology,
            tokens,
            faces: None,
        }
    }

    pub(super) fn matching_tags(
        &self,
        token: &str,
        token_integer: u32,
        reference: i64,
    ) -> &[usize] {
        self.tokens
            .get(token)
            .and_then(|integers| integers.get(&token_integer))
            .and_then(|references| references.get(&reference))
            .map_or(&[], Vec::as_slice)
    }

    pub(super) fn unique_alternate_face(
        &self,
        references: &[crate::records::dimensions::DesignRecipeReference],
        design_reference: i32,
        active_faces: &[cadmpeg_ir::ids::FaceId],
        history_source: Option<&str>,
    ) -> Option<i64> {
        let [active] = active_faces else {
            return None;
        };
        let source_matches = active.as_str().starts_with("f3d:brep:entity#")
            || history_source
                .is_some_and(|source| super::active_brep_face_matches_source(active, source));
        if !source_matches {
            return None;
        }
        let slot = super::stable_ref(active.as_str())?;
        let mut selected = false;
        for reference in references
            .iter()
            .filter(|reference| reference.design_reference == i64::from(design_reference))
        {
            for tag_index in self.matching_tags(
                &reference.token,
                reference.token_integer,
                reference.design_reference,
            ) {
                let tag = &self.topology.persistent_subentity_tags[*tag_index];
                if tag.entity_kind == AsmHistoricalEntityKind::Face
                    && tag.selector != reference.selector
                {
                    if tag.entity_ref != slot {
                        return None;
                    }
                    selected = true;
                }
            }
        }
        selected.then_some(slot)
    }

    pub(super) fn face_tags(&mut self, reference: i64) -> &[usize] {
        let (topology, tokens) = (self.topology, &self.tokens);
        self.faces
            .get_or_insert_with(|| {
                let mut faces: HashMap<_, Vec<_>> = HashMap::new();
                for references in tokens.values().flat_map(|integers| integers.values()) {
                    for (reference, tags) in references {
                        for tag_index in tags {
                            if topology.persistent_subentity_tags[*tag_index].entity_kind
                                == AsmHistoricalEntityKind::Face
                            {
                                faces.entry(*reference).or_default().push(*tag_index);
                            }
                        }
                    }
                }
                for tags in faces.values_mut() {
                    tags.sort_unstable_by(Ord::cmp);
                }
                faces
            })
            .get(&reference)
            .map_or(&[], Vec::as_slice)
    }
}

#[derive(Default)]
pub(super) struct RecipeTopologyCache<'a> {
    entries: HashMap<*const AsmHistoricalTopology, RecipeTopologyIndex<'a>>,
}

impl<'a> RecipeTopologyCache<'a> {
    pub(super) fn get(
        &mut self,
        topology: &'a AsmHistoricalTopology,
    ) -> &mut RecipeTopologyIndex<'a> {
        self.entries
            .entry(std::ptr::from_ref(topology))
            .or_insert_with(|| RecipeTopologyIndex::new(topology))
    }
}

#[cfg(test)]
mod tests {
    use super::RecipeTopologyCache;
    use crate::history_records::{AsmHistoricalPersistentSubentityTag, AsmHistoricalTopology};
    use crate::records::topology::body_recipe::AsmHistoricalEntityKind;

    #[test]
    fn alternate_face_requires_one_matching_live_face_in_both_lanes() {
        use crate::test_support::history::recipe_operand_reference;
        use cadmpeg_ir::ids::FaceId;
        let face = |slot| FaceId::mint(format!("f3d:brep:entity#{slot}")).unwrap();
        let tag = |entity_ref, selector, token_integer, design_reference| {
            AsmHistoricalPersistentSubentityTag {
                entity_kind: AsmHistoricalEntityKind::Face,
                entity_ref,
                selector,
                token: "7".into(),
                token_integer,
                design_references: vec![design_reference],
                ordinal: 0,
            }
        };
        let mut reference = recipe_operand_reference(0, "7", 300, &[]);
        reference.selector = 2;
        let base = AsmHistoricalTopology {
            faces: vec![10, 20],
            persistent_subentity_tags: vec![
                tag(10, 1, 0, 300),
                tag(20, 2, 0, 300),
                tag(20, 1, 8, 300),
                tag(20, 1, 0, 301),
                tag(99, 1, 0, 300),
            ],
            ..Default::default()
        };
        let mut cache = RecipeTopologyCache::default();
        let index = cache.get(&base);
        assert_eq!(
            index.unique_alternate_face(std::slice::from_ref(&reference), 300, &[face(10)], None),
            Some(10)
        );
        for active in [vec![], vec![face(20)], vec![face(10), face(20)]] {
            assert_eq!(
                index.unique_alternate_face(std::slice::from_ref(&reference), 300, &active, None),
                None
            );
        }
        assert_eq!(
            index.unique_alternate_face(std::slice::from_ref(&reference), 301, &[face(10)], None),
            None
        );
        let mut other_token = reference.clone();
        other_token.token = "8".into();
        assert_eq!(
            index.unique_alternate_face(&[other_token], 300, &[face(10)], None),
            None
        );
        let qualified = FaceId::mint("f3d:brep/input.smbh/brep:entity#10").unwrap();
        assert_eq!(
            index.unique_alternate_face(
                std::slice::from_ref(&reference),
                300,
                std::slice::from_ref(&qualified),
                Some("input.smbh")
            ),
            Some(10)
        );
        for source in [None, Some("other.smbh")] {
            assert_eq!(
                index.unique_alternate_face(
                    std::slice::from_ref(&reference),
                    300,
                    std::slice::from_ref(&qualified),
                    source
                ),
                None
            );
        }
        let mut ambiguous = base.clone();
        ambiguous.persistent_subentity_tags.push(tag(20, 1, 0, 300));
        assert_eq!(
            cache.get(&ambiguous).unique_alternate_face(
                std::slice::from_ref(&reference),
                300,
                &[face(10)],
                None
            ),
            None
        );
        let mut absent = base.clone();
        absent.faces = vec![20];
        assert_eq!(
            cache
                .get(&absent)
                .unique_alternate_face(&[reference], 300, &[face(10)], None),
            None
        );
    }

    #[test]
    fn recipe_reference_aliases_union_references_and_intersect_live_names() {
        use crate::records::dimensions::DesignRecipeReference;
        use cadmpeg_ir::ids::FaceId;
        let face = |slot| FaceId::mint(format!("f3d:brep:entity#{slot}")).unwrap();
        let reference = |selector_offset, design_reference, slots: &[i64]| DesignRecipeReference {
            selector: 1,
            selector_offset,
            token: "2".into(),
            token_integer: 0,
            token_offset: selector_offset + 8,
            design_reference,
            design_reference_offset: selector_offset + 17,
            candidate_faces: slots.iter().map(|slot| face(*slot)).collect(),
            candidate_edges: Vec::new(),
            alternate_selector_faces: Vec::new(),
            alternate_selector_edges: Vec::new(),
            operand_offset: Some(0),
        };
        let lane = [
            reference(0, 301, &[10, 20]),
            reference(0, 302, &[30]),
            reference(40, 303, &[20, 30, 40]),
        ];
        assert_eq!(
            super::reference_lane_faces(&lane).faces,
            [face(20), face(30)]
        );
        assert_eq!(
            super::reference_lane_faces(&lane[..2]).faces,
            [face(10), face(20), face(30)]
        );
        let missing = [lane[0].clone(), reference(40, 303, &[])];
        assert_eq!(
            super::reference_lane_faces(&missing).faces,
            [face(10), face(20)]
        );
        let conflict = [lane[0].clone(), reference(40, 303, &[40])];
        let conflict = super::reference_lane_faces(&conflict);
        assert_eq!(conflict.faces, [face(10), face(20), face(40)]);
        assert!(conflict.disjoint_names);
        assert!(!super::reference_lane_faces(&lane).disjoint_names);
        assert!(super::reference_lane_faces(&[]).faces.is_empty());
    }

    #[test]
    fn persistent_name_selects_faces_that_carry_every_design_reference() {
        use crate::records::dimensions::DesignRecipeReference;
        use cadmpeg_ir::ids::FaceId;
        let face = |slot| FaceId::mint(format!("f3d:brep:entity#{slot}")).unwrap();
        let reference = |selector_offset, design_reference, slots: &[i64]| DesignRecipeReference {
            selector: 1,
            selector_offset,
            token: "1".into(),
            token_integer: 0,
            token_offset: selector_offset + 8,
            design_reference,
            design_reference_offset: selector_offset + 17,
            candidate_faces: slots.iter().map(|slot| face(*slot)).collect(),
            candidate_edges: Vec::new(),
            alternate_selector_faces: Vec::new(),
            alternate_selector_edges: Vec::new(),
            operand_offset: Some(0),
        };
        // One name with two Design references: the reference that every
        // fragment of the token carries does not widen the name.
        let name = [reference(0, 920, &[10]), reference(0, 934, &[10, 20, 30])];
        let lane = super::reference_lane_faces_by(&name, super::NameReferences::Common);
        assert_eq!(lane.faces, [face(10)]);
        assert!(!lane.disjoint_names);
        assert_eq!(
            super::reference_lane_faces(&name).faces,
            [face(10), face(20), face(30)]
        );
        // A second name of the same operand intersects the first name's faces.
        let operand = [
            name[0].clone(),
            name[1].clone(),
            reference(40, 922, &[10]),
            reference(40, 934, &[10, 20, 30]),
        ];
        assert_eq!(
            super::reference_lane_faces_by(&operand, super::NameReferences::Common).faces,
            [face(10)]
        );
        // No face carries both references: the name keeps their union.
        let split = [reference(0, 920, &[10]), reference(0, 934, &[20, 30])];
        assert_eq!(
            super::reference_lane_faces_by(&split, super::NameReferences::Common).faces,
            [face(10), face(20), face(30)]
        );
    }

    #[test]
    fn recipe_queries_reuse_indexes_and_keep_snapshots_separate() {
        let topology = |entity_ref| AsmHistoricalTopology {
            faces: vec![entity_ref],
            persistent_subentity_tags: vec![AsmHistoricalPersistentSubentityTag {
                entity_kind: AsmHistoricalEntityKind::Face,
                entity_ref,
                selector: 0,
                token: "rim".into(),
                token_integer: 0,
                design_references: vec![301, 301],
                ordinal: 0,
            }],
            ..AsmHistoricalTopology::default()
        };
        let mut first = topology(10);
        let mut dead = first.persistent_subentity_tags[0].clone();
        dead.entity_ref = 999;
        dead.token = "dead".into();
        first.persistent_subentity_tags.push(dead);
        let mut other_integer = first.persistent_subentity_tags[0].clone();
        other_integer.token_integer = 4012;
        first.persistent_subentity_tags.push(other_integer);
        let second = topology(20);
        let mut cache = RecipeTopologyCache::default();
        for _ in 0..1_000 {
            for (topology, entity) in [(&first, 10), (&second, 20)] {
                let index = cache.get(topology);
                assert_eq!(index.matching_tags("rim", 0, 301), &[0]);
                if entity == 10 {
                    assert_eq!(index.matching_tags("rim", 4012, 301), &[2]);
                }
                assert!(index.matching_tags("rim", 4013, 301).is_empty());
                assert_eq!(
                    index.face_tags(301),
                    if entity == 10 { &[0, 2][..] } else { &[0][..] }
                );
                assert_eq!(
                    index.topology.persistent_subentity_tags[0].entity_ref,
                    entity
                );
                assert!(index.matching_tags("rim", 0, 302).is_empty());
                assert!(index.matching_tags("other", 0, 301).is_empty());
                assert!(index.matching_tags("dead", 0, 301).is_empty());
            }
        }
    }
    #[test]
    fn token_integer_recipe_queries_separate_entities_and_keep_fragments() {
        let tag = |entity_ref, token_integer| AsmHistoricalPersistentSubentityTag {
            entity_kind: AsmHistoricalEntityKind::Face,
            entity_ref,
            selector: 1,
            token: "10".into(),
            token_integer,
            design_references: vec![328],
            ordinal: 0,
        };
        let topology = AsmHistoricalTopology {
            faces: vec![10, 20, 30],
            persistent_subentity_tags: vec![tag(10, 0), tag(20, 4012), tag(30, 4012)],
            ..Default::default()
        };
        let mut cache = RecipeTopologyCache::default();
        let index = cache.get(&topology);
        assert_eq!(index.matching_tags("10", 0, 328), &[0]);
        assert_eq!(index.matching_tags("10", 4012, 328), &[1, 2]);
        assert!(index.matching_tags("10", 4013, 328).is_empty());
        assert_eq!(index.face_tags(328), &[0, 1, 2]);
    }

    #[test]
    fn token_only_recipe_queries_match_tags_without_a_face_index() {
        let topology = AsmHistoricalTopology {
            faces: vec![10],
            persistent_subentity_tags: vec![AsmHistoricalPersistentSubentityTag {
                entity_kind: AsmHistoricalEntityKind::Face,
                entity_ref: 10,
                selector: 0,
                token: "rim".into(),
                token_integer: 0,
                design_references: vec![301],
                ordinal: 0,
            }],
            ..Default::default()
        };
        let mut cache = RecipeTopologyCache::default();
        for _ in 0..1_000 {
            assert_eq!(cache.get(&topology).matching_tags("rim", 0, 301), &[0]);
        }
    }
}
