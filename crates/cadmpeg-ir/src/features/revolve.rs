// SPDX-License-Identifier: Apache-2.0
//! Revolve and rib constructions.

use super::extents::AngularTermination;
use super::geometry::{FeatureDirection3, FinitePoint3};
use super::profiles::{PathRef, PlanarProfileRef};
use super::sweeps::FaceMaker;
use super::{
    deserialize_allow_multi_profile_faces, deserialize_face_maker_class, deserialize_fuse_order,
    deserialize_partial_revolve_construction_axis, deserialize_partial_revolve_construction_extent,
    deserialize_solid, EPS_FEATURE_FULL_TURN,
};
use crate::scalar::{PositiveLength, SlopeAngle};
use cadmpeg_derive::DeserializeName;
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

cadmpeg_core::named_optional_field!(deserialize_profile, PlanarProfileRef, "profile");

cadmpeg_core::named_optional_field!(deserialize_revolution_axis_reference, PathRef, "reference");

cadmpeg_core::named_optional_field!(
    deserialize_rib_construction_direction,
    FeatureDirection3,
    "direction"
);

cadmpeg_core::named_optional_field!(deserialize_rib_construction_side, RibSide, "side");

cadmpeg_core::named_optional_field!(deserialize_thickness, PositiveLength, "thickness");

/// Resolution state and inputs of a profile revolution.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum RevolveConstruction {
    /// The first absent required construction input names the partial state.
    Unresolved(PartialRevolveConstruction),
    /// All required construction inputs are present.
    Resolved {
        /// Profile revolved about the axis.
        profile: PlanarProfileRef,
        /// Placed revolution axis and its optional native source.
        axis: RevolutionAxis,
        /// Angular extent.
        extent: RevolveExtent,
        /// Whether a standalone revolution creates a solid rather than a sheet.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_solid"
        )]
        solid: Option<bool>,
        /// Face-building algorithm used for a standalone solid revolution.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_face_maker_class"
        )]
        #[serde(rename = "face_maker_class")]
        #[cfg_attr(feature = "schema", schemars(with = "Option<String>"))]
        face_maker: Option<FaceMaker>,
        /// Compatibility ordering for fusing a `PartDesign` revolution into its body.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_fuse_order"
        )]
        fuse_order: Option<RevolutionFuseOrder>,
        /// Whether a profile containing multiple faces is accepted as one operation.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_allow_multi_profile_faces"
        )]
        allow_multi_profile_faces: Option<bool>,
    },
}

/// Incomplete inputs of a profile revolution, named by the first absent operand
/// in the order profile, axis, extent.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PartialRevolveConstruction {
    /// The profile is absent.
    Profile {
        /// Placed revolution axis, when decoded.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_partial_revolve_construction_axis"
        )]
        axis: Option<RevolutionAxis>,
        /// Angular extent, when decoded.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_partial_revolve_construction_extent"
        )]
        extent: Option<RevolveExtent>,
        /// Whether a standalone revolution creates a solid rather than a sheet.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_solid"
        )]
        solid: Option<bool>,
        /// Face-building algorithm used for a standalone solid revolution.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_face_maker_class"
        )]
        #[serde(rename = "face_maker_class")]
        #[cfg_attr(feature = "schema", schemars(with = "Option<String>"))]
        face_maker: Option<FaceMaker>,
        /// Compatibility ordering for fusing a `PartDesign` revolution into its body.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_fuse_order"
        )]
        fuse_order: Option<RevolutionFuseOrder>,
        /// Whether a profile containing multiple faces is accepted as one operation.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_allow_multi_profile_faces"
        )]
        allow_multi_profile_faces: Option<bool>,
    },
    /// The profile is present and the axis is absent.
    Axis {
        /// Profile revolved about the axis.
        profile: PlanarProfileRef,
        /// Angular extent, when decoded.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_partial_revolve_construction_extent"
        )]
        extent: Option<RevolveExtent>,
        /// Whether a standalone revolution creates a solid rather than a sheet.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_solid"
        )]
        solid: Option<bool>,
        /// Face-building algorithm used for a standalone solid revolution.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_face_maker_class"
        )]
        #[serde(rename = "face_maker_class")]
        #[cfg_attr(feature = "schema", schemars(with = "Option<String>"))]
        face_maker: Option<FaceMaker>,
        /// Compatibility ordering for fusing a `PartDesign` revolution into its body.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_fuse_order"
        )]
        fuse_order: Option<RevolutionFuseOrder>,
        /// Whether a profile containing multiple faces is accepted as one operation.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_allow_multi_profile_faces"
        )]
        allow_multi_profile_faces: Option<bool>,
    },
    /// The profile and the axis are present and the extent is absent.
    Extent {
        /// Profile revolved about the axis.
        profile: PlanarProfileRef,
        /// Placed revolution axis and its optional native source.
        axis: RevolutionAxis,
        /// Whether a standalone revolution creates a solid rather than a sheet.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_solid"
        )]
        solid: Option<bool>,
        /// Face-building algorithm used for a standalone solid revolution.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_face_maker_class"
        )]
        #[serde(rename = "face_maker_class")]
        #[cfg_attr(feature = "schema", schemars(with = "Option<String>"))]
        face_maker: Option<FaceMaker>,
        /// Compatibility ordering for fusing a `PartDesign` revolution into its body.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_fuse_order"
        )]
        fuse_order: Option<RevolutionFuseOrder>,
        /// Whether a profile containing multiple faces is accepted as one operation.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_allow_multi_profile_faces"
        )]
        allow_multi_profile_faces: Option<bool>,
    },
}

/// The four optional selections a revolution carries in every resolution state.
struct RevolveSelections {
    solid: Option<bool>,
    face_maker: Option<FaceMaker>,
    fuse_order: Option<RevolutionFuseOrder>,
    allow_multi_profile_faces: Option<bool>,
}

impl RevolveConstruction {
    fn selections(&self) -> RevolveSelections {
        let (solid, face_maker, fuse_order, allow_multi_profile_faces) = match self {
            Self::Resolved {
                solid,
                face_maker,
                fuse_order,
                allow_multi_profile_faces,
                ..
            }
            | Self::Unresolved(
                PartialRevolveConstruction::Profile {
                    solid,
                    face_maker,
                    fuse_order,
                    allow_multi_profile_faces,
                    ..
                }
                | PartialRevolveConstruction::Axis {
                    solid,
                    face_maker,
                    fuse_order,
                    allow_multi_profile_faces,
                    ..
                }
                | PartialRevolveConstruction::Extent {
                    solid,
                    face_maker,
                    fuse_order,
                    allow_multi_profile_faces,
                    ..
                },
            ) => (solid, face_maker, fuse_order, allow_multi_profile_faces),
        };
        RevolveSelections {
            solid: *solid,
            face_maker: face_maker.clone(),
            fuse_order: *fuse_order,
            allow_multi_profile_faces: *allow_multi_profile_faces,
        }
    }

    /// Returns whether all required construction inputs are present.
    pub const fn is_resolved(&self) -> bool {
        matches!(self, Self::Resolved { .. })
    }

    /// Returns the profile, when decoded.
    pub const fn profile(&self) -> Option<&PlanarProfileRef> {
        match self {
            Self::Unresolved(PartialRevolveConstruction::Profile { .. }) => None,
            Self::Unresolved(
                PartialRevolveConstruction::Axis { profile, .. }
                | PartialRevolveConstruction::Extent { profile, .. },
            )
            | Self::Resolved { profile, .. } => Some(profile),
        }
    }

    /// Returns the mutable profile, when decoded.
    pub const fn profile_mut(&mut self) -> Option<&mut PlanarProfileRef> {
        match self {
            Self::Unresolved(PartialRevolveConstruction::Profile { .. }) => None,
            Self::Unresolved(
                PartialRevolveConstruction::Axis { profile, .. }
                | PartialRevolveConstruction::Extent { profile, .. },
            )
            | Self::Resolved { profile, .. } => Some(profile),
        }
    }

    /// Replaces the decoded profile, naming the resolution state the new
    /// operand set produces.
    pub fn set_profile(&mut self, profile: Option<PlanarProfileRef>) {
        let RevolveSelections {
            solid,
            face_maker,
            fuse_order,
            allow_multi_profile_faces,
        } = self.selections();
        let axis = self.axis().cloned();
        let extent = self.extent().cloned();
        *self = match (profile, axis, extent) {
            (None, axis, extent) => Self::Unresolved(PartialRevolveConstruction::Profile {
                axis,
                extent,
                solid,
                face_maker,
                fuse_order,
                allow_multi_profile_faces,
            }),
            (Some(profile), None, extent) => Self::Unresolved(PartialRevolveConstruction::Axis {
                profile,
                extent,
                solid,
                face_maker,
                fuse_order,
                allow_multi_profile_faces,
            }),
            (Some(profile), Some(axis), None) => {
                Self::Unresolved(PartialRevolveConstruction::Extent {
                    profile,
                    axis,
                    solid,
                    face_maker,
                    fuse_order,
                    allow_multi_profile_faces,
                })
            }
            (Some(profile), Some(axis), Some(extent)) => Self::Resolved {
                profile,
                axis,
                extent,
                solid,
                face_maker,
                fuse_order,
                allow_multi_profile_faces,
            },
        };
    }

    /// Returns the placed axis, when decoded.
    pub const fn axis(&self) -> Option<&RevolutionAxis> {
        match self {
            Self::Unresolved(
                PartialRevolveConstruction::Profile { axis: None, .. }
                | PartialRevolveConstruction::Axis { .. },
            ) => None,
            Self::Unresolved(
                PartialRevolveConstruction::Profile {
                    axis: Some(axis), ..
                }
                | PartialRevolveConstruction::Extent { axis, .. },
            )
            | Self::Resolved { axis, .. } => Some(axis),
        }
    }

    /// Returns the mutable placed axis, when decoded.
    pub const fn axis_mut(&mut self) -> Option<&mut RevolutionAxis> {
        match self {
            Self::Unresolved(
                PartialRevolveConstruction::Profile { axis: None, .. }
                | PartialRevolveConstruction::Axis { .. },
            ) => None,
            Self::Unresolved(
                PartialRevolveConstruction::Profile {
                    axis: Some(axis), ..
                }
                | PartialRevolveConstruction::Extent { axis, .. },
            )
            | Self::Resolved { axis, .. } => Some(axis),
        }
    }

    /// Replaces the placed axis, naming the resolution state the new operand
    /// set produces.
    pub fn set_axis(&mut self, axis: Option<RevolutionAxis>) {
        let old = std::mem::replace(
            self,
            Self::Unresolved(PartialRevolveConstruction::Profile {
                axis: None,
                extent: None,
                solid: None,
                face_maker: None,
                fuse_order: None,
                allow_multi_profile_faces: None,
            }),
        );
        let (profile, extent, solid, face_maker, fuse_order, allow_multi_profile_faces) = match old
        {
            Self::Unresolved(PartialRevolveConstruction::Profile {
                extent,
                solid,
                face_maker,
                fuse_order,
                allow_multi_profile_faces,
                ..
            }) => (
                None,
                extent,
                solid,
                face_maker,
                fuse_order,
                allow_multi_profile_faces,
            ),
            Self::Unresolved(PartialRevolveConstruction::Axis {
                profile,
                extent,
                solid,
                face_maker,
                fuse_order,
                allow_multi_profile_faces,
            }) => (
                Some(profile),
                extent,
                solid,
                face_maker,
                fuse_order,
                allow_multi_profile_faces,
            ),
            Self::Unresolved(PartialRevolveConstruction::Extent {
                profile,
                solid,
                face_maker,
                fuse_order,
                allow_multi_profile_faces,
                ..
            }) => (
                Some(profile),
                None,
                solid,
                face_maker,
                fuse_order,
                allow_multi_profile_faces,
            ),
            Self::Resolved {
                profile,
                extent,
                solid,
                face_maker,
                fuse_order,
                allow_multi_profile_faces,
                ..
            } => (
                Some(profile),
                Some(extent),
                solid,
                face_maker,
                fuse_order,
                allow_multi_profile_faces,
            ),
        };
        *self = match (profile, axis, extent) {
            (None, axis, extent) => Self::Unresolved(PartialRevolveConstruction::Profile {
                axis,
                extent,
                solid,
                face_maker,
                fuse_order,
                allow_multi_profile_faces,
            }),
            (Some(profile), None, extent) => Self::Unresolved(PartialRevolveConstruction::Axis {
                profile,
                extent,
                solid,
                face_maker,
                fuse_order,
                allow_multi_profile_faces,
            }),
            (Some(profile), Some(axis), None) => {
                Self::Unresolved(PartialRevolveConstruction::Extent {
                    profile,
                    axis,
                    solid,
                    face_maker,
                    fuse_order,
                    allow_multi_profile_faces,
                })
            }
            (Some(profile), Some(axis), Some(extent)) => Self::Resolved {
                profile,
                axis,
                extent,
                solid,
                face_maker,
                fuse_order,
                allow_multi_profile_faces,
            },
        };
    }

    /// Returns the angular extent, when decoded.
    pub const fn extent(&self) -> Option<&RevolveExtent> {
        match self {
            Self::Unresolved(
                PartialRevolveConstruction::Profile { extent: None, .. }
                | PartialRevolveConstruction::Axis { extent: None, .. }
                | PartialRevolveConstruction::Extent { .. },
            ) => None,
            Self::Unresolved(
                PartialRevolveConstruction::Profile {
                    extent: Some(extent),
                    ..
                }
                | PartialRevolveConstruction::Axis {
                    extent: Some(extent),
                    ..
                },
            )
            | Self::Resolved { extent, .. } => Some(extent),
        }
    }

    /// Returns the mutable angular extent, when decoded.
    pub const fn extent_mut(&mut self) -> Option<&mut RevolveExtent> {
        match self {
            Self::Unresolved(
                PartialRevolveConstruction::Profile { extent: None, .. }
                | PartialRevolveConstruction::Axis { extent: None, .. }
                | PartialRevolveConstruction::Extent { .. },
            ) => None,
            Self::Unresolved(
                PartialRevolveConstruction::Profile {
                    extent: Some(extent),
                    ..
                }
                | PartialRevolveConstruction::Axis {
                    extent: Some(extent),
                    ..
                },
            )
            | Self::Resolved { extent, .. } => Some(extent),
        }
    }

    /// Returns the standalone solid selection, when carried.
    pub const fn solid(&self) -> Option<bool> {
        match self {
            Self::Resolved { solid, .. }
            | Self::Unresolved(
                PartialRevolveConstruction::Profile { solid, .. }
                | PartialRevolveConstruction::Axis { solid, .. }
                | PartialRevolveConstruction::Extent { solid, .. },
            ) => *solid,
        }
    }

    /// Returns the standalone face-maker selection, when carried.
    pub const fn face_maker(&self) -> Option<&FaceMaker> {
        match self {
            Self::Resolved { face_maker, .. }
            | Self::Unresolved(
                PartialRevolveConstruction::Profile { face_maker, .. }
                | PartialRevolveConstruction::Axis { face_maker, .. }
                | PartialRevolveConstruction::Extent { face_maker, .. },
            ) => face_maker.as_ref(),
        }
    }

    /// Returns the `PartDesign` fuse ordering, when carried.
    pub const fn fuse_order(&self) -> Option<RevolutionFuseOrder> {
        match self {
            Self::Resolved { fuse_order, .. }
            | Self::Unresolved(
                PartialRevolveConstruction::Profile { fuse_order, .. }
                | PartialRevolveConstruction::Axis { fuse_order, .. }
                | PartialRevolveConstruction::Extent { fuse_order, .. },
            ) => *fuse_order,
        }
    }

    /// Returns the multi-profile-face selection, when carried.
    pub const fn allow_multi_profile_faces(&self) -> Option<bool> {
        match self {
            Self::Resolved {
                allow_multi_profile_faces,
                ..
            }
            | Self::Unresolved(
                PartialRevolveConstruction::Profile {
                    allow_multi_profile_faces,
                    ..
                }
                | PartialRevolveConstruction::Axis {
                    allow_multi_profile_faces,
                    ..
                }
                | PartialRevolveConstruction::Extent {
                    allow_multi_profile_faces,
                    ..
                },
            ) => *allow_multi_profile_faces,
        }
    }
}

/// Operand ordering used to fuse a `PartDesign` revolution result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum RevolutionFuseOrder {
    /// Existing body is the first fuse operand.
    BaseFirst,
    /// Newly revolved feature is the first fuse operand.
    FeatureFirst,
}

/// Complete line placement used as a revolution axis.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct RevolutionAxis {
    /// A point on the axis.
    pub origin: FinitePoint3,
    /// Axis direction with a finite nonzero norm.
    pub direction: FeatureDirection3,
    /// Native edge, datum, or sketch-axis selection used to resolve the axis.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_revolution_axis_reference"
    )]
    pub reference: Option<PathRef>,
}

/// Independently decoded inputs of a thin rib operation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct RibConstruction {
    /// Rib centerline or open profile, when resolved.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_profile"
    )]
    pub profile: Option<PlanarProfileRef>,
    /// Rib growth direction, when resolved.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_rib_construction_direction"
    )]
    pub direction: Option<FeatureDirection3>,
    /// Finished rib thickness, when resolved.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_thickness"
    )]
    pub thickness: Option<PositiveLength>,
    /// Distribution of thickness around the profile, when resolved.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_rib_construction_side"
    )]
    pub side: Option<RibSide>,
    /// Draft state applied to the rib walls.
    pub draft: RibDraft,
}

/// Distribution of rib thickness around its profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum RibSide {
    /// Thickness lies on one side of the profile.
    OneSided,
    /// Thickness is split equally around the profile.
    Centered,
}

/// Draft state of a rib construction.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum RibDraft {
    /// Draft semantics are present but unresolved.
    #[default]
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    Unresolved,
    /// Rib walls have no draft.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
    None,
    /// Rib walls use the specified draft angle.
    Angle(SlopeAngle),
}

/// Revolution sides use the right-hand rule about the stated axis direction.
/// The source profile is at zero radians. One-sided travel runs from zero to
/// the positive angle. Two-sided travel runs from zero to the first angle and
/// from zero to the negative second angle. Symmetric travel runs from minus
/// half the total angle to plus half the total angle.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum RevolveExtent {
    /// Travel on the oriented side only.
    OneSided {
        /// The single traveled side's termination.
        termination: AngularTermination,
    },
    /// Independent terminations on each side of the profile plane.
    TwoSided {
        /// Termination along the revolution direction.
        first: AngularTermination,
        /// Termination opposite the revolution direction.
        second: AngularTermination,
    },
    /// One termination mirrored across the profile plane. An angular travel
    /// states the total travel split evenly around the plane.
    Symmetric {
        /// The mirrored side's termination.
        termination: AngularTermination,
    },
}

impl RevolveExtent {
    /// Whether each fixed angle and their combined travel fit within one turn.
    #[must_use]
    pub fn angles_within_turn(&self) -> bool {
        let angle = |termination: &AngularTermination| match termination {
            AngularTermination::Angle { angle } => angle.get(),
            _ => 0.0,
        };
        let total = match self {
            Self::OneSided { termination } | Self::Symmetric { termination } => angle(termination),
            Self::TwoSided { first, second } => angle(first) + angle(second),
        };
        total <= std::f64::consts::TAU + EPS_FEATURE_FULL_TURN
    }
}
