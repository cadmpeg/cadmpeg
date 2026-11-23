// SPDX-License-Identifier: Apache-2.0
//! Validation of body links, subentity tags, history graphs, and thread offsets.

use super::{push_constant_finding, Ctx};
use crate::{history, records};
use cadmpeg_ir::report::check::{Check, Finding};
use cadmpeg_ir::report::Severity;

/// Validate persistent body links and their history ordering.
pub(super) fn validate_body_links(ctx: &Ctx<'_, '_>, findings: &mut Vec<Finding>) {
    let native = ctx.native;
    let ir = ctx.ir;
    let body_ids = ir
        .geometry
        .bodies
        .iter()
        .map(|body| &body.id)
        .collect::<std::collections::HashSet<_>>();
    let mut body_links: std::collections::BTreeMap<_, Vec<_>> = std::collections::BTreeMap::new();
    for link in &native.persistent_design_links {
        let target_key = match &link.target {
            cadmpeg_ir::attributes::AttributeTarget::Body(id) if body_ids.contains(id) => Some(id),
            _ => None,
        };
        let Some(target_key) = target_key else {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion persistent body link has an invalid target or group payload",
                Some(link.id.clone()),
            );
            continue;
        };
        body_links.entry(target_key).or_default().push(link);
    }
    for links in body_links.values_mut() {
        links.sort_by_key(|left| left.ordinal);
        if links
            .iter()
            .enumerate()
            .any(|(ordinal, link)| u32::try_from(ordinal) != Ok(link.ordinal))
        {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion persistent body links have noncanonical history ordering",
                links.first().map(|link| link.id.clone()),
            );
        }
    }
}

/// Validate persistent subentity tags and their group ordering.
pub(super) fn validate_subentity_tags(ctx: &Ctx<'_, '_>, findings: &mut Vec<Finding>) {
    let native = ctx.native;
    let ir = ctx.ir;
    let face_ids = ir
        .geometry
        .faces
        .iter()
        .map(|face| &face.id)
        .collect::<std::collections::HashSet<_>>();
    let edge_ids = ir
        .geometry
        .edges
        .iter()
        .map(|edge| &edge.id)
        .collect::<std::collections::HashSet<_>>();
    let mut subentity_tags: std::collections::BTreeMap<_, Vec<_>> =
        std::collections::BTreeMap::new();
    for tag in &native.persistent_subentity_tags {
        let Some(target_key) = (match &tag.target {
            cadmpeg_ir::attributes::AttributeTarget::Face(id) if face_ids.contains(id) => {
                Some((1_u8, id.as_str()))
            }
            cadmpeg_ir::attributes::AttributeTarget::Edge(id) if edge_ids.contains(id) => {
                Some((0_u8, id.as_str()))
            }
            _ => None,
        }) else {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion persistent subentity tag has an invalid target or group payload",
                Some(tag.id.clone()),
            );
            continue;
        };
        subentity_tags.entry(target_key).or_default().push(tag);
    }
    for tags in subentity_tags.values_mut() {
        tags.sort_by_key(|left| left.ordinal);
        if tags
            .iter()
            .enumerate()
            .any(|(ordinal, tag)| u32::try_from(ordinal) != Ok(tag.ordinal))
        {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion persistent subentity tags have noncanonical group ordering",
                tags.first().map(|tag| tag.id.clone()),
            );
        }
    }
}

/// Validate each ASM history graph as a coherent state chain.
pub(super) fn validate_history_graphs(ctx: &Ctx, findings: &mut Vec<Finding>) {
    let native = ctx.native;
    if ctx.ir.fragment
        && native.asm_histories.is_empty()
        && native
            .design_parameter_scopes
            .iter()
            .any(|scope| scope.history_state_id().is_some())
    {
        findings.push(Finding { check: Check::FragmentSkipped, severity: Severity::Info,
            entity: None, related: Vec::new(),
            message: "skipped f3d.history_state_links and f3d.historical_candidates: cited ASM states are outside this fragment".into() });
    }
    for history in &native.asm_histories {
        if let Err(error) = history.check_transitions() {
            findings.push(Finding {
                check: Check::NativeLinks,
                severity: Severity::Error,
                message: format!("Fusion ASM history transition is invalid: {error:?}"),
                entity: Some(history.id.clone()),
                related: Vec::new(),
            });
        }
        let coherent = history::graph_is_coherent(history);
        if !coherent {
            push_constant_finding(
                findings,
                Check::NativeLinks,
                "Fusion ASM history graph is not a coherent doubly linked state chain",
                Some(history.id.clone()),
            );
        }
    }
}

/// Whether a Thread form admits the scope's class pair.
pub(super) fn thread_class_pair_holds(
    form: records::feature::thread::DesignThreadForm,
    class_tag: &records::references::DesignClassTag,
    paired_class_tag: &records::references::DesignClassTag,
) -> bool {
    form.class_pair_holds(class_tag.as_str(), paired_class_tag.as_str())
}

pub(super) fn thread_designation_offset_holds(
    form: records::feature::thread::DesignThreadForm,
    designation_offset: u64,
    scope_offset: u64,
) -> bool {
    form.designation_offset_holds(designation_offset, scope_offset)
}
