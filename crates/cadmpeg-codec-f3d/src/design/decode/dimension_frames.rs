// SPDX-License-Identifier: Apache-2.0
//! Parse dimension recipe, locus, and annotation frames.

use crate::bytes::lp_ascii_filtered;
use crate::container::{role, ContainerScan};
use crate::design::construction_recipe_family_name_len;
use crate::design::decode::sketch::{decode_constraint_kinds, next_indexed_record_offset};
use crate::ids::{self, native_stream};
use crate::records::{
    ConstructionRecipe, DesignDimensionAnnotationFrame, DesignDimensionAnnotationOperand,
    DesignDimensionLocus, DesignDimensionLocusGroup, DesignDimensionLocusPair,
    DesignDimensionNullLocusPair, DesignDimensionRecipeRecord, DesignEdgeOperand,
    DesignEntityHeader, DesignObjectKind, DesignParameter, DesignParameterCompanion,
    DesignParameterKind, DesignParameterOwner, DesignParameterScope, DesignRecordHeader,
    PersistentSubentityTag, SketchCurveIdentity, SketchPoint,
};
use cadmpeg_ir::codec::CodecError;
use cadmpeg_ir::le::u32_at;
use std::collections::{HashMap, HashSet};

/// Record slices every dimension-record decode pass reads: the container scan
/// plus the parameter, owner, companion, scope, record-header, and sketch
/// geometry tables that locate each dimension's owning companion and geometry.
pub struct DimensionDecodeInputs<'a> {
    pub(crate) scan: &'a ContainerScan,
    pub(crate) parameters: &'a [DesignParameter],
    pub(crate) owners: &'a [DesignParameterOwner],
    pub(crate) companions: &'a [DesignParameterCompanion],
    pub(crate) scopes: &'a [DesignParameterScope],
    pub(crate) headers: &'a [DesignRecordHeader],
    pub(crate) points: &'a [SketchPoint],
    pub(crate) curves: &'a [SketchCurveIdentity],
}

/// Decode the indexed record that directly contains each construction recipe
/// owned by a dimensional parameter companion.
pub fn decode_dimension_recipe_records(
    scan: &ContainerScan,
    parameters: &[DesignParameter],
    owners: &[DesignParameterOwner],
    companions: &[DesignParameterCompanion],
    recipes: &[ConstructionRecipe],
) -> Result<Vec<DesignDimensionRecipeRecord>, CodecError> {
    let parameters = parameters
        .iter()
        .filter_map(|parameter| {
            Some((
                (native_stream(&parameter.id)?, parameter.record_index),
                parameter,
            ))
        })
        .collect::<HashMap<_, _>>();
    let dimension_owners = owners
        .iter()
        .filter_map(|owner| {
            let stream = native_stream(&owner.id)?;
            parameters
                .get(&(stream, owner.parameter_record_index))
                .is_some_and(|parameter| parameter.kind == DesignParameterKind::Dimension)
                .then_some((stream.to_owned(), owner.record_index))
        })
        .collect::<HashSet<_>>();
    let recipes = recipes
        .iter()
        .map(|recipe| (recipe.id.as_str(), recipe))
        .collect::<HashMap<_, _>>();
    let mut out = Vec::new();
    for companion in companions.iter().filter(|companion| {
        native_stream(&companion.id).is_some_and(|stream| {
            dimension_owners.contains(&(stream.to_owned(), companion.owner_record_index))
        })
    }) {
        let Some(stream) = native_stream(&companion.id) else {
            continue;
        };
        let Some(entry) = scan.entries.iter().find(|entry| {
            entry.role == role::BULKSTREAM
                && entry.name.contains("Design")
                && stream == ids::native_scope(&entry.name)
        }) else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        let Some(start) = usize::try_from(companion.payload_byte_offset).ok() else {
            continue;
        };
        let Some(end) = usize::try_from(companion.payload_byte_length)
            .ok()
            .and_then(|length| start.checked_add(length))
            .filter(|end| *end <= bytes.len())
        else {
            continue;
        };
        for (recipe_ordinal, recipe_id) in companion.owned_recipe_ids.iter().enumerate() {
            let Some(recipe) = recipes.get(recipe_id.as_str()).copied() else {
                continue;
            };
            let Some(recipe_offset) = usize::try_from(recipe.byte_offset).ok() else {
                continue;
            };
            let Some((at, class_tag, record_index, record_end)) =
                indexed_record_containing(bytes, start, end, recipe_offset)
            else {
                continue;
            };
            let Some(program_offset) = recipe_offset
                .checked_add(construction_recipe_family_name_len(recipe.kind))
                .filter(|offset| *offset < record_end)
            else {
                continue;
            };
            let Some((prefix_offset, prefix_bytes)) = recipe_record_prefix(
                bytes,
                at,
                recipe_offset,
                construction_recipe_family_name_len(recipe.kind),
            ) else {
                continue;
            };
            let references = decode_recipe_references(
                &prefix_bytes,
                u64::try_from(prefix_offset).unwrap_or(u64::MAX),
            );
            let Some(program) = contiguous_i32_program(bytes, program_offset, record_end) else {
                continue;
            };
            out.push(DesignDimensionRecipeRecord {
                id: ids::native_design_dimension_recipe_record_id(&entry.name, recipe.byte_offset),
                companion_record_index: companion.record_index,
                recipe_ordinal: u32::try_from(recipe_ordinal).unwrap_or(u32::MAX),
                recipe_id: recipe.id.clone(),
                byte_offset: u64::try_from(at).unwrap_or(u64::MAX),
                class_tag,
                record_index,
                frame_length: u64::try_from(record_end - at).unwrap_or(u64::MAX),
                prefix_offset: u64::try_from(prefix_offset).unwrap_or(u64::MAX),
                prefix_bytes,
                references,
                program_offset: u64::try_from(program_offset).unwrap_or(u64::MAX),
                program,
                matching_edge_operand_ids: Vec::new(),
            });
        }
    }
    out.sort_by_key(|record| record.id.clone());
    Ok(out)
}

pub(crate) fn decode_recipe_references(
    prefix: &[u8],
    prefix_offset: u64,
) -> Vec<crate::records::DesignRecipeReference> {
    if prefix
        .get(..10)
        .is_none_or(|bytes| bytes.iter().any(|byte| *byte != 0))
        || u32_at(prefix, 10) != Some(1)
        || u32_at(prefix, 14) != Some(3)
        || u32_at(prefix, 18).is_none_or(|value| value == 0)
        || u32_at(prefix, 22).is_none_or(|value| value == 0)
    {
        return Vec::new();
    }
    let mut references = Vec::new();
    let mut at = 22usize;
    while prefix.len().saturating_sub(at) > 4 {
        if recipe_reference_suffix(&prefix[at..]) {
            return references;
        }
        let Some(selector) = u32_at(prefix, at).filter(|value| *value != 0) else {
            return Vec::new();
        };
        let token_encoding_at = at + 4;
        let length_prefixed =
            lp_ascii_filtered(prefix, token_encoding_at, 0..=2000, u8::is_ascii_graphic).and_then(
                |(token, marker_at)| {
                    (!token.is_empty()
                        && token.bytes().all(|byte| byte.is_ascii_digit())
                        && u32_at(prefix, marker_at) == Some(0))
                    .then_some((token, token_encoding_at + 4, marker_at + 4))
                },
            );
        let packed = (1usize..=8).find_map(|length| {
            let token = prefix.get(token_encoding_at..token_encoding_at + length)?;
            let zero_at = token_encoding_at.checked_add(length)?;
            (token.iter().all(u8::is_ascii_digit)
                && prefix.get(zero_at..zero_at + 4) == Some(&[0; 4]))
            .then(|| std::str::from_utf8(token).ok())
            .flatten()
            .map(|token| (token.to_owned(), token_encoding_at, zero_at + 4))
        });
        let Some((token, token_at, marker_at)) = length_prefixed.or(packed) else {
            return Vec::new();
        };
        let Some(reference_count) = u32_at(prefix, marker_at).filter(|value| *value != 0) else {
            return Vec::new();
        };
        let Some(reference_bytes) = usize::try_from(reference_count)
            .ok()
            .and_then(|count| count.checked_mul(4))
        else {
            return Vec::new();
        };
        let references_at = marker_at + 4;
        let Some(terminator_at) = references_at.checked_add(reference_bytes) else {
            return Vec::new();
        };
        if u32_at(prefix, terminator_at) != Some(0) {
            return Vec::new();
        }
        for reference_ordinal in 0..reference_count as usize {
            let design_reference_at = references_at + 4 * reference_ordinal;
            let Some(design_reference) =
                u32_at(prefix, design_reference_at).filter(|value| *value != 0)
            else {
                return Vec::new();
            };
            references.push(crate::records::DesignRecipeReference {
                selector: i64::from(selector),
                selector_offset: prefix_offset.saturating_add(at as u64),
                token: token.clone(),
                token_offset: prefix_offset.saturating_add(token_at as u64),
                design_reference: i64::from(design_reference),
                design_reference_offset: prefix_offset.saturating_add(design_reference_at as u64),
                candidate_faces: Vec::new(),
                candidate_edges: Vec::new(),
                alternate_selector_faces: Vec::new(),
                alternate_selector_edges: Vec::new(),
            });
        }
        at = terminator_at + 4;
    }
    if prefix.get(at..) == Some(&[0, 0, 0, 0]) {
        references
    } else {
        Vec::new()
    }
}

fn recipe_reference_suffix(bytes: &[u8]) -> bool {
    if bytes == [0; 4] {
        return true;
    }
    if u32_at(bytes, 0) != Some(1)
        || u32_at(bytes, 4) != Some(1)
        || u32_at(bytes, 8) != Some(0)
        || u32_at(bytes, 12) != Some(0)
    {
        return false;
    }
    let Some(reference_count) = u32_at(bytes, 16).filter(|count| *count != 0) else {
        return false;
    };
    let Some(reference_bytes) = usize::try_from(reference_count)
        .ok()
        .and_then(|count| count.checked_mul(4))
    else {
        return false;
    };
    let Some(terminator_at) = 20usize.checked_add(reference_bytes) else {
        return false;
    };
    matches!(bytes.len().checked_sub(terminator_at), Some(4 | 6))
        && (0..reference_count as usize)
            .all(|ordinal| u32_at(bytes, 20 + 4 * ordinal).is_some_and(|reference| reference != 0))
        && bytes[terminator_at..].iter().all(|byte| *byte == 0)
}

/// Join dimension-recipe selector/reference pairs to active solved subentities.
pub fn bind_dimension_recipe_reference_candidates(
    records: &mut [DesignDimensionRecipeRecord],
    tags: &[PersistentSubentityTag],
) {
    for reference in records.iter_mut().flat_map(|record| &mut record.references) {
        bind_recipe_reference_candidates(reference, tags);
    }
}

pub(crate) fn bind_recipe_reference_candidates(
    reference: &mut crate::records::DesignRecipeReference,
    tags: &[PersistentSubentityTag],
) {
    reference.candidate_faces = recipe_reference_candidate_faces(reference, tags);
    reference.candidate_edges = recipe_reference_candidate_edges(reference, tags);
    reference.alternate_selector_faces = recipe_reference_alternate_selector_faces(reference, tags);
    reference.alternate_selector_edges = recipe_reference_alternate_selector_edges(reference, tags);
}

/// Join dimension programs to byte-identical edge-recipe program tails.
pub fn bind_dimension_recipe_edge_operands(
    records: &mut [DesignDimensionRecipeRecord],
    operands: &[DesignEdgeOperand],
) {
    for record in records {
        record.matching_edge_operand_ids =
            dimension_recipe_matching_edge_operand_ids(record, operands);
    }
}

pub(crate) fn dimension_recipe_matching_edge_operand_ids(
    record: &DesignDimensionRecipeRecord,
    operands: &[DesignEdgeOperand],
) -> Vec<String> {
    let mut ids = operands
        .iter()
        .filter(|operand| {
            let Some(tail) = operand
                .recipe_program
                .get(7..)
                .filter(|tail| !tail.is_empty())
            else {
                return false;
            };
            record
                .program
                .windows(tail.len())
                .any(|window| window == tail)
        })
        .map(|operand| operand.id.clone())
        .collect::<Vec<_>>();
    ids.sort();
    ids.dedup();
    ids
}

pub(crate) fn recipe_reference_candidate_edges(
    reference: &crate::records::DesignRecipeReference,
    tags: &[PersistentSubentityTag],
) -> Vec<cadmpeg_ir::ids::EdgeId> {
    use cadmpeg_ir::attributes::AttributeTarget;

    let mut edges = tags
        .iter()
        .filter(|tag| {
            tag.selector == reference.selector
                && tag.token == reference.token
                && tag.design_references.contains(&reference.design_reference)
        })
        .filter_map(|tag| match &tag.target {
            AttributeTarget::Edge(edge) => Some(edge.clone()),
            _ => None,
        })
        .collect::<Vec<_>>();
    edges.sort_by(|left, right| left.0.cmp(&right.0));
    edges.dedup();
    edges
}

pub(crate) fn recipe_reference_candidate_faces(
    reference: &crate::records::DesignRecipeReference,
    tags: &[PersistentSubentityTag],
) -> Vec<cadmpeg_ir::ids::FaceId> {
    use cadmpeg_ir::attributes::AttributeTarget;

    let mut faces = tags
        .iter()
        .filter(|tag| {
            tag.selector == reference.selector
                && tag.token == reference.token
                && tag.design_references.contains(&reference.design_reference)
        })
        .filter_map(|tag| match &tag.target {
            AttributeTarget::Face(face) => Some(face.clone()),
            _ => None,
        })
        .collect::<Vec<_>>();
    faces.sort_by(|left, right| left.0.cmp(&right.0));
    faces.dedup();
    faces
}

pub(crate) fn recipe_reference_alternate_selector_edges(
    reference: &crate::records::DesignRecipeReference,
    tags: &[PersistentSubentityTag],
) -> Vec<cadmpeg_ir::ids::EdgeId> {
    use cadmpeg_ir::attributes::AttributeTarget;

    let mut edges = tags
        .iter()
        .filter(|tag| {
            tag.selector != reference.selector
                && tag.token == reference.token
                && tag.design_references.contains(&reference.design_reference)
        })
        .filter_map(|tag| match &tag.target {
            AttributeTarget::Edge(edge) => Some(edge.clone()),
            _ => None,
        })
        .collect::<Vec<_>>();
    edges.sort_by(|left, right| left.0.cmp(&right.0));
    edges.dedup();
    edges
}

pub(crate) fn recipe_reference_alternate_selector_faces(
    reference: &crate::records::DesignRecipeReference,
    tags: &[PersistentSubentityTag],
) -> Vec<cadmpeg_ir::ids::FaceId> {
    use cadmpeg_ir::attributes::AttributeTarget;

    let mut faces = tags
        .iter()
        .filter(|tag| {
            tag.selector != reference.selector
                && tag.token == reference.token
                && tag.design_references.contains(&reference.design_reference)
        })
        .filter_map(|tag| match &tag.target {
            AttributeTarget::Face(face) => Some(face.clone()),
            _ => None,
        })
        .collect::<Vec<_>>();
    faces.sort_by(|left, right| left.0.cmp(&right.0));
    faces.dedup();
    faces
}

pub(crate) fn recipe_record_prefix(
    bytes: &[u8],
    record_offset: usize,
    family_name_offset: usize,
    family_name_len: usize,
) -> Option<(usize, Vec<u8>)> {
    let prefix_offset = record_offset.checked_add(11)?;
    let prefix_end = family_name_offset.checked_sub(4)?;
    if u32_at(bytes, prefix_end)? != u32::try_from(family_name_len).ok()? {
        return None;
    }
    let prefix = bytes.get(prefix_offset..prefix_end)?;
    Some((prefix_offset, prefix.to_vec()))
}

pub(crate) fn indexed_record_containing(
    bytes: &[u8],
    start: usize,
    end: usize,
    member_offset: usize,
) -> Option<(usize, String, u32, usize)> {
    if start > member_offset || member_offset >= end || end > bytes.len() {
        return None;
    }
    let mut cursor = start;
    let mut containing = None;
    while let Some(at) = next_indexed_record_offset(bytes, cursor) {
        if at >= end {
            break;
        }
        if at > member_offset {
            return containing
                .map(|(offset, class_tag, record_index)| (offset, class_tag, record_index, at));
        }
        let (class_tag, after_tag) = lp_ascii_filtered(bytes, at, 0..=2000, u8::is_ascii_graphic)?;
        containing = Some((at, class_tag, u32_at(bytes, after_tag)?));
        cursor = at + 11;
    }
    containing.map(|(offset, class_tag, record_index)| (offset, class_tag, record_index, end))
}

pub(crate) fn contiguous_i32_program(bytes: &[u8], start: usize, end: usize) -> Option<Vec<i32>> {
    let program = bytes.get(start..end)?;
    if program.is_empty() || !program.len().is_multiple_of(4) {
        return None;
    }
    Some(
        program
            .chunks_exact(4)
            .map(|word| {
                i32::from_le_bytes(
                    word.try_into()
                        .expect("invariant: chunks_exact(4) yields four-byte slices"),
                )
            })
            .collect(),
    )
}

/// Decode paired typed sketch loci nested immediately after dimensional
/// parameter-companion prefixes.
pub fn decode_dimension_locus_pairs(
    inputs: &DimensionDecodeInputs<'_>,
) -> Result<Vec<DesignDimensionLocusPair>, CodecError> {
    let &DimensionDecodeInputs {
        scan,
        parameters,
        owners,
        companions,
        scopes,
        headers,
        points,
        curves,
    } = inputs;
    let parameters = parameters
        .iter()
        .filter_map(|parameter| {
            Some((
                (native_stream(&parameter.id)?, parameter.record_index),
                parameter,
            ))
        })
        .collect::<HashMap<_, _>>();
    let dimension_companions = owners
        .iter()
        .filter(|owner| {
            let Some(scope) = native_stream(&owner.id) else {
                return false;
            };
            parameters
                .get(&(scope, owner.parameter_record_index))
                .is_some_and(|parameter| parameter.kind == DesignParameterKind::Dimension)
        })
        .filter_map(|owner| {
            Some((
                native_stream(&owner.id)?.to_owned(),
                owner.companion_record_index,
            ))
        })
        .collect::<HashSet<_>>();
    let mut out = Vec::new();
    for companion in companions.iter().filter(|companion| {
        native_stream(&companion.id).is_some_and(|scope| {
            dimension_companions.contains(&(scope.to_owned(), companion.record_index))
        })
    }) {
        let entry = scan.entries.iter().find(|entry| {
            entry.role == role::BULKSTREAM
                && entry.name.contains("Design")
                && companion
                    .id
                    .starts_with(&ids::native_scope_prefix(&entry.name))
        });
        let Some(entry) = entry else {
            continue;
        };
        let scope = native_stream(&companion.id).expect("entry matched companion stream");
        let geometry_indices = points
            .iter()
            .filter(|point| native_stream(&point.id) == Some(scope))
            .map(|point| point.record_index)
            .chain(
                curves
                    .iter()
                    .filter(|curve| native_stream(&curve.id) == Some(scope))
                    .map(|curve| curve.record_index),
            )
            .collect::<HashSet<_>>();
        let bytes = scan.entry_bytes(&entry.name)?;
        let Some((start, end)) = companion_owned_interval(
            companion,
            parameters.values().copied(),
            owners,
            scopes,
            headers,
            bytes.len(),
        ) else {
            continue;
        };
        let Some(mut pair) =
            find_dimension_locus_pair(bytes, start, end, companion.record_index, &geometry_indices)
        else {
            continue;
        };
        pair.id = ids::native_design_dimension_locus_pair_id(&entry.name, pair.byte_offset);
        let Some(governing_companion_record_index) = following_dimension_companion_record_index(
            &pair.id,
            pair.paired_byte_offset,
            owners,
            parameters.values().copied(),
        ) else {
            continue;
        };
        pair.governing_companion_record_index = governing_companion_record_index;
        out.push(pair);
    }
    out.sort_by_key(|pair| pair.id.clone());
    Ok(out)
}

pub(crate) fn following_dimension_companion_record_index<'a>(
    native_id: &str,
    paired_byte_offset: u64,
    owners: &[DesignParameterOwner],
    parameter_records: impl IntoIterator<Item = &'a DesignParameter>,
) -> Option<u32> {
    let scope = native_stream(native_id)?;
    let mut parameters = HashMap::<u32, Option<&DesignParameter>>::new();
    for parameter in parameter_records
        .into_iter()
        .filter(|parameter| native_stream(&parameter.id) == Some(scope))
    {
        parameters
            .entry(parameter.record_index)
            .and_modify(|parameter| *parameter = None)
            .or_insert(Some(parameter));
    }
    let mut matches = owners.iter().filter(|owner| {
        native_stream(&owner.id) == Some(scope)
            && owner.byte_offset == paired_byte_offset.saturating_add(59)
            && parameters
                .get(&owner.parameter_record_index)
                .and_then(|parameter| *parameter)
                .is_some_and(|parameter| parameter.kind == DesignParameterKind::Dimension)
    });
    let owner = matches.next()?;
    matches
        .next()
        .is_none()
        .then_some(owner.companion_record_index)
}

pub(crate) fn find_dimension_locus_pair(
    bytes: &[u8],
    start: usize,
    end: usize,
    companion_record_index: u32,
    geometry_indices: &HashSet<u32>,
) -> Option<DesignDimensionLocusPair> {
    let parse = |at| {
        parse_dimension_locus_pair(bytes, at, companion_record_index, geometry_indices)
            .filter(|pair| usize::try_from(pair.paired_byte_offset).is_ok_and(|at| at < end))
    };
    let mut candidates = parse(start).into_iter().collect::<Vec<_>>();
    let mut position = start.saturating_add(1);
    while let Some(at) = next_indexed_record_offset(bytes, position) {
        if at >= end {
            break;
        }
        if let Some(pair) = parse(at) {
            candidates.push(pair);
        }
        position = at.saturating_add(1);
    }
    let [pair] = candidates.as_slice() else {
        return None;
    };
    Some(pair.clone())
}

pub(crate) fn parse_dimension_locus_pair(
    bytes: &[u8],
    start: usize,
    companion_record_index: u32,
    geometry_indices: &HashSet<u32>,
) -> Option<DesignDimensionLocusPair> {
    let (class_tag, after_tag) = lp_ascii_filtered(bytes, start, 0..=2000, u8::is_ascii_graphic)?;
    let record_index = u32_at(bytes, after_tag)?;
    if after_tag != start.checked_add(7)?
        || class_tag.len() != 3
        || !class_tag.bytes().all(|byte| byte.is_ascii_digit())
        || bytes.get(start + 11..start + 19) != Some(&[0; 8])
        || bytes.get(start + 19) != Some(&1)
        || u32_at(bytes, start + 20) != Some(3)
        || bytes.get(start + 24) != Some(&1)
        || u32_at(bytes, start + 25) != Some(0)
        || bytes.get(start + 29..start + 35) != Some(&[0; 6])
        || bytes.get(start + 39) != Some(&1)
        || bytes.get(start + 44..start + 50) != Some(&[0; 6])
        || bytes.get(start + 54) != Some(&1)
        || bytes.get(start + 59..start + 65) != Some(&[0; 6])
    {
        return None;
    }
    let first_geometry_record_index = u32_at(bytes, start + 40)?;
    let second_geometry_record_index = u32_at(bytes, start + 55)?;
    if !geometry_indices.contains(&first_geometry_record_index)
        || !geometry_indices.contains(&second_geometry_record_index)
    {
        return None;
    }
    let mut position = start.checked_add(69)?;
    let (paired_byte_offset, paired_class_tag) = loop {
        let at = next_indexed_record_offset(bytes, position)?;
        let (candidate_tag, candidate_after_tag) =
            lp_ascii_filtered(bytes, at, 0..=2000, u8::is_ascii_graphic)?;
        if u32_at(bytes, candidate_after_tag) == Some(record_index) {
            break (at, candidate_tag);
        }
        position = at.checked_add(1)?;
    };
    Some(DesignDimensionLocusPair {
        id: String::new(),
        companion_record_index,
        governing_companion_record_index: companion_record_index,
        byte_offset: start as u64,
        class_tag,
        record_index,
        frame_length: u64::try_from(paired_byte_offset.checked_sub(start)?).ok()?,
        opaque_index: u32_at(bytes, start + 35)?,
        opaque_index_offset: (start + 35) as u64,
        first_geometry_record_index,
        first_geometry_reference_offset: (start + 40) as u64,
        first_role: u32_at(bytes, start + 50)?,
        first_role_offset: (start + 50) as u64,
        second_geometry_record_index,
        second_geometry_reference_offset: (start + 55) as u64,
        second_role: u32_at(bytes, start + 65)?,
        second_role_offset: (start + 65) as u64,
        paired_class_tag,
        paired_byte_offset: paired_byte_offset as u64,
    })
}

/// Decode dimension frames whose ordered operand run contains a null record
/// reference followed by one typed sketch-geometry reference.
pub fn decode_dimension_null_locus_pairs(
    inputs: &DimensionDecodeInputs<'_>,
    pairs: &[DesignDimensionLocusPair],
    groups: &[DesignDimensionLocusGroup],
) -> Result<Vec<DesignDimensionNullLocusPair>, CodecError> {
    let &DimensionDecodeInputs {
        scan,
        parameters,
        owners,
        companions,
        scopes,
        headers,
        points,
        curves,
    } = inputs;
    let parameters = parameters
        .iter()
        .filter_map(|parameter| {
            Some((
                (native_stream(&parameter.id)?, parameter.record_index),
                parameter,
            ))
        })
        .collect::<HashMap<_, _>>();
    let dimension_companions = owners
        .iter()
        .filter(|owner| {
            let Some(scope) = native_stream(&owner.id) else {
                return false;
            };
            parameters
                .get(&(scope, owner.parameter_record_index))
                .is_some_and(|parameter| parameter.kind == DesignParameterKind::Dimension)
        })
        .filter_map(|owner| {
            Some((
                native_stream(&owner.id)?.to_owned(),
                owner.companion_record_index,
            ))
        })
        .collect::<HashSet<_>>();
    let typed_companions = pairs
        .iter()
        .filter_map(|pair| {
            Some((
                native_stream(&pair.id)?.to_owned(),
                pair.companion_record_index,
            ))
        })
        .chain(groups.iter().filter_map(|group| {
            Some((
                native_stream(&group.id)?.to_owned(),
                group.companion_record_index,
            ))
        }))
        .collect::<HashSet<_>>();
    let mut out = Vec::new();
    for companion in companions.iter().filter(|companion| {
        native_stream(&companion.id).is_some_and(|scope| {
            let key = (scope.to_owned(), companion.record_index);
            dimension_companions.contains(&key) && !typed_companions.contains(&key)
        })
    }) {
        let entry = scan.entries.iter().find(|entry| {
            entry.role == role::BULKSTREAM
                && entry.name.contains("Design")
                && companion
                    .id
                    .starts_with(&ids::native_scope_prefix(&entry.name))
        });
        let Some(entry) = entry else {
            continue;
        };
        let scope = native_stream(&companion.id).expect("entry matched companion stream");
        let geometry_indices = points
            .iter()
            .filter(|point| native_stream(&point.id) == Some(scope))
            .map(|point| point.record_index)
            .chain(
                curves
                    .iter()
                    .filter(|curve| native_stream(&curve.id) == Some(scope))
                    .map(|curve| curve.record_index),
            )
            .collect::<HashSet<_>>();
        let bytes = scan.entry_bytes(&entry.name)?;
        let Some((start, end)) = companion_owned_interval(
            companion,
            parameters.values().copied(),
            owners,
            scopes,
            headers,
            bytes.len(),
        ) else {
            continue;
        };
        let Some(mut pair) = find_dimension_null_locus_pair(
            bytes,
            start,
            end,
            companion.record_index,
            &geometry_indices,
        ) else {
            continue;
        };
        pair.id = ids::native_design_dimension_null_locus_pair_id(&entry.name, pair.byte_offset);
        let Some(governing_companion_record_index) = following_dimension_companion_record_index(
            &pair.id,
            pair.paired_byte_offset,
            owners,
            parameters.values().copied(),
        ) else {
            continue;
        };
        pair.governing_companion_record_index = governing_companion_record_index;
        out.push(pair);
    }
    out.sort_by_key(|pair| pair.id.clone());
    Ok(out)
}

pub(crate) fn find_dimension_null_locus_pair(
    bytes: &[u8],
    start: usize,
    end: usize,
    companion_record_index: u32,
    geometry_indices: &HashSet<u32>,
) -> Option<DesignDimensionNullLocusPair> {
    let parse = |at| {
        parse_dimension_null_locus_pair(bytes, at, companion_record_index, geometry_indices)
            .filter(|pair| usize::try_from(pair.paired_byte_offset).is_ok_and(|at| at < end))
    };
    let mut candidates = parse(start).into_iter().collect::<Vec<_>>();
    let mut position = start.saturating_add(1);
    while let Some(at) = next_indexed_record_offset(bytes, position) {
        if at >= end {
            break;
        }
        if let Some(pair) = parse(at) {
            candidates.push(pair);
        }
        position = at.saturating_add(1);
    }
    candidates.sort_by_key(|pair| pair.byte_offset);
    candidates.dedup_by_key(|pair| pair.byte_offset);
    let [pair] = candidates.as_slice() else {
        return None;
    };
    Some(pair.clone())
}

pub(crate) fn parse_dimension_null_locus_pair(
    bytes: &[u8],
    start: usize,
    companion_record_index: u32,
    geometry_indices: &HashSet<u32>,
) -> Option<DesignDimensionNullLocusPair> {
    let (class_tag, after_tag) = lp_ascii_filtered(bytes, start, 0..=2000, u8::is_ascii_graphic)?;
    let record_index = u32_at(bytes, after_tag)?;
    if after_tag != start.checked_add(7)?
        || class_tag.len() != 3
        || !class_tag.bytes().all(|byte| byte.is_ascii_digit())
        || bytes.get(start + 11..start + 19) != Some(&[0; 8])
        || bytes.get(start + 19) != Some(&1)
        || u32_at(bytes, start + 20) != Some(2)
        || bytes.get(start + 24) != Some(&1)
        || u32_at(bytes, start + 25) != Some(0)
        || bytes.get(start + 29..start + 35) != Some(&[0; 6])
        || bytes.get(start + 39) != Some(&1)
        || bytes.get(start + 44..start + 50) != Some(&[0; 6])
    {
        return None;
    }
    let geometry_record_index = u32_at(bytes, start + 40)?;
    if !geometry_indices.contains(&geometry_record_index) {
        return None;
    }
    let mut position = start.checked_add(54)?;
    let (paired_byte_offset, paired_class_tag) = loop {
        let at = next_indexed_record_offset(bytes, position)?;
        let (candidate_tag, candidate_after_tag) =
            lp_ascii_filtered(bytes, at, 0..=2000, u8::is_ascii_graphic)?;
        if u32_at(bytes, candidate_after_tag) == Some(record_index) {
            break (at, candidate_tag);
        }
        position = at.checked_add(1)?;
    };
    Some(DesignDimensionNullLocusPair {
        id: String::new(),
        companion_record_index,
        governing_companion_record_index: companion_record_index,
        byte_offset: start as u64,
        class_tag,
        record_index,
        frame_length: u64::try_from(paired_byte_offset.checked_sub(start)?).ok()?,
        null_reference_offset: (start + 25) as u64,
        null_role: u32_at(bytes, start + 35)?,
        null_role_offset: (start + 35) as u64,
        geometry_record_index,
        geometry_reference_offset: (start + 40) as u64,
        geometry_role: u32_at(bytes, start + 50)?,
        geometry_role_offset: (start + 50) as u64,
        paired_class_tag,
        paired_byte_offset: paired_byte_offset as u64,
    })
}

/// Decode paired `EntityGenesis` dimensional frames carrying annotation data
/// and a direct backlink to the governed parameter owner.
pub fn decode_dimension_annotation_frames(
    inputs: &DimensionDecodeInputs<'_>,
    entities: &[DesignEntityHeader],
) -> Result<Vec<DesignDimensionAnnotationFrame>, CodecError> {
    let &DimensionDecodeInputs {
        scan,
        parameters,
        owners,
        companions,
        scopes,
        headers,
        points,
        curves,
    } = inputs;
    let parameters = parameters
        .iter()
        .filter_map(|parameter| {
            Some((
                (native_stream(&parameter.id)?, parameter.record_index),
                parameter,
            ))
        })
        .collect::<HashMap<_, _>>();
    let dimension_companions = owners
        .iter()
        .filter(|owner| {
            let Some(stream) = native_stream(&owner.id) else {
                return false;
            };
            parameters
                .get(&(stream, owner.parameter_record_index))
                .is_some_and(|parameter| parameter.kind == DesignParameterKind::Dimension)
        })
        .filter_map(|owner| {
            Some((
                (
                    native_stream(&owner.id)?.to_owned(),
                    owner.companion_record_index,
                ),
                owner,
            ))
        })
        .collect::<HashMap<_, _>>();
    let mut out = Vec::new();
    let streams = companions
        .iter()
        .filter_map(|companion| native_stream(&companion.id))
        .collect::<HashSet<_>>();
    let mut decoded_offsets = HashSet::new();
    for stream in streams {
        let Some(entry) = scan.entries.iter().find(|entry| {
            entry.role == role::BULKSTREAM
                && entry.name.contains("Design")
                && stream == ids::native_scope(&entry.name)
        }) else {
            continue;
        };
        let geometry_indices = points
            .iter()
            .filter(|point| native_stream(&point.id) == Some(stream))
            .map(|point| point.record_index)
            .chain(
                curves
                    .iter()
                    .filter(|curve| native_stream(&curve.id) == Some(stream))
                    .map(|curve| curve.record_index),
            )
            .collect::<HashSet<_>>();
        let sketch_entities = entities
            .iter()
            .filter(|entity| {
                native_stream(&entity.id) == Some(stream)
                    && entity.object_kind == Some(DesignObjectKind::Sketch)
            })
            .filter_map(|entity| u32::try_from(entity.entity_suffix).ok())
            .collect::<HashSet<_>>();
        let governed_owners = owners
            .iter()
            .filter(|owner| {
                native_stream(&owner.id) == Some(stream)
                    && dimension_companions
                        .contains_key(&(stream.to_owned(), owner.companion_record_index))
            })
            .map(|owner| (owner.record_index, owner.companion_record_index))
            .collect::<HashMap<_, _>>();
        let bytes = scan.entry_bytes(&entry.name)?;
        let mut intervals = companions
            .iter()
            .filter(|companion| native_stream(&companion.id) == Some(stream))
            .filter_map(|companion| {
                let (start, end) = companion_owned_interval(
                    companion,
                    parameters.values().copied(),
                    owners,
                    scopes,
                    headers,
                    bytes.len(),
                )?;
                Some((start, end, Some(companion.record_index)))
            })
            .collect::<Vec<_>>();
        intervals.extend(scopes.iter().filter_map(|scope| {
            if native_stream(&scope.id) != Some(stream) {
                return None;
            }
            let end = owners
                .iter()
                .filter(|owner| {
                    native_stream(&owner.id) == Some(stream)
                        && owner.scope_record_index == scope.record_index
                })
                .filter_map(|owner| {
                    companions
                        .iter()
                        .find(|companion| {
                            native_stream(&companion.id) == Some(stream)
                                && companion.record_index == owner.companion_record_index
                        })
                        .and_then(|companion| usize::try_from(companion.byte_offset).ok())
                })
                .min()?;
            let start = usize::try_from(scope.byte_offset).ok()?;
            (start < end).then_some((start, end, None))
        }));
        for (start, end, containing_companion_record_index) in intervals {
            let mut position = start;
            while position < end {
                let at = next_indexed_record_offset(bytes, position);
                let Some(at) = at.filter(|at| *at < end) else {
                    break;
                };
                if let Some(mut frame) = parse_dimension_annotation_frame(
                    bytes,
                    at,
                    containing_companion_record_index,
                    &governed_owners,
                    &geometry_indices,
                    &sketch_entities,
                )
                .filter(|frame| frame.paired_byte_offset < end as u64)
                {
                    frame.id = ids::native_design_dimension_annotation_frame_id(
                        &entry.name,
                        frame.byte_offset,
                    );
                    position = usize::try_from(frame.paired_byte_offset)
                        .unwrap_or(at)
                        .saturating_add(1);
                    if decoded_offsets.insert((stream.to_owned(), frame.byte_offset)) {
                        out.push(frame);
                    }
                } else {
                    position = at.saturating_add(1);
                }
            }
        }
    }
    out.sort_by_key(|frame| frame.id.clone());
    Ok(out)
}

pub(crate) fn parse_dimension_annotation_frame(
    bytes: &[u8],
    start: usize,
    companion_record_index: Option<u32>,
    governed_owners: &HashMap<u32, u32>,
    geometry_indices: &HashSet<u32>,
    sketch_entities: &HashSet<u32>,
) -> Option<DesignDimensionAnnotationFrame> {
    let (class_tag, after_tag) = lp_ascii_filtered(bytes, start, 0..=2000, u8::is_ascii_graphic)?;
    if after_tag != start.checked_add(7)?
        || class_tag.len() != 3
        || !class_tag.bytes().all(|byte| byte.is_ascii_digit())
        || bytes.get(start + 11..start + 19) != Some(&[0; 8])
        || bytes.get(start + 19) != Some(&1)
    {
        return None;
    }
    let record_index = u32_at(bytes, after_tag)?;
    let count = usize::try_from(u32_at(bytes, start + 20)?).ok()?;
    if !(1..=64).contains(&count) {
        return None;
    }
    let mut position = start.checked_add(24)?;
    let mut operands = Vec::with_capacity(count);
    for _ in 0..count {
        if bytes.get(position) != Some(&1)
            || bytes.get(position + 5..position + 11) != Some(&[0; 6])
        {
            return None;
        }
        let geometry_record_index = u32_at(bytes, position + 1)?;
        if geometry_record_index != 0 && !geometry_indices.contains(&geometry_record_index) {
            return None;
        }
        operands.push(DesignDimensionAnnotationOperand {
            geometry_record_index,
            geometry_reference_offset: (position + 1) as u64,
            role: u32_at(bytes, position + 11)?,
            role_offset: (position + 11) as u64,
        });
        position = position.checked_add(15)?;
    }
    if bytes.get(position) != Some(&1) || u32_at(bytes, position + 1) != Some(1) {
        return None;
    }
    let (key, after_key) = lp_ascii_filtered(bytes, position + 5, 0..=2000, u8::is_ascii_graphic)?;
    let (meta_type, after_type) =
        lp_ascii_filtered(bytes, after_key, 0..=2000, u8::is_ascii_graphic)?;
    if key != "EntityGenesis" || meta_type != "IntrinsicMetaTypeuint64" {
        return None;
    }
    let entity_genesis =
        u64::from_le_bytes(bytes.get(after_type..after_type + 8)?.try_into().ok()?);
    let annotation_byte_offset = after_type.checked_add(8)?;
    let mut paired_search = annotation_byte_offset;
    let (paired_byte_offset, paired_class_tag) = loop {
        let at = next_indexed_record_offset(bytes, paired_search)?;
        let (tag, after) = lp_ascii_filtered(bytes, at, 0..=2000, u8::is_ascii_graphic)?;
        if u32_at(bytes, after) == Some(record_index) {
            break (at, tag);
        }
        paired_search = at.checked_add(1)?;
    };
    let mut tails = Vec::new();
    for tail in annotation_byte_offset..paired_byte_offset.saturating_sub(15) {
        if bytes.get(tail) != Some(&1) || bytes.get(tail + 5..tail + 11) != Some(&[0; 6]) {
            continue;
        }
        let Some(governing_owner_record_index) = u32_at(bytes, tail + 1) else {
            continue;
        };
        let Some(governing_companion_record_index) =
            governed_owners.get(&governing_owner_record_index).copied()
        else {
            continue;
        };
        let Some(return_count) =
            u32_at(bytes, tail + 11).and_then(|count| usize::try_from(count).ok())
        else {
            continue;
        };
        if return_count > 64 {
            continue;
        }
        let mut cursor = tail + 15;
        let mut return_members = Vec::with_capacity(return_count);
        let mut return_member_offsets = Vec::with_capacity(return_count);
        let mut valid = true;
        for _ in 0..return_count {
            if bytes.get(cursor) != Some(&1) || bytes.get(cursor + 5..cursor + 11) != Some(&[0; 6])
            {
                valid = false;
                break;
            }
            let Some(reference) = u32_at(bytes, cursor + 1) else {
                valid = false;
                break;
            };
            if !geometry_indices.contains(&reference) {
                valid = false;
                break;
            }
            return_members.push(reference);
            return_member_offsets.push((cursor + 1) as u64);
            cursor += 11;
        }
        if !valid
            || bytes
                .get(cursor..paired_byte_offset)?
                .iter()
                .any(|byte| *byte != 0)
        {
            continue;
        }
        let mut operand_members = operands
            .iter()
            .filter_map(|operand| {
                (operand.geometry_record_index != 0).then_some(operand.geometry_record_index)
            })
            .collect::<Vec<_>>();
        let mut returned = return_members.clone();
        operand_members.sort_unstable();
        returned.sort_unstable();
        if operand_members != returned {
            continue;
        }
        tails.push((
            tail,
            governing_owner_record_index,
            governing_companion_record_index,
            return_members,
            return_member_offsets,
        ));
    }
    let [(
        tail,
        governing_owner_record_index,
        governing_companion_record_index,
        return_members,
        return_member_offsets,
    )] = tails.as_slice()
    else {
        return None;
    };
    if bytes.get(paired_byte_offset + 11..paired_byte_offset + 19) != Some(&[0; 8])
        || bytes.get(paired_byte_offset + 19) != Some(&1)
        || bytes.get(paired_byte_offset + 24..paired_byte_offset + 30) != Some(&[0; 6])
    {
        return None;
    }
    let owner_reference = u32_at(bytes, paired_byte_offset + 20)?;
    if !sketch_entities.contains(&owner_reference) {
        return None;
    }
    Some(DesignDimensionAnnotationFrame {
        id: String::new(),
        companion_record_index,
        governing_companion_record_index: *governing_companion_record_index,
        byte_offset: start as u64,
        class_tag,
        record_index,
        frame_length: u64::try_from(paired_byte_offset.checked_sub(start)?).ok()?,
        operands,
        entity_genesis,
        annotation_bytes: bytes.get(annotation_byte_offset..*tail)?.to_vec(),
        annotation_byte_offset: annotation_byte_offset as u64,
        governing_owner_record_index: *governing_owner_record_index,
        governing_owner_reference_offset: (*tail + 1) as u64,
        return_members: return_members.clone(),
        return_member_offsets: return_member_offsets.clone(),
        paired_class_tag,
        paired_byte_offset: paired_byte_offset as u64,
        owner_reference,
        owner_reference_offset: (paired_byte_offset + 20) as u64,
    })
}

/// Decode counted typed sketch loci nested immediately after dimensional
/// parameter-companion prefixes.
pub fn decode_dimension_locus_groups(
    inputs: &DimensionDecodeInputs<'_>,
    entities: &[DesignEntityHeader],
) -> Result<Vec<DesignDimensionLocusGroup>, CodecError> {
    let &DimensionDecodeInputs {
        scan,
        parameters,
        owners,
        companions,
        scopes,
        headers,
        points,
        curves,
    } = inputs;
    let parameters = parameters
        .iter()
        .filter_map(|parameter| {
            Some((
                (native_stream(&parameter.id)?, parameter.record_index),
                parameter,
            ))
        })
        .collect::<HashMap<_, _>>();
    let dimension_companions = owners
        .iter()
        .filter(|owner| {
            let Some(scope) = native_stream(&owner.id) else {
                return false;
            };
            parameters
                .get(&(scope, owner.parameter_record_index))
                .is_some_and(|parameter| parameter.kind == DesignParameterKind::Dimension)
        })
        .filter_map(|owner| {
            Some((
                native_stream(&owner.id)?.to_owned(),
                owner.companion_record_index,
            ))
        })
        .collect::<HashSet<_>>();
    let mut out = Vec::new();
    for companion in companions.iter().filter(|companion| {
        native_stream(&companion.id).is_some_and(|scope| {
            dimension_companions.contains(&(scope.to_owned(), companion.record_index))
        })
    }) {
        let entry = scan.entries.iter().find(|entry| {
            entry.role == role::BULKSTREAM
                && entry.name.contains("Design")
                && companion
                    .id
                    .starts_with(&ids::native_scope_prefix(&entry.name))
        });
        let Some(entry) = entry else {
            continue;
        };
        let scope = native_stream(&companion.id).expect("entry matched companion stream");
        let geometry_indices = points
            .iter()
            .filter(|point| native_stream(&point.id) == Some(scope))
            .map(|point| point.record_index)
            .chain(
                curves
                    .iter()
                    .filter(|curve| native_stream(&curve.id) == Some(scope))
                    .map(|curve| curve.record_index),
            )
            .collect::<HashSet<_>>();
        let sketch_entities = entities
            .iter()
            .filter(|entity| {
                native_stream(&entity.id) == Some(scope)
                    && entity.object_kind == Some(DesignObjectKind::Sketch)
            })
            .filter_map(|entity| u32::try_from(entity.entity_suffix).ok())
            .collect::<HashSet<_>>();
        let bytes = scan.entry_bytes(&entry.name)?;
        let Some((start, end)) = companion_owned_interval(
            companion,
            parameters.values().copied(),
            owners,
            scopes,
            headers,
            bytes.len(),
        ) else {
            continue;
        };
        let candidates = find_dimension_locus_groups(
            bytes,
            start,
            end,
            companion.record_index,
            &geometry_indices,
            &sketch_entities,
        );
        out.extend(candidates.into_iter().map(|mut group| {
            group.id = ids::native_design_dimension_locus_group_id(&entry.name, group.byte_offset);
            group
        }));
    }
    out.sort_by_key(|group| group.id.clone());
    Ok(out)
}

pub(crate) fn find_dimension_locus_groups(
    bytes: &[u8],
    start: usize,
    end: usize,
    companion_record_index: u32,
    geometry_indices: &HashSet<u32>,
    sketch_entities: &HashSet<u32>,
) -> Vec<DesignDimensionLocusGroup> {
    let parse = |at| {
        parse_dimension_locus_group(
            bytes,
            at,
            companion_record_index,
            geometry_indices,
            sketch_entities,
        )
        .filter(|group| usize::try_from(group.next_byte_offset).is_ok_and(|at| at <= end))
    };
    let mut candidates = parse(start).into_iter().collect::<Vec<_>>();
    let mut position = start.saturating_add(1);
    while let Some(at) = next_indexed_record_offset(bytes, position) {
        if at >= end {
            break;
        }
        if let Some(group) = parse(at) {
            candidates.push(group);
        }
        position = at.saturating_add(1);
    }
    candidates.sort_by_key(|group| group.byte_offset);
    candidates.dedup_by_key(|group| group.byte_offset);
    candidates
}

pub(crate) fn companion_owned_interval<'a>(
    companion: &DesignParameterCompanion,
    parameters: impl IntoIterator<Item = &'a DesignParameter>,
    owners: &[DesignParameterOwner],
    scopes: &[DesignParameterScope],
    headers: &[DesignRecordHeader],
    stream_length: usize,
) -> Option<(usize, usize)> {
    let native_scope = native_stream(&companion.id)?;
    let owning_scope_record_index = owners
        .iter()
        .find(|owner| {
            native_stream(&owner.id) == Some(native_scope)
                && owner.record_index == companion.owner_record_index
        })
        .map(|owner| owner.scope_record_index);
    let foreign_scope_members = scopes
        .iter()
        .filter(|scope| {
            native_stream(&scope.id) == Some(native_scope)
                && Some(scope.record_index) != owning_scope_record_index
        })
        .flat_map(|scope| scope.reference_members.iter().copied())
        .collect::<HashSet<_>>();
    let start = usize::try_from(companion.byte_offset)
        .ok()?
        .checked_add(58)?;
    let end = owners
        .iter()
        .filter(|owner| {
            native_stream(&owner.id) == Some(native_scope)
                && owner.byte_offset > companion.byte_offset
        })
        .filter_map(|owner| usize::try_from(owner.byte_offset).ok())
        .chain(
            parameters
                .into_iter()
                .filter(|parameter| {
                    native_stream(&parameter.id) == Some(native_scope)
                        && parameter.byte_offset > companion.byte_offset
                })
                .filter_map(|parameter| usize::try_from(parameter.byte_offset).ok()),
        )
        .chain(
            scopes
                .iter()
                .filter(|scope| {
                    native_stream(&scope.id) == Some(native_scope)
                        && scope.byte_offset > companion.byte_offset
                })
                .filter_map(|scope| usize::try_from(scope.byte_offset).ok()),
        )
        .chain(
            headers
                .iter()
                .filter(|header| {
                    native_stream(&header.id) == Some(native_scope)
                        && header.byte_offset > companion.byte_offset
                        && foreign_scope_members.contains(&header.record_index)
                })
                .filter_map(|header| usize::try_from(header.byte_offset).ok()),
        )
        .min()
        .unwrap_or(stream_length);
    (start <= end && end <= stream_length).then_some((start, end))
}

pub(crate) fn parse_dimension_locus_group(
    bytes: &[u8],
    start: usize,
    companion_record_index: u32,
    geometry_indices: &HashSet<u32>,
    sketch_entities: &HashSet<u32>,
) -> Option<DesignDimensionLocusGroup> {
    let (class_tag, after_tag) = lp_ascii_filtered(bytes, start, 0..=2000, u8::is_ascii_graphic)?;
    if after_tag != start.checked_add(7)?
        || class_tag.len() != 3
        || !class_tag.bytes().all(|byte| byte.is_ascii_digit())
        || bytes.get(start + 11..start + 19) != Some(&[0; 8])
        || bytes.get(start + 19) != Some(&1)
    {
        return None;
    }
    let record_index = u32_at(bytes, start + 7)?;
    let count = usize::try_from(u32_at(bytes, start + 20)?).ok()?;
    if !(1..=64).contains(&count) {
        return None;
    }
    let mut position = start.checked_add(24)?;
    let mut loci = Vec::with_capacity(count);
    for _ in 0..count {
        if bytes.get(position) != Some(&1)
            || bytes.get(position + 5..position + 11) != Some(&[0; 6])
        {
            return None;
        }
        let geometry_record_index = u32_at(bytes, position + 1)?;
        if !geometry_indices.contains(&geometry_record_index) {
            return None;
        }
        loci.push(DesignDimensionLocus {
            geometry_record_index,
            geometry_reference_offset: (position + 1) as u64,
            role: u32_at(bytes, position + 11)?,
            role_offset: (position + 11) as u64,
        });
        position = position.checked_add(15)?;
    }
    if bytes.get(position) != Some(&0)
        || bytes.get(position + 1) != Some(&1)
        || bytes.get(position + 6..position + 12) != Some(&[0; 6])
    {
        return None;
    }
    let owner_reference = u32_at(bytes, position + 2)?;
    if !sketch_entities.contains(&owner_reference) {
        return None;
    }
    let owner_reference_offset = (position + 2) as u64;
    let owner_role = u32_at(bytes, position + 12)?;
    let owner_role_offset = (position + 12) as u64;
    position = position.checked_add(16)?;
    let state = u32_at(bytes, position)?;
    let state_offset = position as u64;
    let return_count = usize::try_from(u32_at(bytes, position + 4)?).ok()?;
    if return_count != count {
        return None;
    }
    position = position.checked_add(8)?;
    let mut return_members = Vec::with_capacity(return_count);
    let mut return_member_offsets = Vec::with_capacity(return_count);
    for _ in 0..return_count {
        if bytes.get(position) != Some(&1)
            || bytes.get(position + 5..position + 11) != Some(&[0; 6])
        {
            return None;
        }
        let record_index = u32_at(bytes, position + 1)?;
        if !geometry_indices.contains(&record_index) {
            return None;
        }
        return_members.push(record_index);
        return_member_offsets.push((position + 1) as u64);
        position = position.checked_add(11)?;
    }
    if bytes.get(position) != Some(&0) {
        return None;
    }
    let next_byte_offset = position.checked_add(1)?;
    let (next_class_tag, next_after_tag) =
        lp_ascii_filtered(bytes, next_byte_offset, 0..=2000, u8::is_ascii_graphic)?;
    if next_after_tag != next_byte_offset.checked_add(7)?
        || next_class_tag.len() != 3
        || !next_class_tag.bytes().all(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    let (constraint_kinds, unknown_constraint_bits) = decode_constraint_kinds(u64::from(state));
    Some(DesignDimensionLocusGroup {
        id: String::new(),
        companion_record_index,
        byte_offset: start as u64,
        class_tag,
        record_index,
        frame_length: u64::try_from(next_byte_offset.checked_sub(start)?).ok()?,
        loci,
        owner_reference,
        owner_reference_offset,
        owner_role,
        owner_role_offset,
        state,
        state_offset,
        constraint_kinds,
        unknown_constraint_bits: unknown_constraint_bits as u32,
        return_members,
        return_member_offsets,
        next_class_tag,
        next_record_index: u32_at(bytes, next_after_tag)?,
        next_byte_offset: next_byte_offset as u64,
    })
}
