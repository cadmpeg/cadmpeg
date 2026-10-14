// SPDX-License-Identifier: Apache-2.0
//! Complete surface allocations and their distinct source record ownership.

use super::payload_content::{FeaturePayloadBlock, FeaturePayloadContent};
use super::surface_branches::FeatureSurfaceConstructionBranch;
use super::{offset_data_block_bytes, FeatureOperationLabel, FeatureSurfaceConstructionReference};
use crate::container::Container;
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::BTreeSet;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "Vec<String>")]
pub(in crate::native) struct SurfaceConstructionBranches(Vec<String>);

impl TryFrom<Vec<String>> for SurfaceConstructionBranches {
    type Error = &'static str;

    fn try_from(branches: Vec<String>) -> Result<Self, Self::Error> {
        if !(1..=255).contains(&branches.len()) {
            return Err("construction_branches must contain 1 through 255 branches");
        }
        Ok(Self(branches))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "operation_kind", rename_all = "snake_case")]
pub(in crate::native) enum SurfaceAllocationSources {
    Skin {
        construction_branches: SurfaceConstructionBranches,
    },
    StudioSurface {
        construction_branches: SurfaceConstructionBranches,
        post_branch_references: [String; 8],
    },
}

/// The complete ordered allocation, including envelope and branch storage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(in crate::native) struct FeatureSurfaceConstructionPayload {
    pub(in crate::native) id: String,
    pub(in crate::native) operation_label: String,
    /// The fourteen common-envelope reference records in envelope slot order.
    pub(super) construction_references: [String; 14],
    #[serde(flatten)]
    pub(super) sources: SurfaceAllocationSources,
    #[serde(flatten)]
    pub(super) content: FeaturePayloadContent<Vec<FeaturePayloadBlock>>,
}

impl<'de> Deserialize<'de> for FeatureSurfaceConstructionPayload {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Wire {
            id: String,
            operation_label: String,
            construction_references: [String; 14],
            #[serde(flatten)]
            sources: SurfaceAllocationSources,
            #[serde(flatten)]
            content: FeaturePayloadContent<Vec<FeaturePayloadBlock>>,
        }
        let wire = Wire::deserialize(deserializer)?;
        let (branches, post_count) = match &wire.sources {
            SurfaceAllocationSources::Skin {
                construction_branches,
            } => (construction_branches, 0),
            SurfaceAllocationSources::StudioSurface {
                construction_branches,
                ..
            } => (construction_branches, 8),
        };
        let blocks = wire.content.blocks();
        // Every counted branch has at least one member and one terminal.
        if blocks.len() < 14 + branches.0.len() * 2 + post_count {
            return Err(serde::de::Error::custom(
                "data_blocks cannot omit envelope, branch or Studio post-branch storage",
            ));
        }
        let Some((store, first)) = blocks[0].id.rsplit_once(":block#") else {
            return Err(serde::de::Error::custom(
                "data_blocks must name one consecutive positive object-index run",
            ));
        };
        let first = first.parse::<u32>().map_err(serde::de::Error::custom)?;
        if first == 0
            || blocks.iter().enumerate().any(|(i, b)| {
                let Some(index) = u32::try_from(i).ok().and_then(|i| first.checked_add(i)) else {
                    return true;
                };
                b.id != format!("{store}:block#{index}")
            })
        {
            return Err(serde::de::Error::custom(
                "data_blocks must name one consecutive positive object-index run",
            ));
        }
        if blocks.windows(2).any(|pair| {
            pair[0].source_offset.checked_add(pair[0].byte_len) != Some(pair[1].source_offset)
        }) {
            return Err(serde::de::Error::custom(
                "block_source_offsets must form a byte-adjacent allocation",
            ));
        }
        Ok(Self {
            id: wire.id,
            operation_label: wire.operation_label,
            construction_references: wire.construction_references,
            sources: wire.sources,
            content: wire.content,
        })
    }
}

/// Reconstruct ordered logical payloads from complete surface-construction graphs.
pub(in crate::native) fn feature_surface_construction_payloads(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    container: &Container,
    references: &[FeatureSurfaceConstructionReference],
    operation_labels: &[FeatureOperationLabel],
    branches: &[FeatureSurfaceConstructionBranch],
    post_branch_references: &[FeatureSurfaceConstructionReference],
) -> Result<Vec<FeatureSurfaceConstructionPayload>, cadmpeg_core::CodecError> {
    let blocks = offset_data_block_bytes(ctx, container)?;
    let label_bytes = references
        .len()
        .checked_mul(std::mem::size_of::<&str>() * 4)
        .ok_or_else(|| ctx.refuse_codec_limit("NX surface payload labels", 0, 1))?;
    let _labels_reservation = ctx.reserve_scoped(
        cadmpeg_core::decode::u64_from_index(label_bytes),
        "NX surface payload labels",
    )?;
    let mut labels = BTreeSet::new();
    for reference in references {
        ctx.charge_work(1, "scan NX surface payload labels")?;
        ctx.insert_btree_set(
            &mut labels,
            reference.operation_label.as_str(),
            "NX surface payload labels",
        )?;
    }
    let mut output = Vec::new();
    'operation: for operation_label in labels {
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(operation_labels.len()),
            "select NX surface allocation kind",
        )?;
        let Some(kind) = operation_labels
            .iter()
            .find(|label| label.id == operation_label)
            .map(|label| label.value.as_str())
        else {
            continue;
        };
        if !matches!(kind, "SKIN" | "Studio Surface") {
            continue;
        }
        let scan_work = references
            .len()
            .checked_mul(2)
            .ok_or_else(|| ctx.refuse_codec_limit("scan NX surface construction graph", 0, 1))?;
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(scan_work),
            "scan NX surface construction graph",
        )?;
        let graph_count = references
            .iter()
            .filter(|reference| reference.operation_label == operation_label)
            .count();

        let (mut graph, _graph_reservation) =
            ctx.temporary_vec(graph_count, "NX surface construction graph")?;
        graph.extend(
            references
                .iter()
                .filter(|reference| reference.operation_label == operation_label),
        );
        ctx.stable_sort_by(
            &mut graph,
            |first, second| first.ordinal.cmp(&second.ordinal),
            |_| 0,
            "sort NX surface construction graph",
        )?;
        if graph
            .iter()
            .enumerate()
            .any(|(ordinal, reference)| u32::try_from(ordinal) != Ok(reference.ordinal))
        {
            continue;
        }
        let Ok(graph): Result<[&FeatureSurfaceConstructionReference; 14], _> = graph.try_into()
        else {
            continue;
        };
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(branches.len()),
            "select NX surface allocation branches",
        )?;
        let (mut owned_branches, _branches_reservation) =
            ctx.temporary_vec(branches.len(), "NX surface allocation branches")?;
        owned_branches.extend(
            branches
                .iter()
                .filter(|branch| branch.operation_label == operation_label),
        );
        ctx.stable_sort_by(
            &mut owned_branches,
            |a, b| a.ordinal().cmp(&b.ordinal()),
            |_| 0,
            "sort NX surface allocation branches",
        )?;
        if owned_branches.is_empty()
            || owned_branches
                .iter()
                .enumerate()
                .any(|(i, b)| u32::try_from(i) != Ok(b.ordinal()))
        {
            continue;
        }
        let mut source_rows = Vec::new();
        for reference in graph {
            let Some(block) = reference.data_block.as_deref() else {
                continue 'operation;
            };
            ctx.reserve_vec(&mut source_rows, 1, "NX surface allocation source rows")?;
            source_rows.push((block, reference.token.value(), reference.source_offset));
        }
        let mut branch_ids = Vec::new();
        for branch in owned_branches {
            ctx.reserve_vec(
                &mut branch_ids,
                1,
                "NX surface allocation branch identities",
            )?;
            branch_ids
                .push(ctx.copy_retained_text(&branch.id, "NX surface allocation branch identity")?);
            for ((token, block), offset) in branch
                .references
                .members()
                .as_slice()
                .iter()
                .zip(branch.references.member_offsets())
                .chain(std::iter::once((
                    branch.references.terminal(),
                    branch.references.terminal_offset(),
                )))
            {
                let Some(block) = block.as_deref() else {
                    continue 'operation;
                };
                ctx.reserve_vec(&mut source_rows, 1, "NX surface allocation source rows")?;
                source_rows.push((block, token.value(), offset));
            }
        }
        let Ok(construction_branches) = SurfaceConstructionBranches::try_from(branch_ids) else {
            continue;
        };
        let sources = if kind == "Studio Surface" {
            ctx.charge_work(
                cadmpeg_core::decode::u64_from_index(post_branch_references.len()),
                "select NX Studio post-branch references",
            )?;
            let (mut post, _post_reservation) = ctx.temporary_vec(
                post_branch_references.len(),
                "NX Studio post-branch reference graph",
            )?;
            post.extend(
                post_branch_references
                    .iter()
                    .filter(|r| r.operation_label == operation_label),
            );
            ctx.stable_sort_by(
                &mut post,
                |a, b| a.ordinal.cmp(&b.ordinal),
                |_| 0,
                "sort NX Studio post-branch references",
            )?;
            let Ok(post): Result<[&FeatureSurfaceConstructionReference; 8], _> = post.try_into()
            else {
                continue;
            };
            let mut identities: [String; 8] = std::array::from_fn(|_| String::new());
            for (ordinal, reference) in post.into_iter().enumerate() {
                if u32::try_from(ordinal) != Ok(reference.ordinal) {
                    continue 'operation;
                }
                let Some(block) = reference.data_block.as_deref() else {
                    continue 'operation;
                };
                identities[ordinal] = ctx.copy_retained_text(
                    &reference.id,
                    "NX Studio post-branch reference identity",
                )?;
                ctx.reserve_vec(&mut source_rows, 1, "NX surface allocation source rows")?;
                source_rows.push((block, reference.token.value(), reference.source_offset));
            }
            SurfaceAllocationSources::StudioSurface {
                construction_branches,
                post_branch_references: identities,
            }
        } else {
            SurfaceAllocationSources::Skin {
                construction_branches,
            }
        };
        ctx.stable_sort_by(
            &mut source_rows,
            |a, b| a.2.cmp(&b.2),
            |_| 0,
            "sort NX surface allocation source rows",
        )?;
        let Some((first_block, first_index, _)) = source_rows.first().copied() else {
            continue;
        };
        if first_index == 0
            || source_rows.iter().enumerate().any(|(i, row)| {
                u32::try_from(i)
                    .ok()
                    .and_then(|i| first_index.checked_add(i))
                    != Some(row.1)
            })
        {
            continue;
        }
        let Some(store) = first_block.rsplit_once(":block#").map(|(store, _)| store) else {
            continue;
        };
        if source_rows.iter().any(|(block, _, _)| {
            block
                .rsplit_once(":block#")
                .is_none_or(|(prefix, _)| prefix != store)
        }) {
            continue;
        }
        for pair in source_rows.windows(2) {
            let (Some((bytes, first)), Some((_, second))) =
                (blocks.get(pair[0].0), blocks.get(pair[1].0))
            else {
                continue 'operation;
            };
            if first.checked_add(cadmpeg_core::decode::u64_from_index(bytes.len())) != Some(*second)
            {
                continue 'operation;
            }
        }
        let mut data_blocks = Vec::new();
        for (block, _, _) in source_rows {
            ctx.reserve_vec(&mut data_blocks, 1, "NX surface construction source blocks")?;
            data_blocks
                .push(ctx.copy_retained_text(block, "NX surface construction source block")?);
        }
        let Some(content) = FeaturePayloadContent::from_source(ctx, data_blocks, &blocks)? else {
            continue;
        };
        let Some((_, operation_key)) = operation_label.rsplit_once('#') else {
            continue;
        };
        let prefix = "nx:feature-history:surface-construction-payload#";
        let id_len = prefix
            .len()
            .checked_add(operation_key.len())
            .ok_or_else(|| {
                ctx.refuse_codec_limit("NX surface construction payload identity", 0, 1)
            })?;
        let mut id = ctx.retained_string(id_len, "NX surface construction payload identity")?;
        id.push_str(prefix);
        id.push_str(operation_key);
        let mut construction_references: [String; 14] = std::array::from_fn(|_| String::new());
        for (slot, reference) in graph.into_iter().enumerate() {
            construction_references[slot] = ctx
                .copy_retained_text(&reference.id, "NX surface construction reference identity")?;
        }
        ctx.reserve_vec(&mut output, 1, "NX surface construction payloads")?;
        output.push(FeatureSurfaceConstructionPayload {
            id,
            operation_label: ctx
                .copy_retained_text(operation_label, "NX surface construction operation label")?,
            construction_references,
            sources,
            content,
        });
    }
    Ok(output)
}

/// Admit complete allocation ownership against the retained native records.
pub(in crate::native) fn admit_surface_allocations(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    namespace: &cadmpeg_ir::native::NativeNamespace,
) -> Result<(), cadmpeg_ir::native::NativeConvertError> {
    use cadmpeg_core::decode::u64_from_index;
    use cadmpeg_ir::native::NativeConvertError;
    let fail = || {
        NativeConvertError::InvalidCollection("surface allocation must match its complete ordered reference graph and physical blocks".into())
    };
    if namespace
        .arenas()
        .get("feature_surface_construction_payloads")
        .is_none_or(Vec::is_empty)
    {
        return Ok(());
    }
    let labels =
        namespace.arena_as_for_decode::<FeatureOperationLabel>(ctx, "feature_operation_labels")?;
    let envelopes = namespace.arena_as_for_decode::<FeatureSurfaceConstructionReference>(
        ctx,
        "feature_surface_construction_references",
    )?;
    let post = namespace.arena_as_for_decode::<FeatureSurfaceConstructionReference>(
        ctx,
        "feature_studio_post_branch_references",
    )?;
    let branches = namespace.arena_as_for_decode::<FeatureSurfaceConstructionBranch>(
        ctx,
        "feature_surface_construction_branches",
    )?;
    let blocks = namespace
        .arenas()
        .get("data_blocks")
        .map_or(&[][..], Vec::as_slice);
    for payload in namespace.arena_iter_as_for_decode::<FeatureSurfaceConstructionPayload>(
        ctx,
        "feature_surface_construction_payloads",
    ) {
        let payload = payload?;
        let kind = match &payload.sources {
            SurfaceAllocationSources::Skin { .. } => "SKIN",
            SurfaceAllocationSources::StudioSurface { .. } => "Studio Surface",
        };
        ctx.charge_work(
            u64_from_index(labels.len()),
            "resolve NX surface allocation kind",
        )?;
        let mut matching = labels
            .iter()
            .filter(|label| label.id == payload.operation_label);
        let label = matching.next().ok_or_else(fail)?;
        if matching.next().is_some() || label.value != kind {
            return Err(fail());
        }
        ctx.charge_work(
            u64_from_index(envelopes.len() + post.len()),
            "count NX surface allocation reference lanes",
        )?;
        if envelopes
            .iter()
            .filter(|r| r.operation_label == payload.operation_label)
            .count()
            != 14
            || post
                .iter()
                .filter(|r| r.operation_label == payload.operation_label)
                .count()
                != if kind == "Studio Surface" { 8 } else { 0 }
        {
            return Err(fail());
        }
        let mut rows = Vec::new();
        let mut push_reference = |population: &[FeatureSurfaceConstructionReference],
                                  id: &str,
                                  ordinal: usize|
         -> Result<(), NativeConvertError> {
            ctx.charge_work(
                u64_from_index(population.len()),
                "resolve NX surface allocation reference",
            )?;
            let mut matches = population.iter().filter(|r| r.id == id);
            let reference = matches.next().ok_or_else(fail)?;
            if matches.next().is_some()
                || reference.operation_label != payload.operation_label
                || usize::try_from(reference.ordinal).ok() != Some(ordinal)
            {
                return Err(fail());
            }
            let block = reference.data_block.as_deref().ok_or_else(fail)?;
            ctx.reserve_vec(&mut rows, 1, "NX surface admission references")?;
            rows.push((
                ctx.copy_retained_text(block, "NX surface admission block")?,
                reference.token.value(),
                reference.source_offset,
            ));
            Ok(())
        };
        for (ordinal, id) in payload.construction_references.iter().enumerate() {
            push_reference(&envelopes, id, ordinal)?;
        }
        let owned_branches = match &payload.sources {
            SurfaceAllocationSources::Skin {
                construction_branches,
            } => construction_branches,
            SurfaceAllocationSources::StudioSurface {
                construction_branches,
                post_branch_references,
            } => {
                for (ordinal, id) in post_branch_references.iter().enumerate() {
                    push_reference(&post, id, ordinal)?;
                }
                construction_branches
            }
        };
        ctx.charge_work(
            u64_from_index(branches.len()),
            "count NX surface allocation branches",
        )?;
        if branches
            .iter()
            .filter(|r| r.operation_label == payload.operation_label)
            .count()
            != owned_branches.0.len()
        {
            return Err(fail());
        }
        for (ordinal, id) in owned_branches.0.iter().enumerate() {
            ctx.charge_work(
                u64_from_index(branches.len()),
                "resolve NX surface allocation branch",
            )?;
            let mut matches = branches.iter().filter(|r| r.id == *id);
            let branch = matches.next().ok_or_else(fail)?;
            if matches.next().is_some()
                || branch.operation_label != payload.operation_label
                || usize::try_from(branch.ordinal()).ok() != Some(ordinal)
            {
                return Err(fail());
            }
            for ((token, block), offset) in branch
                .references
                .members()
                .as_slice()
                .iter()
                .zip(branch.references.member_offsets())
                .chain(std::iter::once((
                    branch.references.terminal(),
                    branch.references.terminal_offset(),
                )))
            {
                let block = block.as_deref().ok_or_else(fail)?;
                ctx.reserve_vec(&mut rows, 1, "NX surface admission references")?;
                rows.push((
                    ctx.copy_retained_text(block, "NX surface admission block")?,
                    token.value(),
                    offset,
                ));
            }
        }
        ctx.stable_sort_by(
            &mut rows,
            |a, b| a.2.cmp(&b.2),
            |_| 0,
            "sort NX surface admission references",
        )?;
        if rows.len() != payload.content.blocks().len() {
            return Err(fail());
        }
        for (row, physical) in rows.iter().zip(payload.content.blocks()) {
            if row.0 != physical.id {
                return Err(fail());
            }
            ctx.charge_work(
                u64_from_index(blocks.len()),
                "resolve NX surface allocation block",
            )?;
            let mut matches = blocks.iter().filter(|b| b.id() == physical.id);
            let block = matches.next().ok_or_else(fail)?;
            if matches.next().is_some()
                || block
                    .fields()
                    .get("block_ordinal")
                    .and_then(serde_json::Value::as_u64)
                    != Some(u64::from(row.1))
                || block
                    .fields()
                    .get("source_offset")
                    .and_then(serde_json::Value::as_u64)
                    != Some(physical.source_offset)
                || block
                    .fields()
                    .get("byte_len")
                    .and_then(serde_json::Value::as_u64)
                    != Some(physical.byte_len)
            {
                return Err(fail());
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{feature_surface_construction_payloads, FeatureSurfaceConstructionPayload};

    fn allocation(bytes: Vec<u8>, expected_blocks: usize, kind: &str) {
        crate::test_support::with_decode_context(|ctx| {
            let container = crate::container::scan_bytes(ctx, bytes).unwrap();
            let references =
                super::super::construction_records::feature_surface_construction_references(
                    ctx, &container,
                )
                .unwrap();
            let labels = super::super::feature_operation_labels(ctx, &container).unwrap();
            let branches = super::super::surface_branches::feature_surface_construction_branches(
                ctx, &container,
            )
            .unwrap();
            let post = super::super::construction_records::feature_studio_post_branch_references(
                ctx, &container,
            )
            .unwrap();
            let run = |refs: &[_], branch: &[_], post: &[_]| {
                feature_surface_construction_payloads(ctx, &container, refs, &labels, branch, post)
                    .unwrap()
            };
            let payloads = run(&references, &branches, &post);
            assert_eq!(payloads.len(), 1);
            let payload = &payloads[0];
            assert_eq!(
                payload.content.byte_len(),
                u64::try_from(expected_blocks).unwrap()
            );
            let ids = payload.content.block_ids().collect::<Vec<_>>();
            assert_eq!(ids.len(), expected_blocks);
            for (index, id) in ids.iter().enumerate() {
                assert!(id.ends_with(&format!(":block#{}", index + 1)));
            }
            let value = serde_json::to_value(payload).unwrap();
            assert_eq!(value["operation_kind"], kind);
            assert_eq!(
                serde_json::from_value::<FeatureSurfaceConstructionPayload>(value.clone()).unwrap(),
                *payload
            );
            let mut bad = value.clone();
            bad["block_source_offsets"][1] = serde_json::json!(0);
            assert!(serde_json::from_value::<FeatureSurfaceConstructionPayload>(bad).is_err());
            let mut bad = value.clone();
            bad["data_blocks"][1] = bad["data_blocks"][0].clone();
            assert!(serde_json::from_value::<FeatureSurfaceConstructionPayload>(bad).is_err());
            let mut bad = value.clone();
            for field in [
                "data_blocks",
                "block_payload_offsets",
                "block_byte_lengths",
                "block_source_offsets",
            ] {
                bad[field].as_array_mut().unwrap().pop();
            }
            bad["byte_len"] = serde_json::json!(expected_blocks - 1);
            assert!(serde_json::from_value::<FeatureSurfaceConstructionPayload>(bad).is_err());
            let mut bad = value.clone();
            bad["construction_branches"] = serde_json::json!([]);
            assert!(serde_json::from_value::<FeatureSurfaceConstructionPayload>(bad).is_err());
            if kind == "studio_surface" {
                let mut bad = value.clone();
                bad.as_object_mut()
                    .unwrap()
                    .remove("post_branch_references");
                assert!(serde_json::from_value::<FeatureSurfaceConstructionPayload>(bad).is_err());
                assert!(run(&references, &branches, &post[..7]).is_empty());
            }
            assert!(run(&references[..13], &branches, &post).is_empty());
            assert!(run(&references, &branches[1..], &post).is_empty());
            let mut unresolved = references.clone();
            unresolved[0].data_block = None;
            assert!(run(&unresolved, &branches, &post).is_empty());
            let mut repeated = references.clone();
            repeated[1].token = repeated[0].token;
            repeated[1].data_block = repeated[0].data_block.clone();
            assert!(run(&repeated, &branches, &post).is_empty());
        });
    }

    #[test]
    fn native_surface_admission_checks_complete_graph_and_physical_metadata() {
        use cadmpeg_ir::codec::{Codec, CodecBackend, DecodeOptions};
        let result = crate::NxCodec
            .decode(
                &mut std::io::Cursor::new(crate::test_support::studio_surface::prt()),
                &DecodeOptions::default(),
            )
            .unwrap();
        let (mut ir, _, _) = result.into_parts();
        crate::test_support::with_decode_context(|ctx| {
            assert!(<crate::NxCodec as CodecBackend>::validate_native(ctx, &ir)
                .unwrap()
                .is_empty());
            let namespace = ir.native.namespace_mut("nx");
            let original = serde_json::to_value(
                &namespace.arenas()["feature_surface_construction_payloads"][0],
            )
            .unwrap();
            // A surplus contiguous block passes local column/size checks, but
            // the complete graph has exactly 28 referenced blocks.
            let mut surplus = original.clone();
            let start = surplus["block_source_offsets"][27].as_u64().unwrap();
            for (field, value) in [
                (
                    "data_blocks",
                    serde_json::json!("nx:om-data-blocks-0:block#29"),
                ),
                ("block_byte_lengths", serde_json::json!(1)),
                ("block_payload_offsets", serde_json::json!(28)),
                ("block_source_offsets", serde_json::json!(start + 1)),
            ] {
                surplus[field].as_array_mut().unwrap().push(value);
            }
            surplus["byte_len"] = serde_json::json!(29);
            assert!(
                serde_json::from_value::<FeatureSurfaceConstructionPayload>(surplus.clone())
                    .is_ok()
            );
            namespace
                .set_arena(ctx, "feature_surface_construction_payloads", &[surplus])
                .unwrap();
            assert!(super::admit_surface_allocations(ctx, namespace).is_err());
            for field in [
                "construction_references",
                "construction_branches",
                "post_branch_references",
            ] {
                let mut bad = original.clone();
                bad[field][0] = bad[field][1].clone();
                namespace
                    .set_arena(ctx, "feature_surface_construction_payloads", &[bad])
                    .unwrap();
                assert!(super::admit_surface_allocations(ctx, namespace).is_err());
            }
            let mut shifted = original.clone();
            for offset in shifted["block_source_offsets"].as_array_mut().unwrap() {
                *offset = serde_json::json!(offset.as_u64().unwrap() + 1);
            }
            namespace
                .set_arena(ctx, "feature_surface_construction_payloads", &[shifted])
                .unwrap();
            assert!(super::admit_surface_allocations(ctx, namespace).is_err());
            namespace
                .set_arena(ctx, "feature_surface_construction_payloads", &[original])
                .unwrap();
            assert!(<crate::NxCodec as CodecBackend>::validate_native(ctx, &ir)
                .unwrap()
                .is_empty());
        });
    }

    #[test]
    fn complete_surface_allocation_includes_branch_and_post_branch_storage() {
        allocation(
            crate::test_support::studio_surface::prt(),
            28,
            "studio_surface",
        );
        allocation(crate::test_support::studio_surface::skin_prt(), 20, "skin");
    }
}
