// SPDX-License-Identifier: Apache-2.0
//! Feature-family, input-class, and history-record classifiers.

use crate::classification::{
    classify, classify_type_token, classify_xml_element, native_object_class,
    principal_plane_in_layout, principal_plane_layout, FeatureClass, NativeClassKind,
    PrincipalPlaneLayout,
};
use crate::history::literals::{named_literal, parse_dimension_length_mm};
use crate::records::{Feature, FeatureSource};
use cadmpeg_core::decode::{DecodeContext, ScopedReservation};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{BooleanOp, FeatureTreeNodeRole, PrincipalPlane};

pub(super) fn is_custom_property(feature: &Feature) -> bool {
    feature.xml_tag.eq_ignore_ascii_case("CustomProperty")
}

pub(super) fn is_semantic_note(feature: &Feature) -> bool {
    feature.xml_tag.eq_ignore_ascii_case("Note")
        && feature.kind.eq_ignore_ascii_case("Note")
        && feature.text.as_ref().is_some_and(|text| !text.is_empty())
        && feature.parameters.is_empty()
        && feature.properties.is_empty()
}

fn is_attribute_definition(feature: &Feature) -> bool {
    feature.input_class.is_none()
        && feature.source_id == Some(FeatureSource::Reserved)
        && feature.xml_tag.eq_ignore_ascii_case("Feature")
        && feature.kind.eq_ignore_ascii_case("Attribute-Definition")
        && !feature.name.is_empty()
}

/// A record that is history metadata by its own fields.
fn is_metadata_by_record(feature: &Feature) -> bool {
    is_custom_property(feature)
        || is_semantic_note(feature)
        || is_attribute_definition(feature)
        || matches!(
            feature.input_class.as_deref(),
            Some("moAlignGroup_c" | "moAttribute_c" | "moConfigCommentsFolder_c")
        )
}

/// A classless reserved record whose name an attribute record may extend.
fn may_name_attribute(feature: &Feature) -> bool {
    feature.input_class.is_none()
        && feature.source_id == Some(FeatureSource::Reserved)
        && !feature.name.is_empty()
}

fn is_attribute_record(feature: &Feature) -> bool {
    feature.input_class.as_deref() == Some("moAttribute_c")
}

const METADATA_OPERATION: &str = "classify SLDPRT history metadata";

/// Whether `feature` is history metadata among `features`.
///
/// This scans `features` for each record asked about; a caller asking about
/// many records of one history builds a [`HistoryIndex`] once instead.
pub(crate) fn is_history_metadata_record(
    ctx: &DecodeContext<'_>,
    feature: &Feature,
    features: &[Feature],
) -> Result<bool, CodecError> {
    if is_metadata_by_record(feature) {
        return Ok(true);
    }
    if !may_name_attribute(feature) {
        return Ok(false);
    }
    ctx.any_by(
        features,
        |candidate| {
            Ok(is_attribute_record(candidate)
                && ctx.starts_with(
                    candidate.name.as_str(),
                    feature.name.as_str(),
                    METADATA_OPERATION,
                )?)
        },
        METADATA_OPERATION,
    )
}

/// How many records of one history satisfy a condition, keeping the record
/// when there is exactly one.
#[derive(Debug, Clone, Copy, Default)]
enum Unique<'a> {
    #[default]
    Absent,
    One(&'a Feature),
    Many,
}

impl<'a> Unique<'a> {
    fn add(&mut self, feature: &'a Feature) {
        *self = match self {
            Self::Absent => Self::One(feature),
            Self::One(_) | Self::Many => Self::Many,
        };
    }

    fn one(self) -> Option<&'a Feature> {
        match self {
            Self::One(feature) => Some(feature),
            Self::Absent | Self::Many => None,
        }
    }
}

/// Native classes the built-in feature-manager rosters name.
const ROSTER_CLASSES: [&str; 7] = [
    "moOriginProfileFeature_c",
    "moSurfaceBodyFolder_c",
    "moSolidBodyFolder_c",
    "moDocsFolder_c",
    "moCommentsFolder_c",
    "moDetailCabinet_c",
    "moRefPlane_c",
];

/// The largest reserved source a built-in roster or node role names.
const LAST_BUILTIN_SOURCE: usize = 13;

/// The records of one history holding one reserved built-in source.
#[derive(Debug, Clone, Copy, Default)]
struct BuiltinSlot<'a> {
    /// Records of each [`ROSTER_CLASSES`] class.
    classes: [Unique<'a>; ROSTER_CLASSES.len()],
    /// Classless or scene-light records with no payload.
    classless_or_scene: Unique<'a>,
    /// Classless records with no payload.
    classless: Unique<'a>,
    /// The last record, as a source-keyed map of the history keeps it.
    last: Option<&'a Feature>,
}

/// Per-history facts that classifying one record of the history consults,
/// computed once for all of its records.
pub(crate) struct HistoryIndex<'a, 'ctx> {
    /// Names of the history's attribute records, sorted for prefix search.
    attribute_names: Vec<&'a str>,
    _attribute_storage: ScopedReservation<'ctx>,
    /// Built-in records by reserved source; index 0 is unused.
    builtins: [BuiltinSlot<'a>; LAST_BUILTIN_SOURCE + 1],
    layout: Option<FeatureManagerLayout>,
    principal_layout: PrincipalPlaneLayout,
    /// Classless front, top and right planes at sources 2, 3 and 4.
    reserved_planes: Option<[&'a Feature; 3]>,
    /// The one identity-free plane triplet followed by its origin record.
    unnumbered_planes: Option<[&'a Feature; 3]>,
}

impl<'a, 'ctx> HistoryIndex<'a, 'ctx> {
    /// Index one history's records in one pass plus a sort of its attribute names.
    pub(crate) fn new(
        ctx: &'ctx DecodeContext<'_>,
        features: &'a [Feature],
    ) -> Result<Self, CodecError> {
        const OPERATION: &str = "index SLDPRT history records";
        let mut attribute_storage = ctx.reserve_scoped(0, OPERATION)?;
        let mut attribute_names = Vec::new();
        let mut builtins = [BuiltinSlot::default(); LAST_BUILTIN_SOURCE + 1];
        for feature in ctx.admit_iter(features, OPERATION)? {
            if is_attribute_record(feature) {
                ctx.push_scoped_vec(
                    &mut attribute_storage,
                    &mut attribute_names,
                    feature.name.as_str(),
                    OPERATION,
                )?;
            }
            let Some(slot) = feature
                .source_value()
                .and_then(|source| usize::try_from(source).ok())
                .filter(|source| *source != 0)
                .and_then(|source| builtins.get_mut(source))
            else {
                continue;
            };
            slot.last = Some(feature);
            if let Some(class) = feature.input_class.as_deref() {
                if let Some(index) = ROSTER_CLASSES.iter().position(|known| *known == class) {
                    slot.classes[index].add(feature);
                }
            }
            if classless_or_scene_builtin_node(feature) {
                slot.classless_or_scene.add(feature);
            }
            if classless_builtin_node(feature) {
                slot.classless.add(feature);
            }
        }
        ctx.sort_unstable_by(&mut attribute_names, |name| *name, Ord::cmp, OPERATION)?;
        let mut index = Self {
            attribute_names,
            _attribute_storage: attribute_storage,
            builtins,
            layout: None,
            principal_layout: principal_plane_layout(ctx, features)?,
            reserved_planes: None,
            unnumbered_planes: unnumbered_principal_planes(ctx, features)?,
        };
        index.layout = index.feature_manager_layout();
        index.reserved_planes = index.reserved_principal_planes(ctx)?;
        Ok(index)
    }

    /// Whether `feature`, a record of this history, is history metadata.
    pub(crate) fn is_metadata(
        &self,
        ctx: &DecodeContext<'_>,
        feature: &Feature,
    ) -> Result<bool, CodecError> {
        if is_metadata_by_record(feature) {
            return Ok(true);
        }
        if !may_name_attribute(feature) {
            return Ok(false);
        }
        // The names starting with a prefix follow the first name not below it.
        let name = feature.name.as_str();
        let first = ctx.partition_point(
            &self.attribute_names,
            |candidate| {
                Ok(ctx.compare(*candidate, name, METADATA_OPERATION)? == std::cmp::Ordering::Less)
            },
            METADATA_OPERATION,
        )?;
        match self.attribute_names.get(first) {
            Some(candidate) => ctx.starts_with(*candidate, name, METADATA_OPERATION),
            None => Ok(false),
        }
    }

    fn slot(&self, source: u32) -> Option<&BuiltinSlot<'a>> {
        usize::try_from(source)
            .ok()
            .filter(|source| *source != 0)
            .and_then(|source| self.builtins.get(source))
    }

    /// Exactly one record at each source holds the named class.
    fn matches_roster(&self, roster: &[(u32, &str)]) -> bool {
        roster.iter().all(|(source, class)| {
            ROSTER_CLASSES
                .iter()
                .position(|known| known == class)
                .zip(self.slot(*source))
                .is_some_and(|(class, slot)| slot.classes[class].one().is_some())
        })
    }

    /// Exactly one classless or scene-light built-in record holds each source.
    fn matches_builtin_sources(&self, sources: &[u32]) -> bool {
        sources.iter().all(|source| {
            self.slot(*source)
                .is_some_and(|slot| slot.classless_or_scene.one().is_some())
        })
    }

    fn feature_manager_layout(&self) -> Option<FeatureManagerLayout> {
        let legacy = self.matches_roster(&[
            (6, "moOriginProfileFeature_c"),
            (9, "moSurfaceBodyFolder_c"),
            (10, "moSolidBodyFolder_c"),
            (12, "moDocsFolder_c"),
            (13, "moCommentsFolder_c"),
        ]);
        let current = self.matches_roster(&[
            (7, "moDocsFolder_c"),
            (8, "moCommentsFolder_c"),
            (9, "moSolidBodyFolder_c"),
            (10, "moSurfaceBodyFolder_c"),
        ]);
        let default_frame = self.matches_roster(&[
            (1, "moDetailCabinet_c"),
            (2, "moRefPlane_c"),
            (3, "moRefPlane_c"),
            (4, "moRefPlane_c"),
            (5, "moOriginProfileFeature_c"),
        ]);
        let origin_at_six = self.matches_roster(&[
            (1, "moDetailCabinet_c"),
            (3, "moRefPlane_c"),
            (4, "moRefPlane_c"),
            (5, "moRefPlane_c"),
            (6, "moOriginProfileFeature_c"),
        ]) && self.matches_builtin_sources(&[2, 7, 8])
            && !legacy;
        let lights_at_six = default_frame && self.matches_builtin_sources(&[6, 7, 8]);
        let folders_at_seven = default_frame
            && self.matches_roster(&[(7, "moSolidBodyFolder_c"), (8, "moSurfaceBodyFolder_c")])
            && self.matches_builtin_sources(&[6, 10, 11, 12]);
        let mut layouts = [
            (origin_at_six, FeatureManagerLayout::OriginAtSix),
            (lights_at_six, FeatureManagerLayout::LightsAtSix),
            (folders_at_seven, FeatureManagerLayout::FoldersAtSeven),
            (legacy, FeatureManagerLayout::Legacy),
            (current, FeatureManagerLayout::Current),
        ]
        .into_iter()
        .filter_map(|(matches, layout)| matches.then_some(layout));
        let layout = layouts.next()?;
        layouts.next().is_none().then_some(layout)
    }

    /// Whether `feature` repeats the kind of the one classless node holding the
    /// reserved source of `role` in `layout`.
    fn repeats_builtin_node_kind(
        &self,
        ctx: &DecodeContext<'_>,
        feature: &Feature,
        layout: FeatureManagerLayout,
        role: FeatureTreeNodeRole,
    ) -> Result<bool, CodecError> {
        let reserved_source = match (layout, role) {
            (
                FeatureManagerLayout::OriginAtSix | FeatureManagerLayout::LightsAtSix,
                FeatureTreeNodeRole::AmbientLight,
            ) => 7,
            (
                FeatureManagerLayout::OriginAtSix | FeatureManagerLayout::LightsAtSix,
                FeatureTreeNodeRole::DirectionalLight,
            ) => 8,
            (FeatureManagerLayout::FoldersAtSeven, FeatureTreeNodeRole::AmbientLight) => 10,
            (FeatureManagerLayout::FoldersAtSeven, FeatureTreeNodeRole::DirectionalLight) => 11,
            (FeatureManagerLayout::Legacy, FeatureTreeNodeRole::AmbientLight) => 7,
            (FeatureManagerLayout::Legacy, FeatureTreeNodeRole::DirectionalLight) => 8,
            (FeatureManagerLayout::Current, FeatureTreeNodeRole::AmbientLight) => 12,
            (FeatureManagerLayout::Current, FeatureTreeNodeRole::DirectionalLight) => 13,
            _ => return Ok(false),
        };
        let Some(anchor) = self
            .slot(reserved_source)
            .and_then(|slot| slot.classless.one())
        else {
            return Ok(false);
        };
        Ok(!anchor.kind.is_empty()
            && ctx.equal(
                feature.kind.as_str(),
                anchor.kind.as_str(),
                "compare SLDPRT built-in node kinds",
            )?)
    }

    fn reserved_tree_node_role(
        &self,
        ctx: &DecodeContext<'_>,
        feature: &Feature,
    ) -> Result<Option<FeatureTreeNodeRole>, CodecError> {
        let Some(layout) = self.layout else {
            return Ok(None);
        };
        if !classless_builtin_node(feature) {
            return Ok(None);
        }
        let Some(source) = feature.source_id else {
            return Ok(None);
        };
        let source = source.value();
        Ok(match (layout, feature.xml_tag.as_str(), source) {
            (FeatureManagerLayout::Current, tag, Some(1))
                if tag.eq_ignore_ascii_case("Feature") =>
            {
                Some(FeatureTreeNodeRole::Annotations)
            }
            (FeatureManagerLayout::Current, tag, Some(5)) if tag.eq_ignore_ascii_case("Sketch") => {
                Some(FeatureTreeNodeRole::ModelOrigin)
            }
            (FeatureManagerLayout::Current, tag, Some(6))
                if tag.eq_ignore_ascii_case("Feature") =>
            {
                Some(FeatureTreeNodeRole::LightsAndCameras)
            }
            (FeatureManagerLayout::Current, tag, Some(12))
                if tag.eq_ignore_ascii_case("Feature") =>
            {
                Some(FeatureTreeNodeRole::AmbientLight)
            }
            (FeatureManagerLayout::Current, tag, Some(13..=15))
                if tag.eq_ignore_ascii_case("Feature") =>
            {
                Some(FeatureTreeNodeRole::DirectionalLight)
            }
            (FeatureManagerLayout::Legacy, tag, Some(2)) if tag.eq_ignore_ascii_case("Feature") => {
                Some(FeatureTreeNodeRole::LightsAndCameras)
            }
            (FeatureManagerLayout::Legacy, tag, Some(7)) if tag.eq_ignore_ascii_case("Feature") => {
                Some(FeatureTreeNodeRole::AmbientLight)
            }
            (FeatureManagerLayout::Legacy, tag, Some(8)) if tag.eq_ignore_ascii_case("Feature") => {
                Some(FeatureTreeNodeRole::DirectionalLight)
            }
            (
                FeatureManagerLayout::LightsAtSix | FeatureManagerLayout::FoldersAtSeven,
                tag,
                Some(6),
            ) if tag.eq_ignore_ascii_case("Feature") => Some(FeatureTreeNodeRole::LightsAndCameras),
            (FeatureManagerLayout::LightsAtSix, tag, Some(7))
                if tag.eq_ignore_ascii_case("Feature") =>
            {
                Some(FeatureTreeNodeRole::AmbientLight)
            }
            (FeatureManagerLayout::LightsAtSix, tag, Some(8))
                if tag.eq_ignore_ascii_case("Feature") =>
            {
                Some(FeatureTreeNodeRole::DirectionalLight)
            }
            (FeatureManagerLayout::FoldersAtSeven, tag, Some(10))
                if tag.eq_ignore_ascii_case("Feature") =>
            {
                Some(FeatureTreeNodeRole::AmbientLight)
            }
            (FeatureManagerLayout::FoldersAtSeven, tag, Some(11 | 12))
                if tag.eq_ignore_ascii_case("Feature") =>
            {
                Some(FeatureTreeNodeRole::DirectionalLight)
            }
            (FeatureManagerLayout::OriginAtSix, tag, Some(2))
                if tag.eq_ignore_ascii_case("Feature") =>
            {
                Some(FeatureTreeNodeRole::LightsAndCameras)
            }
            (FeatureManagerLayout::OriginAtSix, tag, Some(7))
                if tag.eq_ignore_ascii_case("Feature") =>
            {
                Some(FeatureTreeNodeRole::AmbientLight)
            }
            (FeatureManagerLayout::OriginAtSix, tag, Some(8))
                if tag.eq_ignore_ascii_case("Feature") =>
            {
                Some(FeatureTreeNodeRole::DirectionalLight)
            }
            (_, tag, _)
                if tag.eq_ignore_ascii_case("Feature")
                    && self.repeats_builtin_node_kind(
                        ctx,
                        feature,
                        layout,
                        FeatureTreeNodeRole::AmbientLight,
                    )? =>
            {
                Some(FeatureTreeNodeRole::AmbientLight)
            }
            (_, tag, _)
                if tag.eq_ignore_ascii_case("Feature")
                    && self.repeats_builtin_node_kind(
                        ctx,
                        feature,
                        layout,
                        FeatureTreeNodeRole::DirectionalLight,
                    )? =>
            {
                Some(FeatureTreeNodeRole::DirectionalLight)
            }
            (_, tag, None) if tag.eq_ignore_ascii_case("Feature") => {
                Some(FeatureTreeNodeRole::SheetMetal)
            }
            (_, _, _) if empty_feature_tree_node(feature) => {
                Some(FeatureTreeNodeRole::ExplodedViews)
            }
            _ => None,
        })
    }

    /// The feature-tree role of `feature`, a record of this history.
    pub(crate) fn tree_node_role(
        &self,
        ctx: &DecodeContext<'_>,
        feature: &Feature,
    ) -> Result<Option<FeatureTreeNodeRole>, CodecError> {
        Ok(self
            .reserved_tree_node_role(ctx, feature)?
            .or_else(|| native_object_class(feature.input_class.as_deref()?).tree_node())
            .or_else(|| equation_container_role(feature)))
    }

    /// Classless planes of one kind at sources 2, 3 and 4.
    fn reserved_principal_planes(
        &self,
        ctx: &DecodeContext<'_>,
    ) -> Result<Option<[&'a Feature; 3]>, CodecError> {
        const OPERATION: &str = "compare SLDPRT reserved plane kinds";
        let legacy_shape = |record: &Feature| {
            record.input_class.is_none()
                && record.xml_tag.eq_ignore_ascii_case("Feature")
                && record.parameters.is_empty()
                && record.properties.is_empty()
                && !record.kind.is_empty()
        };
        let planes = [2, 3, 4].map(|source| self.slot(source).and_then(|slot| slot.last));
        let [Some(front), Some(top), Some(right)] = planes else {
            return Ok(None);
        };
        Ok((legacy_shape(front)
            && legacy_shape(top)
            && legacy_shape(right)
            && ctx.equal(front.kind.as_str(), top.kind.as_str(), OPERATION)?
            && ctx.equal(front.kind.as_str(), right.kind.as_str(), OPERATION)?)
        .then_some([front, top, right]))
    }

    /// The principal plane `feature`, a record of this history, represents.
    pub(crate) fn principal_plane(
        &self,
        ctx: &DecodeContext<'_>,
        feature: &Feature,
    ) -> Result<Option<PrincipalPlane>, CodecError> {
        const OPERATION: &str = "match SLDPRT principal plane records";
        if let Some(plane) = principal_plane_in_layout(self.principal_layout, feature) {
            return Ok(Some(plane));
        }
        if self.reserved_planes.is_some() {
            return Ok(match feature.source_value() {
                Some(2) => Some(PrincipalPlane::Front),
                Some(3) => Some(PrincipalPlane::Top),
                Some(4) => Some(PrincipalPlane::Right),
                _ => None,
            });
        }
        let Some([front, top, right]) = self.unnumbered_planes else {
            return Ok(None);
        };
        for (record, plane) in [
            (front, PrincipalPlane::Front),
            (top, PrincipalPlane::Top),
            (right, PrincipalPlane::Right),
        ] {
            if ctx.equal(feature.id.as_str(), record.id.as_str(), OPERATION)? {
                return Ok(Some(plane));
            }
        }
        Ok(None)
    }
}

/// The one run of three identity-free reference planes of one kind followed
/// by their origin record, in consecutive ordinals.
fn unnumbered_principal_planes<'a>(
    ctx: &DecodeContext<'_>,
    features: &'a [Feature],
) -> Result<Option<[&'a Feature; 3]>, CodecError> {
    const OPERATION: &str = "find SLDPRT unnumbered principal planes";
    let plane_shape = |record: &Feature| {
        record.xml_tag.eq_ignore_ascii_case("Feature")
            && record.parameters.is_empty()
            && !record.kind.is_empty()
            && match record.input_class.as_deref() {
                Some(class) => native_object_class(class) == NativeClassKind::ReferencePlane,
                None => record.properties.is_empty(),
            }
            && record.source_id.is_none()
            && record.tree_parent.is_none()
    };
    let follows =
        |before: &Feature, after: &Feature| before.ordinal.checked_add(1) == Some(after.ordinal);
    let mut records = features.windows(4);
    let candidate = |records: &'a [Feature]| -> Result<Option<[&'a Feature; 3]>, CodecError> {
        let [front, top, right, successor] = records else {
            return Ok(None);
        };
        if !(plane_shape(front)
            && plane_shape(top)
            && plane_shape(right)
            && follows(front, top)
            && follows(top, right)
            && follows(right, successor)
            && successor.xml_tag.eq_ignore_ascii_case("Feature")
            && successor.parameters.is_empty()
            && successor.properties.is_empty()
            && !successor.kind.is_empty()
            && successor.input_class.as_deref().is_none_or(|class| {
                native_object_class(class) == NativeClassKind::OriginProfileFeature
            })
            && successor.source_id.is_none()
            && successor.tree_parent.is_none())
        {
            return Ok(None);
        }
        if !ctx.equal(front.kind.as_str(), top.kind.as_str(), OPERATION)?
            || !ctx.equal(front.kind.as_str(), right.kind.as_str(), OPERATION)?
            || ctx.equal(successor.kind.as_str(), front.kind.as_str(), OPERATION)?
        {
            return Ok(None);
        }
        Ok(Some([front, top, right]))
    };
    let found = ctx.find_map(&mut records, candidate, OPERATION)?;
    if found.is_some() && ctx.find_map(records, candidate, OPERATION)?.is_some() {
        return Ok(None);
    }
    Ok(found)
}

/// The feature-tree role of `feature` in the history `index` describes.
pub(super) fn feature_tree_node_role(
    ctx: &DecodeContext<'_>,
    feature: &Feature,
    index: &HistoryIndex<'_, '_>,
) -> Result<Option<FeatureTreeNodeRole>, CodecError> {
    index.tree_node_role(ctx, feature)
}

/// Keywords operation-family token of the equations container.
pub(super) const EQUATION_DRIVEN_TOKEN: &str = "EquationDriven";

/// The equations container identified by its Keywords operation-family token.
///
/// The token is a role code: it identifies the container without a native class
/// or a reserved source identifier.
fn equation_container_role(feature: &Feature) -> Option<FeatureTreeNodeRole> {
    (feature.input_class.is_none()
        && feature.xml_tag.eq_ignore_ascii_case("Feature")
        && feature.kind.eq_ignore_ascii_case(EQUATION_DRIVEN_TOKEN))
    .then_some(FeatureTreeNodeRole::Equations)
}

pub(super) fn classless_builtin_node(feature: &Feature) -> bool {
    feature.input_class.is_none() && builtin_node_payload(feature)
}

fn builtin_node_payload(feature: &Feature) -> bool {
    feature.parameters.is_empty()
        && feature.dimension_properties.is_empty()
        && feature.properties.is_empty()
        && feature.text.is_none()
        && feature.content.is_empty()
}

fn classless_or_scene_builtin_node(feature: &Feature) -> bool {
    builtin_node_payload(feature)
        && feature.input_class.as_deref().is_none_or(|class| {
            matches!(
                native_object_class(class).tree_node(),
                Some(
                    FeatureTreeNodeRole::AmbientLight
                        | FeatureTreeNodeRole::DirectionalLight
                        | FeatureTreeNodeRole::PointLight
                        | FeatureTreeNodeRole::SpotLight
                )
            )
        })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FeatureManagerLayout {
    OriginAtSix,
    LightsAtSix,
    FoldersAtSeven,
    Legacy,
    Current,
}

fn empty_feature_tree_node(feature: &Feature) -> bool {
    feature.xml_tag.eq_ignore_ascii_case("Feature")
        && feature.name.is_empty()
        && feature.dimension_properties.is_empty()
        && feature.text.is_none()
        && feature.content.is_empty()
}

/// Whether a feature belongs to a family named by a literal tag or type token.
pub(super) fn feature_family(feature: &Feature, family: &'static str) -> bool {
    feature.xml_tag.eq_ignore_ascii_case(family)
        || feature.kind.eq_ignore_ascii_case(family)
        || classify_type_token(family)
            .or_else(|| classify_xml_element(family))
            .is_some_and(|expected| classify(feature) == Some(expected))
}

pub(super) fn feature_input_class(feature: &Feature, class: NativeClassKind) -> bool {
    feature.input_class.as_deref().map(native_object_class) == Some(class)
}

pub(super) fn is_fillet(feature: &Feature) -> bool {
    classify(feature) == Some(FeatureClass::Fillet)
}

pub(super) fn is_chamfer(feature: &Feature) -> bool {
    classify(feature) == Some(FeatureClass::Chamfer)
}

pub(super) fn is_extrude(feature: &Feature) -> bool {
    classify(feature) == Some(FeatureClass::Extrude)
}

/// The boolean operation of an extrusion record. The token test reads every
/// byte of `feature.kind`; a decode caller admits it first.
pub(super) fn extrude_feature_op(feature: &Feature) -> Option<BooleanOp> {
    // DI-58: the native cut class is authoritative over the localized
    // Keywords type token. A localized BossExtrude token can remain on a
    // feature whose feature-input object is the cut class.
    (feature.input_class.as_deref() == Some("moCut_c"))
        .then_some(BooleanOp::Cut)
        .or_else(|| extrude_op(&feature.kind))
}

/// Whether a retained record is a reference plane whose `D1` distance parses,
/// for the writer.
pub(super) fn is_offset_plane_record(feature: &Feature) -> bool {
    classify(feature) == Some(FeatureClass::ReferencePlane)
        && feature
            .parameters
            .get("D1")
            .and_then(|distance| parse_dimension_length_mm(distance))
            .is_some()
}

/// Whether `feature` is a reference plane whose `D1` distance parses.
pub(super) fn is_offset_plane(
    ctx: &DecodeContext<'_>,
    feature: &Feature,
) -> Result<bool, CodecError> {
    if classify(feature) != Some(FeatureClass::ReferencePlane) {
        return Ok(false);
    }
    Ok(named_literal(
        ctx,
        &feature.parameters,
        "D1",
        "parse SLDPRT offset plane distance",
    )?
    .and_then(parse_dimension_length_mm)
    .is_some())
}

/// The principal plane `feature` represents in the history `index` describes.
pub(super) fn principal_plane_in_history(
    ctx: &DecodeContext<'_>,
    feature: &Feature,
    index: &HistoryIndex<'_, '_>,
) -> Result<Option<cadmpeg_ir::features::PrincipalPlane>, CodecError> {
    index.principal_plane(ctx, feature)
}

/// The boolean operation an extrusion type token names.
/// A decode caller admits the text before normalization.
pub(super) fn extrude_op(kind: &str) -> Option<BooleanOp> {
    if matches_alnum_ascii(kind, b"bossextrude") {
        Some(BooleanOp::Join)
    } else if matches_alnum_ascii(kind, b"cutextrude")
        || matches_alnum_ascii(kind, b"cutextrudethin")
    {
        Some(BooleanOp::Cut)
    } else {
        None
    }
}

/// Whether the ASCII letters and digits of `value`, lowercased, spell `expected`.
///
/// A decode caller admits the text before normalization.
pub(crate) fn matches_alnum_ascii(value: &str, expected: &[u8]) -> bool {
    value
        .bytes()
        .filter(u8::is_ascii_alphanumeric)
        .map(|byte| byte.to_ascii_lowercase())
        .eq(expected.iter().copied())
}

pub(super) fn loft_op(kind: &str) -> Option<BooleanOp> {
    if ["bossloft", "boundaryboss"]
        .iter()
        .any(|name| kind.eq_ignore_ascii_case(name))
    {
        Some(BooleanOp::Join)
    } else if ["cutloft", "boundarycut"]
        .iter()
        .any(|name| kind.eq_ignore_ascii_case(name))
    {
        Some(BooleanOp::Cut)
    } else {
        None
    }
}

/// Whether `name` is `prefix` followed by a non-empty run of ASCII digits.
///
/// A decode caller admits the text before checking its digit suffix.
pub(super) fn indexed_name(name: &str, prefix: &str) -> bool {
    name.strip_prefix(prefix).is_some_and(|suffix| {
        !suffix.is_empty() && suffix.bytes().all(|byte| byte.is_ascii_digit())
    })
}

#[cfg(test)]
mod tests {
    use super::unnumbered_principal_planes;
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    #[test]
    fn principal_plane_ambiguity_leaves_unvisited_suffix_unpaid() {
        let mut features = Vec::new();
        for ordinal in 0..8 {
            let mut feature = crate::history::tests::feature("plane", None, ordinal);
            feature.kind = if ordinal % 4 == 3 { "Origin" } else { "Plane" }.into();
            features.push(feature);
        }
        for ordinal in 8..4104 {
            features.push(crate::history::tests::feature("tail", None, ordinal));
        }
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 128;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        assert!(unnumbered_principal_planes(&ctx, &features).unwrap().is_none());
        assert!(ctx.resource_refusal().is_none());
    }
}
