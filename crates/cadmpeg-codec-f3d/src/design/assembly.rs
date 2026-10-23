// SPDX-License-Identifier: Apache-2.0
//! Project exact Design assembly alignments into neutral joints.

use cadmpeg_core::decode::u64_from_index;

use std::collections::BTreeMap;

use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureOperation};
use cadmpeg_ir::products::{
    AssemblyJoint, ExternalDocument, JointConnector, JointLimits, JointOperand, PairedJointKind,
};
use cadmpeg_ir::report::loss::LossNote;

use crate::design::components::ComponentInserts;
use crate::ids::native_stream;
use crate::loss::F3dLossCode;
use crate::records::feature::{
    assembly::{
        DesignAssemblyAxialOperandTarget, DesignAssemblyLimitKind, DesignAssemblyOperandQualifier,
    },
    assembly_features::DesignComponentOccurrence,
    scope::DesignParameterScope,
};

/// One exact generation of the legacy 421-byte `As-built` alignment grammar.
///
/// The scope class pair is the admission key. All other fields are part of
/// that key's grammar and must not be inferred from a neighboring generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LegacyAsBuilt421Generation {
    Class364,
    Class420,
    Class417,
    Class457,
}

/// Operand-frame form for a non-axial `Assemble` scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AssemblyOperandFrameVariant {
    Standard,
    Compact,
    Axial,
    LegacyClass388,
    /// Operand references 18 bytes before their transforms, each transform
    /// followed by an LP-UTF16 GUID (the 945-byte `Assemble` scope).
    Wide,
    /// The wide operand form five bytes earlier (the 806-, 850- and 861-byte
    /// `Assemble` scopes).
    CompactWide,
}

impl AssemblyOperandFrameVariant {
    /// Whether the scope lists its owners first and uses the wide
    /// operand-path locator.
    pub(crate) fn is_wide_family(self) -> bool {
        matches!(self, Self::Wide | Self::CompactWide)
    }
}

/// Whether a scope belongs to the variable-reference `Assemble` generation.
///
/// This generation keeps the standard operand and locator prefix while its
/// owner and reference trailers grow with additional placement and limit
/// groups. The class pair, rather than the total frame length, admits it.
pub(crate) fn variable_reference_assembly_generation(
    class_tag: &str,
    paired_class_tag: &str,
) -> bool {
    matches!(
        (class_tag, paired_class_tag),
        ("283", "264") | ("347", "260")
    )
}

/// The operand, owner-lane, and locator layout of one assembly scope.
#[derive(Debug, Clone, Copy)]
pub(crate) struct AssemblyScopeGeneration {
    operand_frame: Option<AssemblyOperandFrameVariant>,
    alignment: AssemblyAlignmentLanes,
    locator_offsets: Option<[usize; 2]>,
}

#[derive(Debug, Clone, Copy)]
enum AssemblyAlignmentLanes {
    Fixed(Option<(usize, usize, usize)>),
    Variable(Option<(usize, usize, usize)>),
}

impl AssemblyScopeGeneration {
    /// Classify the scope's three coupled layout projections.
    pub(crate) fn new(frame_length: u64, class_tag: &str, paired_class_tag: &str) -> Self {
        use AssemblyOperandFrameVariant::{
            Axial, Compact, CompactWide, LegacyClass388, Standard, Wide,
        };
        let (operand_frame, lanes, locator_offsets) = match frame_length {
            399 => (None, Some((4, 0, 4)), Some([51, 62])),
            604 => (None, Some((8, 4, 8)), None),
            627 | 637 | 692 => (Some(Standard), Some((4, 0, 4)), Some([366, 377])),
            633 => (Some(Compact), Some((4, 0, 4)), Some([362, 373])),
            671 => ((class_tag == "406" && paired_class_tag == "261").then_some(Standard), Some((6, 4, 6)), Some([
                crate::layout::assembly_class_406_261_scope_671::FIRST_LOCATOR_REFERENCE,
                crate::layout::assembly_class_406_261_scope_671::SECOND_LOCATOR_REFERENCE,
            ])),
            705 => (Some(Axial), Some((6, 4, 6)), None),
            732 => (Some(Compact), Some((8, 4, 8)), Some([362, 373])),
            744 => ((class_tag == "430" && paired_class_tag == "262").then_some(Compact), Some((8, 4, 8)), Some([362, 373])),
            748 => ((class_tag == "430" && paired_class_tag == "262").then_some(Standard), Some((8, 4, 8)), Some([366, 377])),
            772 => (Some(Axial), Some((10, 8, 10)), None),
            806 | 861 => (Some(CompactWide), Some((4, 0, 4)), Some([534, 545])),
            850 => (Some(CompactWide), Some((6, 0, 4)), Some([556, 567])),
            945 => (Some(Wide), Some((4, 0, 4)), Some([541, 552])),
            269 => {
                (None, Some((0, 0, 0)), Some([
                    crate::layout::assembly_rigid_as_built_scope::FIRST_LOCATOR_REFERENCE,
                    crate::layout::assembly_rigid_as_built_scope::SECOND_LOCATOR_REFERENCE,
                ]))
            }
            length if length == u64_from_index(crate::layout::assembly_class_388_266_scope_968::LEN) => (
                (class_tag == "388" && paired_class_tag == "266").then_some(LegacyClass388), Some((28, 4, 8)), Some([
                    crate::layout::assembly_class_388_266_scope_968::OPERAND_PATH_LOCATOR_REFERENCES,
                    crate::layout::assembly_class_388_266_scope_968::OPERAND_PATH_LOCATOR_REFERENCES + 11,
                ])),
            length if length == u64_from_index(crate::layout::assembly_class_383_258_scope_1011::LEN) => (
                (class_tag == "383" && paired_class_tag == "258").then_some(Standard), Some((20, 8, 12)), None),
            // A rigid group with more than two members keeps the 269-byte
            // prefix; its first two locators lead the run.
            length if rigid_group_member_count(length).is_some() => (None, Some((0, 0, 0)), Some([
                crate::layout::assembly_rigid_as_built_scope::FIRST_LOCATOR_REFERENCE,
                crate::layout::assembly_rigid_as_built_scope::SECOND_LOCATOR_REFERENCE,
            ])),
            _ => (None, None, None),
        };
        if variable_reference_assembly_generation(class_tag, paired_class_tag) {
            Self {
                operand_frame: Some(Standard),
                alignment: AssemblyAlignmentLanes::Variable(lanes),
                locator_offsets: Some([366, 377]),
            }
        } else {
            Self {
                operand_frame,
                alignment: AssemblyAlignmentLanes::Fixed(lanes),
                locator_offsets,
            }
        }
    }

    /// Operand-frame grammar admitted by the scope classes.
    pub(crate) fn operand_frame_variant(self) -> Option<AssemblyOperandFrameVariant> {
        self.operand_frame
    }

    /// Half-open alignment range for the supplied owner count.
    pub(crate) fn alignment_lane_bounds(self, owner_count: usize) -> Option<(usize, usize)> {
        let fallback = match self.alignment {
            AssemblyAlignmentLanes::Variable(_)
                if owner_count >= 12 && matches!((owner_count - 12) % 4, 0 | 2) =>
            {
                return Some((8, 12))
            }
            AssemblyAlignmentLanes::Fixed(fallback)
            | AssemblyAlignmentLanes::Variable(fallback) => fallback,
        };
        let (count, start, end) = fallback?;
        (owner_count == count).then_some((start, end))
    }

    /// Marker offsets of the two ordered operand-path locators.
    pub(crate) fn operand_path_locator_offsets(self) -> Option<[usize; 2]> {
        self.locator_offsets
    }
}

/// Inserted-design occurrences that store the identity while a joint places
/// them. A joint names an occurrence directly, or names an inserted design
/// through an external operand container, which the neutral model cannot tie
/// to one occurrence: each distinct external document then accounts for at
/// most one identity occurrence. Their pose is the one their joints define,
/// which is not solved.
pub(crate) fn joint_placed_identity_occurrences<'a>(
    occurrences: &[cadmpeg_ir::products::Occurrence],
    containers: impl IntoIterator<Item = &'a cadmpeg_ir::OperandContainer>,
) -> usize {
    let mut named = std::collections::HashSet::new();
    let mut documents = Vec::new();
    for container in containers {
        match container {
            cadmpeg_ir::OperandContainer::Occurrence { occurrence, .. } => {
                named.insert(occurrence);
            }
            cadmpeg_ir::OperandContainer::External { external_document } => {
                if !documents.contains(&external_document) {
                    documents.push(external_document);
                }
            }
            cadmpeg_ir::OperandContainer::Root {} => {}
        }
    }
    let (direct, unnamed) = occurrences
        .iter()
        .filter(|occurrence| {
            matches!(
                occurrence.prototype,
                cadmpeg_ir::products::PrototypeReference::External { .. }
            ) && occurrence.transform.rows() == cadmpeg_ir::transform::Transform::identity().rows()
        })
        .fold((0_usize, 0_usize), |(direct, unnamed), occurrence| {
            if named.contains(&occurrence.id) {
                (direct + 1, unnamed)
            } else {
                (direct, unnamed + 1)
            }
        });
    direct + unnamed.min(documents.len())
}

/// The loss note for joint-placed identity occurrences, when there are any.
pub(crate) fn joint_placed_occurrence_loss(
    joint_placed: usize,
) -> Option<cadmpeg_ir::report::loss::LossNote> {
    (joint_placed != 0).then(|| {
        F3dLossCode::JointPlacedOccurrenceUnsolved.note(format!(
            "{joint_placed} inserted-design occurrence(s) store the identity and are placed by their joints; the joint-solved pose is not computed, so they are placed at the identity."
        ))
    })
}

/// The rigid `As-built` scope of a rigid group: no parameter owners and one
/// wide operand-path locator per member. The 269-byte frame has two members;
/// each further member adds one 11-byte locator reference. Its class tags are
/// per-file dynamic values, so its kind and frame length identify it.
pub(crate) fn rigid_as_built_scope(frame_length: u64, as_built: bool) -> bool {
    as_built && rigid_group_member_count(frame_length).is_some()
}

/// The member count a rigid `As-built` frame length implies.
pub(crate) fn rigid_group_member_count(frame_length: u64) -> Option<usize> {
    let extra = frame_length.checked_sub(269)?;
    if extra % 11 != 0 {
        return None;
    }
    usize::try_from(extra / 11).ok()?.checked_add(2)
}

/// Admit the legacy 383/258 assembly scope only as its exact generation.
pub(super) fn legacy_class_383_258_scope(
    frame_length: u64,
    class_tag: &str,
    paired_class_tag: &str,
) -> bool {
    frame_length == u64_from_index(crate::layout::assembly_class_383_258_scope_1011::LEN)
        && class_tag == "383"
        && paired_class_tag == "258"
}

impl LegacyAsBuilt421Generation {
    /// Owner-frame primary class for the six scalar lanes.
    pub(super) const fn owner_class_tag(self) -> &'static str {
        match self {
            Self::Class364 => "293",
            Self::Class420 => "378",
            Self::Class417 => "318",
            Self::Class457 => "418",
        }
    }

    /// Owner-frame paired class.
    const fn owner_paired_class_tag(self) -> &'static str {
        match self {
            Self::Class364 => "272",
            Self::Class420 => "262",
            Self::Class417 => "263",
            Self::Class457 => "258",
        }
    }

    /// Solved connector-frame primary class named by reference-table entry 8.
    pub(crate) const fn frame_class_tag(self) -> &'static str {
        match self {
            Self::Class364 => "376",
            Self::Class420 => "327",
            Self::Class417 => "448",
            Self::Class457 => "297",
        }
    }

    /// Solved connector-frame paired class.
    pub(super) const fn frame_paired_class_tag(self) -> &'static str {
        self.owner_paired_class_tag()
    }

    /// Byte length from the solved frame primary header to its paired header.
    pub(super) const fn frame_length(self) -> usize {
        match self {
            Self::Class364 => 389,
            Self::Class420 | Self::Class417 => 390,
            Self::Class457 => 385,
        }
    }

    /// Offset of the four-byte marker immediately before the solved matrix.
    pub(super) const fn matrix_prefix(self) -> usize {
        match self {
            Self::Class420 | Self::Class417 => 46,
            Self::Class364 | Self::Class457 => 45,
        }
    }

    /// Offset of the first f64 in the solved row-major matrix.
    pub(crate) const fn matrix_offset(self) -> usize {
        match self {
            Self::Class420 | Self::Class417 => 50,
            Self::Class364 | Self::Class457 => 49,
        }
    }

    /// Domain of the two limit lanes.
    pub(crate) const fn limit_kind(self) -> DesignAssemblyLimitKind {
        match self {
            Self::Class364 => DesignAssemblyLimitKind::Angular,
            Self::Class420 | Self::Class417 | Self::Class457 => DesignAssemblyLimitKind::Linear,
        }
    }

    /// Whether source limit lanes are stored maximum then minimum.
    pub(crate) const fn reverse_limit_order(self) -> bool {
        matches!(self, Self::Class420 | Self::Class417)
    }
}

/// Admit one exact 421-byte `As-built` scope generation.
pub(crate) fn legacy_as_built_421_generation(
    frame_length: u64,
    class_tag: &str,
    paired_class_tag: &str,
) -> Option<LegacyAsBuilt421Generation> {
    if frame_length != 421 {
        return None;
    }
    match (class_tag, paired_class_tag) {
        ("364", "272") => Some(LegacyAsBuilt421Generation::Class364),
        ("420", "262") => Some(LegacyAsBuilt421Generation::Class420),
        ("417", "263") => Some(LegacyAsBuilt421Generation::Class417),
        ("457", "258") => Some(LegacyAsBuilt421Generation::Class457),
        _ => None,
    }
}

/// Assembly joints and the losses of the joint scopes that do not project.
#[derive(Debug, Default)]
pub(crate) struct AssemblyJointProjection {
    pub(crate) joints: Vec<AssemblyJoint>,
    pub(crate) losses: Vec<LossNote>,
}

/// Project assembly scopes whose connector frames and operand qualifiers are complete.
///
/// Each connector frame is in its operand's own space: the prototype of an
/// occurrence operand, the external document of an external operand, or the
/// document root. It is not composed with the occurrence placement.
///
/// Joints are product structure. They read scopes, local occurrence carriers,
/// and the `Component Insert` occurrences, and no features. A joint scope
/// whose operands, frames, limits, or alignment values do not project is
/// skipped and counted in one loss.
pub(crate) fn project_assembly_joints(
    scopes: &[DesignParameterScope],
    native_occurrences: &[DesignComponentOccurrence],
    inserts: &ComponentInserts,
    motions: &[crate::records::feature::joint_motion::DesignJointMotion],
) -> AssemblyJointProjection {
    let mut occurrences = BTreeMap::new();
    for occurrence in native_occurrences {
        let Some(stream) = native_stream(&occurrence.id) else {
            continue;
        };
        let key = (
            stream,
            occurrence.occurrence_guid.as_str().to_ascii_lowercase(),
        );
        match occurrences.entry(key) {
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(Some(occurrence));
            }
            std::collections::btree_map::Entry::Occupied(mut entry) => {
                *entry.get_mut() = None;
            }
        }
    }
    let mut joints = BTreeMap::<String, AssemblyJoint>::new();
    let mut unprojected = 0_usize;
    let mut undecoded_motions = std::collections::BTreeSet::new();
    let mut undecoded_motion_count = 0_usize;
    let mut paired_joints = 0_usize;
    let mut unprojected_members = 0_usize;
    for scope in scopes {
        let Some(stream) = native_stream(&scope.id) else {
            continue;
        };
        let motion = scope_joint_motion(scope, stream, motions);
        match project_assembly_joint(scope, stream, &occurrences, scopes, inserts, motion) {
            Ok(Some((mut joint, undecoded))) => {
                let paired = scope
                    .assembly_alignment()
                    .and_then(|alignment| alignment.operand_path_refs())
                    .is_some_and(|paths| {
                        paths
                            .iter()
                            .any(|path| !path.paired_occurrence_keys().is_empty())
                    });
                if paired {
                    paired_joints += 1;
                }
                // Every projected `Assemble` joint is paired.
                joint.set_frame_sense(
                    scope
                        .assembly_alignment()
                        .and_then(|alignment| alignment.frame_sense)
                        .map(|sense| match sense {
                            crate::records::feature::assembly::DesignJointFrameSense::Aligned => {
                                cadmpeg_ir::products::PlaneSense::Aligned
                            }
                            crate::records::feature::assembly::DesignJointFrameSense::Opposed => {
                                cadmpeg_ir::products::PlaneSense::Opposed
                            }
                        }),
                );
                if let Some(reason) = undecoded {
                    undecoded_motion_count += 1;
                    undecoded_motions.insert(reason);
                }
                joints.entry(joint.id.as_str().to_owned()).or_insert(joint);
                let members =
                    rigid_group_member_joints(scope, stream, &occurrences, scopes, inserts);
                paired_joints += members.paired;
                unprojected_members += members.unprojected;
                for member in members.joints {
                    joints
                        .entry(member.id.as_str().to_owned())
                        .or_insert(member);
                }
            }
            Ok(None) => {}
            Err(_) => unprojected += 1,
        }
    }
    let mut losses = Vec::new();
    if undecoded_motion_count != 0 {
        let reasons = undecoded_motions.into_iter().collect::<Vec<_>>().join("; ");
        losses.push(F3dLossCode::AssemblyJointMotionUndecoded.note(format!(
            "{undecoded_motion_count} Design assembly joint(s) were projected as fixed: their motion does not map to a joint kind ({reasons})."
        )));
    }
    if paired_joints != 0 {
        losses.push(F3dLossCode::AssemblyOperandPathPairsNative.note(format!(
            "{paired_joints} projected Design assembly joint(s) have operand paths with occurrence identity pairs whose effect on the joint is not known; the pairs are retained natively."
        )));
    }
    if unprojected != 0 {
        losses.push(F3dLossCode::AssemblyJointUnprojected.note(format!(
            "{unprojected} Design assembly joint scope(s) were retained natively: their operands, connector frames, limits, or alignment values do not project."
        )));
    }
    if unprojected_members != 0 {
        losses.push(F3dLossCode::AssemblyJointUnprojected.note(format!(
            "{unprojected_members} rigid group member(s) after their group's first two were retained natively: their operands or connector frames do not project. Each group's joint between its first two members projects."
        )));
    }
    AssemblyJointProjection {
        joints: joints.into_values().collect(),
        losses,
    }
}

/// The decoded motion of an `Assemble` scope's joint: the joint record's
/// index is one less than the scope's.
fn scope_joint_motion<'a>(
    scope: &DesignParameterScope,
    stream: &str,
    motions: &'a [crate::records::feature::joint_motion::DesignJointMotion],
) -> Option<&'a crate::records::feature::joint_motion::DesignJointMotion> {
    let mut matching = motions.iter().filter(|motion| {
        native_stream(&motion.id) == Some(stream)
            && motion.joint_record_index.checked_add(1) == Some(scope.record_index)
    });
    let motion = matching.next()?;
    matching.next().is_none().then_some(motion)
}

/// The joint kind a decoded motion selects.
enum JointMotionKind {
    /// Rotation about the connector z axis; `turn_y_to_z` when the freedom is
    /// about the joint's local y axis.
    Revolute {
        turn_y_to_z: bool,
        limits: Option<JointLimits>,
    },
    /// Translation along the connector z axis.
    Slider,
}

/// The kind a motion's freedoms and limit lanes select, or the reason they
/// select none.
fn joint_motion_kind(
    motion: &crate::records::feature::joint_motion::DesignJointMotion,
) -> Result<JointMotionKind, String> {
    let codes = motion
        .freedoms
        .iter()
        .map(|freedom| freedom.value)
        .collect::<Vec<_>>();
    let limited = match motion
        .limits
        .as_ref()
        .map(|lanes| (lanes.first_flag, lanes.second_flag))
    {
        None | Some((0, 0)) => false,
        Some((1, 1)) => true,
        Some((first, second)) => return Err(format!("limit flags {first} {second}")),
    };
    match codes.as_slice() {
        [code @ (1 | 2)] => {
            let limits = match motion.limits.as_ref().filter(|_| limited) {
                Some(lanes) => Some(
                    JointLimits::new(Some(lanes.minimum), Some(lanes.maximum))
                        .ok_or_else(|| "rotation limits are not ordered".to_owned())?,
                ),
                None => None,
            };
            Ok(JointMotionKind::Revolute {
                turn_y_to_z: *code == 1,
                limits,
            })
        }
        [8] if !limited => Ok(JointMotionKind::Slider),
        [8] => Err("translation limits".to_owned()),
        codes => Err(format!("freedom codes {codes:?}")),
    }
}

/// Turn a connector frame so that its local y axis becomes its z axis: the
/// new axes are (x, -z, y).
fn turn_frame_y_to_z(mut rows: [[f64; 4]; 4]) -> [[f64; 4]; 4] {
    for row in &mut rows {
        let (y, z) = (row[1], row[2]);
        row[1] = -z;
        row[2] = y;
    }
    rows
}

fn project_assembly_joint(
    scope: &DesignParameterScope,
    stream: &str,
    occurrences: &BTreeMap<(&str, String), Option<&DesignComponentOccurrence>>,
    scopes: &[DesignParameterScope],
    inserts: &ComponentInserts,
    motion: Option<&crate::records::feature::joint_motion::DesignJointMotion>,
) -> Result<Option<(AssemblyJoint, Option<String>)>, CodecError> {
    let Some(alignment) = scope.assembly_alignment() else {
        return Ok(None);
    };
    // A rigid group's joint record has a motion record with no freedom entry.
    if rigid_as_built_scope(
        scope.frame_length(),
        scope.kind() == crate::records::feature::scope::DesignFeatureKind::AsBuilt,
    ) && motion.is_some_and(|motion| !motion.freedoms.is_empty())
    {
        return Err(CodecError::NotImplemented(
            "rigid group joint record states a freedom".into(),
        ));
    }
    let (frames, operands, limits) = match alignment.form.as_ref() {
        Some(
            crate::records::feature::assembly::DesignAssemblyAlignmentForm::LegacyAsBuilt421 {
                carriers,
                solved_frame,
                limits,
                ..
            },
        ) => (
            carriers
                .frames(solved_frame)
                .map_err(cadmpeg_core::CodecError::NotImplemented)?,
            {
                let [first, second] = carriers.selections().map(|selection| {
                    JointOperand::root(
                        crate::ids::neutral_assembly_legacy_object_id(selection),
                        Vec::new(),
                    )
                });
                [first, second]
            },
            limits.as_ref(),
        ),
        Some(crate::records::feature::assembly::DesignAssemblyAlignmentForm::Qualified(
            operands,
        )) => {
            let projected = project_qualified_operands(
                operands.each_ref().map(|operand| &operand.qualifier),
                stream,
                occurrences,
                scopes,
                inserts,
            )
            .ok_or_else(|| {
                CodecError::NotImplemented(
                    "assembly joint operand names no projected occurrence or origin".into(),
                )
            })?;
            (
                operands.each_ref().map(|operand| operand.frame.clone()),
                projected,
                None,
            )
        }
        // A wide scope always qualifies its operands; frames without
        // qualifiers are an unprojected joint, not an absent one.
        Some(crate::records::feature::assembly::DesignAssemblyAlignmentForm::Frames { .. })
            if AssemblyScopeGeneration::new(
                scope.frame_length(),
                scope.class_tag.as_str(),
                scope.paired_class_tag.as_str(),
            )
            .operand_frame_variant()
            .is_some_and(AssemblyOperandFrameVariant::is_wide_family) =>
        {
            return Err(CodecError::NotImplemented(
                "wide assembly joint operand paths are unresolved".into(),
            ));
        }
        _ => return Ok(None),
    };
    let (angular_limits, linear_limits) = match limits {
        Some(limits) => {
            let projected = JointLimits::new(Some(limits.minimum()), Some(limits.maximum()))
                .ok_or_else(|| {
                    CodecError::Malformed(
                        "joint limits minimum/maximum must be finite and ordered".into(),
                    )
                })?;
            match limits.kind {
                DesignAssemblyLimitKind::Angular => (Some(projected), None),
                DesignAssemblyLimitKind::Linear => (None, Some(projected)),
            }
        }
        None => (None, None),
    };
    let [first_operand, second_operand] = operands;
    let angle = cadmpeg_ir::scalar::FiniteReal::new(alignment.angle())
        .ok_or_else(|| CodecError::Malformed("joint angle must be finite".into()))?;
    let [x, y, z] = alignment.offset().map(|value| {
        cadmpeg_ir::scalar::FiniteReal::new(value * 10.0)
            .ok_or_else(|| CodecError::Malformed("joint translation_offset must be finite".into()))
    });
    let translation_offset = [x?, y?, z?];
    let scope_limits = angular_limits.is_some() || linear_limits.is_some();
    let fixed = PairedJointKind::Fixed {
        angle: Some(angle),
        translation_offset: Some(translation_offset),
        angular_limits,
        linear_limits,
    };
    let (kind, turn_y_to_z, undecoded) = match motion.map(joint_motion_kind) {
        None => (fixed, false, None),
        Some(Err(reason)) => (fixed, false, Some(reason)),
        Some(Ok(_)) if scope_limits => (fixed, false, Some("scope and motion limits".to_owned())),
        Some(Ok(JointMotionKind::Revolute {
            turn_y_to_z,
            limits,
        })) => {
            if alignment.offset().iter().any(|value| *value != 0.0) {
                (
                    fixed,
                    false,
                    Some("revolute with a translation offset".to_owned()),
                )
            } else {
                (
                    PairedJointKind::Revolute {
                        angle: Some(angle),
                        angular_limits: limits,
                    },
                    turn_y_to_z,
                    None,
                )
            }
        }
        Some(Ok(JointMotionKind::Slider)) => {
            if alignment.angle().abs() > 0.0 {
                (fixed, false, Some("slider with an angle offset".to_owned()))
            } else {
                (
                    PairedJointKind::Slider {
                        distance: None,
                        translation_offset: Some(translation_offset),
                        linear_limits: None,
                    },
                    false,
                    None,
                )
            }
        }
    };
    let frame = |rows: [[f64; 4]; 4]| {
        super::components::neutral_transform(if turn_y_to_z {
            turn_frame_y_to_z(rows)
        } else {
            rows
        })
    };
    let first_frame = frame(frames[0].transform.rows())?;
    let second_frame = frame(frames[1].transform.rows())?;
    let mut joint = AssemblyJoint::paired(
        crate::ids::neutral_assembly_joint_id(scope),
        kind,
        [
            JointConnector {
                operand: first_operand,
                frame: first_frame,
                detached: false,
            },
            JointConnector {
                operand: second_operand,
                frame: second_frame,
                detached: false,
            },
        ],
        None,
    );
    joint.native_ref = Some(scope.id.clone());
    Ok(Some((joint, undecoded)))
}

/// The fixed joints of a rigid group's members after its first two, with the
/// number of those joints whose operand paths carry occurrence identity pairs
/// and the number of members that do not project.
#[derive(Default)]
struct RigidGroupMemberJoints {
    joints: Vec<AssemblyJoint>,
    paired: usize,
    unprojected: usize,
}

/// The fixed joints that tie each rigid-group member after the first two to
/// the group's first member, at their as-built relative pose. Every such joint
/// names the group's scope as its native record, as the joint between the
/// first two members does. A member whose operand or frame does not project is
/// counted and skipped; the group's other joints still project.
fn rigid_group_member_joints(
    scope: &DesignParameterScope,
    stream: &str,
    occurrences: &BTreeMap<(&str, String), Option<&DesignComponentOccurrence>>,
    scopes: &[DesignParameterScope],
    inserts: &ComponentInserts,
) -> RigidGroupMemberJoints {
    let mut projected = RigidGroupMemberJoints::default();
    let Some(alignment) = scope.assembly_alignment() else {
        return projected;
    };
    let Some(crate::records::feature::assembly::DesignAssemblyAlignmentForm::Qualified([first, _])) =
        alignment.form.as_ref()
    else {
        return projected;
    };
    let angle = cadmpeg_ir::scalar::FiniteReal::new(alignment.angle());
    let offset = alignment
        .offset()
        .map(|value| cadmpeg_ir::scalar::FiniteReal::new(value * 10.0));
    let first_frame = super::components::neutral_transform(first.frame.transform.rows());
    let (Some(angle), [Some(x), Some(y), Some(z)], Ok(first_frame)) = (angle, offset, first_frame)
    else {
        projected.unprojected = alignment.additional_members.len();
        return projected;
    };
    let first_paired = first
        .qualifier
        .occurrence_path()
        .is_some_and(|path| !path.paired_occurrence_keys().is_empty());
    for (ordinal, member) in alignment.additional_members.iter().enumerate() {
        let operands = project_qualified_operands(
            [&first.qualifier, &member.qualifier],
            stream,
            occurrences,
            scopes,
            inserts,
        );
        let member_frame = super::components::neutral_transform(member.frame.transform.rows());
        let (Some([first_operand, member_operand]), Ok(member_frame)) = (operands, member_frame)
        else {
            projected.unprojected += 1;
            continue;
        };
        let mut joint = AssemblyJoint::paired(
            crate::ids::neutral_assembly_group_joint_id(scope, ordinal + 2),
            PairedJointKind::Fixed {
                angle: Some(angle),
                translation_offset: Some([x, y, z]),
                angular_limits: None,
                linear_limits: None,
            },
            [
                JointConnector {
                    operand: first_operand,
                    frame: first_frame,
                    detached: false,
                },
                JointConnector {
                    operand: member_operand,
                    frame: member_frame,
                    detached: false,
                },
            ],
            None,
        );
        joint.native_ref = Some(scope.id.clone());
        if first_paired
            || member
                .qualifier
                .occurrence_path()
                .is_some_and(|path| !path.paired_occurrence_keys().is_empty())
        {
            projected.paired += 1;
        }
        projected.joints.push(joint);
    }
    projected
}

/// Bind each native `Assemble` feature to the joint that product projection
/// emitted for its scope.
pub(crate) fn bind_assembly_joint_features(features: &mut [Feature], joints: &[AssemblyJoint]) {
    // A rigid group's joints share its scope; the feature binds the first.
    let mut by_scope = BTreeMap::new();
    for joint in joints {
        if let Some(scope) = joint.native_ref.as_deref() {
            by_scope.entry(scope).or_insert(&joint.id);
        }
    }
    let joints = by_scope;
    for feature in features {
        let Some(joint) = feature
            .native_ref
            .as_deref()
            .and_then(|scope| joints.get(scope))
        else {
            continue;
        };
        if !matches!(
            feature.evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::Native { .. })
        ) {
            continue;
        }
        feature
            .evaluation
            .set_definition(FeatureDefinition::Operation(
                FeatureOperation::AssemblyJoint {
                    joint: (*joint).clone(),
                },
            ));
    }
}

fn project_qualified_operands(
    qualifiers: [&DesignAssemblyOperandQualifier; 2],
    stream: &str,
    occurrences: &BTreeMap<(&str, String), Option<&DesignComponentOccurrence>>,
    scopes: &[DesignParameterScope],
    inserts: &ComponentInserts,
) -> Option<[JointOperand; 2]> {
    let project = |qualifier: &DesignAssemblyOperandQualifier| -> Option<JointOperand> {
        match qualifier {
            DesignAssemblyOperandQualifier::OccurrencePath { path } => {
                let root_guid = &path.occurrence_guids().first()?.value;
                let lookup_guid = root_guid.as_str().to_ascii_lowercase();
                let occurrence = occurrences.get(&(stream, lookup_guid)).copied().flatten();
                if occurrence.is_none()
                    && !path.admitted_by_type()
                    && !matches!(path.class_tag().as_str(), "330" | "386")
                {
                    return None;
                }
                let object = root_guid.as_str().to_ascii_lowercase();
                let subelement_guids = &path.occurrence_guids()[1..];

                let mut subelements = Vec::new();
                for guid in subelement_guids {
                    subelements.push(guid.value.as_str().to_ascii_lowercase());
                }
                Some(match occurrence {
                    Some(_) => JointOperand::occurrence(
                        crate::ids::neutral_component_occurrence_id(root_guid),
                        object,
                        subelements,
                    ),
                    None => JointOperand::external(
                        ExternalDocument::document_id(
                            path.identity_guids().first()?.value.as_str().to_owned(),
                        ),
                        object,
                        subelements,
                    ),
                })
            }
            DesignAssemblyOperandQualifier::AxialTarget { target } => match target {
                DesignAssemblyAxialOperandTarget::ComponentInsertOccurrence {
                    component_insert_scope_record_index,
                    selectors,
                    ..
                } => {
                    let target_scope = unique_scope(
                        scopes,
                        stream,
                        *component_insert_scope_record_index,
                        &crate::records::feature::scope::DesignFeatureKind::ComponentInsert,
                    )?;
                    Some(JointOperand::occurrence(
                        inserts.occurrence(&target_scope.id)?.clone(),
                        crate::ids::neutral_assembly_axial_object_id(&selectors[0]),
                        Vec::new(),
                    ))
                }
                DesignAssemblyAxialOperandTarget::DocumentRootJointOrigin {
                    scope_record_index,
                } => project_joint_origin_operand(*scope_record_index, stream, scopes),
            },
            DesignAssemblyOperandQualifier::JointOrigin {
                scope_record_index, ..
            } => project_joint_origin_operand(*scope_record_index, stream, scopes),
        }
    };
    Some([project(qualifiers[0])?, project(qualifiers[1])?])
}

/// The current-document-root operand named by a `JointOrigin` scope whose
/// solved frame projects.
fn project_joint_origin_operand(
    scope_record_index: u32,
    stream: &str,
    scopes: &[DesignParameterScope],
) -> Option<JointOperand> {
    let target_scope = unique_scope(
        scopes,
        stream,
        scope_record_index,
        &crate::records::feature::scope::DesignFeatureKind::JointOrigin,
    )?;
    crate::design::feature_project::joint_origin_frame(target_scope).map(|_| {
        JointOperand::root(
            crate::ids::neutral_feature_id(target_scope).into_string(),
            Vec::new(),
        )
    })
}

fn unique_scope<'a>(
    scopes: &'a [DesignParameterScope],
    stream: &str,
    record_index: u32,
    kind: &crate::records::feature::scope::DesignFeatureKind,
) -> Option<&'a DesignParameterScope> {
    let mut matches = scopes.iter().filter(|scope| {
        native_stream(&scope.id) == Some(stream)
            && scope.record_index == record_index
            && scope.kind() == *kind
    });
    let scope = matches.next()?;
    matches.next().is_none().then_some(scope)
}

#[cfg(test)]
mod tests {
    use cadmpeg_core::decode::u64_from_index;

    use crate::records::feature::assembly::DesignAssemblyOperandQualifier;
    use std::collections::BTreeMap;

    use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureId, FeatureOperation};

    use crate::records::feature::{
        assembly::{
            DesignAssemblyAxialOperandTarget, DesignAssemblyAxialSelectorIdentity,
            DesignAssemblyLimitKind,
        },
        scope::DesignParameterScope,
    };

    fn selector() -> DesignAssemblyAxialSelectorIdentity {
        DesignAssemblyAxialSelectorIdentity {
            axis_record_index: 10,
            axis_class_tag: crate::records::references::DesignClassTag::try_from("316".to_owned())
                .unwrap(),
            axis_byte_offset: 100,
            axis_paired_class_tag: crate::records::references::DesignClassTag::try_from(
                "261".to_owned(),
            )
            .unwrap(),
            axis_paired_byte_offset: 120,
            selector_record_index: 13,
            selector_class_tag: crate::records::references::DesignClassTag::try_from(
                "277".to_owned(),
            )
            .unwrap(),
            selector_byte_offset: 200,
            selector_paired_class_tag: crate::records::references::DesignClassTag::try_from(
                "261".to_owned(),
            )
            .unwrap(),
            selector_paired_byte_offset: 560,
            nested_record_index: 16,
            nested_record_index_offset: 223,
            selector_asset_id: "abcdefab-cdef-4abc-8def-abcdefabcdef"
                .to_owned()
                .try_into()
                .expect("GUID"),
            selector_asset_id_offset: 241,
            selector_context_id: "bcdefabc-defa-4bcd-8efa-bcdefabcdefa"
                .to_owned()
                .try_into()
                .expect("GUID"),
            selector_context_id_offset: 317,
            occurrence_reference: 1_001,
            occurrence_reference_offset: 402,
            external_object_reference: 2_001,
            external_object_reference_offset: 417,
            external_segment: 7,
            external_segment_offset: 426,
            external_asset_id: "abcdefab-cdef-4abc-8def-abcdefabcdef"
                .to_owned()
                .try_into()
                .expect("GUID"),
            external_asset_id_offset: 434,
            external_link_name: "component-link".into(),
            external_link_name_offset: 511,
            external_version: None,
            role_record_index: 18,
            role_class_tag: crate::records::references::DesignClassTag::try_from("298".to_owned())
                .unwrap(),
            role_byte_offset: 600,
            occurrence_role: "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa"
                .to_owned()
                .try_into()
                .expect("GUID"),
            occurrence_role_offset: 629,
        }
    }

    fn second_selector() -> DesignAssemblyAxialSelectorIdentity {
        let mut selector = selector();
        selector.axis_record_index = 30;
        selector.axis_byte_offset = 720;
        selector.axis_paired_byte_offset = 740;
        selector.selector_record_index = 33;
        selector.selector_byte_offset = 760;
        selector.selector_paired_byte_offset = 1_120;
        selector.nested_record_index = 36;
        selector.nested_record_index_offset = 783;
        selector.selector_asset_id_offset = 801;
        selector.selector_context_id_offset = 877;
        selector.occurrence_reference = 1_002;
        selector.occurrence_reference_offset = 962;
        selector.external_object_reference_offset = 977;
        selector.external_segment_offset = 986;
        selector.external_asset_id_offset = 994;
        selector.external_link_name_offset = 1_071;
        selector.role_record_index = 38;
        selector.role_byte_offset = 1_140;
        selector.occurrence_role_offset = 1_169;
        selector
    }

    fn feature(native_ref: &str, definition: FeatureDefinition) -> Feature {
        Feature {
            id: FeatureId::mint(format!(
                "test:model:feature#{}",
                native_ref.replace('#', ":")
            ))
            .expect("identity grammar"),
            ordinal: 0,
            name: None,
            suppressed: Some(false),
            dependencies: cadmpeg_ir::features::DistinctMembers::default(),
            source_properties: BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: cadmpeg_ir::features::FeatureContent::default(),

            evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(definition),
            native_ref: Some(native_ref.into()),
        }
    }

    #[test]
    fn a_joint_frame_translation_overflow_skips_the_joint_with_a_loss() {
        use crate::records::feature::{
            assembly::{
                DesignAssemblyAlignment, DesignAssemblyAlignmentForm, DesignAssemblyOperandFrame,
            },
            scope::{DesignFeatureKind, DesignScopePayload},
        };
        let mut scopes = Vec::new();
        for index in [1, 2] {
            let mut scope = DesignParameterScope::empty(
                &format!("f3d:Design/BulkStream.dat:design-parameter-scope#{index}"),
                DesignFeatureKind::JointOrigin,
                index,
            );
            scope.with_joint_origin_transform(
                crate::records::sketch_placement::SketchPlacementMatrix::IDENTITY,
            );
            scopes.push(scope);
        }
        let mut rows = cadmpeg_ir::transform::Transform::identity().rows();
        rows[0][3] = f64::MAX;
        let frames = [1, 2].map(|index| DesignAssemblyOperandFrame {
            reference_record_index: index,
            reference_offset: 0,
            transform: rows.try_into().unwrap(),
            transform_offset: 0,
        });
        let qualifiers =
            [1, 2].map(
                |scope_record_index| DesignAssemblyOperandQualifier::AxialTarget {
                    target: DesignAssemblyAxialOperandTarget::DocumentRootJointOrigin {
                        scope_record_index,
                    },
                },
            );
        let mut scope = DesignParameterScope::empty(
            "f3d:Design/BulkStream.dat:design-parameter-scope#3",
            DesignFeatureKind::Assemble,
            3,
        );
        scope
            .try_edit(|draft| {
                draft.payload = DesignScopePayload::Assemble(Some(
                    DesignAssemblyAlignment::try_new(
                        0.0,
                        [0.0; 3],
                        Vec::new(),
                        Some(DesignAssemblyAlignmentForm::qualified(frames, qualifiers)),
                    )
                    .unwrap(),
                ));
            })
            .unwrap();
        scopes.push(scope);

        let projection = super::project_assembly_joints(
            &scopes,
            &[],
            &crate::design::components::ComponentInserts::default(),
            &[],
        );
        assert!(projection.joints.is_empty());
        assert_eq!(
            projection
                .losses
                .iter()
                .map(|loss| loss.code.clone())
                .collect::<Vec<_>>(),
            [crate::loss::F3dLossCode::AssemblyJointUnprojected.kind()]
        );

        let native = crate::native::F3dNative {
            design_parameter_scopes: scopes,
            ..Default::default()
        };
        let native = serde_json::from_value(serde_json::to_value(native).unwrap()).unwrap();
        crate::writer::primitives::validate_assembly_projection(
            &cadmpeg_ir::document::CadIr::empty(),
            Some(&native),
        )
        .expect("the writer re-projects the same skipped joint");
    }

    #[test]
    fn a_typed_path_with_no_local_occurrence_projects_an_external_operand() {
        use crate::records::feature::{
            assembly::{
                DesignAssemblyAlignment, DesignAssemblyAlignmentForm, DesignAssemblyOperandFrame,
                DesignAssemblyOperandPath, DesignAssemblyOperandPathLink,
                TYPED_OCCURRENCE_PATH_TYPE_GUID,
            },
            scope::{DesignFeatureKind, DesignScopePayload},
        };
        let guid = |text: &str, offset| crate::records::identity::Located {
            value: text.to_owned().try_into().unwrap(),
            offset,
        };
        let path = |record_index: u32, typed: bool| {
            let link = DesignAssemblyOperandPathLink {
                locator_reference_offset: 0,
                locator_record_index: record_index - 1,
                locator_class_tag: "337".to_owned().try_into().unwrap(),
                locator_byte_offset: 0,
                locator_scope_reference_offset: 0,
                wrapper_record_index: record_index + 1,
                wrapper_reference_offset: 0,
                wrapper_class_tag: "341".to_owned().try_into().unwrap(),
                wrapper_byte_offset: 0,
                path_reference_offset: 0,
            };
            let typed_guid = typed.then(|| {
                TYPED_OCCURRENCE_PATH_TYPE_GUID
                    .to_owned()
                    .try_into()
                    .unwrap()
            });
            DesignAssemblyOperandPath::try_new_typed(
                link,
                record_index,
                "384".to_owned().try_into().unwrap(),
                typed_guid,
                100,
                vec![{
                    let occurrence: crate::records::identity::Located<
                        crate::records::mesh::DesignRelaxedGuidText,
                    > = guid("11111111-1111-1111-1111-11111111111a", 200);
                    crate::records::identity::Located {
                        value: occurrence.value.into(),
                        offset: occurrence.offset,
                    }
                }],
                (0..4)
                    .map(|ordinal| guid("22222222-2222-2222-2222-22222222222b", 300 + ordinal))
                    .collect(),
            )
        };
        let scopes = |typed: bool, paired: bool| {
            let frames = [10, 20].map(|index| DesignAssemblyOperandFrame {
                reference_record_index: index,
                reference_offset: 0,
                transform: cadmpeg_ir::transform::Transform::identity()
                    .rows()
                    .try_into()
                    .unwrap(),
                transform_offset: 0,
            });
            let qualifiers = [11, 21].map(|index| {
                let mut path = path(index, typed).unwrap();
                if paired {
                    path = path
                        .with_paired_occurrence_keys(vec![
                            crate::records::identity::Located {
                                value: "43".to_owned().try_into().unwrap(),
                                offset: 400,
                            },
                            crate::records::identity::Located {
                                value: "33333333-3333-3333-3333-33333333333c"
                                    .to_owned()
                                    .try_into()
                                    .unwrap(),
                                offset: 410,
                            },
                        ])
                        .unwrap();
                }
                DesignAssemblyOperandQualifier::OccurrencePath { path }
            });
            let mut scope = DesignParameterScope::empty(
                "f3d:Design/BulkStream.dat:design-parameter-scope#3",
                DesignFeatureKind::Assemble,
                3,
            );
            scope
                .try_edit(|draft| {
                    draft.payload = DesignScopePayload::Assemble(Some(
                        DesignAssemblyAlignment::try_new(
                            0.0,
                            [0.0; 3],
                            Vec::new(),
                            Some(DesignAssemblyAlignmentForm::qualified(frames, qualifiers)),
                        )
                        .unwrap(),
                    ));
                })
                .unwrap();
            vec![scope]
        };
        let inserts = crate::design::components::ComponentInserts::default();
        let projection = super::project_assembly_joints(&scopes(true, false), &[], &inserts, &[]);
        assert_eq!(projection.joints.len(), 1);
        assert!(projection.losses.is_empty());

        // Occurrence identity pairs keep the joint and say it with a loss.
        let projection = super::project_assembly_joints(&scopes(true, true), &[], &inserts, &[]);
        assert_eq!(projection.joints.len(), 1);
        assert_eq!(
            projection
                .losses
                .iter()
                .map(|loss| loss.code.clone())
                .collect::<Vec<_>>(),
            [crate::loss::F3dLossCode::AssemblyOperandPathPairsNative.kind()]
        );

        // A stored frame sense is the neutral joint's frame sense.
        let mut sensed = scopes(true, false);
        sensed[0]
            .try_edit(|draft| {
                if let DesignScopePayload::Assemble(Some(alignment)) = &mut draft.payload {
                    alignment.frame_sense =
                        Some(crate::records::feature::assembly::DesignJointFrameSense::Opposed);
                }
            })
            .unwrap();
        let projection = super::project_assembly_joints(&sensed, &[], &inserts, &[]);
        assert_eq!(projection.joints.len(), 1);
        assert_eq!(
            projection.joints[0].frame_sense(),
            Some(cadmpeg_ir::products::PlaneSense::Opposed)
        );
        assert!(projection.losses.is_empty());

        // A rigid group's members after its first two each tie to its first
        // member, and every joint names the group's scope.
        let member =
            |qualifier| crate::records::feature::assembly::DesignQualifiedAssemblyOperand {
                frame: DesignAssemblyOperandFrame {
                    reference_record_index: 30,
                    reference_offset: 0,
                    transform: cadmpeg_ir::transform::Transform::identity()
                        .rows()
                        .try_into()
                        .unwrap(),
                    transform_offset: 0,
                },
                qualifier,
            };
        let grouped = |paired: bool, members: Vec<_>| {
            let mut group = scopes(true, paired);
            group[0]
                .try_edit(|draft| {
                    if let DesignScopePayload::Assemble(Some(alignment)) = &mut draft.payload {
                        alignment.additional_members = members;
                    }
                })
                .unwrap();
            group
        };
        let third = || {
            member(DesignAssemblyOperandQualifier::OccurrencePath {
                path: path(31, true).unwrap(),
            })
        };
        let group = grouped(false, vec![third()]);
        let projection = super::project_assembly_joints(&group, &[], &inserts, &[]);
        assert_eq!(projection.joints.len(), 2);
        assert!(projection
            .joints
            .iter()
            .all(|joint| joint.native_ref.as_deref() == Some(group[0].id.as_str())));
        assert_ne!(projection.joints[0].id, projection.joints[1].id);
        assert!(projection.losses.is_empty());

        // The pairs loss counts each projected joint whose paths carry pairs.
        let projection =
            super::project_assembly_joints(&grouped(true, vec![third()]), &[], &inserts, &[]);
        assert_eq!(projection.joints.len(), 2);
        let [loss] = projection.losses.as_slice() else {
            panic!("one pairs loss");
        };
        assert_eq!(
            loss.code,
            crate::loss::F3dLossCode::AssemblyOperandPathPairsNative.kind()
        );
        assert!(loss.message.starts_with("2 projected"));

        // A member whose operand does not project is counted on its own, and
        // the group's other joints still project.
        let unresolved = member(DesignAssemblyOperandQualifier::JointOrigin {
            scope_record_index: 99,
            class_tag: "300".to_owned().try_into().unwrap(),
            byte_offset: 0,
            paired_class_tag: "301".to_owned().try_into().unwrap(),
            paired_byte_offset: 0,
        });
        let projection = super::project_assembly_joints(
            &grouped(false, vec![unresolved, third()]),
            &[],
            &inserts,
            &[],
        );
        assert_eq!(projection.joints.len(), 2);
        let [loss] = projection.losses.as_slice() else {
            panic!("one unprojected-member loss");
        };
        assert_eq!(
            loss.code,
            crate::loss::F3dLossCode::AssemblyJointUnprojected.kind()
        );
        assert!(loss.message.starts_with("1 rigid group member(s)"));

        // The class tag alone admits no external path and no untyped arity.
        assert!(path(11, false).is_err());
    }

    #[test]
    fn joint_placed_identity_occurrences_count_each_inserted_design_once() {
        let occurrence = |id: &str, scope: &str| -> cadmpeg_ir::products::Occurrence {
            let prototype = if scope == "external" {
                serde_json::json!({"scope": "external", "document": {"resolution": "path", "path": format!("{id}.f3d")}})
            } else {
                serde_json::json!({"scope": "local", "definition": "f3d:model:product_definition#1"})
            };
            serde_json::from_value(serde_json::json!({
                "id": format!("f3d:model:occurrence#{id}"),
                "prototype": prototype,
                "parent": {"kind": "root"},
                "ordinal": 0,
                "transform": [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0]],
                "scale": [1.0, 1.0, 1.0],
            }))
            .unwrap()
        };
        let occurrences = [
            occurrence("xref-0-0", "external"),
            occurrence("xref-1-0", "external"),
            occurrence("local", "local"),
        ];
        let container = |value: serde_json::Value| -> cadmpeg_ir::OperandContainer {
            serde_json::from_value(value).unwrap()
        };
        let external = container(serde_json::json!({
            "container": "external",
            "external_document": {"resolution": "document_id", "document_id": "7b14bb25-3d8a-422c-9b0c-0805a6ef037c"},
        }));
        let named = container(serde_json::json!({
            "container": "occurrence",
            "occurrence": "f3d:model:occurrence#xref-1-0",
        }));
        // One inserted design named twice accounts for one occurrence.
        assert_eq!(
            super::joint_placed_identity_occurrences(&occurrences, [&external, &external]),
            1
        );
        assert_eq!(
            super::joint_placed_identity_occurrences(&occurrences, [&named]),
            1
        );
        assert_eq!(
            super::joint_placed_identity_occurrences(&occurrences, [&named, &external]),
            2
        );
        assert_eq!(
            super::joint_placed_identity_occurrences(&occurrences, []),
            0
        );
        assert_eq!(
            super::joint_placed_occurrence_loss(1).map(|loss| loss.code),
            Some(crate::loss::F3dLossCode::JointPlacedOccurrenceUnsolved.kind())
        );
        assert!(super::joint_placed_occurrence_loss(0).is_none());
    }

    #[test]
    fn joint_motion_selects_the_projected_joint_kind() {
        use crate::records::feature::{
            assembly::{
                DesignAssemblyAlignment, DesignAssemblyAlignmentForm, DesignAssemblyOperandFrame,
            },
            joint_motion::{DesignJointLimitLanes, DesignJointMotion},
            scope::{DesignFeatureKind, DesignScopePayload},
        };
        let mut scopes = Vec::new();
        for index in [1, 2] {
            let mut scope = DesignParameterScope::empty(
                &format!("f3d:Design/BulkStream.dat:design-parameter-scope#{index}"),
                DesignFeatureKind::JointOrigin,
                index,
            );
            scope.with_joint_origin_transform(
                crate::records::sketch_placement::SketchPlacementMatrix::IDENTITY,
            );
            scopes.push(scope);
        }
        // The second connector sits 1 cm up its operand's own z axis.
        let frames = [1, 2].map(|index| {
            let mut rows = cadmpeg_ir::transform::Transform::identity().rows();
            rows[2][3] = if index == 2 { 1.0 } else { 0.0 };
            DesignAssemblyOperandFrame {
                reference_record_index: index,
                reference_offset: 0,
                transform: rows.try_into().unwrap(),
                transform_offset: 0,
            }
        });
        let qualifiers =
            [1, 2].map(
                |scope_record_index| DesignAssemblyOperandQualifier::AxialTarget {
                    target: DesignAssemblyAxialOperandTarget::DocumentRootJointOrigin {
                        scope_record_index,
                    },
                },
            );
        // The joint record is 9; its Assemble scope is 10.
        let mut scope = DesignParameterScope::empty(
            "f3d:Design/BulkStream.dat:design-parameter-scope#10",
            DesignFeatureKind::Assemble,
            10,
        );
        scope
            .try_edit(|draft| {
                draft.payload = DesignScopePayload::Assemble(Some(
                    DesignAssemblyAlignment::try_new(
                        0.0,
                        [0.0; 3],
                        Vec::new(),
                        Some(DesignAssemblyAlignmentForm::qualified(frames, qualifiers)),
                    )
                    .unwrap(),
                ));
            })
            .unwrap();
        scopes.push(scope);
        let motion = |codes: &[u32], flags: [u8; 2]| DesignJointMotion {
            id: "f3d:Design/BulkStream.dat:design-joint-motion#900".into(),
            joint_record_index: 9,
            joint_byte_offset: 800,
            motion_record_index: 20,
            motion_byte_offset: 900,
            freedoms: codes
                .iter()
                .map(|code| crate::records::identity::Located {
                    value: *code,
                    offset: 950,
                })
                .collect(),
            limits: Some(DesignJointLimitLanes {
                flags_offset: 811,
                first_flag: flags[0],
                second_flag: flags[1],
                maximum: 0.5,
                minimum: -0.25,
                values_offset: 823,
            }),
        };
        let project = |motions: &[DesignJointMotion]| {
            super::project_assembly_joints(
                &scopes,
                &[],
                &crate::design::components::ComponentInserts::default(),
                motions,
            )
        };
        let kind = |projection: &super::AssemblyJointProjection| {
            serde_json::to_value(&projection.joints[0]).unwrap()["operands"]["kind"].clone()
        };

        let rigid = project(&[]);
        assert_eq!(kind(&rigid)["kind"], "fixed");
        // Connector frames stay in their operand's own space, in millimetres.
        let connectors =
            serde_json::to_value(&rigid.joints[0]).unwrap()["operands"]["connectors"].clone();
        assert_eq!(connectors[0]["frame"][2][3], 0.0);
        assert_eq!(connectors[1]["frame"][2][3], 10.0);

        let revolute = project(&[motion(&[2], [1, 1])]);
        assert_eq!(kind(&revolute)["kind"], "revolute");
        assert_eq!(kind(&revolute)["angular_limits"]["minimum"], -0.25);
        assert!(revolute.losses.is_empty());

        // Rotation about local y turns each connector frame so y becomes z.
        let turned = project(&[motion(&[1], [0, 0])]);
        assert_eq!(kind(&turned)["kind"], "revolute");
        let frame = serde_json::to_value(&turned.joints[0]).unwrap()["operands"]["connectors"][0]
            ["frame"]
            .clone();
        assert_eq!(frame[1][2], 1.0);
        assert_eq!(frame[2][1], -1.0);

        assert_eq!(kind(&project(&[motion(&[8], [0, 0])]))["kind"], "slider");

        // A rigid group is fixed: one whose joint record states a freedom is
        // left unprojected.
        let rigid_scopes = {
            let mut rigid_scopes = scopes.clone();
            let group = rigid_scopes.last_mut().unwrap();
            group
                .try_edit(|draft| {
                    let DesignScopePayload::Assemble(alignment) = &draft.payload else {
                        panic!("an Assemble payload");
                    };
                    draft.payload = DesignScopePayload::AsBuilt(alignment.clone());
                    crate::design::test_support::lengthen_scope_frame(draft, 269);
                })
                .unwrap();
            rigid_scopes
        };
        let project_rigid = |motions: &[DesignJointMotion]| {
            super::project_assembly_joints(
                &rigid_scopes,
                &[],
                &crate::design::components::ComponentInserts::default(),
                motions,
            )
        };
        assert_eq!(
            kind(&project_rigid(&[motion(&[], [0, 0])]))["kind"],
            "fixed"
        );
        let freed = project_rigid(&[motion(&[2], [0, 0])]);
        assert!(freed.joints.is_empty());
        assert_eq!(
            freed
                .losses
                .iter()
                .map(|loss| loss.code.clone())
                .collect::<Vec<_>>(),
            [crate::loss::F3dLossCode::AssemblyJointUnprojected.kind()]
        );

        // Unconfirmed codes, one-sided flags and translation limits stay fixed.
        for undecoded in [
            motion(&[0], [0, 0]),
            motion(&[2, 7], [0, 0]),
            motion(&[2], [1, 0]),
            motion(&[8], [1, 1]),
        ] {
            let projection = project(&[undecoded]);
            assert_eq!(kind(&projection)["kind"], "fixed");
            assert_eq!(
                projection
                    .losses
                    .iter()
                    .map(|loss| loss.code.clone())
                    .collect::<Vec<_>>(),
                [crate::loss::F3dLossCode::AssemblyJointMotionUndecoded.kind()]
            );
        }
    }

    #[test]
    fn a_wide_scope_without_operand_qualifiers_is_an_unprojected_joint() {
        use crate::records::feature::{
            assembly::{
                DesignAssemblyAlignment, DesignAssemblyAlignmentForm, DesignAssemblyOperandFrame,
            },
            scope::{DesignFeatureKind, DesignScopePayload},
        };
        let frames = [17, 45].map(|index| DesignAssemblyOperandFrame {
            reference_record_index: index,
            reference_offset: 0,
            transform: cadmpeg_ir::transform::Transform::identity()
                .rows()
                .try_into()
                .unwrap(),
            transform_offset: 0,
        });
        let mut scope = DesignParameterScope::empty(
            "f3d:Design/BulkStream.dat:design-parameter-scope#10",
            DesignFeatureKind::Assemble,
            10,
        );
        scope
            .try_edit(|draft| {
                draft.frame_length = 806;
                draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
                draft.layout_fixture_references();
                draft.layout_fixture_tail();
                draft.payload = DesignScopePayload::Assemble(Some(
                    DesignAssemblyAlignment::try_new(
                        0.0,
                        [0.0; 3],
                        Vec::new(),
                        Some(DesignAssemblyAlignmentForm::Frames { frames }),
                    )
                    .unwrap(),
                ));
            })
            .unwrap();
        let projection = super::project_assembly_joints(
            &[scope],
            &[],
            &crate::design::components::ComponentInserts::default(),
            &[],
        );
        assert!(projection.joints.is_empty());
        assert_eq!(
            projection
                .losses
                .iter()
                .map(|loss| loss.code.clone())
                .collect::<Vec<_>>(),
            [crate::loss::F3dLossCode::AssemblyJointUnprojected.kind()]
        );
    }

    #[test]
    fn affine_projection_rejects_nonfinite_cadir_coefficients() {
        let mut transform = cadmpeg_ir::transform::Transform::identity().rows();
        transform[0][0] = f64::NAN;
        assert!(matches!(
            crate::design::components::neutral_transform(transform),
            Err(cadmpeg_core::CodecError::NotImplemented(_))
        ));
    }

    #[test]
    fn affine_projection_rejects_translation_overflow() {
        let mut transform = cadmpeg_ir::transform::Transform::identity().rows();
        transform[0][3] = f64::MAX;
        assert!(matches!(
            crate::design::components::neutral_transform(transform),
            Err(cadmpeg_core::CodecError::NotImplemented(_))
        ));
    }

    #[test]
    fn assembly_frame_conversion_scales_only_translation() {
        let transform = [
            [0.0, -1.0, 0.0, 1.25],
            [1.0, 0.0, 0.0, -2.5],
            [0.0, 0.0, 1.0, 3.75],
            [0.0, 0.0, 0.0, 1.0],
        ];
        assert_eq!(
            crate::design::components::neutral_transform(transform)
                .unwrap()
                .rows(),
            [
                [0.0, -1.0, 0.0, 12.5],
                [1.0, 0.0, 0.0, -25.0],
                [0.0, 0.0, 1.0, 37.5],
                [0.0, 0.0, 0.0, 1.0],
            ]
        );
    }

    #[test]
    fn legacy_as_built_421_generation_map_is_exact() {
        for (
            scope_class,
            scope_paired_class,
            owner_class,
            owner_paired_class,
            frame_class,
            frame_paired_class,
            frame_length,
            matrix_prefix,
            matrix_offset,
            limit_kind,
            reverse_limit_order,
        ) in [
            (
                "364",
                "272",
                "293",
                "272",
                "376",
                "272",
                389,
                45,
                49,
                DesignAssemblyLimitKind::Angular,
                false,
            ),
            (
                "420",
                "262",
                "378",
                "262",
                "327",
                "262",
                390,
                46,
                50,
                DesignAssemblyLimitKind::Linear,
                true,
            ),
            (
                "417",
                "263",
                "318",
                "263",
                "448",
                "263",
                390,
                46,
                50,
                DesignAssemblyLimitKind::Linear,
                true,
            ),
            (
                "457",
                "258",
                "418",
                "258",
                "297",
                "258",
                385,
                45,
                49,
                DesignAssemblyLimitKind::Linear,
                false,
            ),
        ] {
            let generation =
                super::legacy_as_built_421_generation(421, scope_class, scope_paired_class)
                    .expect("generation is admitted");
            assert_eq!(generation.owner_class_tag(), owner_class);
            assert_eq!(generation.owner_paired_class_tag(), owner_paired_class);
            assert_eq!(generation.frame_class_tag(), frame_class);
            assert_eq!(generation.frame_paired_class_tag(), frame_paired_class);
            assert_eq!(generation.frame_length(), frame_length);
            assert_eq!(generation.matrix_prefix(), matrix_prefix);
            assert_eq!(generation.matrix_offset(), matrix_offset);
            assert_eq!(generation.limit_kind(), limit_kind);
            assert_eq!(generation.reverse_limit_order(), reverse_limit_order);
        }
        assert!(super::legacy_as_built_421_generation(420, "364", "272").is_none());
        assert!(super::legacy_as_built_421_generation(421, "364", "262").is_none());
        assert!(super::legacy_as_built_421_generation(421, "999", "272").is_none());
    }

    #[test]
    fn alignment_lane_bounds_require_the_exact_frame_and_owner_count() {
        for (frame_length, owner_count, expected) in [
            (
                u64_from_index(crate::layout::assembly_class_388_266_scope_968::LEN),
                28,
                (4, 8),
            ),
            (
                u64_from_index(crate::layout::assembly_class_383_258_scope_1011::LEN),
                20,
                (8, 12),
            ),
            (627, 4, (0, 4)),
            (633, 4, (0, 4)),
            (637, 4, (0, 4)),
            (692, 4, (0, 4)),
            (604, 8, (4, 8)),
            (732, 8, (4, 8)),
            (744, 8, (4, 8)),
            (748, 8, (4, 8)),
            (705, 6, (4, 6)),
            (806, 4, (0, 4)),
            (850, 6, (0, 4)),
            (861, 4, (0, 4)),
            (945, 4, (0, 4)),
            (671, 6, (4, 6)),
            (772, 10, (8, 10)),
        ] {
            assert_eq!(
                super::AssemblyScopeGeneration::new(frame_length, "", "")
                    .alignment_lane_bounds(owner_count),
                Some(expected)
            );
        }
        for (frame_length, locators) in [(806, [534, 545]), (850, [556, 567]), (861, [534, 545])] {
            assert_eq!(
                super::AssemblyScopeGeneration::new(frame_length, "", "")
                    .operand_path_locator_offsets(),
                Some(locators)
            );
        }
        for (frame_length, owner_count) in [(627, 6), (732, 6), (705, 8), (772, 8), (604, 4)] {
            assert_eq!(
                super::AssemblyScopeGeneration::new(frame_length, "", "")
                    .alignment_lane_bounds(owner_count),
                None
            );
        }
        for (class_tag, paired_class_tag) in [("283", "264"), ("347", "260")] {
            for owner_count in [12, 14, 16, 20, 22, 36, 38, 44, 60] {
                assert_eq!(
                    super::AssemblyScopeGeneration::new(
                        800 + u64_from_index(owner_count),
                        class_tag,
                        paired_class_tag
                    )
                    .alignment_lane_bounds(owner_count),
                    Some((8, 12))
                );
            }
            for owner_count in [0, 10, 13, 15] {
                assert_eq!(
                    super::AssemblyScopeGeneration::new(
                        800 + u64_from_index(owner_count),
                        class_tag,
                        paired_class_tag
                    )
                    .alignment_lane_bounds(owner_count),
                    None
                );
            }
        }
        assert_eq!(
            super::AssemblyScopeGeneration::new(869, "283", "260").alignment_lane_bounds(12),
            None
        );
    }

    #[test]
    fn operand_path_locator_offsets_follow_the_frame_layout() {
        let class_388_length = u64_from_index(crate::layout::assembly_class_388_266_scope_968::LEN);
        assert_eq!(
            super::AssemblyScopeGeneration::new(class_388_length, "388", "266")
                .operand_path_locator_offsets(),
            Some([366, 377])
        );
        for frame_length in [627, 637, 692, 748] {
            assert_eq!(
                super::AssemblyScopeGeneration::new(frame_length, "", "")
                    .operand_path_locator_offsets(),
                Some([366, 377])
            );
        }
        for frame_length in [633, 732, 744] {
            assert_eq!(
                super::AssemblyScopeGeneration::new(frame_length, "", "")
                    .operand_path_locator_offsets(),
                Some([362, 373])
            );
        }
        assert_eq!(
            super::AssemblyScopeGeneration::new(671, "406", "261").operand_path_locator_offsets(),
            Some([388, 399])
        );
        for frame_length in [604, 705, 772] {
            assert_eq!(
                super::AssemblyScopeGeneration::new(frame_length, "", "")
                    .operand_path_locator_offsets(),
                None
            );
        }
        assert_eq!(
            super::AssemblyScopeGeneration::new(869, "283", "264").operand_path_locator_offsets(),
            Some([366, 377])
        );
        assert_eq!(
            super::AssemblyScopeGeneration::new(843, "347", "260").operand_path_locator_offsets(),
            Some([366, 377])
        );
        assert_eq!(
            super::AssemblyScopeGeneration::new(869, "283", "260").operand_path_locator_offsets(),
            None
        );
    }

    #[test]
    fn operand_frames_are_scoped_by_class_pair() {
        assert_eq!(
            super::AssemblyScopeGeneration::new(
                u64_from_index(crate::layout::assembly_class_388_266_scope_968::LEN),
                "388",
                "266"
            )
            .operand_frame_variant(),
            Some(super::AssemblyOperandFrameVariant::LegacyClass388)
        );
        assert_eq!(
            super::AssemblyScopeGeneration::new(
                u64_from_index(crate::layout::assembly_class_388_266_scope_968::LEN),
                "388",
                "258"
            )
            .operand_frame_variant(),
            None
        );
        assert_eq!(
            super::AssemblyScopeGeneration::new(
                u64_from_index(crate::layout::assembly_class_383_258_scope_1011::LEN),
                "383",
                "258"
            )
            .operand_frame_variant(),
            Some(super::AssemblyOperandFrameVariant::Standard)
        );
        assert_eq!(
            super::AssemblyScopeGeneration::new(
                u64_from_index(crate::layout::assembly_class_383_258_scope_1011::LEN),
                "383",
                "261"
            )
            .operand_frame_variant(),
            None
        );
        assert!(super::legacy_class_383_258_scope(
            u64_from_index(crate::layout::assembly_class_383_258_scope_1011::LEN),
            "383",
            "258"
        ));
        assert!(!super::legacy_class_383_258_scope(
            u64_from_index(crate::layout::assembly_class_383_258_scope_1011::LEN) - 1,
            "383",
            "258"
        ));
        assert_eq!(
            super::AssemblyScopeGeneration::new(744, "430", "262").operand_frame_variant(),
            Some(super::AssemblyOperandFrameVariant::Compact)
        );
        assert_eq!(
            super::AssemblyScopeGeneration::new(748, "430", "262").operand_frame_variant(),
            Some(super::AssemblyOperandFrameVariant::Standard)
        );
        assert_eq!(
            super::AssemblyScopeGeneration::new(671, "406", "261").operand_frame_variant(),
            Some(super::AssemblyOperandFrameVariant::Standard)
        );
        assert_eq!(
            super::AssemblyScopeGeneration::new(671, "406", "258").operand_frame_variant(),
            None
        );
        assert_eq!(
            super::AssemblyScopeGeneration::new(671, "430", "261").operand_frame_variant(),
            None
        );
        assert_eq!(
            super::AssemblyScopeGeneration::new(744, "327", "262").operand_frame_variant(),
            None
        );
        assert_eq!(
            super::AssemblyScopeGeneration::new(748, "430", "261").operand_frame_variant(),
            None
        );
    }

    fn joint_origin_scope(record_index: u32, translation_x: f64) -> DesignParameterScope {
        let mut scope = DesignParameterScope::empty(
            &format!("f3d:Design/BulkStream.dat:joint-origin#{record_index}"),
            crate::records::feature::scope::DesignFeatureKind::JointOrigin,
            record_index,
        );
        scope.with_joint_origin_transform(
            [
                [1.0, 0.0, 0.0, translation_x],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ]
            .try_into()
            .unwrap(),
        );
        scope
    }

    #[test]
    fn axial_operands_project_component_and_document_root_qualifiers() {
        let mut component_scope = DesignParameterScope::empty(
            "f3d:Design/BulkStream.dat:component-insert#200",
            crate::records::feature::scope::DesignFeatureKind::ComponentInsert,
            200,
        );
        if let crate::records::feature::scope::DesignScopePayloadMut::ComponentInsert(slot) =
            component_scope.payload_mut()
        {
            *slot = Some(
                crate::records::feature::assembly_features::DesignComponentInsertConstruction {
                    relation_record_index: 201,
                    carrier_record_index: 202,
                    occurrence_identity: None,
                    neutron_role: "component-role".into(),
                    neutron_role_offset: 0,
                    placement: None,
                },
            );
        }
        let origin_scope = joint_origin_scope(80, 0.0);
        let targets = [
            DesignAssemblyAxialOperandTarget::ComponentInsertOccurrence {
                component_insert_scope_record_index: 200,
                construction_record_index: 70,
                construction_class_tag: crate::records::references::DesignClassTag::try_from(
                    "305".to_owned(),
                )
                .unwrap(),
                construction_byte_offset: 1_300,
                construction_transform_offset: 1_348,
                axis_record_index_offsets: [1_493, 1_509],
                construction_paired_class_tag:
                    crate::records::references::DesignClassTag::try_from("261".to_owned()).unwrap(),
                construction_paired_byte_offset: 1_680,
                selectors: Box::new([selector(), second_selector()]),
            },
            DesignAssemblyAxialOperandTarget::DocumentRootJointOrigin {
                scope_record_index: 80,
            },
        ];

        let qualifiers =
            targets.map(|target| DesignAssemblyOperandQualifier::AxialTarget { target });
        let scopes = vec![component_scope.clone(), origin_scope.clone()];
        let inserts = crate::design::components::ComponentInserts::decide(
            &scopes,
            &crate::design::test_support::synthetic_feature_timelines(std::slice::from_ref(
                &component_scope,
            )),
            &[],
        );
        let project =
            |scopes: &[DesignParameterScope],
             inserts: &crate::design::components::ComponentInserts| {
                super::project_qualified_operands(
                    qualifiers.each_ref(),
                    "f3d:Design/BulkStream.dat",
                    &BTreeMap::new(),
                    scopes,
                    inserts,
                )
            };
        let operands = project(&scopes, &inserts).expect("complete axial operands");

        assert_eq!(
            operands[0].container,
            cadmpeg_ir::OperandContainer::Occurrence {
                occurrence: crate::ids::neutral_component_insert_occurrence_id(&component_scope),
                occurrence_path: Vec::new(),
            }
        );
        assert!(operands[0]
            .object
            .starts_with("f3d:feature-input:connector#"));
        assert_eq!(operands[1].container, cadmpeg_ir::OperandContainer::Root {});
        assert_eq!(
            operands[1].object,
            crate::ids::neutral_feature_id(&origin_scope).as_str()
        );

        assert!(
            project(
                &scopes,
                &crate::design::components::ComponentInserts::default()
            )
            .is_none(),
            "an insert without a projected occurrence is no operand"
        );
        let unframed = [component_scope, joint_origin_scope(80, 1.0e308)];
        assert!(
            project(&unframed, &inserts).is_none(),
            "a JointOrigin whose frame does not project is no operand"
        );
    }

    #[test]
    fn assemble_features_bind_only_to_projected_joints() {
        let native = FeatureDefinition::Operation(FeatureOperation::Native {
            kind: "Assemble".into(),
            parameters: BTreeMap::new(),
        });
        let scope = "f3d:Design/BulkStream.dat:design-parameter-scope#3";
        let mut features = [
            feature(scope, native.clone()),
            feature(
                "f3d:Design/BulkStream.dat:design-parameter-scope#4",
                native.clone(),
            ),
        ];
        let mut joint = cadmpeg_ir::products::AssemblyJoint::paired(
            crate::ids::neutral_assembly_joint_id(&DesignParameterScope::empty(
                scope,
                crate::records::feature::scope::DesignFeatureKind::Assemble,
                3,
            )),
            cadmpeg_ir::products::PairedJointKind::Fixed {
                angle: None,
                translation_offset: None,
                angular_limits: None,
                linear_limits: None,
            },
            [0, 1].map(|index| cadmpeg_ir::products::JointConnector {
                operand: cadmpeg_ir::products::JointOperand::root(
                    format!("origin-{index}"),
                    Vec::new(),
                ),
                frame: cadmpeg_ir::transform::Transform::identity(),
                detached: false,
            }),
            None,
        );
        joint.native_ref = Some(scope.into());

        super::bind_assembly_joint_features(&mut features, std::slice::from_ref(&joint));

        assert_eq!(
            *features[0].evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::AssemblyJoint {
                joint: joint.id.clone()
            })
        );
        assert_eq!(*features[1].evaluation.definition(), native);
    }

    #[test]
    fn axial_connector_identity_excludes_axis_specific_occurrence_references() {
        let first = selector();
        let mut second = first.clone();
        second.occurrence_reference += 1;
        second.occurrence_reference_offset += 100;
        second.selector_asset_id = second
            .selector_asset_id
            .as_str()
            .to_ascii_uppercase()
            .try_into()
            .expect("GUID");
        second.selector_context_id = second
            .selector_context_id
            .as_str()
            .to_ascii_uppercase()
            .try_into()
            .expect("GUID");
        second.external_asset_id = second
            .external_asset_id
            .as_str()
            .to_ascii_uppercase()
            .try_into()
            .expect("GUID");
        assert!(first.selects_same_object(&second));
        assert_eq!(
            crate::ids::neutral_assembly_axial_object_id(&first),
            crate::ids::neutral_assembly_axial_object_id(&second)
        );

        second.external_object_reference += 1;
        assert_ne!(
            crate::ids::neutral_assembly_axial_object_id(&first),
            crate::ids::neutral_assembly_axial_object_id(&second)
        );
    }
}
