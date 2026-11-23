// SPDX-License-Identifier: Apache-2.0
//! Edge, surface patch, and face recipe structure.

use super::FaceRecipeProgramKind;
use crate::design::decode::sketch::{
    indexed_record_header_at, next_indexed_record_offset, next_indexed_record_offset_with_index,
};
use crate::records::topology::edge_identity::DesignEdgeOperand;
use crate::records::topology::edge_recipe::{
    DesignTopologyRecipeEntry, DesignTopologyRecipeSide, DesignTopologyRecipeTriplet,
};
use cadmpeg_core::decode::{DecodeContext, WorkBudget};
use cadmpeg_core::CodecError;

/// Work cap for the side-sequence search of one recipe program.
///
/// Each search level tries every payload-prefix split of a candidate side,
/// and each candidate side branches the search, so a crafted program grows
/// the work combinatorially. Decoded Design recipe programs stay several
/// orders of magnitude below this cap.
pub(crate) const RECIPE_SIDE_SEARCH_WORK: usize = 1 << 22;

pub(crate) fn edge_recipe_structure(
    ctx: &DecodeContext<'_>,
    program: &[i32],
) -> Result<Option<crate::records::topology::edge_recipe::DesignEdgeRecipeStructure>, CodecError> {
    edge_recipe_structure_within(ctx, &WorkBudget::new(RECIPE_SIDE_SEARCH_WORK), program)
}

/// Count the recipe programs whose side-sequence search stops at
/// [`RECIPE_SIDE_SEARCH_WORK`]. Their recipe structures stay unresolved.
pub(crate) fn recipe_side_searches_exhausted(
    ctx: &DecodeContext<'_>,
    edge_operands: &[DesignEdgeOperand],
    face_operands: &[crate::records::topology::face::DesignFaceOperand],
) -> Result<usize, CodecError> {
    let mut exhausted = 0;
    for operand in edge_operands
        .iter()
        .filter(|operand| operand.recipe_structure.is_none())
    {
        let budget = WorkBudget::new(RECIPE_SIDE_SEARCH_WORK);
        edge_recipe_structure_within(ctx, &budget, &operand.recipe_program)?;
        exhausted += usize::from(budget.exhausted());
    }
    for node in face_operands
        .iter()
        .flat_map(|operand| &operand.recipe_nodes)
        .filter(|node| node.recipe_structure.is_none())
    {
        let Some(tail) = node.program.get(3..) else {
            continue;
        };
        let budget = WorkBudget::new(RECIPE_SIDE_SEARCH_WORK);
        face_recipe_structure_within(ctx, &budget, tail)?;
        exhausted += usize::from(budget.exhausted());
    }
    Ok(exhausted)
}

/// Decode the edge-recipe structure within `budget`. A search that exhausts
/// the budget yields no structure.
pub(crate) fn edge_recipe_structure_within(
    ctx: &DecodeContext<'_>,
    budget: &WorkBudget,
    program: &[i32],
) -> Result<Option<crate::records::topology::edge_recipe::DesignEdgeRecipeStructure>, CodecError> {
    let Some(prelude_length) = edge_recipe_prelude_length(program, budget) else {
        return Ok(None);
    };
    let Some(tail) = program.get(prelude_length..) else {
        return Ok(None);
    };
    let structure = edge_recipe_structure_tail(ctx, budget, tail)?;
    Ok(structure.filter(|_| !budget.exhausted()))
}

pub(in crate::design::decode) fn edge_recipe_prelude_length(
    program: &[i32],
    budget: &WorkBudget,
) -> Option<usize> {
    let tag = |word: i32| matches!(word, -1..=1);
    if let Some([-1, -1, 2, 0, first, 1, second]) = program.get(..7) {
        return (budget.charge_by(7) && tag(*first) && tag(*second)).then_some(7);
    }
    let [0, first_tag, count] = program.get(..3)? else {
        return None;
    };
    let count = usize::try_from(*count).ok().filter(|count| *count != 0)?;
    let length = count.checked_mul(2)?.checked_add(3)?;
    if !tag(*first_tag) || length > program.len() || !budget.charge_by(length) {
        return None;
    }
    for (ordinal, pair) in program[3..length].chunks_exact(2).enumerate() {
        if pair[0] != i32::try_from(ordinal.checked_add(1)?).ok()? || !tag(pair[1]) {
            return None;
        }
    }
    Some(length)
}

/// Decode the alternate two-clause edge-recipe grammar owned by `SurfacePatch`.
///
/// The six fields in each clause are delimiter-bounded by `-1`. The first and
/// second fields name face references, the third and fifth fields name edge
/// references, and the fourth and sixth fields are zero pairs. The final field
/// is a counted sequence of the standard eight-word topology entries.
pub(crate) fn surface_patch_recipe_structure(
    program: &[i32],
    reference_count: usize,
) -> Option<crate::records::topology::edge_recipe::DesignSurfacePatchRecipeStructure> {
    let mut remaining = program.get(7..)?;
    let (&root, tail) = remaining.split_first()?;
    if root != 2 {
        return None;
    }
    remaining = tail;
    let mut clauses = Vec::new();
    for _ in 0..2 {
        let mut fields = Vec::new();
        for _ in 0..6 {
            let delimiter_at = remaining.iter().position(|word| *word == -1)?;
            if delimiter_at > 2 {
                return None;
            }
            let field = remaining.get(..delimiter_at)?;
            let field = field.to_vec();
            if field.is_empty() || field.iter().any(|word| *word < 0) {
                return None;
            }
            let tail = remaining.get(delimiter_at + 1..)?;
            remaining = tail;
            fields.push(field);
        }
        let (&payload_entry_count, tail) = remaining.split_first()?;
        let Ok(payload_entry_count) = u32::try_from(payload_entry_count) else {
            return None;
        };
        let payload_word_count = usize::try_from(payload_entry_count)
            .ok()
            .and_then(|count| count.checked_mul(8))?;
        let payload = tail.get(..payload_word_count)?;
        let entries = edge_recipe_entries(payload)?;
        if u32::try_from(entries.len()).ok() != Some(payload_entry_count) {
            return None;
        }
        let (&delimiter, tail) = tail
            .get(payload_word_count..)
            .and_then(|tail| tail.split_first())?;
        if delimiter != -1 {
            return None;
        }
        remaining = tail;
        let [first, second, third, fourth, fifth, sixth] = fields.as_slice() else {
            return None;
        };
        if !(first.len() == 1 || first.len() == 2 && first[0] == 2)
            || second.len() != 1
            || third.len() != 2
            || third[0] != 2
            || fourth.as_slice() != [0, 0]
            || fifth.len() != 1
            || sixth.as_slice() != [0, 0]
        {
            return None;
        }
        let ordinal = |field: &[i32], position: usize| {
            let ordinal = usize::try_from(*field.get(position)?).ok()?;
            (ordinal < reference_count).then_some(u32::try_from(ordinal).ok()?)
        };
        let face_reference_ordinals =
            (|| Some([ordinal(first, first.len() - 1)?, ordinal(second, 0)?]))()?;
        let edge_reference_ordinals = (|| Some([ordinal(third, 1)?, ordinal(fifth, 0)?]))()?;
        clauses.push(
            crate::records::topology::edge_recipe::DesignSurfacePatchRecipeClause {
                fields,
                face_reference_ordinals,
                edge_reference_ordinals,
                entries,
            },
        );
    }
    if let Some(&delimiter) = remaining.first() {
        if delimiter != 0 {
            return None;
        }
        remaining = &remaining[1..];
    }
    if !remaining.is_empty() {
        return None;
    }
    clauses.try_into().ok().map(|clauses| {
        crate::records::topology::edge_recipe::DesignSurfacePatchRecipeStructure { clauses }
    })
}

pub(super) fn edge_recipe_local_topology_references(
    structure: &crate::records::topology::edge_recipe::DesignEdgeRecipeStructure,
    reference_count: usize,
) -> Option<Vec<std::num::NonZeroU32>> {
    topology_recipe_references(
        structure.sides.iter().flat_map(|side| {
            std::iter::once(side.header_value).chain(side.scalars.iter().copied())
        }),
        reference_count,
    )
}

pub(super) fn edge_recipe_structure_tail(
    ctx: &DecodeContext<'_>,
    budget: &WorkBudget,
    program: &[i32],
) -> Result<Option<crate::records::topology::edge_recipe::DesignEdgeRecipeStructure>, CodecError> {
    let Some((&root, mut remaining)) = program.split_first() else {
        return Ok(None);
    };
    let Ok(side_count) = usize::try_from(root) else {
        return Ok(None);
    };
    if side_count == 0 {
        return Ok(None);
    }
    let Some(delimited) = edge_recipe_delimiter(remaining) else {
        return Ok(None);
    };
    remaining = delimited;
    let mut structure = None;
    for RecipeSideSequence {
        sides,
        remaining: tail,
    } in
        edge_recipe_side_sequences(ctx, budget, remaining, side_count, RecipeSideGrammar::Edge)?
    {
        if !matches!(tail, [-1..=1]) {
            continue;
        }
        if structure.is_some() {
            return Ok(None);
        }
        structure =
            Some(crate::records::topology::edge_recipe::DesignEdgeRecipeStructure { root, sides });
    }
    Ok(structure)
}

pub(super) struct RecipeSideSequence<'a> {
    pub(super) sides: Vec<DesignTopologyRecipeSide>,
    pub(super) remaining: &'a [i32],
}

pub(super) fn edge_recipe_side_sequences<'w>(
    ctx: &DecodeContext<'_>,
    budget: &WorkBudget,
    words: &'w [i32],
    side_count: usize,
    grammar: RecipeSideGrammar,
) -> Result<Vec<RecipeSideSequence<'w>>, CodecError> {
    let _depth = ctx.enter_nested("f3d recipe side recursion")?;
    if !budget.charge() {
        return Ok(Vec::new());
    }
    if side_count == 0 {
        return Ok(vec![RecipeSideSequence {
            sides: Vec::new(),
            remaining: words,
        }]);
    }
    let mut out = Vec::new();
    for (side, tail) in edge_recipe_counted_side_candidates(budget, words, grammar) {
        let remaining = if side_count == 1 {
            tail
        } else if let Some(remaining) = grammar.delimiter(tail) {
            remaining
        } else {
            continue;
        };
        for RecipeSideSequence {
            sides: mut following,
            remaining: tail,
        } in edge_recipe_side_sequences(ctx, budget, remaining, side_count - 1, grammar)?
        {
            if !budget.charge() {
                return Ok(Vec::new());
            }
            following.insert(0, side.clone());
            out.push(RecipeSideSequence {
                sides: following,
                remaining: tail,
            });
        }
    }
    Ok(out)
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum RecipeSideGrammar {
    Edge,
    Face,
}

impl RecipeSideGrammar {
    pub(super) fn delimiter(self, words: &[i32]) -> Option<&[i32]> {
        match self {
            Self::Edge => edge_recipe_delimiter(words),
            Self::Face => recipe_delimiter(words),
        }
    }
}

pub(super) fn edge_recipe_delimiter(words: &[i32]) -> Option<&[i32]> {
    matches!(words.first(), Some(-1..=1)).then(|| &words[1..])
}

pub(super) fn recipe_delimiter(words: &[i32]) -> Option<&[i32]> {
    matches!(words.first(), Some(-1 | 0)).then(|| &words[1..])
}

pub(super) fn complete_recipe_payload_prefix(budget: &WorkBudget, prefix: &[i32]) -> bool {
    if prefix == [0] {
        return true;
    }
    if prefix.is_empty() {
        return false;
    }
    let mut pending = vec![prefix];
    while let Some(remaining) = pending.pop() {
        if remaining.is_empty() {
            return true;
        }
        if !budget.charge_by(remaining.len()) {
            return false;
        }
        let Some(at) = remaining.iter().position(|word| *word < 0) else {
            continue;
        };
        if remaining[at] != -1 {
            continue;
        }
        if at >= 1 && remaining.get(at + 1..at + 4) == Some(&[0, 0, -1]) {
            pending.push(&remaining[at + 4..]);
        }
        if at >= 4 && remaining.get(at - 3..at) == Some(&[0, 0, 0]) {
            pending.push(&remaining[at + 1..]);
        }
    }
    false
}

pub(super) fn complete_face_recipe_payload_prefix(prefix: &[i32]) -> bool {
    if prefix == [0] {
        return true;
    }
    let mut remaining = prefix;
    let mut field_count = 0;
    while !remaining.is_empty() {
        let Some(delimiter_at) = remaining.iter().position(|word| *word == -1) else {
            return false;
        };
        let field = &remaining[..delimiter_at];
        if field.is_empty() || field[0] <= 0 || field.iter().any(|word| *word < 0) {
            return false;
        }
        remaining = &remaining[delimiter_at + 1..];
        if !matches!(remaining.get(..3), Some([0, 0, -1])) {
            return false;
        }
        remaining = &remaining[3..];
        field_count += 1;
    }
    field_count > 0
}

pub(super) fn edge_recipe_counted_side_candidates<'w>(
    budget: &WorkBudget,
    words: &'w [i32],
    grammar: RecipeSideGrammar,
) -> Vec<(DesignTopologyRecipeSide, &'w [i32])> {
    let Some(field_count) = words
        .first()
        .and_then(|word| u32::try_from(*word).ok())
        .and_then(std::num::NonZeroU32::new)
    else {
        return Vec::new();
    };
    if grammar == RecipeSideGrammar::Face && field_count.get() < 2 {
        return Vec::new();
    }
    let Some(scalar_count) = usize::try_from(field_count.get())
        .ok()
        .and_then(|count| count.checked_sub(1))
    else {
        return Vec::new();
    };
    let Some(&header_value) = words.get(1) else {
        return Vec::new();
    };
    let Some(mut remaining) = words.get(2..).and_then(|tail| grammar.delimiter(tail)) else {
        return Vec::new();
    };
    // Each scalar consumes at least one remaining word; a larger count is
    // corrupt and must not reach the allocator.
    if scalar_count > remaining.len() {
        return Vec::new();
    }
    let mut scalars = Vec::new();
    for _ in 0..scalar_count {
        let Some((&scalar, tail)) = remaining.split_first() else {
            return Vec::new();
        };
        scalars.push(scalar);
        let Some(tail) = grammar.delimiter(tail) else {
            return Vec::new();
        };
        remaining = tail;
    }
    let mut candidates = Vec::new();
    for entry_count_at in 0..remaining.len() {
        // The prefix check scans `entry_count_at` words.
        if !budget.charge_by(entry_count_at + 1) {
            return Vec::new();
        }
        let prefix = &remaining[..entry_count_at];
        let complete_prefix = match grammar {
            RecipeSideGrammar::Edge => complete_recipe_payload_prefix(budget, prefix),
            RecipeSideGrammar::Face => complete_face_recipe_payload_prefix(prefix),
        };
        if !complete_prefix {
            continue;
        }
        let candidate = (|| {
            let payload_entry_count = u32::try_from(*remaining.get(entry_count_at)?).ok()?;
            if grammar == RecipeSideGrammar::Face && entry_count_at != 1 && payload_entry_count == 0
            {
                return None;
            }
            let payload_len = usize::try_from(payload_entry_count).ok()?.checked_mul(8)?;
            let entries_at = entry_count_at.checked_add(1)?;
            let entries_end = entries_at.checked_add(payload_len)?;
            Some((entries_at, entries_end, remaining.get(entries_end..)?))
        })();
        let Some((entries_at, entries_end, tail)) = candidate else {
            continue;
        };
        let Some(entries) = edge_recipe_entries(&remaining[entries_at..entries_end]) else {
            continue;
        };
        let side = DesignTopologyRecipeSide {
            header_value,
            scalars: scalars.clone(),
            payload_prefix: remaining[..entry_count_at].to_vec(),
            entries,
        };
        candidates.push((side, tail));
    }
    candidates
}

pub(crate) fn face_recipe_structure(
    ctx: &DecodeContext<'_>,
    program: &[i32],
) -> Result<Option<crate::records::topology::face::DesignFaceRecipeStructure>, CodecError> {
    face_recipe_structure_within(ctx, &WorkBudget::new(RECIPE_SIDE_SEARCH_WORK), program)
}

/// Decode the face-recipe structure within `budget`. A search that exhausts
/// the budget yields no structure.
pub(crate) fn face_recipe_structure_within(
    ctx: &DecodeContext<'_>,
    budget: &WorkBudget,
    program: &[i32],
) -> Result<Option<crate::records::topology::face::DesignFaceRecipeStructure>, CodecError> {
    let Some((&root, remaining)) = program.split_first() else {
        return Ok(None);
    };
    let Some(remaining) = recipe_delimiter(remaining) else {
        return Ok(None);
    };
    let Some((&first_prelude, remaining)) = remaining.split_first() else {
        return Ok(None);
    };
    let Some(remaining) = recipe_delimiter(remaining) else {
        return Ok(None);
    };
    let Some((&second_prelude, remaining)) = remaining.split_first() else {
        return Ok(None);
    };
    let Some(remaining) = recipe_delimiter(remaining) else {
        return Ok(None);
    };
    let side_count = match second_prelude {
        1 => 1,
        2 => 2,
        _ => return Ok(None),
    };
    let mut structure = None;
    for RecipeSideSequence {
        sides,
        remaining: tail,
    } in
        edge_recipe_side_sequences(ctx, budget, remaining, side_count, RecipeSideGrammar::Face)?
    {
        let postlude = match tail {
            [] | [-1 | 0] => None,
            [-1, value, -1, 0, 0, -1] => Some(*value),
            _ => continue,
        };
        if structure.is_some() {
            return Ok(None);
        }
        structure = Some(crate::records::topology::face::DesignFaceRecipeStructure {
            root,
            prelude: [first_prelude, second_prelude],
            sides,
            postlude_value: postlude,
        });
    }
    Ok(structure.filter(|_| !budget.exhausted()))
}

pub(super) fn face_recipe_nodes(
    ctx: &DecodeContext<'_>,
    recipe_program: &[i32],
    recipe_program_at: usize,
    program_kind: FaceRecipeProgramKind,
) -> Result<Option<Vec<crate::records::topology::face::DesignFaceRecipeNode>>, CodecError> {
    let mut recipe_node_indices = Vec::new();
    for (index, values) in recipe_program.windows(3).enumerate() {
        if values != [-1, -1, 2] {
            continue;
        }
        recipe_node_indices.push(index);
    }
    if recipe_node_indices.first().is_some_and(|index| *index < 3) {
        return Ok(None);
    }
    if program_kind == FaceRecipeProgramKind::Terminal && !recipe_node_indices.is_empty() {
        return Ok(None);
    }
    let node_ranges = recipe_node_indices.iter().copied().zip(
        recipe_node_indices
            .iter()
            .copied()
            .skip(1)
            .chain(std::iter::once(recipe_program.len())),
    );
    let mut recipe_nodes = Vec::new();
    for (start, end) in node_ranges {
        let Some(program) = recipe_program.get(start..end) else {
            return Ok(None);
        };
        let Some(byte_offset) = start
            .checked_mul(4)
            .and_then(|offset| recipe_program_at.checked_add(offset))
            .and_then(|offset| u64::try_from(offset).ok())
        else {
            return Ok(None);
        };
        let Some(end_byte_offset) = end
            .checked_mul(4)
            .and_then(|offset| recipe_program_at.checked_add(offset))
            .and_then(|offset| u64::try_from(offset).ok())
        else {
            return Ok(None);
        };
        let recipe_structure = match program.get(3..) {
            Some(tail) => face_recipe_structure(ctx, tail)?,
            None => None,
        };
        let program = program.to_vec();
        recipe_nodes.push(crate::records::topology::face::DesignFaceRecipeNode {
            byte_offset,
            end_byte_offset,
            program,
            recipe_structure,
        });
    }
    Ok(Some(recipe_nodes))
}

pub(super) fn topology_recipe_references(
    words: impl IntoIterator<Item = i32>,
    reference_count: usize,
) -> Option<Vec<std::num::NonZeroU32>> {
    let mut references = Vec::new();
    for word in words {
        if word == 0 {
            continue;
        }
        let ordinal = u32::try_from(word)
            .ok()
            .and_then(std::num::NonZeroU32::new)?;
        if usize::try_from(ordinal.get())
            .ok()
            .is_none_or(|index| index > reference_count)
        {
            return None;
        }
        references.push(ordinal);
    }
    Some(references)
}

pub(super) fn edge_recipe_entries(words: &[i32]) -> Option<Vec<DesignTopologyRecipeEntry>> {
    let mut entries = Vec::new();
    for entry in words.chunks_exact(8) {
        let parsed = (|| {
            let selector = entry[0];
            if selector < 0 {
                return None;
            }
            let boundary_edge_count = std::num::NonZeroU32::new(u32::try_from(entry[1]).ok()?)?;
            let topology_triplets = [
                edge_recipe_topology_triplet(&entry[2..5], boundary_edge_count)?,
                edge_recipe_topology_triplet(&entry[5..8], boundary_edge_count)?,
            ];
            topology_triplets
                .iter()
                .all(|triplet| triplet.outer.get() <= boundary_edge_count.get())
                .then_some(DesignTopologyRecipeEntry {
                    selector,
                    boundary_edge_count,
                    topology_triplets,
                })
        })()?;
        if entries
            .last()
            .is_some_and(|last: &DesignTopologyRecipeEntry| last.selector >= parsed.selector)
        {
            return None;
        }
        entries.push(parsed);
    }
    Some(entries)
}

pub(super) fn edge_recipe_topology_triplet(
    words: &[i32],
    boundary_edge_count: std::num::NonZeroU32,
) -> Option<DesignTopologyRecipeTriplet> {
    let [outer, middle, repeated_outer] = words else {
        return None;
    };
    if outer != repeated_outer {
        return None;
    }
    let outer = std::num::NonZeroU32::new(u32::try_from(*outer).ok()?)?;
    let vertex_ordinal = outer.get().checked_sub(1)?;
    let incident = if *middle == i32::try_from(outer.get()).ok()? {
        Some((
            crate::records::topology::edge_recipe::DesignTopologyIncidentSide::Following,
            vertex_ordinal,
        ))
    } else if *middle >= 0 && middle.checked_add(1) == i32::try_from(outer.get()).ok() {
        Some((
            crate::records::topology::edge_recipe::DesignTopologyIncidentSide::Preceding,
            vertex_ordinal
                .checked_add(boundary_edge_count.get())?
                .checked_sub(1)?
                % boundary_edge_count.get(),
        ))
    } else {
        None
    };
    Some(DesignTopologyRecipeTriplet {
        outer,
        middle: *middle,
        incident: incident.map(|(side, ordinal)| {
            crate::records::topology::edge_recipe::DesignTopologyIncident { ordinal, side }
        }),
    })
}

/// Find the indexed header that terminates a face-recipe member.
///
/// The ordinary envelope terminates at `N+4`. One serialized generation
/// omits that header and terminates at `N+5`. When neither expected index is
/// present within the enclosing boundary, the first following valid indexed
/// header terminates the envelope; its record index has no fixed delta from
/// `N`. Select an expected continuation before applying that fallback.
/// Without an enclosing boundary and a following header, the stream end
/// terminates the envelope and the returned record index is `0`.
pub(super) fn face_recipe_next_boundary(
    bytes: &[u8],
    position: usize,
    record_index: u32,
    limit: Option<u64>,
) -> Option<(usize, u32)> {
    let within_limit = |offset: usize| {
        limit.is_none_or(|limit| {
            usize::try_from(limit)
                .ok()
                .is_some_and(|limit| offset <= limit)
        })
    };
    [record_index.checked_add(4)?, record_index.checked_add(5)?]
        .into_iter()
        .filter_map(|expected_index| {
            next_indexed_record_offset_with_index(bytes, position, expected_index)
                .map(|offset| (expected_index, offset))
        })
        .filter(|(_, offset)| within_limit(*offset))
        .min_by_key(|(_, offset)| *offset)
        .or_else(|| match next_indexed_record_offset(bytes, position) {
            Some(offset) => {
                let record_index = indexed_record_header_at(bytes, offset)?.record_index;
                within_limit(offset).then_some((record_index, offset))
            }
            None => limit.is_none().then_some((0, bytes.len())),
        })
        .map(|(record_index, offset)| (offset, record_index))
}
