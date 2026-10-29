// SPDX-License-Identifier: Apache-2.0
//! Mirror operands and reflection in the part coordinate system.

use std::collections::{HashMap, HashSet};

use cadmpeg_core::decode::{unique_index, DecodeContext};
use cadmpeg_core::text::NonBlankString;
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::patterns::{PatternKind, PatternSeed, PatternTransform};
use cadmpeg_ir::features::{
    BodySelection, Feature, FeatureDefinition, FeatureDirection3, FeatureId, FeatureOperation,
    FeatureResultMembers, FeatureResultTopology, FinitePoint3,
};
use cadmpeg_ir::ids::FeatureResultTopologyId;
use cadmpeg_ir::math::{Point3, Vector3};

use super::{
    history_feature_id, native_feature, ClassId, PmDcFeatureLabel, PmDcFeatureProperty,
    PmDcFeaturePropertyKind, PmDcFeatureReferenceFamily, PmDcPatternFamily, PmDcPatternFeature,
    PmDcWorkPlane,
};
use crate::pmdc::PmDcReference;
use crate::sketch::PmDcTransform;

pub(crate) const SELECTION_TYPE: [u8; 16] = [
    0xbe, 0xf0, 0x6e, 0x8d, 0x35, 0x4c, 0xc7, 0xaa, 0xd7, 0xc8, 0x8d, 0xb7, 0xea, 0x9b, 0x0f, 0xfb,
];
pub(crate) const OPTIONS_TYPE: [u8; 16] = [
    0x5a, 0x4a, 0x68, 0x99, 0xd2, 0x11, 0x7f, 0xec, 0x60, 0x00, 0xb0, 0xab, 0xc8, 0x43, 0x3c, 0xb0,
];

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum Property {
    Selection {
        control: u32,
        value: u32,
    },
    Options {
        bytes: cadmpeg_ir::native::bytes::NativeBytes<[u8; 9]>,
    },
}

pub(super) fn parse_selection(
    _: &DecodeContext<'_>,
    source: cadmpeg_core::decode::View<'_>,
    version: u8,
) -> Result<Option<super::PmDcFeaturePropertyPayload>, CodecError> {
    use crate::layout::pm_dc_mirror_selection_tail as layout;

    let mut cursor = crate::pmdc::Cursor::new(source);
    let header = crate::pmdc::content_header(&mut cursor, version)?;
    if cursor.remaining() != layout::LEN {
        return Ok(None);
    }
    let mut tail = cursor.into_view();
    let control = tail.req_u32_le()?;
    let value = tail.req_u32_le()?;
    Ok(Some(super::property(
        version,
        header,
        PmDcFeaturePropertyKind::Mirror {
            definition: Property::Selection { control, value },
        },
    )))
}

pub(super) fn parse_options(
    _: &DecodeContext<'_>,
    source: cadmpeg_core::decode::View<'_>,
    version: u8,
) -> Result<Option<super::PmDcFeaturePropertyPayload>, CodecError> {
    use crate::layout::pm_dc_mirror_options_tail as layout;

    let mut cursor = crate::pmdc::Cursor::new(source);
    let header = crate::pmdc::content_header(&mut cursor, version)?;
    if cursor.remaining() != layout::LEN {
        return Ok(None);
    }
    let bytes = cursor.take_array::<{ layout::LEN }>("mirror option bytes")?;
    Ok(Some(super::property(
        version,
        header,
        PmDcFeaturePropertyKind::Mirror {
            definition: Property::Options {
                bytes: bytes.into(),
            },
        },
    )))
}

const EPS_MIRROR_FRAME: f64 = 1.0e-10;
const MIRROR_CLASS: ClassId = ClassId(cadmpeg_ir::native::bytes::NativeBytes::new([
    0xa3, 0x26, 0xc2, 0xb6, 0xd2, 0x11, 0x26, 0x36, 0x60, 0x00, 0x2c, 0xab, 0x01, 0xf3, 0x1b, 0xb0,
]));
pub(crate) const IDENTITY: [[f64; 4]; 4] = [
    [1.0, 0.0, 0.0, 0.0],
    [0.0, 1.0, 0.0, 0.0],
    [0.0, 0.0, 1.0, 0.0],
    [0.0, 0.0, 0.0, 1.0],
];

pub(crate) struct MirrorIndex<'a> {
    properties: HashMap<(&'a str, u32), Option<&'a PmDcFeatureProperty>>,
    transforms: HashMap<(&'a str, u32), Option<&'a PmDcTransform>>,
    planes: HashMap<(&'a str, u32), Option<&'a PmDcWorkPlane>>,
    record_lengths: Option<HashMap<(&'a str, u32), Option<u64>>>,
}

struct ResolvedMirror {
    definition: FeatureDefinition,
    result_native: String,
    members: FeatureResultMembers,
}

impl<'a> MirrorIndex<'a> {
    pub(crate) fn new(
        properties: &'a [PmDcFeatureProperty],
        transforms: &'a [PmDcTransform],
        planes: &'a [PmDcWorkPlane],
    ) -> Self {
        Self {
            record_lengths: None,
            properties: unique_index(properties.iter().map(|r| {
                (
                    (r.identity.segment_token.as_str(), r.identity.record_ordinal),
                    r,
                )
            })),
            transforms: unique_index(transforms.iter().map(|r| {
                (
                    (r.identity.segment_token.as_str(), r.identity.record_ordinal),
                    r,
                )
            })),
            planes: unique_index(planes.iter().map(|r| {
                (
                    (r.identity.segment_token.as_str(), r.identity.record_ordinal),
                    r,
                )
            })),
        }
    }

    pub(crate) fn with_record_lengths(
        mut self,
        records: impl Iterator<Item = (&'a str, u32, u64)>,
    ) -> Self {
        self.record_lengths = Some(unique_index(
            records.map(|(token, ordinal, length)| ((token, ordinal), length)),
        ));
        self
    }

    fn property(&self, token: &str, reference: PmDcReference) -> Option<&'a PmDcFeatureProperty> {
        let property = self
            .properties
            .get(&(token, reference.record_ordinal()?))?
            .as_ref()
            .copied()?;
        if let (PmDcFeaturePropertyKind::Mirror { .. }, Some(lengths)) =
            (&property.kind, &self.record_lengths)
        {
            let expected = property_payload_len(property)?;
            if lengths.get(&(token, property.identity.record_ordinal)) != Some(&Some(expected)) {
                return None;
            }
        }
        let type_id = match &property.kind {
            PmDcFeaturePropertyKind::Mirror { definition } => match definition {
                Property::Selection { .. } => SELECTION_TYPE,
                Property::Options { .. } => OPTIONS_TYPE,
            },
            PmDcFeaturePropertyKind::Boolean { .. } => super::BOOLEAN_TYPE,
            PmDcFeaturePropertyKind::RdxVariable { .. } => super::RDX_VARIABLE_TYPE,
            PmDcFeaturePropertyKind::SurfaceBody { .. } => super::SURFACE_BODY_TYPE,
            PmDcFeaturePropertyKind::References {
                family: PmDcFeatureReferenceFamily::ObjectCollection,
                ..
            } => super::OBJECT_COLLECTION_TYPE,
            PmDcFeaturePropertyKind::References {
                family: PmDcFeatureReferenceFamily::FeatureDimensions,
                ..
            } => super::FEATURE_DIMENSIONS_TYPE,
            _ => return None,
        };
        (property.identity.type_id == crate::record_identity::RecordTypeId::from_bytes(type_id))
            .then_some(property)
    }

    fn resolve(
        &self,
        source: &PmDcPatternFeature,
        label: &PmDcFeatureLabel,
        mut seed_feature: impl FnMut(u32) -> Option<FeatureId>,
    ) -> Option<ResolvedMirror> {
        let extension_count = match source.save_version_major {
            0..=20 => 0,
            21..=28 => 6,
            _ => 14,
        };
        if !crate::pmdc::record_version_supported(source.save_version_major)
            || source.family != PmDcPatternFamily::Mirror
            || !is_mirror_record(source)
            || source.property_slots.len() != 13
            || source.extension_values.len() != extension_count
            || source.control != 0
            || source.value != 0
            || source.properties.marker() != 2
            || source.participants.marker() != 2
            || label.participants.marker() != 2
            || !source.properties.references().is_empty()
            || label.header.owner.record_ordinal() != Some(source.identity.record_ordinal)
            || label.identity.segment_token != source.identity.segment_token
            || label.identity.type_id
                != crate::record_identity::RecordTypeId::from_bytes(super::FEATURE_LABEL_TYPE)
            || label.class_id() != MIRROR_CLASS
        {
            return None;
        }
        let token = source.identity.segment_token.as_str();
        let slot = |n: usize| source.property_slots.get(n).copied();
        if !matches!(
            self.property(token, slot(1)?)?.kind,
            PmDcFeaturePropertyKind::Mirror {
                definition: Property::Selection {
                    control: crate::layout::pm_dc_mirror_selection_tail::CONTROL_VALUE,
                    value: crate::layout::pm_dc_mirror_selection_tail::VALUE_VALUE
                }
            }
        ) || !matches!(
            &self.property(token, slot(4)?)?.kind,
            PmDcFeaturePropertyKind::Mirror {
                definition: Property::Options { bytes }
            } if *bytes == crate::layout::pm_dc_mirror_options_tail::OPTIONS_VALUE
        ) {
            return None;
        }
        if !matches!(
            &self.property(token, slot(5)?)?.kind,
            PmDcFeaturePropertyKind::References {
                family: PmDcFeatureReferenceFamily::FeatureDimensions,
                items
            } if items.marker() == 2 && items.references().is_empty()
        ) {
            return None;
        }
        let boolean = |n| match &self.property(token, slot(n)?)?.kind {
            PmDcFeaturePropertyKind::Boolean {
                name_value: 0,
                value,
                ..
            } => Some(*value),
            _ => None,
        };
        if !self.extension_options(source) {
            return None;
        }
        let bodies_mode = boolean(6)?;
        if boolean(7)?
            || slot(3)?.index() != 0
            || slot(8)?.index() != 0
            || slot(if bodies_mode { 9 } else { 10 })?.index() != 0
            || !label.participants.references().is_empty()
            || (slot(11)?.index() != 0 && boolean(11)?)
            || !matches!(
                self.property(token, slot(2)?)?.kind,
                PmDcFeaturePropertyKind::RdxVariable {
                    name_value: 0,
                    nominal_value: 0,
                    model_value: 0,
                    ..
                }
            )
        {
            return None;
        }
        let placement = self
            .transforms
            .get(&(token, slot(0)?.record_ordinal()?))?
            .as_ref()?;
        if placement.identity.type_id
            != crate::record_identity::RecordTypeId::from_bytes(crate::sketch::TRANSFORM_TYPE)
            || placement.matrix.rows() != IDENTITY
        {
            return None;
        }
        let plane = self
            .planes
            .get(&(token, slot(12)?.record_ordinal()?))?
            .as_ref()?;
        if plane.identity.type_id
            != crate::record_identity::RecordTypeId::from_bytes(super::WORK_PLANE_TYPE)
            || plane.entity_flags != 0
            || plane.marker
            || plane.values.len() != if source.save_version_major > 21 { 4 } else { 3 }
            || plane.values.iter().any(|&value| value != 0)
        {
            return None;
        }
        let u = Vector3::from(plane.u_axis.map(cadmpeg_ir::scalar::FiniteReal::get));
        let v = Vector3::from(plane.v_axis.map(cadmpeg_ir::scalar::FiniteReal::get));
        if (u.norm() - 1.0).abs() > EPS_MIRROR_FRAME
            || (v.norm() - 1.0).abs() > EPS_MIRROR_FRAME
            || u.dot(v).abs() > EPS_MIRROR_FRAME
        {
            return None;
        }
        let normal =
            FeatureDirection3::from(cadmpeg_ir::units::UnitVector3::normalized(u.cross(v))?);
        let origin = plane.origin.map(cadmpeg_ir::scalar::FiniteReal::get);
        let origin = FinitePoint3::new(Point3::new(
            origin[0] * 10.0,
            origin[1] * 10.0,
            origin[2] * 10.0,
        ))?;
        let collection = self.property(token, slot(if bodies_mode { 10 } else { 9 })?)?;
        let PmDcFeaturePropertyKind::References {
            family: PmDcFeatureReferenceFamily::ObjectCollection,
            items,
        } = &collection.kind
        else {
            return None;
        };
        if items.marker() != 2 {
            return None;
        }
        let mut bodies = Vec::new();
        for reference in items.references() {
            let body = self.property(token, *reference)?;
            if !matches!(body.kind, PmDcFeaturePropertyKind::SurfaceBody { .. }) {
                return None;
            }
            bodies.push(NonBlankString::new(body.id())?);
        }
        if bodies.is_empty() {
            return None;
        }
        let result_native = collection.id();
        let members = FeatureResultMembers::new(bodies, Vec::new(), Vec::new(), Vec::new()).ok()?;
        let seeds = if bodies_mode {
            if !source.participants.references().is_empty() {
                return None;
            }
            vec![PatternSeed::Bodies(BodySelection::Native(collection.id()))]
        } else {
            if source.participants.references().is_empty() {
                return None;
            }
            let mut seen = HashSet::new();
            let mut seeds = Vec::new();
            for reference in source.participants.references() {
                let ordinal = reference.record_ordinal()?;
                if !seen.insert(ordinal) {
                    return None;
                }
                seeds.push(PatternSeed::Feature(seed_feature(ordinal)?));
            }
            seeds
        };
        let pattern = PatternKind::new(PatternTransform::Mirror {
            plane_origin: origin,
            plane_normal: normal,
        })
        .ok()?;
        Some(ResolvedMirror {
            definition: FeatureDefinition::Operation(FeatureOperation::Pattern { seeds, pattern }),
            result_native,
            members,
        })
    }

    fn extension_options(&self, source: &PmDcPatternFeature) -> bool {
        let values = &source.extension_values;
        if source.save_version_major <= 20 {
            return values.is_empty();
        }
        let Some((&reference, rest)) = values.split_first() else {
            return false;
        };
        if !matches!(
            self.property(
                source.identity.segment_token.as_str(),
                PmDcReference::from_packed(reference)
            )
            .map(|p| &p.kind),
            Some(PmDcFeaturePropertyKind::Boolean {
                name_value: 0,
                value: false,
                ..
            })
        ) {
            return false;
        }
        if source.save_version_major >= 29 {
            rest == [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2, 0, 0]
        } else {
            rest == [0; 5]
        }
    }

    pub(crate) fn project(
        &self,
        ctx: &DecodeContext<'_>,
        source: &PmDcPatternFeature,
        label: &PmDcFeatureLabel,
        seed_feature: impl FnMut(u32) -> Option<FeatureId>,
    ) -> Option<Result<(Feature, FeatureResultTopology), CodecError>> {
        self.resolve(source, label, seed_feature).map(|resolved| {
            let mut feature = native_feature(ctx, &source.identity, Some(label), source.id())?;
            feature.source_tag = Some("mirror".into());
            feature.evaluation =
                cadmpeg_ir::features::FeatureEvaluation::from_definition(resolved.definition);
            ctx.charge_entities(1, "project Inventor mirror result topology")?;
            let result = FeatureResultTopology::new(
                FeatureResultTopologyId::compose(
                    &cadmpeg_ir::identity_namespace!("inventor", "design", "feature-result"),
                    source.identity.key()?,
                ),
                history_feature_id(&source.identity)?,
                resolved.members,
                Some(resolved.result_native),
            );
            Ok((feature, result))
        })
    }

    pub(crate) fn matches_projection(
        &self,
        source: &PmDcPatternFeature,
        label: &PmDcFeatureLabel,
        feature: &Feature,
        result: &FeatureResultTopology,
        seed_feature: impl FnMut(u32) -> Option<FeatureId>,
    ) -> bool {
        let Some(resolved) = self.resolve(source, label, seed_feature) else {
            return false;
        };
        let Ok(feature_id) = history_feature_id(&source.identity) else {
            return false;
        };
        let Ok(key) = source.identity.key() else {
            return false;
        };
        if feature.id != feature_id
            || feature.native_ref.as_deref() != Some(source.id().as_str())
            || feature.evaluation.definition() != &resolved.definition
        {
            return false;
        }
        let expected = FeatureResultTopology::new(
            FeatureResultTopologyId::compose(
                &cadmpeg_ir::identity_namespace!("inventor", "design", "feature-result"),
                key,
            ),
            feature_id,
            resolved.members,
            Some(resolved.result_native),
        );
        result == &expected
    }
}

pub(crate) fn seed_feature_id(
    history: &super::history::History,
    mirror_ordinal: u32,
    seed: &crate::record_identity::RecordIdentity,
) -> Option<FeatureId> {
    let before = history.placement(seed.record_ordinal)?;
    let current = history.placement(mirror_ordinal)?;
    if before.ordinal >= current.ordinal || before.model_root != current.model_root {
        return None;
    }
    let seed_type = match before.kind {
        super::history::HistoryNodeKind::Feature => super::FEATURE_TYPE,
        super::history::HistoryNodeKind::Pattern => {
            if seed.type_id
                == crate::record_identity::RecordTypeId::from_bytes(super::MIRROR_FEATURE_TYPE)
            {
                super::MIRROR_FEATURE_TYPE
            } else {
                super::RECTANGULAR_PATTERN_FEATURE_TYPE
            }
        }
        super::history::HistoryNodeKind::Sketch | super::history::HistoryNodeKind::WorkPlane => {
            return None
        }
    };
    if seed.type_id != crate::record_identity::RecordTypeId::from_bytes(seed_type) {
        return None;
    }
    history_feature_id(seed).ok()
}

#[cfg(test)]
mod tests;

pub(super) fn native_loss() -> cadmpeg_ir::report::loss::LossNote {
    crate::loss::InventorLossCode::FeatureMirrorOperandsNative.note(
        "Mirror construction operands remain native outside the identity-frame, zero-option solid-feature or same-body form.",
    )
}

pub(super) fn native_losses(
    patterns: &[PmDcPatternFeature],
    features: &[Feature],
) -> Vec<cadmpeg_ir::report::loss::LossNote> {
    let neutral = features.iter().filter(|feature| {
        matches!(feature.evaluation.definition(), FeatureDefinition::Operation(FeatureOperation::Pattern { pattern, .. }) if matches!(pattern.definition(), PatternTransform::Mirror { .. }))
    }).filter_map(|feature| feature.native_ref.as_deref()).collect::<HashSet<_>>();
    patterns
        .iter()
        .filter(|source| is_mirror_record(source) && !neutral.contains(source.id().as_str()))
        .map(|_| native_loss())
        .collect()
}

pub(crate) fn is_mirror_record(source: &PmDcPatternFeature) -> bool {
    source.identity.type_id
        == crate::record_identity::RecordTypeId::from_bytes(super::MIRROR_FEATURE_TYPE)
}

fn operand_references(patterns: &[PmDcPatternFeature]) -> HashSet<(&str, u32)> {
    patterns
        .iter()
        .filter(|source| is_mirror_record(source))
        .flat_map(|source| {
            [1, 4].into_iter().filter_map(move |slot| {
                Some((
                    source.identity.segment_token.as_str(),
                    source.property_slots.get(slot)?.record_ordinal()?,
                ))
            })
        })
        .collect()
}

pub(super) fn admit_operands(
    ctx: &DecodeContext<'_>,
    document: &crate::rse::RseInventory<'_>,
    inventory: &mut super::FeatureInventory,
) -> Result<(), CodecError> {
    use crate::rse::{RecordFrameState, SegmentBulkState, SegmentKind};

    let needed = operand_references(&inventory.pattern_features);
    if needed.is_empty() {
        return Ok(());
    }
    for segment in &document.segments {
        if segment.kind != SegmentKind::PmDc {
            continue;
        }
        let Some(version) = segment
            .registry
            .map(|join| join.version_major)
            .filter(|&version| crate::pmdc::record_version_supported(version))
        else {
            continue;
        };
        let SegmentBulkState::Framed(bulk) = &segment.bulk else {
            continue;
        };
        let RecordFrameState::Framed(table) = &bulk.records else {
            continue;
        };
        for record in &table.records {
            if !needed.contains(&(segment.pair.token.key().as_str(), record.ordinal)) {
                continue;
            }
            let parsed = match record.type_id {
                SELECTION_TYPE => parse_selection(ctx, record.payload, version),
                OPTIONS_TYPE => parse_options(ctx, record.payload, version),
                _ => continue,
            };
            match parsed {
                Ok(Some(property)) => super::push_record(
                    ctx,
                    &mut inventory.properties,
                    property,
                    record.type_id,
                    segment.pair.token.key(),
                    record.ordinal,
                    "admit Inventor mirror operand",
                )?,
                Ok(None) => {}
                Err(error) => {
                    if matches!(error, CodecError::ResourceLimit(_)) {
                        return Err(error);
                    }
                    ctx.charge_entities(1, "admit Inventor mirror operand issue")?;
                    inventory.issues.push(crate::record_issue::RecordIssue {
                        family: crate::record_issue::RecordIssueFamily::Feature {
                            type_id: crate::record_identity::RecordTypeId::from_bytes(
                                record.type_id,
                            ),
                        },
                        segment_token: segment.pair.token.key().clone(),
                        record_ordinal: record.ordinal,
                        detail: crate::issue_detail(error)?,
                    });
                }
            }
        }
    }
    Ok(())
}

fn property_payload_len(property: &PmDcFeatureProperty) -> Option<u64> {
    use crate::layout::{pm_dc_content_header, pm_dc_content_header_v28};

    let tail = match property.kind {
        PmDcFeaturePropertyKind::Mirror {
            definition: Property::Selection { .. },
        } => crate::layout::pm_dc_mirror_selection_tail::LEN,
        PmDcFeaturePropertyKind::Mirror {
            definition: Property::Options { .. },
        } => crate::layout::pm_dc_mirror_options_tail::LEN,
        _ => return None,
    };
    let version = property.save_version_major;
    if !crate::pmdc::record_version_supported(version) {
        return None;
    }
    let prefix = if version >= 28 {
        pm_dc_content_header_v28::LEN
    } else {
        pm_dc_content_header::LEN
    };
    let stamps = if version >= 25 {
        2
    } else {
        usize::from(version >= 23)
    };
    Some(cadmpeg_core::decode::u64_from_index(
        prefix + stamps * std::mem::size_of::<i32>() + tail,
    ))
}
