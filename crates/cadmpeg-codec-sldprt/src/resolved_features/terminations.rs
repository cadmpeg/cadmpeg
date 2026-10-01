//! Extrusion terminations, combine selections and sweep paths.

use super::component_paths::{
    component_path_features, component_path_terminal_feature, is_profile_feature_object,
};
use super::is_class_token;
use super::parameters::value_only_scalar_offset;
use super::scalars::feature_object_name;
use super::selections::{
    compact_general_curve_ref_at, compact_heterogeneous_component_path,
    compact_mixed_component_path, compact_profile_general_curve_ref_at,
    component_profile_source_at, component_reference_curve_path_at,
    declared_general_curve_profile_prefix, is_component_vector_selector,
    is_component_vector_selector_for_role, COMPACT_EDGE_VECTOR_MARKER,
};
use crate::classification::{native_object_class, NativeClassKind};
use crate::records::{FeatureInputComponentPathEntry, FeatureInputLane};
use cadmpeg_core::decode::View;
use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};
use std::collections::HashMap;

const EPS_TERMINATIONS_ENRICH_HISTORY_EXTRUSION_TERMINATIONS_E9: f64 = 1.0e-9;

/// Add semantic termination forms carried by compact extrusion end-spec children.
#[cfg_attr(test, derive(Clone))]
enum TerminationVote {
    Blind {
        depth_m: Option<f64>,
    },
    /// Blind termination with a through-all second direction.
    BlindSecondThroughAll,
    Symmetric,
    ThroughAllBoth,
    ThroughAll,
    ThroughNext,
    ToVertex {
        reference: String,
    },
    Face {
        condition: FaceCondition,
        reference: FaceReference,
        identity: String,
    },
}

/// A lane reference with its fallback or a resolved consensus reference.
#[cfg_attr(test, derive(Clone))]
enum FaceReference {
    Lane {
        reference: String,
        canonical: Option<String>,
    },
    Canonical(String),
    Unresolved,
}

impl FaceReference {
    fn as_str(&self) -> Option<&str> {
        match self {
            Self::Lane { reference, .. } | Self::Canonical(reference) => Some(reference),
            Self::Unresolved => None,
        }
    }

    fn canonical(
        &self,
        ctx: &DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<Self, cadmpeg_core::CodecError> {
        Ok(match self {
            Self::Lane {
                canonical: Some(reference),
                ..
            }
            | Self::Canonical(reference) => {
                Self::Canonical(copy_termination_text(ctx, reference, operation)?)
            }
            Self::Lane {
                canonical: None, ..
            }
            | Self::Unresolved => Self::Unresolved,
        })
    }

    fn copy_charged(
        &self,
        ctx: &DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<Self, cadmpeg_core::CodecError> {
        Ok(match self {
            Self::Lane {
                reference,
                canonical,
            } => Self::Lane {
                reference: copy_termination_text(ctx, reference, operation)?,
                canonical: canonical
                    .as_deref()
                    .map(|text| copy_termination_text(ctx, text, operation))
                    .transpose()?,
            },
            Self::Canonical(reference) => {
                Self::Canonical(copy_termination_text(ctx, reference, operation)?)
            }
            Self::Unresolved => Self::Unresolved,
        })
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum FaceCondition {
    OffsetFromFace,
    ToFace,
}

impl TerminationVote {
    fn copy_charged(
        &self,
        ctx: &DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<Self, cadmpeg_core::CodecError> {
        Ok(match self {
            Self::Blind { depth_m } => Self::Blind { depth_m: *depth_m },
            Self::BlindSecondThroughAll => Self::BlindSecondThroughAll,
            Self::Symmetric => Self::Symmetric,
            Self::ThroughAllBoth => Self::ThroughAllBoth,
            Self::ThroughAll => Self::ThroughAll,
            Self::ThroughNext => Self::ThroughNext,
            Self::ToVertex { reference } => Self::ToVertex {
                reference: copy_termination_text(ctx, reference, operation)?,
            },
            Self::Face {
                condition,
                reference,
                identity,
            } => Self::Face {
                condition: *condition,
                reference: reference.copy_charged(ctx, operation)?,
                identity: copy_termination_text(ctx, identity, operation)?,
            },
        })
    }

    fn condition(&self) -> &'static str {
        match self {
            Self::Blind { .. } | Self::BlindSecondThroughAll => "Blind",
            Self::Symmetric => "Symmetric",
            Self::ThroughAllBoth => "ThroughAllBoth",
            Self::ThroughAll => "ThroughAll",
            Self::ThroughNext => "ThroughNext",
            Self::ToVertex { .. } => "ToVertex",
            Self::Face {
                condition: FaceCondition::OffsetFromFace,
                ..
            } => "OffsetFromFace",
            Self::Face {
                condition: FaceCondition::ToFace,
                ..
            } => "ToFace",
        }
    }

    fn reference(&self) -> Option<&str> {
        match self {
            Self::ToVertex { reference } => Some(reference),
            Self::Face { reference, .. } => reference.as_str(),
            _ => None,
        }
    }

    fn agrees_with(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Blind { depth_m: a }, Self::Blind { depth_m: b }) => {
                a.map(f64::to_bits) == b.map(f64::to_bits)
            }
            (Self::BlindSecondThroughAll, Self::BlindSecondThroughAll)
            | (Self::Symmetric, Self::Symmetric)
            | (Self::ThroughAllBoth, Self::ThroughAllBoth)
            | (Self::ThroughAll, Self::ThroughAll)
            | (Self::ThroughNext, Self::ThroughNext) => true,
            (Self::ToVertex { reference: a }, Self::ToVertex { reference: b }) => a == b,
            (
                Self::Face {
                    condition: a,
                    identity: a_id,
                    ..
                },
                Self::Face {
                    condition: b,
                    identity: b_id,
                    ..
                },
            ) => a == b && a_id == b_id,
            _ => false,
        }
    }
}

pub(crate) fn enrich_history_extrusion_terminations(
    ctx: &DecodeContext<'_>,
    histories: &mut [crate::records::FeatureHistory],
    lanes: &[FeatureInputLane],
) -> Result<(), cadmpeg_core::CodecError> {
    const OPERATION: &str = "enrich SLDPRT extrusion terminations";
    let mut terminations = HashMap::<String, Vec<Option<TerminationVote>>>::new();
    for lane in lanes {
        let mut names_by_id = HashMap::new();
        for name in &lane.names {
            ctx.charge_work(
                u64_from_index(name.id.len())
                    .checked_mul(2)
                    .and_then(|work| work.checked_add(1))
                    .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?,
                OPERATION,
            )?;
            ctx.insert_hash_map(&mut names_by_id, name.id.as_str(), name, OPERATION)?;
        }
        let scan_end = if lane.native_payload.len() >= 103 {
            lane.native_payload.len() - 103
        } else {
            0
        };
        ctx.charge_work(
            u64_from_index(scan_end)
                .checked_mul(64)
                .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?,
            OPERATION,
        )?;
        let mut grouped_blind = HashMap::<String, usize>::new();
        for offset in
            (0..scan_end).filter(|offset| compact_extrusion_blind_at(&lane.native_payload, *offset))
        {
            ctx.charge_work(u64_from_index(lane.scalars.len()), OPERATION)?;
            let Some(scalar) = lane
                .scalars
                .iter()
                .filter(|scalar| scalar.offset > u64_from_index(offset))
                .min_by_key(|scalar| scalar.offset)
            else {
                continue;
            };
            ctx.charge_work(u64_from_index(scalar.name.len()), OPERATION)?;
            let Some(name) = names_by_id.get(scalar.name.as_str()) else {
                continue;
            };
            let mut owners = Vec::new();
            for feature in histories.iter().flat_map(|history| &history.features) {
                ctx.charge_work(
                    u64_from_index(feature.input_class.as_ref().map_or(0, String::len))
                        .checked_add(u64_from_index(feature.xml_tag.len()))
                        .and_then(|work| work.checked_add(1))
                        .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?,
                    OPERATION,
                )?;
                if !is_extrusion_end_spec_owner(feature) || feature.parameters.len() != 1 {
                    continue;
                }
                ctx.charge_work(u64_from_index(name.value.len()), OPERATION)?;
                let Some(value) = feature.parameters.get(name.value.as_str()) else {
                    continue;
                };
                ctx.charge_work(
                    u64_from_index(value.len())
                        .checked_mul(17)
                        .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?,
                    OPERATION,
                )?;
                if crate::history::literals::parse_dimension_length_mm(value).is_some_and(|value| {
                    (value.get() - scalar.value.get() * 1000.0).abs()
                        <= EPS_TERMINATIONS_ENRICH_HISTORY_EXTRUSION_TERMINATIONS_E9
                }) {
                    ctx.reserve_collection_vec(&mut owners, 1, OPERATION)?;
                    owners.push(feature);
                }
            }
            let [owner] = owners.as_slice() else {
                continue;
            };
            ctx.charge_work(u64_from_index(owner.id.len()), OPERATION)?;
            if !grouped_blind.contains_key(owner.id.as_str()) {
                let key = copy_termination_text(ctx, &owner.id, OPERATION)?;
                ctx.insert_hash_map(&mut grouped_blind, key, 0, OPERATION)?;
            }
            let count = grouped_blind.get_mut(owner.id.as_str()).ok_or_else(|| {
                cadmpeg_core::CodecError::malformed("missing admitted blind vote count")
            })?;
            *count = count
                .checked_add(1)
                .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
        }
        let objects = history_object_offsets(ctx, histories, lane, OPERATION)?;
        for (index, (start, feature_id)) in objects.iter().enumerate() {
            for feature in histories.iter().flat_map(|history| &history.features) {
                let work = u64_from_index(feature.id.len())
                    .checked_add(u64_from_index(feature_id.len()))
                    .and_then(|work| {
                        work.checked_add(u64_from_index(
                            feature.input_class.as_ref().map_or(0, String::len),
                        ))
                    })
                    .and_then(|work| work.checked_add(u64_from_index(feature.xml_tag.len())))
                    .and_then(|work| work.checked_add(1))
                    .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
                ctx.charge_work(work, OPERATION)?;
            }
            let Some(feature) = histories
                .iter()
                .flat_map(|history| &history.features)
                .find(|feature| feature.id == *feature_id)
            else {
                continue;
            };
            if !is_extrusion_end_spec_owner(feature) {
                continue;
            }
            for (_, next_id) in &objects[index + 1..] {
                let work = u64_from_index(next_id.len())
                    .checked_add(u64_from_index(feature_id.len()))
                    .and_then(|work| work.checked_add(1))
                    .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
                ctx.charge_work(
                    work.checked_mul(2)
                        .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?,
                    OPERATION,
                )?;
                for candidate in histories.iter().flat_map(|history| &history.features) {
                    let work = u64_from_index(candidate.id.len())
                        .checked_add(u64_from_index(next_id.len()))
                        .and_then(|work| {
                            work.checked_add(u64_from_index(
                                candidate.input_class.as_ref().map_or(0, String::len),
                            ))
                        })
                        .and_then(|work| work.checked_add(u64_from_index(candidate.xml_tag.len())))
                        .and_then(|work| work.checked_add(u64_from_index(candidate.kind.len())))
                        .and_then(|work| work.checked_add(u64_from_index(candidate.name.len())))
                        .and_then(|work| work.checked_add(1))
                        .and_then(|work| work.checked_mul(2))
                        .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
                    ctx.charge_work(work, OPERATION)?;
                }
            }
            let is_cosmetic_thread = |candidate: &crate::records::Feature| {
                native_object_class(candidate.input_class.as_deref().unwrap_or_default())
                    == NativeClassKind::CosmeticThread
            };
            let has_depth =
                feature.parameters.contains_key("Depth") || feature.parameters.contains_key("D1");
            let Ok(start) = usize::try_from(*start) else {
                continue;
            };
            // Cosmetic-thread children may be serialized between an extrusion
            // object and its end spec. Other following objects still delimit
            // the scan so a later feature cannot supply the termination.
            let end_spec_end = objects[index + 1..]
                .iter()
                .find(|(_, next_id)| {
                    if next_id == feature_id {
                        return false;
                    }
                    !histories
                        .iter()
                        .flat_map(|history| &history.features)
                        .find(|candidate| candidate.id == *next_id)
                        .is_some_and(is_cosmetic_thread)
                })
                .and_then(|object| usize::try_from(object.0).ok())
                .unwrap_or(lane.native_payload.len());
            let mut end_index = index + 1;
            while let Some((_, next_id)) = objects.get(end_index) {
                if next_id == feature_id {
                    end_index += 1;
                    continue;
                }
                let skip = histories
                    .iter()
                    .flat_map(|history| &history.features)
                    .find(|feature| feature.id == *next_id)
                    .is_some_and(|feature| {
                        let class = feature.input_class.as_deref().unwrap_or_default();
                        is_profile_feature_object(feature)
                            || native_object_class(class) == NativeClassKind::CosmeticThread
                    });
                if !skip {
                    break;
                }
                end_index += 1;
            }
            let end = objects
                .get(end_index)
                .and_then(|object| usize::try_from(object.0).ok())
                .unwrap_or(lane.native_payload.len());
            let lane_key = lane
                .id
                .rsplit_once('#')
                .map_or(lane.id.as_str(), |(_, key)| key);
            let mut candidates = Vec::new();
            let scan_end = (103..end_spec_end).len();
            for offset in start..scan_end {
                ctx.charge_work(64, OPERATION)?;
                let candidate =
                    (|| -> Result<Option<TerminationVote>, cadmpeg_core::CodecError> {
                        if compact_extrusion_blind_at(&lane.native_payload, offset) {
                            for scalar in &lane.scalars {
                                ctx.charge_work(
                                    u64_from_index(scalar.name.len())
                                        .checked_add(32)
                                        .ok_or_else(|| {
                                            ctx.refuse_codec_limit(
                                                OPERATION,
                                                u64::MAX - 1,
                                                u64::MAX,
                                            )
                                        })?,
                                    OPERATION,
                                )?;
                            }
                            let depth_m = lane
                                .scalars
                                .iter()
                                .filter(|scalar| {
                                    scalar.offset > u64_from_index(offset)
                                        && scalar.offset < u64_from_index(end)
                                })
                                .filter_map(|scalar| {
                                    let name = names_by_id.get(scalar.name.as_str())?;
                                    matches!(name.value.as_str(), "D1" | "Depth")
                                        .then_some((scalar, *name))
                                })
                                .filter(|(scalar, name)| {
                                    value_only_scalar_offset(&lane.native_payload, name)
                                        == usize::try_from(scalar.offset).ok()
                                })
                                .min_by_key(|(scalar, _)| scalar.offset)
                                .map(|(scalar, _)| scalar.value.get());
                            return Ok(Some(TerminationVote::Blind { depth_m }));
                        }
                        if compact_extrusion_mid_plane_at(&lane.native_payload, offset) {
                            return Ok(Some(TerminationVote::Symmetric));
                        }
                        if let Some(reference) = compact_extrusion_offset_from_face_at(
                            ctx,
                            &lane.native_payload,
                            offset,
                            end_spec_end,
                        )? {
                            return Ok(Some(compact_termination_face_vote(
                                ctx,
                                FaceCondition::OffsetFromFace,
                                lane,
                                feature_id,
                                lane_key,
                                reference,
                            )?));
                        }
                        if compact_extrusion_through_all_both_at(&lane.native_payload, offset) {
                            return Ok(Some(TerminationVote::ThroughAllBoth));
                        }
                        if has_depth
                            && compact_extrusion_blind_through_all_second_at(
                                &lane.native_payload,
                                offset,
                            )
                        {
                            return Ok(Some(TerminationVote::BlindSecondThroughAll));
                        }
                        if compact_extrusion_through_all_at(&lane.native_payload, offset) {
                            return Ok(Some(TerminationVote::ThroughAll));
                        }
                        if compact_extrusion_through_next_at(&lane.native_payload, offset) {
                            return Ok(Some(TerminationVote::ThroughNext));
                        }
                        if has_depth {
                            return Ok(None);
                        }
                        if let Some((reference, kind)) = compact_extrusion_to_vertex_at(
                            ctx,
                            &lane.native_payload,
                            offset,
                            end_spec_end,
                        )? {
                            let prefix = match kind {
                                CompactPointReferenceKind::Point => "point-ref",
                                CompactPointReferenceKind::EdgeEndpoint { .. } => {
                                    "edge-endpoint-ref"
                                }
                            };
                            ctx.charge_work(u64_from_index(lane_key.len()), OPERATION)?;
                            let reference = crate::text_admission::format_retained(
                                ctx,
                                format_args!(
                                    "sldprt:feature-input:{prefix}:{lane_key}:{reference}"
                                ),
                                OPERATION,
                            )?;
                            return Ok(Some(TerminationVote::ToVertex { reference }));
                        }
                        compact_extrusion_to_face_at(
                            ctx,
                            &lane.native_payload,
                            offset,
                            end_spec_end,
                        )?
                        .map(|reference| {
                            compact_termination_face_vote(
                                ctx,
                                FaceCondition::ToFace,
                                lane,
                                feature_id,
                                lane_key,
                                reference,
                            )
                        })
                        .transpose()
                    })()?;
                if let Some(candidate) = candidate {
                    ctx.reserve_collection_vec(&mut candidates, 1, OPERATION)?;
                    candidates.push(candidate);
                }
            }
            ctx.charge_work(
                u64_from_index(feature_id.len())
                    .checked_mul(3)
                    .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?,
                OPERATION,
            )?;
            let grouped = (grouped_blind.get(feature_id.as_str()) == Some(&1))
                .then_some(TerminationVote::Blind { depth_m: None });
            let vote = if candidates.len() == 1 {
                candidates.pop()
            } else {
                None
            }
            .or(grouped);
            if !terminations.contains_key(feature_id.as_str()) {
                let key = copy_termination_text(ctx, feature_id, OPERATION)?;
                ctx.insert_hash_map(&mut terminations, key, Vec::new(), OPERATION)?;
            }
            let votes = terminations.get_mut(feature_id.as_str()).ok_or_else(|| {
                cadmpeg_core::CodecError::malformed("missing admitted termination vote bucket")
            })?;
            ctx.reserve_collection_vec(votes, 1, OPERATION)?;
            votes.push(vote);
        }
    }
    for feature in histories
        .iter_mut()
        .flat_map(|history| &mut history.features)
    {
        ctx.charge_work(
            u64_from_index(feature.id.len())
                .checked_add(1)
                .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?,
            OPERATION,
        )?;
        if feature.properties.contains_key("EndCondition") {
            continue;
        }
        let Some(votes) = terminations.get(&feature.id) else {
            continue;
        };
        let Some(vote) = consensus_termination_vote(ctx, votes)? else {
            continue;
        };
        let condition = copy_termination_text(ctx, vote.condition(), OPERATION)?;
        insert_termination_field(
            ctx,
            &mut feature.properties,
            "EndCondition",
            condition,
            OPERATION,
        )?;
        match vote {
            TerminationVote::ToVertex { reference } => {
                if !feature.properties.contains_key("Vertex") {
                    insert_termination_field(
                        ctx,
                        &mut feature.properties,
                        "Vertex",
                        reference,
                        OPERATION,
                    )?;
                }
            }
            TerminationVote::Face {
                reference:
                    FaceReference::Lane { reference, .. } | FaceReference::Canonical(reference),
                ..
            } => {
                if !feature.properties.contains_key("Face") {
                    insert_termination_field(
                        ctx,
                        &mut feature.properties,
                        "Face",
                        reference,
                        OPERATION,
                    )?;
                }
            }
            TerminationVote::BlindSecondThroughAll => {
                let value = copy_termination_text(ctx, "ThroughAll", OPERATION)?;
                insert_termination_field(
                    ctx,
                    &mut feature.properties,
                    "EndCondition2",
                    value,
                    OPERATION,
                )?;
            }
            TerminationVote::Blind {
                depth_m: Some(depth_m),
            } if !feature.parameters.contains_key("D1")
                && !feature.parameters.contains_key("Depth") =>
            {
                if let Some(depth) = cadmpeg_ir::scalar::Length::new(depth_m * 1000.0) {
                    ctx.charge_work(1, OPERATION)?;
                    let value = crate::text_admission::format_retained(
                        ctx,
                        format_args!("{}", crate::history::literals::LengthLiteral(depth)),
                        OPERATION,
                    )?;
                    insert_termination_field(ctx, &mut feature.parameters, "D1", value, OPERATION)?;
                }
            }
            _ => {}
        }
    }
    Ok(())
}

fn insert_termination_field(
    ctx: &DecodeContext<'_>,
    fields: &mut std::collections::BTreeMap<cadmpeg_core::text::NonBlankString, String>,
    name: &'static str,
    value: String,
    operation: &'static str,
) -> Result<(), cadmpeg_core::CodecError> {
    let name =
        cadmpeg_core::text::NonBlankString::new(copy_termination_text(ctx, name, operation)?)
            .ok_or_else(|| cadmpeg_core::CodecError::malformed("blank termination field name"))?;
    ctx.insert_btree_map(fields, name, value, operation)?;
    Ok(())
}

fn consensus_termination_vote(
    ctx: &DecodeContext<'_>,
    votes: &[Option<TerminationVote>],
) -> Result<Option<TerminationVote>, cadmpeg_core::CodecError> {
    const OPERATION: &str = "agree SLDPRT extrusion terminations";
    let Some(first) = votes.first().and_then(Option::as_ref) else {
        return Ok(None);
    };
    for vote in votes {
        ctx.charge_work(1, OPERATION)?;
        let Some(vote) = vote else {
            return Ok(None);
        };
        for value in [first, vote] {
            let fields = match value {
                TerminationVote::ToVertex { reference } => [Some(reference.as_str()), None, None],
                TerminationVote::Face {
                    reference,
                    identity,
                    ..
                } => match reference {
                    FaceReference::Lane {
                        reference,
                        canonical,
                    } => [
                        Some(reference.as_str()),
                        canonical.as_deref(),
                        Some(identity.as_str()),
                    ],
                    FaceReference::Canonical(reference) => {
                        [Some(reference.as_str()), None, Some(identity.as_str())]
                    }
                    FaceReference::Unresolved => [None, None, Some(identity.as_str())],
                },
                _ => [None, None, None],
            };
            for text in fields.into_iter().flatten() {
                ctx.charge_work(u64_from_index(text.len()), OPERATION)?;
            }
        }
        if !vote.agrees_with(first) {
            return Ok(None);
        }
    }
    let mut consensus = first.copy_charged(ctx, OPERATION)?;
    if let TerminationVote::Face { reference, .. } = &mut consensus {
        if !votes
            .iter()
            .filter_map(Option::as_ref)
            .all(|vote| vote.reference() == first.reference())
        {
            *reference = reference.canonical(ctx, OPERATION)?;
        }
    }
    Ok(Some(consensus))
}

fn compact_termination_face_vote(
    ctx: &DecodeContext<'_>,
    condition: FaceCondition,
    lane: &FeatureInputLane,
    feature_ref: &str,
    lane_key: &str,
    offset: usize,
) -> Result<TerminationVote, cadmpeg_core::CodecError> {
    const OPERATION: &str = "build SLDPRT extrusion face vote";
    ctx.charge_work(u64_from_index(lane_key.len()), OPERATION)?;
    let reference = crate::text_admission::format_retained(
        ctx,
        format_args!("sldprt:feature-input:single-face-ref:{lane_key}:{offset}"),
        OPERATION,
    )?;
    for selection in &lane.surface_selections {
        let work = u64_from_index(selection.feature_ref.len())
            .checked_add(u64_from_index(feature_ref.len()))
            .and_then(|work| work.checked_add(1))
            .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
        ctx.charge_work(work, OPERATION)?;
    }
    let selection = lane.surface_selections.iter().find(|selection| {
        selection.feature_ref == feature_ref
            && usize::try_from(selection.offset).ok() == Some(offset)
    });
    let canonical_reference = selection
        .map(|selection| compact_surface_selection_value(ctx, &selection.components))
        .transpose()?;
    let identity = match selection {
        Some(selection) => {
            let canonical = canonical_reference.as_deref().unwrap_or_default();
            let terminal = selection
                .terminal_feature_ref
                .as_deref()
                .unwrap_or_default();
            let mut size = canonical
                .len()
                .checked_add(terminal.len())
                .and_then(|size| size.checked_add(2))
                .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
            ctx.charge_work(u64_from_index(size), OPERATION)?;
            for (index, producer) in selection.producer_feature_refs.iter().enumerate() {
                ctx.charge_work(
                    u64_from_index(producer.len())
                        .checked_add(1)
                        .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?,
                    OPERATION,
                )?;
                size = size
                    .checked_add(producer.len())
                    .and_then(|size| size.checked_add(usize::from(index != 0)))
                    .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
            }
            let mut identity = String::new();
            crate::text_admission::reserve_retained_string(ctx, &mut identity, size, OPERATION)?;
            identity.push_str(canonical);
            identity.push('|');
            for (index, producer) in selection.producer_feature_refs.iter().enumerate() {
                if index != 0 {
                    identity.push(',');
                }
                identity.push_str(producer);
            }
            identity.push('|');
            identity.push_str(terminal);
            identity
        }
        None => copy_termination_text(ctx, &reference, OPERATION)?,
    };
    Ok(TerminationVote::Face {
        condition,
        reference: FaceReference::Lane {
            reference,
            canonical: canonical_reference,
        },
        identity,
    })
}

pub(super) fn is_extrusion_end_spec_owner(feature: &crate::records::Feature) -> bool {
    native_object_class(feature.input_class.as_deref().unwrap_or_default())
        == NativeClassKind::Extrusion
        || matches!(feature.xml_tag.as_str(), "Extrusion" | "Cut")
}

#[derive(PartialEq)]
struct CombineSelection {
    target: String,
    tools: String,
    operation: Option<String>,
}

/// Add target and tool body paths carried by compact combine objects.
pub(crate) fn enrich_history_combine_selections(
    ctx: &DecodeContext<'_>,
    histories: &mut [crate::records::FeatureHistory],
    lanes: &[FeatureInputLane],
) -> Result<(), cadmpeg_core::CodecError> {
    const OPERATION: &str = "enrich SLDPRT combine selections";
    let mut selections = HashMap::<String, Vec<Option<CombineSelection>>>::new();
    for lane in lanes {
        let objects = history_object_offsets(ctx, histories, lane, OPERATION)?;
        for (index, (start, feature_id)) in objects.iter().enumerate() {
            for feature in histories.iter().flat_map(|history| &history.features) {
                let work = u64_from_index(feature.id.len())
                    .checked_add(u64_from_index(feature_id.len()))
                    .and_then(|work| work.checked_add(1))
                    .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
                ctx.charge_work(work, OPERATION)?;
            }
            let Some(feature) = histories
                .iter()
                .flat_map(|history| &history.features)
                .find(|feature| feature.id == *feature_id)
            else {
                continue;
            };
            ctx.charge_work(
                u64_from_index(feature.input_class.as_ref().map_or(0, String::len)),
                OPERATION,
            )?;
            if native_object_class(feature.input_class.as_deref().unwrap_or_default())
                != NativeClassKind::Combine
            {
                continue;
            }
            let Ok(start) = usize::try_from(*start) else {
                continue;
            };
            let end = objects
                .get(index + 1)
                .and_then(|object| usize::try_from(object.0).ok())
                .unwrap_or(lane.native_payload.len());
            let mut first = None;
            let mut last = None;
            if let (Some(scan_start), Some(scan_end)) = (
                start.checked_add(12),
                end.checked_sub(COMPACT_EDGE_VECTOR_MARKER.len()),
            ) {
                for marker in scan_start..scan_end {
                    ctx.charge_work(32, OPERATION)?;
                    if compact_body_component_path_at(ctx, &lane.native_payload, marker)?.is_some()
                    {
                        if first.is_none() {
                            first = Some(marker);
                        }
                        last = Some(marker);
                    }
                }
            }
            let selection = match (first, last) {
                (Some(target), Some(tools)) if target != tools => {
                    let operation = compact_combine_operation_at(&lane.native_payload, start);
                    let lane_key = lane
                        .id
                        .rsplit_once('#')
                        .map_or(lane.id.as_str(), |(_, key)| key);
                    ctx.charge_work(
                        u64_from_index(lane_key.len())
                            .checked_mul(2)
                            .ok_or_else(|| {
                                ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX)
                            })?,
                        OPERATION,
                    )?;
                    Some(CombineSelection {
                        target: crate::text_admission::format_retained(
                            ctx,
                            format_args!("sldprt:feature-input:body-path:{lane_key}:{target}"),
                            OPERATION,
                        )?,
                        tools: crate::text_admission::format_retained(
                            ctx,
                            format_args!("sldprt:feature-input:body-path:{lane_key}:{tools}"),
                            OPERATION,
                        )?,
                        operation: operation
                            .map(|operation| copy_termination_text(ctx, operation, OPERATION))
                            .transpose()?,
                    })
                }
                _ => None,
            };
            ctx.charge_work(u64_from_index(feature_id.len()), OPERATION)?;
            if !selections.contains_key(feature_id.as_str()) {
                let key = copy_termination_text(ctx, feature_id, OPERATION)?;
                ctx.insert_hash_map(&mut selections, key, Vec::new(), OPERATION)?;
            }
            let votes = selections.get_mut(feature_id.as_str()).ok_or_else(|| {
                cadmpeg_core::CodecError::malformed("missing admitted combine vote bucket")
            })?;
            ctx.reserve_collection_vec(votes, 1, OPERATION)?;
            votes.push(selection);
        }
    }
    for feature in histories
        .iter_mut()
        .flat_map(|history| &mut history.features)
    {
        ctx.charge_work(
            u64_from_index(feature.id.len())
                .checked_add(1)
                .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?,
            OPERATION,
        )?;
        let Some(votes) = selections.get(&feature.id) else {
            continue;
        };
        let Some(Some(first)) = votes.first() else {
            continue;
        };
        let mut agreement = true;
        for vote in votes {
            for text in [
                Some(first.target.as_str()),
                Some(first.tools.as_str()),
                first.operation.as_deref(),
                vote.as_ref().map(|vote| vote.target.as_str()),
                vote.as_ref().map(|vote| vote.tools.as_str()),
                vote.as_ref().and_then(|vote| vote.operation.as_deref()),
            ]
            .into_iter()
            .flatten()
            {
                ctx.charge_work(u64_from_index(text.len()), OPERATION)?;
            }
            ctx.charge_work(1, OPERATION)?;
            if vote.as_ref() != Some(first) {
                agreement = false;
                break;
            }
        }
        if !agreement {
            continue;
        }
        for (key, value) in [
            ("Target", Some(first.target.as_str())),
            ("Tools", Some(first.tools.as_str())),
            ("Operation", first.operation.as_deref()),
        ] {
            let Some(value) = value else {
                continue;
            };
            ctx.charge_work(1, OPERATION)?;
            if feature.properties.contains_key(key) {
                continue;
            }
            let value = copy_termination_text(ctx, value, OPERATION)?;
            insert_termination_field(ctx, &mut feature.properties, key, value, OPERATION)?;
        }
    }
    Ok(())
}

fn compact_combine_operation_at(payload: &[u8], name_offset: usize) -> Option<&'static str> {
    let name_prefix = payload.get(name_offset..name_offset.checked_add(5)?)?;
    let name_token = View::u16_le_at(name_prefix, 0)?;
    if !is_class_token(name_token) || name_prefix[2..] != [0xff, 0xfe, 0xff] {
        return None;
    }
    let name_units = usize::from(*payload.get(name_offset.checked_add(5)?)?);
    let operation = name_offset.checked_add(117usize.checked_add(name_units.checked_mul(2)?)?)?;
    let operation_end = operation.checked_add(4)?;
    let standard_tail = payload
        .get(operation_end..operation_end.checked_add(10)?)
        .is_some_and(|tail| tail == [0, 0, 0, 0, 0, 0, 0xff, 0xff, 0xff, 0xff]);
    let alternate_tail = payload
        .get(operation_end..operation_end.checked_add(6)?)
        .is_some_and(|tail| tail == [0, 0, 0xff, 0xff, 0xff, 0xff]);
    if payload
        .get(operation - 12..operation)?
        .iter()
        .any(|byte| *byte != 0)
        || !(standard_tail || alternate_tail)
    {
        return None;
    }
    match View::u32_le_at(payload, operation)? {
        0 => Some("Join"),
        1 => Some("Cut"),
        2 => Some("Intersect"),
        _ => None,
    }
}

/// Add compact general-curve reference identities carried by solid sweeps.
pub(crate) fn enrich_history_sweep_paths(
    ctx: &DecodeContext<'_>,
    histories: &mut [crate::records::FeatureHistory],
    lanes: &[FeatureInputLane],
) -> Result<(), cadmpeg_core::CodecError> {
    const OPERATION: &str = "enrich SLDPRT sweep paths";
    let mut paths = HashMap::<String, Vec<Option<String>>>::new();
    for lane in lanes {
        let objects = history_object_offsets(ctx, histories, lane, OPERATION)?;
        for (index, &(start, ref feature_id)) in objects.iter().enumerate() {
            let mut feature = None;
            for candidate in histories.iter().flat_map(|history| &history.features) {
                let work = u64_from_index(candidate.id.len())
                    .checked_add(u64_from_index(feature_id.len()))
                    .and_then(|work| work.checked_add(1))
                    .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
                ctx.charge_work(work, OPERATION)?;
                if candidate.id == *feature_id {
                    feature = Some(candidate);
                    break;
                }
            }
            let Some(feature) = feature else {
                continue;
            };
            if !matches!(
                native_object_class(feature.input_class.as_deref().unwrap_or_default()),
                NativeClassKind::Sweep | NativeClassKind::SweepReferenceSurface
            ) || feature.properties.contains_key("Path")
            {
                continue;
            }
            let (Ok(start), end) = (
                usize::try_from(start),
                objects
                    .get(index + 1)
                    .and_then(|object| usize::try_from(object.0).ok())
                    .unwrap_or(lane.native_payload.len()),
            ) else {
                continue;
            };
            let mut path_offset = None;
            let mut ambiguous_offset = false;
            let mut source = None;
            let mut ambiguous_source = false;
            let mut observe_offset = |offset| {
                if path_offset.is_some_and(|existing| existing != offset) {
                    ambiguous_offset = true;
                } else if path_offset.is_none() {
                    path_offset = Some(offset);
                }
            };
            let mut observe_source = |value| {
                if source.is_some_and(|existing| existing != value) {
                    ambiguous_source = true;
                } else if source.is_none() {
                    source = Some(value);
                }
            };
            for class in &lane.classes {
                ctx.charge_work(1, OPERATION)?;
                if class.name != "moGeneralCurveRef_w"
                    || class.offset < u64_from_index(start)
                    || class.offset >= u64_from_index(end)
                {
                    continue;
                }
                let Ok(offset) = usize::try_from(class.offset) else {
                    continue;
                };
                observe_offset(offset);
                ctx.charge_work(320, OPERATION)?;
                if let Some(value) =
                    declared_general_curve_profile_prefix(&lane.native_payload, offset).and_then(
                        |prefix| component_profile_source_at(&lane.native_payload, prefix),
                    )
                {
                    observe_source(value);
                }
            }
            if let Some(scan_end) = end.checked_sub(16) {
                for offset in start..scan_end {
                    ctx.charge_work(32, OPERATION)?;
                    if compact_general_curve_ref_at(&lane.native_payload, offset) {
                        observe_offset(offset);
                    }
                    if compact_profile_general_curve_ref_at(&lane.native_payload, offset) {
                        observe_offset(offset);
                        ctx.charge_work(224, OPERATION)?;
                        if let Some(value) =
                            component_profile_source_at(&lane.native_payload, offset + 6)
                        {
                            observe_source(value);
                        }
                    }
                }
            }
            let path = if let Some(source) = source.filter(|_| !ambiguous_source) {
                Some(crate::text_admission::format_retained(
                    ctx,
                    format_args!("{source}"),
                    OPERATION,
                )?)
            } else if let Some(offset) = path_offset.filter(|_| !ambiguous_offset) {
                let lane_key = lane
                    .id
                    .rsplit_once('#')
                    .map_or(lane.id.as_str(), |(_, key)| key);
                ctx.charge_work(u64_from_index(lane_key.len()), OPERATION)?;
                Some(crate::text_admission::format_retained(
                    ctx,
                    format_args!("sldprt:feature-input:general-curve-ref:{lane_key}:{offset}"),
                    OPERATION,
                )?)
            } else {
                None
            };
            ctx.charge_work(
                u64_from_index(feature_id.len())
                    .checked_mul(3)
                    .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?,
                OPERATION,
            )?;
            if !paths.contains_key(feature_id) {
                let key = copy_termination_text(ctx, feature_id, OPERATION)?;
                ctx.insert_hash_map(&mut paths, key, Vec::new(), OPERATION)?;
            }
            if let Some(votes) = paths.get_mut(feature_id) {
                ctx.reserve_collection_vec(votes, 1, OPERATION)?;
                votes.push(path);
            }
        }
    }
    for feature in histories
        .iter_mut()
        .flat_map(|history| &mut history.features)
    {
        ctx.charge_work(
            u64_from_index(feature.id.len())
                .checked_add(1)
                .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?,
            OPERATION,
        )?;
        if feature.properties.contains_key("Path") {
            continue;
        }
        let Some(votes) = paths.get(&feature.id) else {
            continue;
        };
        let Some(Some(first)) = votes.first() else {
            continue;
        };
        let mut agreement = true;
        for vote in votes {
            let work = u64_from_index(first.len())
                .checked_add(u64_from_index(vote.as_ref().map_or(0, String::len)))
                .and_then(|work| work.checked_add(1))
                .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
            ctx.charge_work(work, OPERATION)?;
            if vote.as_ref() != Some(first) {
                agreement = false;
                break;
            }
        }
        if agreement {
            let path = copy_termination_text(ctx, first, OPERATION)?;
            ctx.insert_btree_map(
                &mut feature.properties,
                cadmpeg_core::nonblank_literal!("Path"),
                path,
                OPERATION,
            )?;
        }
    }
    Ok(())
}

fn history_object_offsets(
    ctx: &DecodeContext<'_>,
    histories: &[crate::records::FeatureHistory],
    lane: &FeatureInputLane,
    operation: &'static str,
) -> Result<Vec<(u64, String)>, cadmpeg_core::CodecError> {
    let mut objects = Vec::new();
    for feature in histories.iter().flat_map(|history| &history.features) {
        ctx.charge_work(1, operation)?;
        for name in &lane.names {
            let work = u64_from_index(name.value.len())
                .checked_add(u64_from_index(feature.name.len()))
                .and_then(|work| work.checked_add(2))
                .ok_or_else(|| ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX))?;
            ctx.charge_work(work, operation)?;
        }
        let Some(name) = feature_object_name(feature, lane) else {
            continue;
        };
        let id = copy_termination_text(ctx, &feature.id, operation)?;
        ctx.reserve_collection_vec(&mut objects, 1, operation)?;
        objects.push((name.offset, id));
    }
    let levels = if objects.len() > 1 {
        objects.len().ilog2() + 1
    } else {
        1
    };
    ctx.charge_work(
        u64_from_index(objects.len())
            .checked_mul(u64::from(levels))
            .ok_or_else(|| ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX))?,
        operation,
    )?;
    ctx.sort_unstable_by(
        &mut objects,
        |left, right| left.0.cmp(&right.0),
        |_| 0,
        operation,
    )?;
    Ok(objects)
}

fn copy_termination_text(
    ctx: &DecodeContext<'_>,
    text: &str,
    operation: &'static str,
) -> Result<String, cadmpeg_core::CodecError> {
    ctx.charge_work(u64_from_index(text.len()), operation)?;
    let mut copy = String::new();
    crate::text_admission::reserve_retained_string(ctx, &mut copy, text.len(), operation)?;
    copy.push_str(text);
    Ok(copy)
}

fn copy_termination_feature_id(
    ctx: &DecodeContext<'_>,
    id: &cadmpeg_ir::features::FeatureId,
    operation: &'static str,
) -> Result<cadmpeg_ir::features::FeatureId, cadmpeg_core::CodecError> {
    ctx.charge_work(
        u64_from_index(id.as_str().len())
            .checked_mul(3)
            .ok_or_else(|| ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX))?,
        operation,
    )?;
    cadmpeg_ir::features::FeatureId::mint(copy_termination_text(ctx, id.as_str(), operation)?)
        .map_err(|_| cadmpeg_core::CodecError::malformed("invalid SLDPRT sweep feature identity"))
}

fn component_local_ids(
    ctx: &DecodeContext<'_>,
    components: &[FeatureInputComponentPathEntry],
    operation: &'static str,
) -> Result<String, cadmpeg_core::CodecError> {
    use std::fmt::Write;
    let size = components
        .len()
        .checked_mul(11)
        .ok_or_else(|| ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX))?;
    ctx.charge_work(u64_from_index(size), operation)?;
    let mut local_id = String::new();
    crate::text_admission::reserve_retained_string(ctx, &mut local_id, size, operation)?;
    for (index, component) in components.iter().enumerate() {
        if index != 0 {
            local_id.push(',');
        }
        match component.local_id {
            Some(id) => write!(&mut local_id, "{id}").map_err(|_| {
                cadmpeg_core::CodecError::malformed("SLDPRT sweep local identity formatting failed")
            })?,
            None => local_id.push('_'),
        }
    }
    Ok(local_id)
}

/// Bind reference-curve cross sections consumed by surface sweeps.
pub(crate) fn project_surface_sweep_profiles(
    ctx: &DecodeContext<'_>,
    features: &mut [cadmpeg_ir::features::Feature],
    histories: &[crate::records::FeatureHistory],
    lanes: &[FeatureInputLane],
) -> Result<(), cadmpeg_core::CodecError> {
    use cadmpeg_ir::features::{GeneratedCurveRef, PlanarProfileRef};
    const OPERATION: &str = "project SLDPRT surface sweep profile";
    let mut history_features = Vec::new();
    for feature in histories.iter().flat_map(|history| &history.features) {
        ctx.charge_work(1, OPERATION)?;
        ctx.reserve_collection_vec(&mut history_features, 1, OPERATION)?;
        history_features.push(feature);
    }
    let mut feature_ids_by_native = HashMap::new();
    for feature in features.iter() {
        ctx.charge_work(1, OPERATION)?;
        let Some(native) = feature.native_ref.as_deref() else {
            continue;
        };
        ctx.charge_work(
            u64_from_index(native.len())
                .checked_mul(2)
                .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?,
            OPERATION,
        )?;
        ctx.insert_hash_map(&mut feature_ids_by_native, native, &feature.id, OPERATION)?;
    }
    let mut projections = HashMap::new();
    for lane in lanes {
        for class in &lane.classes {
            ctx.charge_work(
                u64_from_index(class.name.len())
                    .checked_add(1)
                    .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?,
                OPERATION,
            )?;
        }
        let Some(reference_class) = lane
            .classes
            .iter()
            .find(|class| class.name == "moCompReferenceCurve_c")
        else {
            continue;
        };
        let Some(class_offset) = usize::try_from(reference_class.offset).ok() else {
            continue;
        };
        let Some(wrapper_token) = class_offset
            .checked_sub(2)
            .and_then(|offset| lane.native_payload.get(offset..offset + 2))
        else {
            continue;
        };
        let wrapper_token = [wrapper_token[0], wrapper_token[1]];
        let declared_prefix = class_offset.checked_add(6 + reference_class.name.len());
        let lane_key = lane
            .id
            .rsplit_once('#')
            .map_or(lane.id.as_str(), |(_, key)| key);
        let mut objects = Vec::new();
        for feature in &history_features {
            for name in &lane.names {
                let work = u64_from_index(name.value.len())
                    .checked_add(u64_from_index(feature.name.len()))
                    .and_then(|work| work.checked_add(2))
                    .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
                ctx.charge_work(work, OPERATION)?;
            }
            if let Some(name) = feature_object_name(feature, lane) {
                ctx.reserve_collection_vec(&mut objects, 1, OPERATION)?;
                objects.push((name.offset, *feature));
            }
        }
        let levels = if objects.len() > 1 {
            objects.len().ilog2() + 1
        } else {
            1
        };
        ctx.charge_work(
            u64_from_index(objects.len())
                .checked_mul(u64::from(levels))
                .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?,
            OPERATION,
        )?;
        ctx.sort_unstable_by(
            &mut objects,
            |(left, _), (right, _)| left.cmp(right),
            |_| 0,
            OPERATION,
        )?;
        for (index, &(start, feature)) in objects.iter().enumerate() {
            ctx.charge_work(
                u64_from_index(feature.input_class.as_ref().map_or(0, String::len))
                    .checked_add(1)
                    .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?,
                OPERATION,
            )?;
            if native_object_class(feature.input_class.as_deref().unwrap_or_default())
                != NativeClassKind::SweepReferenceSurface
            {
                continue;
            }
            let (Ok(start), end) = (
                usize::try_from(start),
                objects
                    .get(index + 1)
                    .and_then(|(offset, _)| usize::try_from(*offset).ok())
                    .unwrap_or(lane.native_payload.len()),
            ) else {
                continue;
            };
            let direct_source = declared_prefix
                .filter(|prefix| (start..end).contains(prefix))
                .and_then(|prefix| component_profile_source_at(&lane.native_payload, prefix));
            let direct = if let Some(source) = direct_source {
                ctx.charge_work(u64_from_index(history_features.len()), OPERATION)?;
                if let Some(native) = history_features
                    .iter()
                    .find(|candidate| candidate.source_value() == Some(source))
                {
                    ctx.charge_work(u64_from_index(native.id.len()), OPERATION)?;
                    feature_ids_by_native
                        .get(native.id.as_str())
                        .map(|id| {
                            copy_termination_feature_id(ctx, id, OPERATION)
                                .map(PlanarProfileRef::Feature)
                        })
                        .transpose()?
                } else {
                    None
                }
            } else {
                None
            };
            let mut generated = Vec::new();
            if let Some(scan_end) = end.checked_sub(6) {
                for wrapper in start..scan_end {
                    ctx.charge_work(1, OPERATION)?;
                    if lane.native_payload.get(wrapper..wrapper + 2) != Some(&wrapper_token)
                        || lane.native_payload.get(wrapper + 4..wrapper + 9)
                            != Some(&[0x2b, 0x80, 0x02, 0, 0])
                        || wrapper.checked_sub(2).is_some_and(|prefix| {
                            lane.native_payload.get(prefix..wrapper) == Some(&[1, 0])
                        })
                    {
                        continue;
                    }
                    let mut candidates = Vec::new();
                    if let Some(marker_end) = end.checked_sub(16) {
                        for marker in wrapper + 4..marker_end {
                            ctx.charge_work(1, OPERATION)?;
                            if lane.native_payload.get(marker..marker + 16)
                                != Some(COMPACT_EDGE_VECTOR_MARKER.as_slice())
                            {
                                continue;
                            }
                            if let Some(components) = component_reference_curve_path_at(
                                ctx,
                                &lane.native_payload,
                                marker,
                            )? {
                                ctx.reserve_collection_vec(&mut candidates, 1, OPERATION)?;
                                candidates.push((marker, components));
                            }
                        }
                    }
                    if candidates.len() != 1 {
                        continue;
                    }
                    let Some((_, components)) = candidates.pop() else {
                        continue;
                    };
                    let Some(owner) = component_path_terminal_feature(
                        ctx,
                        &components,
                        history_features.iter().copied(),
                    )?
                    else {
                        continue;
                    };
                    ctx.charge_work(u64_from_index(owner.len()), OPERATION)?;
                    let Some(id) = feature_ids_by_native.get(owner.as_str()) else {
                        continue;
                    };
                    let feature_id = copy_termination_feature_id(ctx, id, OPERATION)?;
                    let local_id = component_local_ids(ctx, &components, OPERATION)?;
                    ctx.charge_work(u64_from_index(lane_key.len()), OPERATION)?;
                    let native = crate::text_admission::format_retained(
                        ctx,
                        format_args!(
                            "sldprt:feature-input:component-reference-curve:{lane_key}:{wrapper}"
                        ),
                        OPERATION,
                    )?;
                    let Ok(curve) = GeneratedCurveRef::new(feature_id, local_id) else {
                        continue;
                    };
                    let mut curves = Vec::new();
                    ctx.reserve_collection_vec(&mut curves, 1, OPERATION)?;
                    curves.push(curve);
                    let Ok(profile) = PlanarProfileRef::generated(curves, native) else {
                        continue;
                    };
                    ctx.reserve_collection_vec(&mut generated, 1, OPERATION)?;
                    generated.push((profile, components));
                }
            }
            let (profile, components) = match (direct, generated.len()) {
                (Some(profile), 0) => (profile, None),
                (None, 1) => {
                    let Some((profile, components)) = generated.pop() else {
                        continue;
                    };
                    (profile, Some(components))
                }
                _ => continue,
            };
            let mut dependencies = Vec::new();
            if let Some(components) = &components {
                for native in
                    component_path_features(ctx, components, history_features.iter().copied())?
                {
                    ctx.charge_work(u64_from_index(native.len()), OPERATION)?;
                    if let Some(id) = feature_ids_by_native.get(native.as_str()) {
                        let id = copy_termination_feature_id(ctx, id, OPERATION)?;
                        ctx.reserve_collection_vec(&mut dependencies, 1, OPERATION)?;
                        dependencies.push(id);
                    }
                }
            }
            match &profile {
                PlanarProfileRef::Feature(id) => {
                    let id = copy_termination_feature_id(ctx, id, OPERATION)?;
                    ctx.reserve_collection_vec(&mut dependencies, 1, OPERATION)?;
                    dependencies.push(id);
                }
                PlanarProfileRef::Generated { curves, .. } => {
                    for curve in curves {
                        let id = copy_termination_feature_id(ctx, &curve.feature, OPERATION)?;
                        ctx.reserve_collection_vec(&mut dependencies, 1, OPERATION)?;
                        dependencies.push(id);
                    }
                }
                _ => {}
            }
            ctx.charge_work(u64_from_index(feature.id.len()), OPERATION)?;
            let key = copy_termination_text(ctx, &feature.id, OPERATION)?;
            ctx.insert_hash_map(&mut projections, key, (profile, dependencies), OPERATION)?;
        }
    }
    drop(feature_ids_by_native);
    for feature in features {
        ctx.charge_work(1, OPERATION)?;
        let Some(native) = feature.native_ref.as_deref() else {
            continue;
        };
        ctx.charge_work(u64_from_index(native.len()), OPERATION)?;
        let Some((profile, dependencies)) = projections.remove(native) else {
            continue;
        };
        if !matches!(feature.evaluation.definition(), FeatureDefinition::Operation(FeatureOperation::Sweep { shape, .. }) if shape.section_is_unresolved())
        {
            continue;
        }
        feature.evaluation.edit(|definition, _| {
            if let FeatureDefinition::Operation(FeatureOperation::Sweep { shape, .. }) = definition
            {
                shape.set_referenced_profile(profile);
            }
        });
        for dependency in dependencies {
            let work = u64_from_index(dependency.as_str().len())
                .checked_add(u64_from_index(feature.id.as_str().len()))
                .and_then(|work| work.checked_add(1))
                .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
            ctx.charge_work(work, OPERATION)?;
            for existing in &feature.dependencies {
                let work = u64_from_index(existing.as_str().len())
                    .checked_add(u64_from_index(dependency.as_str().len()))
                    .and_then(|work| work.checked_add(1))
                    .and_then(|work| work.checked_mul(2))
                    .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
                ctx.charge_work(work, OPERATION)?;
            }
            if dependency != feature.id && !feature.dependencies.contains(&dependency) {
                ctx.charge_collection_items(1, OPERATION)?;
                feature.dependencies.insert(dependency);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
pub(crate) fn compact_body_path_at(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    marker: usize,
) -> Result<Option<Vec<u32>>, cadmpeg_core::CodecError> {
    let components = compact_body_component_path_at(ctx, payload, marker)?;
    Ok(components.and_then(|components| {
        components
            .into_iter()
            .map(|component| component.local_id)
            .collect()
    }))
}

fn compact_body_component_path_at(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    marker: usize,
) -> Result<Option<Vec<FeatureInputComponentPathEntry>>, cadmpeg_core::CodecError> {
    ctx.charge_work(32, "decode SLDPRT body component path")?;
    let count = (|| {
        if marker < 12
            || payload.get(marker..marker + 16) != Some(COMPACT_EDGE_VECTOR_MARKER.as_slice())
            || !payload
                .get(marker - 8..marker - 4)
                .is_some_and(|selector| is_component_vector_selector_for_role(selector, 3))
            || payload.get(marker + 16..marker + 18) != Some(&[0, 0])
        {
            return None;
        }
        let count = usize::try_from(View::u32_le_at(payload, marker - 12)?)
            .ok()
            .filter(|count| *count != 0)?;
        Some(count)
    })();
    let Some(count) = count else {
        return Ok(None);
    };
    compact_body_component_entries_at(ctx, payload, marker + 18, count)
}

fn compact_body_component_entries_at(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    cursor: usize,
    count: usize,
) -> Result<Option<Vec<FeatureInputComponentPathEntry>>, cadmpeg_core::CodecError> {
    if count == 0 {
        return Ok(None);
    }
    let parse = |count| -> Result<Option<_>, cadmpeg_core::CodecError> {
        if let Some(path) = compact_heterogeneous_component_path(
            ctx,
            payload,
            cursor,
            count,
            "decode SLDPRT component path layout",
        )? {
            return Ok(Some(path));
        }
        let Some((components, end)) = compact_mixed_component_path(
            ctx,
            payload,
            cursor,
            count,
            true,
            "decode SLDPRT mixed component path",
        )?
        else {
            return Ok(None);
        };
        ctx.charge_work(
            u64_from_index(components.len()),
            "decode SLDPRT body mixed path",
        )?;
        Ok(components
            .iter()
            .any(|component| component.instance.is_none() || component.local_id.is_none())
            .then_some((components, end)))
    };
    if let Some((components, _)) = parse(count)? {
        return Ok(Some(components));
    }
    if count > 1 {
        let Some((components, end)) = parse(count - 1)? else {
            return Ok(None);
        };
        return Ok(compact_body_null_slot_at(payload, end).then_some(components));
    }
    Ok(None)
}

fn compact_body_null_slot_at(payload: &[u8], end: usize) -> bool {
    payload.get(end..end + 8) == Some(&[0xff, 0xff, 0xff, 0xff, 0, 0, 0, 0])
        || payload.get(end..end + 10) == Some(&[0; 10])
}

pub(crate) fn project_compact_combine_paths(
    ctx: &DecodeContext<'_>,
    features: &mut [cadmpeg_ir::features::Feature],
    histories: &[crate::records::FeatureHistory],
    lanes: &[FeatureInputLane],
) -> Result<(), cadmpeg_core::CodecError> {
    use cadmpeg_ir::features::{BodySelection, CombineOperands, GeneratedBodyRef};
    const OPERATION: &str = "project SLDPRT combine paths";
    struct Projection {
        target: BodySelection,
        tools: BodySelection,
        dependencies: Vec<cadmpeg_ir::features::FeatureId>,
    }
    let mut feature_ids_by_native = HashMap::new();
    for feature in features.iter() {
        ctx.charge_work(1, OPERATION)?;
        let Some(native) = feature.native_ref.as_deref() else {
            continue;
        };
        ctx.charge_work(
            u64_from_index(native.len())
                .checked_mul(2)
                .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?,
            OPERATION,
        )?;
        ctx.insert_hash_map(&mut feature_ids_by_native, native, &feature.id, OPERATION)?;
    }
    let mut history_features = Vec::new();
    for feature in histories.iter().flat_map(|history| &history.features) {
        ctx.charge_work(1, OPERATION)?;
        ctx.reserve_collection_vec(&mut history_features, 1, OPERATION)?;
        history_features.push(feature);
    }
    let mut projections = HashMap::<String, Projection>::new();
    for history_feature in &history_features {
        ctx.charge_work(1, OPERATION)?;
        let (Some(target), Some(tools)) = (
            history_feature.properties.get("Target"),
            history_feature.properties.get("Tools"),
        ) else {
            continue;
        };
        let project = |native: &str| -> Result<_, cadmpeg_core::CodecError> {
            ctx.charge_work(
                u64_from_index(native.len())
                    .checked_mul(4)
                    .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?,
                OPERATION,
            )?;
            let Some((prefix, offset)) = native.rsplit_once(':') else {
                return Ok(None);
            };
            let Ok(offset) = offset.parse::<usize>() else {
                return Ok(None);
            };
            let Some((_, lane_key)) = prefix.rsplit_once(':') else {
                return Ok(None);
            };
            for lane in lanes {
                let work = u64_from_index(lane.id.len())
                    .checked_add(u64_from_index(lane_key.len()))
                    .and_then(|work| work.checked_add(1))
                    .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
                ctx.charge_work(work, OPERATION)?;
            }
            let Some(lane) = lanes.iter().find(|lane| {
                lane.id
                    .rsplit_once('#')
                    .map_or(lane.id.as_str(), |(_, key)| key)
                    == lane_key
            }) else {
                return Ok(None);
            };
            let Some(components) =
                compact_body_component_path_at(ctx, &lane.native_payload, offset)?
            else {
                return Ok(None);
            };
            let Some(producer) = component_path_terminal_feature(
                ctx,
                &components,
                history_features.iter().copied(),
            )?
            else {
                return Ok(None);
            };
            ctx.charge_work(u64_from_index(producer.len()), OPERATION)?;
            let Some(id) = feature_ids_by_native.get(producer.as_str()) else {
                return Ok(None);
            };
            let owner = copy_termination_feature_id(ctx, id, OPERATION)?;
            let body_owner = copy_termination_feature_id(ctx, id, OPERATION)?;
            let local_id = component_local_ids(ctx, &components, OPERATION)?;
            let Ok(body) = GeneratedBodyRef::new(body_owner, local_id) else {
                return Ok(None);
            };
            let mut bodies = Vec::new();
            ctx.reserve_collection_vec(&mut bodies, 1, OPERATION)?;
            bodies.push(body);
            let native = copy_termination_text(ctx, native, OPERATION)?;
            let Ok(selection) = BodySelection::generated(bodies, native) else {
                return Ok(None);
            };
            Ok(Some((selection, components, owner)))
        };
        let (
            Some((target, target_components, target_owner)),
            Some((tools, tool_components, tool_owner)),
        ) = (project(target)?, project(tools)?)
        else {
            continue;
        };
        let mut dependencies = Vec::new();
        for component in target_components.iter().chain(&tool_components) {
            let Some(native) = component_path_terminal_feature(
                ctx,
                std::slice::from_ref(component),
                history_features.iter().copied(),
            )?
            else {
                continue;
            };
            ctx.charge_work(u64_from_index(native.len()), OPERATION)?;
            if let Some(feature) = feature_ids_by_native.get(native.as_str()) {
                let feature = copy_termination_feature_id(ctx, feature, OPERATION)?;
                ctx.reserve_collection_vec(
                    &mut dependencies,
                    1,
                    "project SLDPRT combine dependencies",
                )?;
                dependencies.push(feature);
            }
        }
        ctx.reserve_collection_vec(&mut dependencies, 2, "project SLDPRT combine dependencies")?;
        dependencies.push(target_owner);
        dependencies.push(tool_owner);
        let levels = if dependencies.len() > 1 {
            dependencies.len().ilog2() + 1
        } else {
            1
        };
        for dependency in &dependencies {
            for feature in features.iter() {
                let work = u64_from_index(dependency.as_str().len())
                    .checked_add(u64_from_index(feature.id.as_str().len()))
                    .and_then(|work| work.checked_add(1))
                    .and_then(|work| work.checked_mul(u64::from(levels).checked_mul(2)?))
                    .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
                ctx.charge_work(work, OPERATION)?;
            }
        }
        let mut ordered = Vec::new();
        for (ordinal, dependency) in dependencies.into_iter().enumerate() {
            let order = features
                .iter()
                .find(|feature| feature.id == dependency)
                .map_or(u64::MAX, |feature| feature.ordinal);
            ctx.reserve_collection_vec(&mut ordered, 1, OPERATION)?;
            ordered.push((order, ordinal, dependency));
        }
        ctx.sort_unstable_by(
            &mut ordered,
            |(left_order, left_ordinal, _), (right_order, right_ordinal, _)| {
                (left_order, left_ordinal).cmp(&(right_order, right_ordinal))
            },
            |_| 0,
            OPERATION,
        )?;
        let mut dependencies = Vec::<cadmpeg_ir::features::FeatureId>::new();
        for (_, _, dependency) in ordered {
            if let Some(previous) = dependencies.last() {
                let work = u64_from_index(dependency.as_str().len())
                    .checked_add(u64_from_index(previous.as_str().len()))
                    .and_then(|work| work.checked_add(1))
                    .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
                ctx.charge_work(work, OPERATION)?;
                if previous == &dependency {
                    continue;
                }
            }
            ctx.reserve_precharged_vec(&mut dependencies, 1, OPERATION)?;
            dependencies.push(dependency);
        }
        ctx.charge_work(u64_from_index(history_feature.id.len()), OPERATION)?;
        let key = copy_termination_text(ctx, &history_feature.id, OPERATION)?;
        ctx.insert_hash_map(
            &mut projections,
            key,
            Projection {
                target,
                tools,
                dependencies,
            },
            OPERATION,
        )?;
    }
    drop(feature_ids_by_native);
    for feature in features {
        ctx.charge_work(1, OPERATION)?;
        let Some(native) = feature.native_ref.as_deref() else {
            continue;
        };
        ctx.charge_work(u64_from_index(native.len()), OPERATION)?;
        let Some(projection) = projections.remove(native) else {
            continue;
        };
        if !matches!(
            feature.evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::Combine { .. })
        ) {
            continue;
        }
        for dependency in projection.dependencies {
            let work = u64_from_index(dependency.as_str().len())
                .checked_add(u64_from_index(feature.id.as_str().len()))
                .and_then(|work| work.checked_add(1))
                .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
            ctx.charge_work(work, OPERATION)?;
            for existing in &feature.dependencies {
                let work = u64_from_index(existing.as_str().len())
                    .checked_add(u64_from_index(dependency.as_str().len()))
                    .and_then(|work| work.checked_add(1))
                    .and_then(|work| work.checked_mul(2))
                    .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
                ctx.charge_work(work, OPERATION)?;
            }
            if dependency != feature.id && !feature.dependencies.contains(&dependency) {
                ctx.charge_collection_items(1, OPERATION)?;
                feature.dependencies.insert(dependency);
            }
        }
        for selection in [&projection.target, &projection.tools] {
            if let BodySelection::Generated { bodies, .. } = selection {
                for body in bodies {
                    let work = u64_from_index(body.feature.as_str().len())
                        .checked_add(u64_from_index(body.local_id.as_str().len()))
                        .and_then(|work| work.checked_add(2))
                        .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
                    ctx.charge_work(work, OPERATION)?;
                }
            }
        }
        let operands = CombineOperands::new(projection.target, projection.tools)
            .map_err(cadmpeg_core::CodecError::malformed)?;
        feature.evaluation.edit(|definition, _| {
            if let FeatureDefinition::Operation(FeatureOperation::Combine {
                operands: target,
                ..
            }) = definition
            {
                *target = operands;
            }
        });
    }
    Ok(())
}

fn compact_extrusion_through_all_at(payload: &[u8], offset: usize) -> bool {
    compact_extrusion_end_spec_header(payload, offset, 1)
        && (compact_extrusion_traversal_tail_at(payload, offset)
            || compact_extrusion_dimensioned_traversal_at(payload, offset)
            || (payload.get(offset + 22..offset + 26) == Some(&[0, 0, 0, 0])
                && compact_extrusion_dimension_child_at(payload, offset + 26).is_some()))
}

fn compact_extrusion_dimensioned_traversal_at(payload: &[u8], offset: usize) -> bool {
    payload.get(offset + 22..offset + 30) == Some(&[0; 8])
        && payload.get(offset + 30..offset + 34) == Some(&[1, 0, 0, 1])
        && payload
            .get(offset + 34..offset + 44)
            .is_some_and(|bytes| bytes.iter().all(|byte| *byte == 0))
        && payload.get(offset + 44..offset + 48) == Some(&1u32.to_le_bytes())
        && payload
            .get(offset + 48..offset + 68)
            .is_some_and(|bytes| bytes.iter().all(|byte| *byte == 0))
        && compact_extrusion_dimension_child_at(payload, offset + 68).is_some()
}

fn compact_extrusion_blind_at(payload: &[u8], offset: usize) -> bool {
    compact_extrusion_end_spec_header(payload, offset, 0)
        && ((payload.get(offset + 22..offset + 26) == Some(&[0, 0, 0, 0])
            && compact_extrusion_dimension_child_at(payload, offset + 26).is_some())
            || compact_extrusion_dimension_child_at(payload, offset + 22).is_some())
}

fn compact_extrusion_through_next_at(payload: &[u8], offset: usize) -> bool {
    compact_extrusion_end_spec_header(payload, offset, 2)
        && compact_extrusion_traversal_tail_at(payload, offset)
}

/// Through-all in both directions. Two carriers exist: a first-direction
/// traversal code `1` with second-direction code `1` and the shared traversal
/// tail, and the dedicated code `9` whose second-direction word is `1` and
/// whose retained blind dimension child follows immediately.
fn compact_extrusion_through_all_both_at(payload: &[u8], offset: usize) -> bool {
    (compact_extrusion_two_direction_header(payload, offset, 1)
        && payload.get(offset + 26..offset + 30) == Some(&[0, 0, 0, 0])
        && compact_extrusion_traversal_body_at(payload, offset))
        || (compact_extrusion_two_direction_header(payload, offset, 9)
            && compact_extrusion_dimension_child_at(payload, offset + 26).is_some())
}

/// Blind first direction with a through-all second direction: a code `0`
/// header whose second-direction word is `1`, owning the blind dimension
/// child.
fn compact_extrusion_blind_through_all_second_at(payload: &[u8], offset: usize) -> bool {
    compact_end_spec_identity_at(payload, offset)
        && payload.get(offset + 2..offset + 12) == Some(&[0, 0, 1, 0, 0, 0, 0, 0, 0, 0])
        && View::u32_le_at(payload, offset + 12).is_some_and(|flag| flag <= 1)
        && payload.get(offset + 16..offset + 22) == Some(&[0, 0, 0, 0, 0, 0])
        && payload.get(offset + 22..offset + 26) == Some(&[1, 0, 0, 0])
        && compact_extrusion_dimension_child_at(payload, offset + 26).is_some()
}

/// Two-direction end-spec header: the words at `+4` and `+8` carry `0` or
/// `1`, the first-direction code sits at `+18`, and the second-direction
/// code `1` sits at `+22`.
fn compact_extrusion_two_direction_header(payload: &[u8], offset: usize, code: u32) -> bool {
    compact_end_spec_identity_at(payload, offset)
        && payload.get(offset + 2..offset + 4) == Some(&[0, 0])
        && View::u32_le_at(payload, offset + 4).is_some_and(|word| word <= 1)
        && View::u32_le_at(payload, offset + 8).is_some_and(|word| word <= 1)
        && View::u32_le_at(payload, offset + 12).is_some_and(|flag| flag <= 1)
        && payload.get(offset + 16..offset + 18) == Some(&[0, 0])
        && payload.get(offset + 18..offset + 22) == Some(code.to_le_bytes().as_slice())
        && payload.get(offset + 22..offset + 26) == Some(&[1, 0, 0, 0])
}

fn compact_extrusion_traversal_tail_at(payload: &[u8], offset: usize) -> bool {
    payload.get(offset + 22..offset + 30) == Some(&[0, 0, 0, 0, 0, 0, 0, 0])
        && compact_extrusion_traversal_body_from(payload, offset + 30)
}

/// Shared traversal run from `+30`: the `[1, 0, 0, 1]` marker and the fixed
/// zero fill through the `+90` word.
fn compact_extrusion_traversal_body_at(payload: &[u8], offset: usize) -> bool {
    compact_extrusion_traversal_body_from(payload, offset + 30)
}

fn compact_extrusion_traversal_body_from(payload: &[u8], start: usize) -> bool {
    payload.get(start..start + 4) == Some(&[1, 0, 0, 1])
        && payload
            .get(start + 4..start + 60)
            .is_some_and(|bytes| bytes.iter().all(|byte| *byte == 0))
        && payload
            .get(start + 60..start + 64)
            .is_some_and(|word| word == [0, 0, 1, 0] || word == [1, 0, 0, 0])
        && payload
            .get(start + 64..start + 70)
            .is_some_and(|bytes| bytes.iter().all(|byte| *byte == 0))
        && compact_extrusion_traversal_follow_on_at(payload, start + 70)
}

fn compact_extrusion_traversal_follow_on_at(payload: &[u8], offset: usize) -> bool {
    let Some(bytes) = payload.get(offset..offset + 4) else {
        return false;
    };
    if bytes == [0, 0, 0, 0]
        || (bytes[1] & 0x80 != 0 && bytes[2..4] == [0, 0])
        || bytes == [0xff, 0xff, 1, 0]
    {
        return true;
    }
    bytes[1] & 0x80 != 0
        && payload.get(offset + 2..offset + 6) == Some(&5u32.to_le_bytes())
        && compact_extrusion_dimension_child_at(payload, offset + 6).is_some()
}

fn compact_extrusion_mid_plane_at(payload: &[u8], offset: usize) -> bool {
    compact_extrusion_end_spec_header(payload, offset, 6)
        && payload.get(offset + 22..offset + 26) == Some(&[0, 0, 0, 0])
        && compact_extrusion_dimension_child_at(payload, offset + 26).is_some()
}

/// Validate the owned dimension child at `child` and return the offset just
/// past its fixed tail.
fn compact_extrusion_dimension_child_at(payload: &[u8], child: usize) -> Option<usize> {
    let declaration = b"\xff\xff\x01\x00\x16\x00moDisplayDistanceDim_c";
    let block = if payload.get(child..child + declaration.len()) == Some(declaration) {
        child + declaration.len()
    } else if payload
        .get(child + 1)
        .is_some_and(|byte| byte & 0x80 != 0 && *byte != 0xff)
    {
        child + 2
    } else {
        return None;
    };
    (payload.get(block..block + 16).is_some_and(|bytes| {
        bytes.iter().enumerate().all(|(index, byte)| match index {
            8 => matches!(*byte, 0 | 0x40),
            9 => byte.trailing_zeros() >= 3,
            _ => *byte == 0,
        })
    }) && payload.get(block + 16..block + 20) == Some(&[0xff, 0xff, 0, 0])
        && payload
            .get(block + 20)
            .is_some_and(|byte| *byte == 1 || *byte == 3)
        && payload.get(block + 21..block + 25) == Some(&[0xff, 0xff, 0xff, 0xff])
        && payload.get(block + 25..block + 31) == Some(&[0, 0, 0, 0, 0, 0])
        && payload.get(block + 31..block + 33) == Some(&[0x80, 0xbf]))
    .then_some(block + 33)
}

/// Form of the point reference owned by an up-to-vertex end spec.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::resolved_features) enum CompactPointReferenceKind {
    /// Direct vertex reference; the final path entry's component id is the
    /// feature-local vertex id.
    Point,
    /// Edge endpoint reference; the path selects an edge and the endpoint
    /// selector stays native.
    EdgeEndpoint { selector: u32 },
}

impl CompactPointReferenceKind {
    pub(super) fn endpoint_selector(self) -> Option<u32> {
        match self {
            Self::Point => None,
            Self::EdgeEndpoint { selector } => Some(selector),
        }
    }
}

pub(super) fn compact_extrusion_to_vertex_at(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    offset: usize,
    end: usize,
) -> Result<Option<(usize, CompactPointReferenceKind)>, cadmpeg_core::CodecError> {
    ctx.charge_work(64, "decode SLDPRT extrusion vertex termination")?;
    let header = (|| {
        let end = super::DeclaredEnd::of(end, payload.len())?.get();
        let payload = payload.get(..end)?;
        if !compact_extrusion_end_spec_header(payload, offset, 3)
            || payload.get(offset + 22..offset + 30) != Some(&[0, 0, 0, 0, 0, 0, 0, 0])
        {
            return None;
        }
        let child = offset + 30;
        Some((payload, child, end))
    })();
    let Some((payload, child, end)) = header else {
        return Ok(None);
    };
    let point_declaration = b"\xff\xff\x01\x00\x0c\x00moPointRef_w";
    let endpoint_declaration = b"\xff\xff\x01\x00\x0f\x00moEndPointRef_w";
    let point_body_at = |body: usize| {
        payload.get(body + 1).is_some_and(|byte| byte & 0x80 != 0)
            && payload
                .get(body + 2..body + 4)
                .is_some_and(|bytes| bytes == [0xa9, 0x80] || bytes == [0x2b, 0x80])
            && payload.get(body + 4..body + 9) == Some(&[2, 0, 0, 0, 0])
    };
    let ReferenceOffsets::One(marker) =
        compact_termination_reference_offsets(ctx, payload, child, end, true)?
    else {
        return Ok(None);
    };
    let kind = (|| {
        let kind = if (payload.get(child..child + point_declaration.len())
            == Some(point_declaration)
            && point_body_at(child + point_declaration.len()))
            || point_body_at(child)
        {
            CompactPointReferenceKind::Point
        } else if payload.get(child..child + endpoint_declaration.len())
            == Some(endpoint_declaration)
        {
            let edge_declaration = b"\xff\xff\x01\x00\x0c\x00moCompEdge_c";
            let inner = child + endpoint_declaration.len();
            let body = inner + edge_declaration.len();
            if payload.get(inner..inner + edge_declaration.len()) != Some(edge_declaration)
                || payload.get(body + 1).is_none_or(|byte| byte & 0x80 == 0)
                || payload.get(body + 2..body + 7) != Some(&[2, 0, 0, 0, 0x40])
            {
                return None;
            }
            CompactPointReferenceKind::EdgeEndpoint {
                selector: View::u32_le_at(payload, marker.checked_sub(4)?)?,
            }
        } else {
            return None;
        };
        Some(kind)
    })();
    Ok(kind.map(|kind| (marker, kind)))
}

pub(super) fn compact_extrusion_offset_from_face_at(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    offset: usize,
    end: usize,
) -> Result<Option<usize>, cadmpeg_core::CodecError> {
    ctx.charge_work(64, "decode SLDPRT extrusion face offset")?;
    let header = (|| {
        let end = super::DeclaredEnd::of(end, payload.len())?.get();
        let payload = payload.get(..end)?;
        if !compact_extrusion_end_spec_header(payload, offset, 5)
            || payload.get(offset + 22..offset + 26) != Some(&[0, 0, 0, 0])
        {
            return None;
        }
        let resume = compact_extrusion_dimension_child_at(payload, offset + 26)?;
        Some((payload, resume, end))
    })();
    let Some((payload, resume, end)) = header else {
        return Ok(None);
    };
    let declaration = b"\xff\xff\x01\x00\x11\x00moSingleFaceRef_w";
    let mut candidates = ReferenceOffsets::Empty;
    let Some(scan_end) = end.checked_sub(2) else {
        return Ok(None);
    };
    for anchor in resume..scan_end {
        ctx.charge_work(128, "decode SLDPRT extrusion face offset")?;
        if payload.get(anchor..anchor + 3) != Some(&[1, 1, 0]) {
            continue;
        }
        let child = anchor + 3;
        let body = if payload.get(child..child + declaration.len()) == Some(declaration) {
            child + declaration.len()
        } else {
            child
        };
        // The reference body opens with lane tokens followed by the selector.
        let Some(open_start) = body.checked_add(2) else {
            return Ok(None);
        };
        let Some(open_end) = body.checked_add(9).map(|offset| offset.min(end)) else {
            return Ok(None);
        };
        for open in (open_start..open_end).filter(|cursor| {
            cursor.checked_add(7).is_some_and(|stop| stop <= end)
                && payload.get(cursor - 1).is_some_and(|byte| byte & 0x80 != 0)
                && payload.get(*cursor..cursor + 7) == Some(&[2, 0, 0, 0, 0x40, 0, 0])
        }) {
            match compact_termination_reference_offsets(ctx, payload, open, end, true)? {
                ReferenceOffsets::Empty => {}
                ReferenceOffsets::One(marker) => candidates.insert(marker),
                ReferenceOffsets::Ambiguous => candidates = ReferenceOffsets::Ambiguous,
            }
        }
    }
    Ok(match candidates {
        ReferenceOffsets::One(marker) => Some(marker),
        _ => None,
    })
}

pub(super) fn compact_extrusion_to_face_at(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    offset: usize,
    end: usize,
) -> Result<Option<usize>, cadmpeg_core::CodecError> {
    ctx.charge_work(128, "decode SLDPRT extrusion face termination")?;
    let header = (|| {
        let end = super::DeclaredEnd::of(end, payload.len())?.get();
        let payload = payload.get(..end)?;
        // Older end-spec streams encode the `moEndSpec_c` class as the fixed
        // two-byte token `03 00`; their remaining header and child grammar is
        // identical. Keep that token scoped to the to-face form whose required
        // single-face child independently validates the interpretation.
        let legacy_header = payload.get(offset..offset + 2) == Some(&[3, 0])
            && payload.get(offset + 2..offset + 12) == Some(&[0, 0, 1, 0, 0, 0, 0, 0, 0, 0])
            && View::u32_le_at(payload, offset + 12).is_some_and(|flag| flag <= 1)
            && payload.get(offset + 16..offset + 18) == Some(&[0, 0])
            && payload.get(offset + 18..offset + 22) == Some(&[4, 0, 0, 0]);
        if !(compact_extrusion_end_spec_header(payload, offset, 4) || legacy_header)
            || View::u32_le_at(payload, offset + 22).is_none_or(|flag| flag > 1)
            || payload.get(offset + 26..offset + 30) != Some(&[0, 0, 0, 0])
            || payload.get(offset + 30..offset + 33) != Some(&[1, 1, 0])
        {
            return None;
        }
        let declaration = b"\xff\xff\x01\x00\x11\x00moSingleFaceRef_w";
        let child = offset + 33;
        let declared = payload.get(child..child + declaration.len()) == Some(declaration);
        let body_offset = if declared {
            child + declaration.len()
        } else if compact_single_face_child_body_at(payload, child + 2) {
            child + 2
        } else {
            return None;
        };
        if !declared && !compact_single_face_child_body_at(payload, body_offset) {
            return None;
        }
        // A declared child begins with a fixed body header. Starting the marker
        // search at that header lets the legacy path decoder reinterpret the
        // header as a second, spurious compact reference. The modern marker is
        // always after the header; an undeclared legacy child still needs the
        // body offset as its fallback anchor.
        let search_start = if declared {
            body_offset.checked_add(11)?
        } else {
            body_offset
        };
        Some((payload, search_start, end, declared, body_offset))
    })();
    let Some((payload, search_start, end, declared, body_offset)) = header else {
        return Ok(None);
    };
    let compact_candidates =
        compact_termination_reference_offsets(ctx, payload, search_start, end, declared)?;
    match compact_candidates {
        ReferenceOffsets::One(candidate) => Ok(Some(candidate)),
        ReferenceOffsets::Empty
            if declared && compact_tokenized_single_face_child_at(payload, body_offset) =>
        {
            Ok(Some(body_offset))
        }
        ReferenceOffsets::Empty => {
            Ok(
                legacy_single_face_reference_path_at(ctx, payload, body_offset)?
                    .map(|_| body_offset),
            )
        }
        ReferenceOffsets::Ambiguous => Ok(None),
    }
}

fn compact_termination_reference_offsets(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    start: usize,
    end: usize,
    require_path: bool,
) -> Result<ReferenceOffsets, cadmpeg_core::CodecError> {
    let end = super::DeclaredEnd::of(end, payload.len()).map_or(start, super::DeclaredEnd::get);
    let mut candidates = ReferenceOffsets::Empty;
    for marker in start..end {
        ctx.charge_work(1, "scan SLDPRT termination reference offsets")?;
        let present = if require_path {
            compact_termination_reference_path_at(ctx, payload, marker)?.is_some()
        } else {
            compact_termination_reference_frame_at(payload, marker).is_some()
        };
        if present {
            candidates.insert(marker);
        }
    }
    Ok(candidates)
}

enum ReferenceOffsets {
    Empty,
    One(usize),
    Ambiguous,
}

impl ReferenceOffsets {
    fn insert(&mut self, offset: usize) {
        match self {
            Self::Empty => *self = Self::One(offset),
            Self::One(first) if *first != offset => *self = Self::Ambiguous,
            Self::One(_) | Self::Ambiguous => {}
        }
    }
}

fn compact_tokenized_single_face_child_at(payload: &[u8], offset: usize) -> bool {
    if compact_tokenized_face_body_at(payload, offset, 2) {
        return true;
    }
    let declaration = b"\xff\xff\x01\x00\x0c\x00moCompFace_c";
    payload.get(offset..offset + declaration.len()) == Some(declaration)
        && compact_tokenized_face_body_at(payload, offset + declaration.len(), 1)
}

fn compact_tokenized_face_body_at(
    payload: &[u8],
    offset: usize,
    leading_class_tokens: usize,
) -> bool {
    let word_count = leading_class_tokens + 7;
    let Some(body) = payload.get(offset..offset + word_count * 2) else {
        return false;
    };
    let token_at = |index: usize| View::u16_le_at(body, index * 2);
    (1..=2).contains(&leading_class_tokens)
        && (0..leading_class_tokens).all(|index| token_at(index).is_some_and(is_class_token))
        && token_at(leading_class_tokens) == Some(2)
        && token_at(leading_class_tokens + 1).is_some_and(is_class_token)
        && token_at(leading_class_tokens + 2) == Some(0)
        && token_at(leading_class_tokens + 3).is_some_and(is_class_token)
        && token_at(leading_class_tokens + 4) == Some(1)
        && token_at(leading_class_tokens + 5) == Some(0)
        && token_at(leading_class_tokens + 6).is_some_and(is_class_token)
}

fn compact_single_face_child_body_at(payload: &[u8], offset: usize) -> bool {
    let Some(body) = payload.get(offset..offset + 11) else {
        return false;
    };
    View::u16_le_at(body, 0).is_some_and(is_class_token)
        && View::u16_le_at(body, 2).is_some_and(is_class_token)
        && body[4..8] == 2u32.to_le_bytes()
        && matches!(body[8], 0 | 0x40)
        && body[9..11] == [0, 0]
}

/// End-spec children carry their class at the anchor: either a lane-scoped
/// class token or a direct `moEndSpec_c` declaration ending at the anchor.
/// Header-shaped runs without this identity belong to fillet edge-set records.
fn compact_end_spec_identity_at(payload: &[u8], offset: usize) -> bool {
    payload
        .get(offset..offset + 2)
        .and_then(|bytes| View::u16_le_at(bytes, 0))
        .is_some_and(is_class_token)
        || offset
            .checked_sub(15)
            .and_then(|start| payload.get(start..offset + 2))
            == Some(b"\xff\xff\x01\x00\x0b\x00moEndSpec_c".as_slice())
}

fn compact_extrusion_end_spec_header(payload: &[u8], offset: usize, code: u32) -> bool {
    compact_end_spec_identity_at(payload, offset)
        && payload.get(offset + 2..offset + 4) == Some(&[0, 0])
        && payload.get(offset + 4..offset + 8) == Some(&1u32.to_le_bytes())
        && View::u32_le_at(payload, offset + 8).is_some_and(|word| word <= 1)
        && View::u32_le_at(payload, offset + 12).is_some_and(|flag| flag <= 1)
        && payload.get(offset + 16..offset + 18) == Some(&[0, 0])
        && payload.get(offset + 18..offset + 22) == Some(code.to_le_bytes().as_slice())
}

fn compact_single_face_reference_path_at(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    marker: usize,
) -> Result<Option<Vec<FeatureInputComponentPathEntry>>, cadmpeg_core::CodecError> {
    if let Some(super::selections::ComponentPathReference(components, _)) =
        compact_single_face_reference_record_at(ctx, payload, marker)?
    {
        return Ok(Some(components));
    }
    legacy_single_face_reference_path_at(ctx, payload, marker)
}

struct LegacyFacePathSearch<'a, 'b> {
    ctx: &'a DecodeContext<'b>,
    payload: &'a [u8],
    entries: Vec<FeatureInputComponentPathEntry>,
    complete: Option<Vec<FeatureInputComponentPathEntry>>,
    ambiguous: bool,
}

impl LegacyFacePathSearch<'_, '_> {
    fn entry_at(&self, offset: usize) -> Option<FeatureInputComponentPathEntry> {
        let instance = self.payload.get(offset..offset + 4)?;
        let signature: [u8; 12] = self.payload.get(offset + 4..offset + 16)?.try_into().ok()?;
        (View::u16_le_at(instance, 0).is_some_and(is_class_token)
            && instance[2..4] == [0, 0]
            && signature[0..2] != [0, 0])
        .then(|| FeatureInputComponentPathEntry {
            instance: View::u16_le_at(instance, 0),
            type_signature: signature,
            local_id: None,
        })
    }

    fn terminal_at(&self, offset: usize) -> bool {
        self.payload.get(offset..offset + 8) == Some(&[0xff, 0xff, 0xff, 0xff, 0, 0, 0, 0])
            || [20usize, 24].into_iter().any(|zero_count| {
                self.payload
                    .get(offset..offset + zero_count)
                    .is_some_and(|bytes| bytes.iter().all(|byte| *byte == 0))
                    && View::u32_le_at(self.payload, offset + zero_count)
                        .is_some_and(|source| source != 0)
            })
    }

    fn visit(
        &mut self,
        cursor: usize,
        remaining: usize,
        has_path_slots: bool,
    ) -> Result<(), cadmpeg_core::CodecError> {
        const OPERATION: &str = "decode SLDPRT legacy face component path";
        if self.ambiguous {
            return Ok(());
        }
        let ctx = self.ctx;
        let _depth = ctx.enter_nested(OPERATION)?;
        ctx.charge_work(256, OPERATION)?;
        if remaining == 0 {
            if !self.terminal_at(cursor) {
                return Ok(());
            }
            if let Some(complete) = &self.complete {
                ctx.charge_work(
                    u64_from_index(self.entries.len())
                        .checked_mul(4)
                        .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?,
                    OPERATION,
                )?;
                if complete != &self.entries {
                    self.ambiguous = true;
                }
            } else {
                let mut complete = Vec::new();
                ctx.reserve_collection_vec(&mut complete, self.entries.len(), OPERATION)?;
                complete.extend_from_slice(&self.entries);
                self.complete = Some(complete);
            }
            return Ok(());
        }
        let Some(entry) = self.entry_at(cursor) else {
            return Ok(());
        };
        for with_local_id in [true, false] {
            let mut entry = entry.clone();
            let end = if with_local_id {
                let Some(bytes) = self.payload.get(cursor + 16..cursor + 20) else {
                    continue;
                };
                entry.local_id = View::u32_le_at(bytes, 0);
                cursor + 20
            } else {
                cursor + 16
            };
            let entry_count = self.entries.len();
            ctx.reserve_collection_vec(&mut self.entries, 1, OPERATION)?;
            self.entries.push(entry);
            for slot_bytes in [0usize, 4] {
                if slot_bytes == 4
                    && (!has_path_slots
                        || !View::u32_le_at(self.payload, end)
                            .is_some_and(|slot| (1..=u32::from(u16::MAX)).contains(&slot)))
                {
                    continue;
                }
                for gap in [0usize, 2, 4, 6, 8] {
                    let next = end + slot_bytes;
                    if self
                        .payload
                        .get(next..next + gap)
                        .is_some_and(|bytes| bytes.iter().all(|byte| *byte == 0))
                    {
                        self.visit(next + gap, remaining - 1, has_path_slots)?;
                    }
                }
            }
            self.entries.truncate(entry_count);
        }
        Ok(())
    }
}

fn legacy_single_face_reference_path_at(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    body: usize,
) -> Result<Option<Vec<FeatureInputComponentPathEntry>>, cadmpeg_core::CodecError> {
    ctx.charge_work(32, "decode SLDPRT legacy face header")?;
    let header_valid = (|| {
        let header = payload.get(body..body + 19)?;
        let class_token = View::u16_le_at(header, 0)?;
        let component_token = View::u16_le_at(header, 2)?;
        let owner = View::u32_le_at(header, 11)?;
        if !is_class_token(class_token)
            || !is_class_token(component_token)
            || header[4..8] != 2u32.to_le_bytes()
            || !matches!(header[8], 0 | 0x40)
            || header[9..11] != [0, 0]
            || owner == 0
            || header[15..19] != owner.to_le_bytes()
        {
            return None;
        }
        Some(())
    })();
    if header_valid.is_none() {
        return Ok(None);
    }
    let mut search = LegacyFacePathSearch {
        ctx,
        payload,
        entries: Vec::new(),
        complete: None,
        ambiguous: false,
    };
    for control in [body + 44, body + 48, body + 84, body + 88] {
        let Some(prefix) = payload.get(control..control + 40) else {
            continue;
        };
        let Some(filler) = payload.get(body + 19..control) else {
            continue;
        };
        ctx.charge_work(8192, "decode SLDPRT legacy face path controls")?;
        let padded = filler.len() >= 16
            && filler.windows(16).enumerate().any(|(start, window)| {
                window.iter().all(|byte| *byte == 0xff)
                    && filler[..start].iter().all(|byte| *byte == 0)
                    && filler[start + 16..].iter().all(|byte| *byte == 0)
            });
        if !filler.iter().all(|byte| *byte == 0) && !padded {
            continue;
        }
        let Some(token) = View::u16_le_at(prefix, 0) else {
            return Ok(None);
        };
        let Some(count) = View::u32_le_at(prefix, 10).and_then(|count| usize::try_from(count).ok())
        else {
            return Ok(None);
        };
        if !is_class_token(token)
            || prefix[2..6] != 1u32.to_le_bytes()
            || prefix[6..10] != [0; 4]
            || !(1..=64).contains(&count)
            || !is_component_vector_selector(&prefix[14..18])
            || prefix[22..30] != prefix[30..38]
            || prefix[38..40] != [0, 0]
        {
            continue;
        }
        for serialized_roots in [0usize, 2] {
            let Some(entry_count) = count
                .checked_sub(serialized_roots)
                .filter(|count| *count > 0)
            else {
                continue;
            };
            search.visit(control + 40, entry_count, prefix[15] == 3)?;
        }
    }
    Ok(if search.ambiguous {
        None
    } else {
        search.complete
    })
}

pub(super) fn compact_single_face_reference_record_at(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    marker: usize,
) -> Result<Option<super::selections::ComponentPathReference>, cadmpeg_core::CodecError> {
    ctx.charge_work(32, "decode SLDPRT single face record")?;
    let count = (|| {
        let count = marker
            .checked_sub(12)
            .and_then(|offset| View::u32_le_at(payload, offset))?;
        let count = usize::try_from(count)
            .ok()
            .filter(|count| (1..=64).contains(count))?;
        if payload.get(marker..marker + 16) != Some(COMPACT_EDGE_VECTOR_MARKER.as_slice())
            || !payload
                .get(marker - 8..marker - 4)
                .is_some_and(is_component_vector_selector)
            || payload.get(marker + 16..marker + 18) != Some(&[0, 0])
        {
            return None;
        }
        Some(count)
    })();
    let Some(count) = count else {
        return Ok(None);
    };
    if let Some((components, _)) = compact_heterogeneous_component_path(
        ctx,
        payload,
        marker + 18,
        count,
        "decode SLDPRT component path layout",
    )? {
        return Ok(Some(super::selections::ComponentPathReference(
            components, None,
        )));
    }
    for serialized_roots in [1usize, 2] {
        let Some(entry_count) = count.checked_sub(serialized_roots) else {
            continue;
        };
        let Some((components, end)) = compact_heterogeneous_component_path(
            ctx,
            payload,
            marker + 18,
            entry_count,
            "decode SLDPRT component path layout",
        )?
        else {
            continue;
        };
        ctx.charge_work(128, "decode SLDPRT single face terminal")?;
        let source = [0usize, 4, 8].into_iter().find_map(|gap| {
            let filler = match gap {
                0 => true,
                4 => payload.get(end..end + 4) == Some(&[0; 4]),
                8 => matches!(
                    payload.get(end..end + 8),
                    Some(
                        [0, 0, 0, 0, 0, 0, 0, 0]
                            | [0xff, 0xff, 0xff, 0xff, 0, 0, 0, 0]
                            | [0xa0, 0x86, 0x01, 0x00, 0, 0, 0, 0]
                    )
                ),
                _ => false,
            };
            if !filler {
                return None;
            }
            let terminal = end + gap;
            if payload.get(terminal..terminal + 8) == Some(&[0xff, 0xff, 0xff, 0xff, 0, 0, 0, 0]) {
                return Some(None);
            }
            let source = View::u32_le_at(payload, terminal + 20)?;
            (payload.get(terminal..terminal + 20)? == [0; 20] && source != 0)
                .then_some(Some(source))
        });
        if let Some(source) = source {
            return Ok(Some(super::selections::ComponentPathReference(
                components, source,
            )));
        }
    }
    Ok(None)
}

/// Decode the component path of an up-to-vertex or offset-from-face
/// termination reference. These vectors share the single-face-reference
/// grammar and may additionally carry a leading identifier-less component
/// cell, `a0 86 01 00` filler words, or an `01 00 00 00` slot word between
/// counted entries.
pub(super) fn compact_termination_reference_path_at(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    marker: usize,
) -> Result<Option<Vec<FeatureInputComponentPathEntry>>, cadmpeg_core::CodecError> {
    const OPERATION: &str = "decode SLDPRT termination reference path";
    if let Some(components) = compact_single_face_reference_path_at(ctx, payload, marker)? {
        return Ok(Some(components));
    }
    let count = compact_termination_reference_frame_at(payload, marker)
        .and_then(|count| usize::try_from(count).ok());
    let Some(count) = count else {
        return Ok(None);
    };
    let entry_at = |offset: usize| -> Option<FeatureInputComponentPathEntry> {
        let instance = payload.get(offset..offset + 4)?;
        if instance[0..2] == [0, 0]
            || instance[0..2] == [0xff, 0xff]
            || instance[2..4] != [0, 0]
            || payload.get(offset + 4..offset + 6)? == [0, 0]
        {
            return None;
        }
        Some(FeatureInputComponentPathEntry {
            instance: Some(View::u16_le_at(instance, 0)?),
            type_signature: payload.get(offset + 4..offset + 16)?.try_into().ok()?,
            local_id: Some(View::u32_le_at(payload, offset + 16)?),
        })
    };
    let mut cursor = marker + 18;
    // A leading identifier-less cell repeats the first counted entry's
    // signature immediately after its own.
    if entry_at(cursor).is_some()
        && entry_at(cursor + 16).is_some()
        && payload.get(cursor + 20..cursor + 32) == payload.get(cursor + 4..cursor + 16)
    {
        cursor += 16;
    }
    let mut entries = Vec::new();
    while entries.len() < count {
        ctx.charge_work(128, OPERATION)?;
        let ordinal_gap = payload
            .get(cursor..cursor + 4)
            .and_then(|bytes| {
                let ordinal = View::u16_le_at(bytes, 0)?;
                (ordinal != 0 && ordinal & 0x8000 == 0 && bytes[2..4] == [0, 0]).then_some(ordinal)
            })
            .is_some()
            && entry_at(cursor + 4).is_some();
        if ordinal_gap {
            cursor += 4;
            continue;
        }
        if let Some(entry) = entry_at(cursor) {
            ctx.reserve_collection_vec(&mut entries, 1, OPERATION)?;
            entries.push(entry);
            cursor += 20;
            continue;
        }
        let gap = [4usize, 8].into_iter().find(|gap| {
            let filler_ok = match gap {
                4 => matches!(
                    payload.get(cursor..cursor + 4),
                    Some([0, 0, 0, 0] | [0xa0, 0x86, 0x01, 0x00])
                ),
                8 => matches!(
                    payload.get(cursor..cursor + 8),
                    Some(
                        [0, 0, 0, 0, 0, 0, 0, 0]
                            | [0xff, 0xff, 0xff, 0xff, 0, 0, 0, 0]
                            | [0xa0, 0x86, 0x01, 0x00, 0, 0, 0, 0]
                            | [0x01, 0x00, 0x00, 0x00, 0, 0, 0, 0]
                    )
                ),
                _ => false,
            };
            filler_ok && entry_at(cursor + gap).is_some()
        });
        match gap {
            Some(gap) => cursor += gap,
            None => break,
        }
    }
    Ok((!entries.is_empty()).then_some(entries))
}

fn compact_termination_reference_frame_at(payload: &[u8], marker: usize) -> Option<u32> {
    let count = marker
        .checked_sub(12)
        .and_then(|offset| View::u32_le_at(payload, offset))?;
    if !(1..=64).contains(&count)
        || payload.get(marker..marker + 16) != Some(COMPACT_EDGE_VECTOR_MARKER.as_slice())
        || !payload
            .get(marker - 8..marker - 4)
            .is_some_and(is_component_vector_selector)
        || payload.get(marker + 16..marker + 18) != Some(&[0, 0])
    {
        return None;
    }
    Some(count)
}

pub(crate) fn compact_surface_selection_value(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    components: &[FeatureInputComponentPathEntry],
) -> Result<String, cadmpeg_core::CodecError> {
    use std::fmt::Write;

    const OPERATION: &str = "format SLDPRT surface component selection";
    const PREFIX: &str = "sldprt:feature-input:surface-component-ids:";
    let mut size = PREFIX.len();
    for (index, component) in components.iter().enumerate() {
        ctx.charge_work(1, OPERATION)?;
        let digits = match component.local_id {
            Some(0) | None => 1,
            Some(local_id) => usize::try_from(local_id.ilog10())
                .ok()
                .and_then(|log| log.checked_add(1))
                .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?,
        };
        size = size
            .checked_add(usize::from(index != 0))
            .and_then(|value| value.checked_add(digits))
            .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
    }
    ctx.charge_work(u64_from_index(size), OPERATION)?;
    let mut value = String::new();
    crate::text_admission::reserve_retained_string(ctx, &mut value, size, OPERATION)?;
    value.push_str(PREFIX);
    for (index, component) in components.iter().enumerate() {
        if index != 0 {
            value.push(',');
        }
        match component.local_id {
            Some(local_id) => write!(value, "{local_id}").map_err(|_| {
                cadmpeg_core::CodecError::malformed("cannot format SLDPRT surface component")
            })?,
            None => value.push('_'),
        }
    }
    Ok(value)
}

#[cfg(test)]
mod terminations_tests;
