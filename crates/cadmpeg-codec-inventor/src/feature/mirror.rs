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

pub(crate) mod body_result;
pub(crate) mod instance_map;
pub(crate) mod plane_reference;

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
    InstanceMap {
        source: PmDcReference,
        result: PmDcReference,
        owner: PmDcReference,
    },
    PlaneReference {
        owner: PmDcReference,
        target: PmDcReference,
        role: u32,
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
    /// Plane-reference records by the plane record that they own.
    plane_references: HashMap<(&'a str, u32), Option<&'a PmDcFeatureProperty>>,
    record_lengths: Option<HashMap<(&'a str, u32), Option<u64>>>,
    generated_nodes: Option<(&'a [super::PmDcFeature], &'a [PmDcFeatureLabel])>,
    bodies: Option<&'a HashMap<String, cadmpeg_ir::ids::BodyId>>,
}

struct ResolvedMirror {
    definition: FeatureDefinition,
    result_native: String,
    members: FeatureResultMembers,
}

#[derive(Clone, Copy)]
pub(crate) enum Refusal {
    Slot7,
    Slot11,
    InstanceMap,
}

impl Refusal {
    fn note(self) -> cadmpeg_ir::report::loss::LossNote {
        let slot = match self {
            Self::Slot7 => 7,
            Self::Slot11 => 11,
            Self::InstanceMap => return crate::loss::InventorLossCode::FeatureMirrorInstanceMapNative.note(
                "Mirror instance-map or generated-body operands remain native outside the resolved body-copy form.",
            ),
        };
        crate::loss::InventorLossCode::FeatureMirrorBooleanNative.note(format!(
            "Mirror slot {slot} has a true Boolean with an unresolved construction role."
        ))
    }
}

impl<'a> MirrorIndex<'a> {
    pub(crate) fn new(
        properties: &'a [PmDcFeatureProperty],
        transforms: &'a [PmDcTransform],
        planes: &'a [PmDcWorkPlane],
    ) -> Self {
        Self {
            record_lengths: None,
            generated_nodes: None,
            bodies: None,
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
            plane_references: unique_index(properties.iter().filter_map(|r| {
                let PmDcFeaturePropertyKind::Mirror {
                    definition: Property::PlaneReference { owner, .. },
                } = &r.kind
                else {
                    return None;
                };
                Some((
                    (r.identity.segment_token.as_str(), owner.record_ordinal()?),
                    r,
                ))
            })),
        }
    }

    /// Record ordinal of the work plane that supplied the plane record
    /// `plane`, when a plane-reference record owned by `plane` targets a work
    /// plane that gives the same plane.
    pub(crate) fn supplying_plane(&self, token: &str, plane: u32) -> Option<u32> {
        let record = self
            .plane_references
            .get(&(token, plane))?
            .as_ref()
            .copied()?;
        let PmDcFeaturePropertyKind::Mirror {
            definition: Property::PlaneReference { owner, target, .. },
        } = &record.kind
        else {
            return None;
        };
        if let Some(lengths) = &self.record_lengths {
            let expected = property_payload_len(record)?;
            if lengths.get(&(token, record.identity.record_ordinal)) != Some(&Some(expected))
                || record.identity.type_id
                    != crate::record_identity::RecordTypeId::from_bytes(plane_reference::TYPE)
            {
                return None;
            }
        }
        if !plane_reference::reference_form(*owner, *target) || owner.record_ordinal()? != plane {
            return None;
        }
        let supplier = self
            .planes
            .get(&(token, target.record_ordinal()?))?
            .as_ref()?;
        let snapshot = self.planes.get(&(token, plane))?.as_ref()?;
        (supplier.identity.type_id
            == crate::record_identity::RecordTypeId::from_bytes(super::WORK_PLANE_TYPE)
            && plane_reference::same_plane(supplier, snapshot))
        .then_some(supplier.identity.record_ordinal)
    }

    pub(crate) fn with_generated_nodes(
        mut self,
        features: &'a [super::PmDcFeature],
        labels: &'a [PmDcFeatureLabel],
    ) -> Self {
        self.generated_nodes = Some((features, labels));
        self
    }

    /// Current-model bodies of the surface-body wrappers, which the body
    /// selection pass substitutes into a projected mirror source.
    pub(crate) fn with_bodies(
        mut self,
        bodies: &'a HashMap<String, cadmpeg_ir::ids::BodyId>,
    ) -> Self {
        self.bodies = Some(bodies);
        self
    }

    /// Source of a body-mode mirror: the surface-body wrappers that the
    /// object collection lists, grouped under the collection record.
    fn body_source(
        &self,
        token: &str,
        collection: &PmDcFeatureProperty,
        items: &[PmDcReference],
    ) -> Option<BodySelection> {
        let mut wrappers = Vec::new();
        for reference in items {
            let wrapper = self.property(token, *reference)?;
            if !matches!(wrapper.kind, PmDcFeaturePropertyKind::SurfaceBody { .. }) {
                return None;
            }
            wrappers.push(wrapper.id());
        }
        BodySelection::local(wrappers, collection.id()).ok()
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
                Property::InstanceMap {
                    source,
                    result,
                    owner,
                } if instance_map::reference_form(*source, *result, *owner) => instance_map::TYPE,
                Property::InstanceMap { .. } => return None,
                Property::PlaneReference { .. } => return None,
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
        seed_feature: impl FnMut(u32) -> Option<FeatureId>,
    ) -> Option<ResolvedMirror> {
        self.resolve_with_refusal(source, label, seed_feature, |_| {})
    }

    fn resolve_with_refusal(
        &self,
        source: &PmDcPatternFeature,
        label: &PmDcFeatureLabel,
        mut seed_feature: impl FnMut(u32) -> Option<FeatureId>,
        mut refused: impl FnMut(Refusal),
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
        let replace_source = boolean(7)?;
        let replacement = replace_source
            .then(|| body_result::replacement(self, source, label))
            .flatten();
        if replace_source && (!bodies_mode || replacement.is_none()) {
            refused(Refusal::Slot7);
            return None;
        }
        if slot(3)?.index() != 0 || slot(if bodies_mode { 9 } else { 10 })?.index() != 0 {
            return None;
        }
        let has_instance_map = slot(8)?.index() != 0;
        if has_instance_map && !bodies_mode {
            refused(Refusal::InstanceMap);
            return None;
        }
        if !has_instance_map && !label.participants.references().is_empty() {
            return None;
        }
        let new_body = slot(11)?.index() != 0 && boolean(11)?;
        if new_body && !has_instance_map {
            refused(Refusal::Slot11);
            return None;
        }
        if has_instance_map && !new_body {
            refused(Refusal::InstanceMap);
            return None;
        }
        if !matches!(
            self.property(token, slot(2)?)?.kind,
            PmDcFeaturePropertyKind::RdxVariable {
                name_value: 0,
                nominal_value: 0,
                model_value: 0,
                ..
            }
        ) {
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
        let unit_normal = cadmpeg_ir::units::UnitVector3::normalized(u.cross(v))?;
        let normal = FeatureDirection3::from(unit_normal);
        let origin = plane.origin.map(cadmpeg_ir::scalar::FiniteReal::get);
        let origin = FinitePoint3::new(Point3::new(
            origin[0] * 10.0,
            origin[1] * 10.0,
            origin[2] * 10.0,
        ))?;
        // History drops this reference where the supplying work plane becomes
        // a dependency of the Mirror (`without_supplied_plane_reference`).
        let plane_reference = Some(cadmpeg_ir::features::FaceSelection::Native(plane.id()));
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
        if let Some(output) = replacement {
            let members = FeatureResultMembers::new(
                vec![NonBlankString::new(output.id())?],
                Vec::new(),
                Vec::new(),
                Vec::new(),
            )
            .ok()?;
            return Some(ResolvedMirror {
                definition: FeatureDefinition::Operation(FeatureOperation::MirrorShape {
                    source: self.body_source(token, collection, items.references())?,
                    plane_origin: origin,
                    plane_normal: unit_normal,
                    plane_reference,
                }),
                result_native: collection.id(),
                members,
            });
        }
        if new_body {
            if !source.participants.references().is_empty() {
                return None;
            }
            let Some(map) = self.property(token, slot(8)?) else {
                refused(Refusal::InstanceMap);
                return None;
            };
            let Some(output) = body_result::resolve(self, source, label, map, collection) else {
                refused(Refusal::InstanceMap);
                return None;
            };
            let members = FeatureResultMembers::new(
                vec![NonBlankString::new(output.id())?],
                Vec::new(),
                Vec::new(),
                Vec::new(),
            )
            .ok()?;
            return Some(ResolvedMirror {
                definition: FeatureDefinition::Operation(FeatureOperation::MirrorShape {
                    source: self.body_source(token, collection, items.references())?,
                    plane_origin: origin,
                    plane_normal: unit_normal,
                    plane_reference,
                }),
                result_native: map.id(),
                members,
            });
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
            vec![PatternSeed::Bodies(
                self.body_source(token, collection, items.references())
                    .unwrap_or_else(|| BodySelection::Native(collection.id())),
            )]
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
        refused: impl FnMut(Refusal),
    ) -> Option<Result<(Feature, FeatureResultTopology), CodecError>> {
        self.resolve_with_refusal(source, label, seed_feature, refused)
            .map(|resolved| {
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

    /// Whether `feature` and `result` are the projection of `source`;
    /// `plane_supplied` is whether the supplying work
    /// plane is a dependency of the Mirror.
    pub(crate) fn matches_projection(
        &self,
        source: &PmDcPatternFeature,
        label: &PmDcFeatureLabel,
        feature: &Feature,
        result: &FeatureResultTopology,
        plane_supplied: bool,
        seed_feature: impl FnMut(u32) -> Option<FeatureId>,
    ) -> bool {
        let Some(mut resolved) = self.resolve(source, label, seed_feature) else {
            return false;
        };
        if plane_supplied {
            without_supplied_plane_reference(&mut resolved.definition);
        }
        let Ok(feature_id) = history_feature_id(&source.identity) else {
            return false;
        };
        let Ok(key) = source.identity.key() else {
            return false;
        };
        if feature.id != feature_id
            || feature.native_ref.as_deref() != Some(source.id().as_str())
            || {
                let mut expected = resolved.definition.clone();
                if let Some(bodies) = self.bodies {
                    super::body_selection::definition(&mut expected, bodies, None);
                }
                feature.evaluation.definition() != &expected
            }
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

/// Drop the native plane reference of a Mirror whose slot-12 plane is the
/// supplying work plane, which the Mirror consumes as a dependency.
pub(crate) fn without_supplied_plane_reference(definition: &mut FeatureDefinition) {
    if let FeatureDefinition::Operation(FeatureOperation::MirrorShape {
        plane_reference, ..
    }) = definition
    {
        *plane_reference = None;
    }
}

/// History feature of the work plane that supplied the slot-12 plane of a
/// Mirror, when the work plane is a browser node before the Mirror.
pub(crate) fn supplying_feature(
    index: &MirrorIndex<'_>,
    history: &super::history::History,
    source: &PmDcPatternFeature,
) -> Option<FeatureId> {
    let token = source.identity.segment_token.as_str();
    let ordinal = supplying_plane(index, token, source)?;
    let supplier = index.planes.get(&(token, ordinal))?.as_ref()?;
    let before = history.placement(ordinal)?;
    let current = history.placement(source.identity.record_ordinal)?;
    (before.kind == super::history::HistoryNodeKind::WorkPlane && before.ordinal < current.ordinal)
        .then(|| history_feature_id(&supplier.identity).ok())
        .flatten()
}

/// Record ordinal of the work plane that supplied the slot-12 plane of a
/// projected Mirror.
pub(crate) fn supplying_plane(
    index: &MirrorIndex<'_>,
    token: &str,
    source: &PmDcPatternFeature,
) -> Option<u32> {
    if !is_mirror_record(source) {
        return None;
    }
    index.supplying_plane(token, source.property_slots.get(12)?.record_ordinal()?)
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
    refusals: &[Option<Refusal>],
) -> Vec<cadmpeg_ir::report::loss::LossNote> {
    let neutral = features.iter().filter(|feature| {
        matches!(feature.evaluation.definition(), FeatureDefinition::Operation(FeatureOperation::MirrorShape { .. }))
            || matches!(feature.evaluation.definition(), FeatureDefinition::Operation(FeatureOperation::Pattern { pattern, .. }) if matches!(pattern.definition(), PatternTransform::Mirror { .. }))
    }).filter_map(|feature| feature.native_ref.as_deref()).collect::<HashSet<_>>();
    patterns
        .iter()
        .enumerate()
        .filter(|(_, source)| is_mirror_record(source) && !neutral.contains(source.id().as_str()))
        .map(|(index, _)| {
            refusals
                .get(index)
                .copied()
                .flatten()
                .map_or_else(native_loss, Refusal::note)
        })
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
            [1, 4, 8].into_iter().filter_map(move |slot| {
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
    let planes = plane_operands(&inventory.pattern_features);
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
            let key = (segment.pair.token.key().as_str(), record.ordinal);
            let parsed = match record.type_id {
                SELECTION_TYPE if needed.contains(&key) => {
                    parse_selection(ctx, record.payload, version)
                }
                OPTIONS_TYPE if needed.contains(&key) => {
                    parse_options(ctx, record.payload, version)
                }
                instance_map::TYPE if needed.contains(&key) => {
                    instance_map::parse(ctx, record.payload, version)
                }
                plane_reference::TYPE => {
                    plane_reference::parse(ctx, record.payload, version).map(|parsed| {
                        parsed.filter(|property| {
                            matches!(
                                &property.kind,
                                PmDcFeaturePropertyKind::Mirror {
                                    definition: Property::PlaneReference { owner, .. },
                                } if owner
                                    .record_ordinal()
                                    .is_some_and(|plane| planes.contains(&(key.0, plane)))
                            )
                        })
                    })
                }
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

/// Slot-12 plane records of the mirrors, by segment token and ordinal.
fn plane_operands(patterns: &[PmDcPatternFeature]) -> HashSet<(&str, u32)> {
    patterns
        .iter()
        .filter(|source| is_mirror_record(source))
        .filter_map(|source| {
            Some((
                source.identity.segment_token.as_str(),
                source.property_slots.get(12)?.record_ordinal()?,
            ))
        })
        .collect()
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
        PmDcFeaturePropertyKind::Mirror {
            definition: Property::InstanceMap { .. },
        } if property.save_version_major == 19 => {
            crate::layout::pm_dc_mirror_instance_map_single_tail_v19::LEN
        }
        PmDcFeaturePropertyKind::Mirror {
            definition: Property::PlaneReference { .. },
        } if property.save_version_major == 19 => {
            crate::layout::pm_dc_mirror_plane_reference_tail_v19::LEN
        }
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
