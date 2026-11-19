// SPDX-License-Identifier: Apache-2.0
//! Contour flanges: one open-profile sheet whose plate, bend, and corner
//! children project as a single operation.

use std::collections::{BTreeMap, HashSet};

use cadmpeg_core::decode::{unique_index, DecodeContext};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{
    sheet_metal::{SheetMetalContourFlangeConstruction, SheetMetalContourThicknessSide},
    DistinctMembers, ExtrudeExtent, ExtrudeSide, Feature, FeatureContent, FeatureDefinition,
    FeatureDirection3, FeatureEvaluation, FeatureId, FeatureOperation, FeatureResultTopology,
    LinearTermination, PlanarProfileRef,
};
use cadmpeg_ir::scalar::{Length, NonNegativeLength, NonZeroLength, PositiveLength};
use cadmpeg_ir::units::UnitVector3;

use super::flange::{Flange, FlangeKind};
use super::operands::{self, boolean, enumeration, same, sheet, PlateForm};
use super::projection::{self, Members};
use super::refusal::{require, PlateRefusal, Refuse};
use super::style::Style;
use super::SheetMetalProperty;
use crate::feature::{
    length_reference, resolve_property, FeatureInventory, PmDcFeature, PmDcFeatureEnumFamily,
    PmDcFeatureLabel, PmDcFeatureProperty, PmDcFeaturePropertyKind, PmDcFeatureReferenceFamily,
    ProjectionIndex,
};
use crate::pmdc::PmDcReference;

mod attachment;

/// Largest deviation of the travel direction from the profile-plane normal.
const EPS_TRAVEL_NORMAL: f64 = 1.0e-10;
/// Millimetres in one internal centimetre.
const INTERNAL_CM_TO_MM: f64 = 10.0;

/// Plate slots that the contour form requires null.
const RESERVED_SLOTS: [usize; 3] = [5, 6, 14];
/// Plate slots and the bend slots that repeat them.
const BEND_SHARED: [(usize, usize); 5] = [(7, 0), (4, 3), (10, 4), (11, 7), (15, 9)];
/// Plate slots and the corner slots that repeat them.
const CORNER_SHARED: [(usize, usize); 4] = [(9, 0), (12, 3), (0, 4), (15, 6)];

/// The first admission check that a contour flange fails.
///
/// The variants appear in the order the projection evaluates them. The kebab
/// string of each variant is the pinned `detail` of the loss note.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) enum ContourRefusal {
    /// The label participants and children are not one planar sketch and the
    /// plate, bend, and corner records in that order.
    Members,
    /// The participant sketch has no neutral placement.
    ProfileSketch,
    /// The plate has no unique sheet-metal style record in its segment.
    Style,
    /// The plate does not have the 17-property form.
    PropertyCount,
    /// The style and the plate come from different save-version majors.
    StyleVersion,
    /// A reserved operand or an unresolved attachment list blocks projection.
    ReservedSlot,
    /// Slot 12 selects this many input edges.
    EdgeAttached(usize),
    /// Slot 16 is not the new-body part operation `(5, 1)`.
    PartOperation,
    /// Slot 0 is not the plate profile selection with value 0.
    ProfileSelection,
    /// Slot 7 is not an empty edge collection.
    EdgeCollection,
    /// Slot 8 is not a feature-dimension list.
    FeatureDimensions,
    /// Slot 9 is not a well-formed corner control.
    CornerControl,
    /// Slot 13 is not the plate scalar 0.
    PlateControl,
    /// Slot 4 is not a contour attachment control.
    AttachmentControl,
    /// An attachment Boolean is not false.
    AttachmentBoolean,
    /// The attachment modes are not the enumerations `(9, 1)` and `(9, 4)`.
    AttachmentMode,
    /// The attachment state bytes are not `[1, 1, 1]` or the final byte is not 0.
    AttachmentStates,
    /// The extension is not the state-10 record that repeats the attachment operands.
    AttachmentExtension,
    /// The first attachment direction is not slot 1, or the second is not a
    /// stored direction.
    AttachmentDirection,
    /// Slot 2 is not the plate-side enumeration `(2, 0)`.
    Side,
    /// Slot 1 is not a stored direction.
    Direction,
    /// Slot 1 is not along the profile-plane normal.
    TravelDirection,
    /// Slot 10 is not a relief record that repeats the attachment operands.
    Relief,
    /// Slot 11 is not a transition record with the style form.
    Transition,
    /// A relief or transition length does not resolve.
    Length,
    /// Slot 3 is not a positive travel length.
    Distance,
    /// The attachment thickness is not a positive length.
    Thickness,
    /// The attachment radius is not a nonnegative length.
    Radius,
    /// The bend does not repeat the plate operands.
    Bend,
    /// The corner does not repeat the plate operands.
    Corner,
    /// The named edges of the slot-0 wire do not select an open chain of
    /// curves of the participant sketch.
    Profile,
    /// The slot-15 result collection does not resolve to one new body.
    Result,
}

impl ContourRefusal {
    /// The pinned kebab string of this refusal.
    pub(crate) const fn detail(self) -> &'static str {
        match self {
            Self::Members => "members",
            Self::ProfileSketch => "profile-sketch",
            Self::Style => "style",
            Self::PropertyCount => "property-count",
            Self::StyleVersion => "style-version",
            Self::ReservedSlot => "reserved-slot",
            Self::EdgeAttached(_) => "edge-attached",
            Self::PartOperation => "part-operation",
            Self::ProfileSelection => "profile-selection",
            Self::EdgeCollection => "edge-collection",
            Self::FeatureDimensions => "feature-dimensions",
            Self::CornerControl => "corner-control",
            Self::PlateControl => "plate-control",
            Self::AttachmentControl => "attachment-control",
            Self::AttachmentBoolean => "attachment-boolean",
            Self::AttachmentMode => "attachment-mode",
            Self::AttachmentStates => "attachment-states",
            Self::AttachmentExtension => "attachment-extension",
            Self::AttachmentDirection => "attachment-direction",
            Self::Side => "side",
            Self::Direction => "direction",
            Self::TravelDirection => "travel-direction",
            Self::Relief => "relief",
            Self::Transition => "transition",
            Self::Length => "length",
            Self::Distance => "distance",
            Self::Thickness => "thickness",
            Self::Radius => "radius",
            Self::Bend => "bend",
            Self::Corner => "corner",
            Self::Profile => "profile",
            Self::Result => "result",
        }
    }

    /// The contour refusal for a failed shared relief or transition check.
    const fn from_operand(refusal: PlateRefusal) -> Self {
        match refusal {
            PlateRefusal::Relief => Self::Relief,
            PlateRefusal::Transition => Self::Transition,
            _ => Self::Length,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct AttachmentEdges {
    pub(crate) selection: PmDcReference,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_attachment_state"
    )]
    pub(crate) state: Option<crate::feature::history_states::StateRef>,
    pub(crate) edges: Vec<AttachmentEdge>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct AttachmentEdge {
    pub(crate) item: PmDcReference,
    pub(crate) selector_source: u32,
    pub(crate) entity_index: i64,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_attachment_revision"
    )]
    pub(crate) revision: Option<i64>,
}

// Each optional key below names itself in whatever it refuses.
cadmpeg_core::named_optional_field!(
    deserialize_attachment_state,
    crate::feature::history_states::StateRef,
    "state"
);
cadmpeg_core::named_optional_field!(deserialize_attachment_revision, i64, "revision");

fn attachment_edges<'a>(
    plate: &PmDcFeature,
    property: impl Fn(PmDcReference) -> Option<&'a PmDcFeatureProperty>,
    resolve: impl Fn(PmDcReference, u32) -> Option<i64>,
) -> Option<AttachmentEdges> {
    let selection = *plate.properties.references().get(12)?;
    let PmDcFeaturePropertyKind::References {
        family: PmDcFeatureReferenceFamily::SheetMetalAttachmentSelections,
        items,
    } = &property(selection)?.kind
    else {
        return None;
    };
    let mut edges = Vec::new();
    let mut sources = HashSet::new();
    let mut owners = HashSet::new();
    for wrapper in items.references() {
        let SheetMetalProperty::EdgeSelection { edges: collection } = sheet(property(*wrapper)?)?
        else {
            return None;
        };
        let PmDcFeaturePropertyKind::References {
            family: PmDcFeatureReferenceFamily::EdgeCollection,
            items,
        } = &property(*collection)?.kind
        else {
            return None;
        };
        if items.references().is_empty() {
            return None;
        }
        for item in items.references() {
            let PmDcFeaturePropertyKind::EdgeItem {
                index_references,
                index_reference_value: -1,
                value: 0,
            } = &property(*item)?.kind
            else {
                return None;
            };
            if index_references.values().is_empty() {
                return None;
            }
            for source in index_references.values() {
                let owner = resolve(*item, *source)?;
                if *source == 0 || owner < 0 || !sources.insert(*source) || !owners.insert(owner) {
                    return None;
                }
                edges.push(AttachmentEdge {
                    item: *item,
                    selector_source: *source,
                    entity_index: owner,
                    revision: None,
                });
            }
        }
    }
    (!edges.is_empty()).then_some(AttachmentEdges {
        selection,
        state: None,
        edges,
    })
}

pub(in crate::feature) fn retain_attachments(
    ctx: &DecodeContext<'_>,
    inventory: &mut FeatureInventory,
    resolved: &std::collections::HashMap<(String, u32), AttachmentEdges>,
) -> Result<(), CodecError> {
    for flange in &mut inventory.sheet_metal_flanges {
        if FlangeKind::of(flange) != Some(FlangeKind::Contour) {
            continue;
        }
        let attachment = flange
            .children
            .references()
            .first()
            .and_then(|reference| reference.record_ordinal())
            .and_then(|ordinal| {
                resolved.get(&(flange.identity.segment_token.as_str().to_owned(), ordinal))
            })
            .cloned();
        if let Some(edges) = &attachment {
            ctx.charge_entities(
                cadmpeg_core::decode::u64_from_index(edges.edges.len()),
                "retain Inventor contour attachment edges",
            )?;
        }
        *flange = flange.with_attachment_edges(attachment);
    }
    Ok(())
}

pub(in crate::feature) fn read_attachments(
    ctx: &DecodeContext<'_>,
    document: &crate::rse::RseInventory<'_>,
    inventory: &mut FeatureInventory,
) -> Result<(), CodecError> {
    let resolved = attachment::read(ctx, document, inventory)?;
    retain_attachments(ctx, inventory, &resolved)
}

/// The construction scalars of a contour flange.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ContourOperands {
    /// Travel direction, plate slot 1, in model coordinates.
    pub(crate) direction: UnitVector3,
    /// Travel length, plate slot 3.
    pub(crate) distance: PositiveLength,
    /// Sheet thickness, the attachment thickness.
    pub(crate) thickness: PositiveLength,
    /// Inside bend radius, the attachment radius.
    pub(crate) inside_radius: NonNegativeLength,
    side: SheetMetalContourThicknessSide,
}

impl ContourOperands {
    /// The construction these operands state for `profile`.
    pub(crate) fn construction(
        self,
        profile: PlanarProfileRef,
    ) -> Option<SheetMetalContourFlangeConstruction> {
        Some(SheetMetalContourFlangeConstruction {
            profile: Some(profile),
            direction: Some(FeatureDirection3::from(self.direction)),
            extent: Some(ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: LinearTermination::Blind {
                        length: NonZeroLength::new(self.distance.get())?,
                    },
                    draft: None,
                },
            }),
            thickness: Some(self.thickness),
            inside_radius: Some(self.inside_radius),
            side: Some(self.side),
            neutral_factor: None,
        })
    }
}

/// The travel length that a plate slot-3 parameter value states: a length,
/// or a dimensionless value in internal centimetres.
pub(crate) fn travel_length(value: &cadmpeg_ir::features::ParameterValue) -> Option<Length> {
    match value {
        cadmpeg_ir::features::ParameterValue::Length(length) => Some(*length),
        cadmpeg_ir::features::ParameterValue::Real(value) => {
            Length::new(value.get() * INTERNAL_CM_TO_MM)
        }
        _ => None,
    }
}

/// A length reader for the operands of `plate`: slot 3 reads its parameter
/// through [`travel_length`], every other reference through `length`.
pub(crate) fn plate_lengths<'a>(
    plate: &PmDcFeature,
    value: impl Fn(PmDcReference) -> Option<&'a cadmpeg_ir::features::ParameterValue>,
    length: impl Fn(PmDcReference) -> Option<Length>,
) -> impl Fn(PmDcReference) -> Option<Length> {
    let travel = plate.properties.references().get(3).copied();
    move |reference| {
        if travel.is_some_and(|travel| same(travel, reference)) {
            value(reference).and_then(travel_length)
        } else {
            length(reference)
        }
    }
}

/// The plate, bend, and corner records of one contour flange.
#[derive(Clone, Copy)]
pub(crate) struct Children<'a> {
    pub(crate) plate: &'a PmDcFeature,
    pub(crate) bend: &'a PmDcFeature,
    pub(crate) corner: &'a PmDcFeature,
    pub(crate) attachment_edges: Option<&'a AttachmentEdges>,
}

/// The construction scalars of a contour flange, after every operand check.
///
/// `stored` reports whether a reference names a stored direction record;
/// `direction` normalizes one. `normal` is the profile-plane normal in model
/// coordinates.
pub(crate) fn operands<'a>(
    children: Children<'_>,
    style: Option<&Style>,
    property: impl Fn(PmDcReference) -> Option<&'a PmDcFeatureProperty>,
    length: impl Fn(PmDcReference) -> Option<Length>,
    direction: impl Fn(PmDcReference) -> Option<UnitVector3>,
    stored: impl Fn(PmDcReference) -> bool,
    normal: UnitVector3,
) -> Result<ContourOperands, ContourRefusal> {
    let Children {
        plate,
        bend,
        corner,
        attachment_edges,
    } = children;
    let style = style.refuse(ContourRefusal::Style)?;
    let slots = plate.properties.references();
    require(
        PlateForm::of(plate) == Some(PlateForm::Collection),
        ContourRefusal::PropertyCount,
    )?;
    require(
        style.save_version_major == plate.save_version_major,
        ContourRefusal::StyleVersion,
    )?;
    require(
        RESERVED_SLOTS.iter().all(|&slot| slots[slot].index() == 0),
        ContourRefusal::ReservedSlot,
    )?;
    if slots[12].index() != 0 {
        return Err(match attachment_edges {
            Some(attachment)
                if attachment.selection == slots[12] && !attachment.edges.is_empty() =>
            {
                ContourRefusal::EdgeAttached(attachment.edges.len())
            }
            _ => ContourRefusal::ReservedSlot,
        });
    }
    require(
        matches!(
            property(slots[16]).map(|p| &p.kind),
            Some(PmDcFeaturePropertyKind::Enumeration {
                family: PmDcFeatureEnumFamily::PartOperation,
                type_value: 5,
                value: 1
            })
        ),
        ContourRefusal::PartOperation,
    )?;
    require(
        property(slots[0]).is_some_and(|selection| {
            selection.identity.type_id.as_str()
                == crate::pmdc::type_id_string(crate::feature::PLATE_PROFILE_SELECTION_TYPE)
                && matches!(
                    selection.kind,
                    PmDcFeaturePropertyKind::ProfileSelection { value: 0, .. }
                )
        }),
        ContourRefusal::ProfileSelection,
    )?;
    require(
        matches!(
            property(slots[7]).map(|p| &p.kind),
            Some(PmDcFeaturePropertyKind::References {
                family: PmDcFeatureReferenceFamily::EdgeCollection,
                items,
            }) if items.references().is_empty()
        ),
        ContourRefusal::EdgeCollection,
    )?;
    require(
        feature_dimensions(property(slots[8])),
        ContourRefusal::FeatureDimensions,
    )?;
    require(
        property(slots[9]).is_some_and(|p| operands::corner(p, &property, &length)),
        ContourRefusal::CornerControl,
    )?;
    require(
        property(slots[13])
            .and_then(sheet)
            .is_some_and(|kind| matches!(kind, SheetMetalProperty::PlateControl { value: 0 })),
        ContourRefusal::PlateControl,
    )?;
    let Some(SheetMetalProperty::ContourAttachment {
        control,
        thickness,
        first_mode,
        directions,
        radius,
        second_mode,
        relief_width,
        minimum_remnant,
        first_option,
        states,
        second_option,
        extension,
        value,
    }) = property(slots[4]).and_then(sheet)
    else {
        return Err(ContourRefusal::AttachmentControl);
    };
    require(
        [*control, *first_option, *second_option]
            .iter()
            .all(|&reference| property(reference).and_then(boolean) == Some(false)),
        ContourRefusal::AttachmentBoolean,
    )?;
    require(
        property(*first_mode).and_then(enumeration) == Some((9, 1))
            && property(*second_mode).and_then(enumeration) == Some((9, 4)),
        ContourRefusal::AttachmentMode,
    )?;
    require(
        states.as_ref() == [1, 1, 1] && *value == 0,
        ContourRefusal::AttachmentStates,
    )?;
    require(
        operands::extension_repeats(
            property(*extension),
            slots[4],
            [*thickness, *radius, *relief_width, *minimum_remnant],
            [*first_option, *second_option],
        ),
        ContourRefusal::AttachmentExtension,
    )?;
    require(
        same(directions[0], slots[1]) && stored(directions[1]),
        ContourRefusal::AttachmentDirection,
    )?;
    let side = thickness_side(property(slots[2])).refuse(ContourRefusal::Side)?;
    let travel = direction(slots[1]).refuse(ContourRefusal::Direction)?;
    require(
        travel.as_raw().dot(*normal.as_raw()).abs() >= 1.0 - EPS_TRAVEL_NORMAL,
        ContourRefusal::TravelDirection,
    )?;
    operands::relief(
        property(slots[10]),
        style,
        [*relief_width, *minimum_remnant],
        &property,
        &length,
    )
    .map_err(ContourRefusal::from_operand)?;
    operands::transition(property(slots[11]), style, &property, &length)
        .map_err(ContourRefusal::from_operand)?;
    let distance = length(slots[3])
        .and_then(|v| PositiveLength::try_from(v).ok())
        .refuse(ContourRefusal::Distance)?;
    let sheet_thickness = length(*thickness)
        .and_then(|v| PositiveLength::try_from(v).ok())
        .refuse(ContourRefusal::Thickness)?;
    let inside_radius = length(*radius)
        .and_then(|v| NonNegativeLength::try_from(v).ok())
        .refuse(ContourRefusal::Radius)?;
    require(
        bend_repeats(plate, bend, *thickness, *radius, &property),
        ContourRefusal::Bend,
    )?;
    require(
        corner_repeats(plate, corner, &property),
        ContourRefusal::Corner,
    )?;
    Ok(ContourOperands {
        direction: travel,
        distance,
        thickness: sheet_thickness,
        inside_radius,
        side,
    })
}

fn thickness_side(
    property: Option<&PmDcFeatureProperty>,
) -> Option<SheetMetalContourThicknessSide> {
    matches!(
        property.map(|p| &p.kind),
        Some(PmDcFeaturePropertyKind::Enumeration {
            family: PmDcFeatureEnumFamily::PlateSide,
            type_value: 2,
            value: 0,
        })
    )
    .then_some(SheetMetalContourThicknessSide::Right)
}

fn feature_dimensions(property: Option<&PmDcFeatureProperty>) -> bool {
    matches!(
        property.map(|p| &p.kind),
        Some(PmDcFeaturePropertyKind::References {
            family: PmDcFeatureReferenceFamily::FeatureDimensions,
            ..
        })
    )
}

/// Whether the 11-property bend repeats the plate operands, the attachment
/// thickness in slot 2, and the attachment radius in slot 1.
fn bend_repeats<'a>(
    plate: &PmDcFeature,
    bend: &PmDcFeature,
    thickness: PmDcReference,
    radius: PmDcReference,
    property: &impl Fn(PmDcReference) -> Option<&'a PmDcFeatureProperty>,
) -> bool {
    let p = plate.properties.references();
    let b = bend.properties.references();
    b.len() == PlateForm::Collection.bend_count()
        && plate.identity.segment_token == bend.identity.segment_token
        && BEND_SHARED
            .iter()
            .all(|&(left, right)| same(p[left], b[right]))
        && same(b[1], radius)
        && same(b[2], thickness)
        && b[5].index() == 0
        && feature_dimensions(property(b[6]))
        && matches!(
            property(b[8]).and_then(sheet),
            Some(SheetMetalProperty::BendControl {
                value: 6,
                state: Some(1)
            })
        )
        && property(b[10]).and_then(boolean) == Some(true)
}

/// Whether the seven-property corner repeats the plate corner control,
/// attachment selection, profile selection, and result collection.
fn corner_repeats<'a>(
    plate: &PmDcFeature,
    corner: &PmDcFeature,
    property: &impl Fn(PmDcReference) -> Option<&'a PmDcFeatureProperty>,
) -> bool {
    let p = plate.properties.references();
    let c = corner.properties.references();
    c.len() == 7
        && plate.identity.segment_token == corner.identity.segment_token
        && CORNER_SHARED
            .iter()
            .all(|&(left, right)| same(p[left], c[right]))
        && c[1].index() == 0
        && c[5].index() == 0
        && feature_dimensions(property(c[2]))
}

/// The plate, bend, and corner records of `members`.
pub(crate) fn children<'a>(
    features: &'a [PmDcFeature],
    token: &str,
    members: Members,
) -> Option<Children<'a>> {
    let source = |record| {
        let mut found = features.iter().filter(|source| {
            source.identity.segment_token.as_str() == token
                && source.identity.record_ordinal == record
        });
        let first = found.next()?;
        found.next().is_none().then_some(first)
    };
    Some(Children {
        plate: source(members.plate)?,
        bend: source(members.bend)?,
        corner: source(members.corner)?,
        attachment_edges: None,
    })
}

/// One projected contour flange.
pub(in crate::feature) struct Projected<'a> {
    pub(in crate::feature) feature: Feature,
    pub(in crate::feature) result: FeatureResultTopology,
    /// The parent label: its name and participants describe the operation.
    pub(in crate::feature) label: &'a PmDcFeatureLabel,
    pub(in crate::feature) members: Members,
}

/// Why a contour-flange projection stops.
#[derive(Debug)]
pub(in crate::feature) enum ContourError {
    /// A named admission check fails; the children stay native.
    Refused(ContourRefusal),
    /// The decode itself fails.
    Codec(CodecError),
}

impl From<ContourRefusal> for ContourError {
    fn from(refusal: ContourRefusal) -> Self {
        Self::Refused(refusal)
    }
}

impl From<CodecError> for ContourError {
    fn from(error: CodecError) -> Self {
        Self::Codec(error)
    }
}

/// Every contour flange of the inventory, keyed by segment token and plate
/// record ordinal. A flange whose plate is not unique has no entry.
pub(in crate::feature) fn project_all<'a>(
    ctx: &DecodeContext<'_>,
    inventory: &'a FeatureInventory,
    index: &ProjectionIndex<'_>,
    label_of: impl Fn(&str, u32) -> Option<&'a PmDcFeatureLabel>,
) -> Result<Outcomes<'a>, CodecError> {
    let sketches: HashSet<_> = index.sketches.keys().copied().collect();
    let flanges = unique_index(
        inventory
            .sheet_metal_flanges
            .iter()
            .filter(|flange| FlangeKind::of(flange) == Some(FlangeKind::Contour))
            .filter_map(|flange| {
                let plate = flange.children.references().first()?.record_ordinal()?;
                Some(((flange.identity.segment_token.as_str(), plate), flange))
            }),
    );
    let mut outcomes = Outcomes::default();
    for (key, flange) in flanges {
        let Some(flange) = flange else {
            continue;
        };
        let members = projection::members(flange, &inventory.features, &inventory.labels, |o| {
            sketches.contains(&(key.0, o))
        });
        let outcome = match members {
            None => Err(ContourRefusal::Members),
            Some(members) => match project(ctx, flange, members, index, inventory, &label_of) {
                Ok(projected) => Ok(projected),
                Err(ContourError::Refused(refusal)) => Err(refusal),
                Err(ContourError::Codec(error)) => return Err(error),
            },
        };
        outcomes.0.insert((key.0.to_owned(), key.1), outcome);
    }
    Ok(outcomes)
}

/// The contour-flange outcome of each contour plate.
#[derive(Default)]
pub(in crate::feature) struct Outcomes<'a>(
    BTreeMap<(String, u32), Result<Projected<'a>, ContourRefusal>>,
);

impl<'a> Outcomes<'a> {
    /// Take the outcome of the plate at `ordinal`, when it is a contour plate.
    pub(in crate::feature) fn take(
        &mut self,
        token: &str,
        ordinal: u32,
    ) -> Option<Result<Projected<'a>, ContourRefusal>> {
        self.0.remove(&(token.to_owned(), ordinal))
    }

    /// The bend and corner records that a projected contour flange consumes,
    /// each with the contour-flange feature that represents it.
    pub(in crate::feature) fn consumed(&self) -> BTreeMap<(String, u32), FeatureId> {
        self.0
            .iter()
            .filter_map(|((token, _), outcome)| {
                let projected = outcome.as_ref().ok()?;
                let members = projected.members;
                Some([
                    ((token.clone(), members.bend), projected.feature.id.clone()),
                    (
                        (token.clone(), members.corner),
                        projected.feature.id.clone(),
                    ),
                ])
            })
            .flatten()
            .collect()
    }
}

fn project<'a>(
    ctx: &DecodeContext<'_>,
    flange: &Flange,
    members: Members,
    index: &ProjectionIndex<'_>,
    inventory: &FeatureInventory,
    label_of: &impl Fn(&str, u32) -> Option<&'a PmDcFeatureLabel>,
) -> Result<Projected<'a>, ContourError> {
    let token = flange.identity.segment_token.as_str();
    let label = label_of(token, flange.identity.record_ordinal).refuse(ContourRefusal::Members)?;
    let mut children =
        children(&inventory.features, token, members).refuse(ContourRefusal::Members)?;
    children.attachment_edges = flange.attachment_edges.as_ref();
    let sketch = index
        .sketches
        .get(&(token, members.sketch))
        .and_then(Option::as_ref)
        .and_then(|sketch| index.sketch_ids.get(sketch.id().as_str()))
        .and_then(|id| index.sketches_by_id.get(id.as_str()))
        .refuse(ContourRefusal::ProfileSketch)?;
    let (_, normal, _) = sketch
        .resolved_placement()
        .refuse(ContourRefusal::ProfileSketch)?;
    let normal = UnitVector3::normalized(*normal.as_raw()).refuse(ContourRefusal::ProfileSketch)?;
    let direction = |reference: PmDcReference| {
        index
            .directions
            .get(&(token, reference.record_ordinal()?))?
            .as_ref()
            .copied()
    };
    let values = operands(
        children,
        projection::style(&inventory.sheet_metal_styles, token),
        |reference| resolve_property(token, reference.index(), index),
        plate_lengths(
            children.plate,
            |reference| {
                let parameter = index
                    .parameters
                    .get(&(token, reference.record_ordinal()?))?
                    .as_ref()?;
                index
                    .parameter_values
                    .get(parameter.id().as_str())?
                    .as_ref()?
                    .value
                    .as_ref()
            },
            |reference| length_reference(token, reference.index(), index),
        ),
        |reference| {
            let record = direction(reference)?;
            UnitVector3::normalized(cadmpeg_ir::math::Vector3::new(
                record.direction[0].get(),
                record.direction[1].get(),
                record.direction[2].get(),
            ))
        },
        |reference| direction(reference).is_some(),
        normal,
    )?;
    let selection = resolve_property(
        token,
        children.plate.properties.references()[0].index(),
        index,
    )
    .refuse(ContourRefusal::ProfileSelection)?;
    let PmDcFeaturePropertyKind::ProfileSelection { entity_link, .. } = &selection.kind else {
        return Err(ContourRefusal::ProfileSelection.into());
    };
    let curves = index
        .profiles
        .named_open_path(
            token,
            entity_link
                .record_ordinal()
                .refuse(ContourRefusal::Profile)?,
            members.sketch,
            sketch,
        )
        .refuse(ContourRefusal::Profile)?;
    let profile = PlanarProfileRef::sketch_entities(sketch.id.clone(), curves)
        .ok()
        .refuse(ContourRefusal::Profile)?;
    let construction = values
        .construction(profile)
        .refuse(ContourRefusal::Distance)?;
    let (id, result) = crate::feature::plate_result(ctx, children.plate, index)
        .refuse(ContourRefusal::Result)??;
    ctx.charge_entities(1, "project Inventor contour flange")?;
    let mut source_properties = BTreeMap::new();
    for (name, value) in [
        ("owning_flange", flange.id()),
        ("bend", children.bend.id()),
        ("corner", children.corner.id()),
    ] {
        source_properties.insert(
            cadmpeg_core::text::NonBlankString::new(name.to_owned())
                .ok_or_else(|| CodecError::malformed("Inventor contour property key is blank"))?,
            value,
        );
    }
    let name = label
        .name()
        .or_else(|| label_of(token, members.plate).and_then(|plate| plate.name()));
    Ok(Projected {
        feature: Feature {
            product_definition: None,
            id,
            ordinal: u64::from(label.index),
            name: name.map(str::to_owned),
            suppressed: None,
            dependencies: DistinctMembers::default(),
            source_properties,
            source_tag: Some("sheet_metal_contour_flange".into()),
            source_text: None,
            source_content: FeatureContent::default(),
            evaluation: FeatureEvaluation::from_definition(FeatureDefinition::Operation(
                FeatureOperation::SheetMetalContourFlange { construction },
            )),
            native_ref: Some(children.plate.id()),
        },
        result,
        label,
        members,
    })
}

/// The first refused check of each contour flange that stays native.
#[derive(Debug, Default)]
pub(in crate::feature) struct Refusals(BTreeMap<String, ContourRefusal>);

impl Refusals {
    /// Whether the plate record `source` has a refusal.
    pub(in crate::feature) fn contains(&self, source: &str) -> bool {
        self.0.contains_key(source)
    }

    /// Record the refusal of the contour plate `source`.
    pub(in crate::feature) fn insert(&mut self, source: String, refusal: ContourRefusal) {
        self.0.entry(source).or_insert(refusal);
    }

    /// One loss note for each contour plate that stays native in the final
    /// features.
    pub(in crate::feature) fn losses(
        &self,
        features: &[Feature],
    ) -> Vec<cadmpeg_ir::report::loss::LossNote> {
        features
            .iter()
            .filter(|feature| {
                matches!(
                    feature.evaluation.definition(),
                    FeatureDefinition::Operation(FeatureOperation::Native { .. })
                )
            })
            .filter_map(|feature| {
                let refusal = self.0.get(feature.native_ref.as_deref()?)?;
                let attachment = match refusal {
                    ContourRefusal::EdgeAttached(count) => format!("; decoded attachment selects {count} input edge(s)"),
                    _ => String::new(),
                };
                Some(
                    crate::loss::InventorLossCode::FeatureSheetMetalContourFlangeRefused.note(
                        format!(
                            "Retained {} and its bend and corner as native: the first refused contour-flange check is {}{}.",
                            feature.id,
                            refusal.detail(),
                            attachment
                        ),
                    ),
                )
            })
            .collect()
    }
}

/// Whether `source` is the plate, bend, or corner of a contour flange.
pub(crate) fn is_child(flanges: &[Flange], source: &PmDcFeature) -> bool {
    flanges.iter().any(|flange| {
        FlangeKind::of(flange) == Some(FlangeKind::Contour)
            && flange.identity.segment_token == source.identity.segment_token
            && flange
                .children
                .references()
                .iter()
                .any(|child| child.record_ordinal() == Some(source.identity.record_ordinal))
    })
}

#[cfg(test)]
mod tests;
