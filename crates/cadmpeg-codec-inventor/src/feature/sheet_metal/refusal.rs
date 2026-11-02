// SPDX-License-Identifier: Apache-2.0
//! The first check that keeps an attached plate in its native form.

/// The first admission check that an attached plate fails.
///
/// The variants appear in the order the projection evaluates them. The kebab
/// string of each variant is the pinned `detail` of the loss note.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) enum PlateRefusal {
    /// The plate has no unique sheet-metal style record in its segment.
    Style,
    /// The plate does not have the 17-property form.
    PropertyCount,
    /// The style and the plate come from different save-version majors.
    StyleVersion,
    /// A property slot the form requires null is not null.
    ReservedSlot,
    /// Slot 16 is not the part-operation enumeration `(5, 3)`.
    PartOperation,
    /// Slot 6 is not a surface-body property.
    ResultBody,
    /// Slot 0 is not the plate profile selection with value 0.
    ProfileSelection,
    /// Slot 7 is not an empty edge collection.
    EdgeCollection,
    /// Slot 9 is not a well-formed corner control.
    CornerControl,
    /// Slot 8 is not a feature-dimension list.
    FeatureDimensions,
    /// Slot 13 is not the plate scalar 0.
    PlateControl,
    /// Slot 4 is not an attachment control.
    AttachmentControl,
    /// Slot 1 or an attachment direction is not a stored direction.
    Direction,
    /// Slot 1 is not antiparallel to the first attachment direction.
    PlateDirection,
    /// The first attachment direction is not along the sketch u axis.
    FirstDirection,
    /// The second attachment direction is not along the sketch v axis.
    SecondDirection,
    /// The attachment thickness is not the slot-3 reference.
    ThicknessReference,
    /// An attachment Boolean has a value the form does not admit.
    AttachmentBoolean,
    /// An attachment mode is not the enumeration `(9, 1)`.
    AttachmentMode,
    /// The attachment state bytes are not `[1, 1, 1]`.
    AttachmentStates,
    /// The optional extension state does not repeat the attachment operands.
    AttachmentExtension,
    /// Slot 10 is not a relief record that repeats the attachment operands.
    Relief,
    /// Slot 11 is not a transition record with the style form.
    Transition,
    /// A dimension reference does not resolve to a positive or non-negative length.
    Length,
    /// Slot 2 is not the along-normal thickness side.
    Side,
    /// Slot 12 does not hold a list of selection wrappers over edge collections of edge items.
    SelectionList,
    /// A selected edge has no unique generated edge name.
    EdgeName,
    /// A selected attachment edge has no stored geometry.
    InputGeometry,
    /// The plate has no unique planar profile sketch.
    ProfileSketch,
    /// The profile sketch has no closed whole-sketch region or placement.
    Profile,
    /// The result collection does not resolve to one body.
    Result,
    /// A selected attachment edge has no earlier neutral producer.
    InputProducer,
    /// The plate has no unique neutral bend after it.
    BendPair,
}

impl PlateRefusal {
    /// The pinned kebab string of this refusal.
    pub(crate) const fn detail(self) -> &'static str {
        match self {
            Self::Style => "style",
            Self::PropertyCount => "property-count",
            Self::StyleVersion => "style-version",
            Self::ReservedSlot => "reserved-slot",
            Self::PartOperation => "part-operation",
            Self::ResultBody => "result-body",
            Self::ProfileSelection => "profile-selection",
            Self::EdgeCollection => "edge-collection",
            Self::CornerControl => "corner-control",
            Self::FeatureDimensions => "feature-dimensions",
            Self::PlateControl => "plate-control",
            Self::AttachmentControl => "attachment-control",
            Self::Direction => "direction",
            Self::PlateDirection => "plate-direction",
            Self::FirstDirection => "first-direction",
            Self::SecondDirection => "second-direction",
            Self::ThicknessReference => "thickness-reference",
            Self::AttachmentBoolean => "attachment-boolean",
            Self::AttachmentMode => "attachment-mode",
            Self::AttachmentStates => "attachment-states",
            Self::AttachmentExtension => "attachment-extension",
            Self::Relief => "relief",
            Self::Transition => "transition",
            Self::Length => "length",
            Self::Side => "side",
            Self::SelectionList => "selection-list",
            Self::EdgeName => "edge-name",
            Self::InputGeometry => "input-geometry",
            Self::ProfileSketch => "profile-sketch",
            Self::Profile => "profile",
            Self::Result => "result",
            Self::InputProducer => "input-producer",
            Self::BendPair => "bend-pair",
        }
    }
}

/// Turn a failed check into the refusal that names it.
pub(crate) trait Refuse<T> {
    fn refuse(self, refusal: PlateRefusal) -> Result<T, PlateRefusal>;
}

impl<T> Refuse<T> for Option<T> {
    fn refuse(self, refusal: PlateRefusal) -> Result<T, PlateRefusal> {
        self.ok_or(refusal)
    }
}

/// Fail with `refusal` unless `condition` holds.
pub(crate) fn require(condition: bool, refusal: PlateRefusal) -> Result<(), PlateRefusal> {
    if condition {
        Ok(())
    } else {
        Err(refusal)
    }
}

/// Why an attached-plate projection stops.
#[derive(Debug)]
pub(crate) enum PlateError {
    /// A named admission check fails; the plate stays native.
    Refused(PlateRefusal),
    /// The decode itself fails.
    Codec(cadmpeg_core::CodecError),
}

impl From<PlateRefusal> for PlateError {
    fn from(refusal: PlateRefusal) -> Self {
        Self::Refused(refusal)
    }
}

impl From<cadmpeg_core::CodecError> for PlateError {
    fn from(error: cadmpeg_core::CodecError) -> Self {
        Self::Codec(error)
    }
}
