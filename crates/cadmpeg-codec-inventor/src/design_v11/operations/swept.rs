// SPDX-License-Identifier: Apache-2.0
//! Swept operation operands and limits.

use std::collections::BTreeMap;

use cadmpeg_ir::design::construction::{
    kinds, AxisValue, Construction, ConstructionInput, InputReference, InputRole, PlaneValue,
    Relation, Subject,
};
use cadmpeg_ir::design::dimension::Dimension;
use cadmpeg_ir::design::feature::FeatureOperation;
use cadmpeg_ir::design::ops::{
    shared::{Limit, Span},
    swept as new,
};
use cadmpeg_ir::design::options::{
    BodyForm, Boolean, BooleanOp, NativeOption, NativeOptions, SolidOutput, SymmetricMeasure,
};
use cadmpeg_ir::features::{self as old, FeatureOperation as LegacyOperation};
use cadmpeg_ir::scalar::{Angle, Length};
use cadmpeg_ir::sketches::Sketch;
use cadmpeg_ir::units::UnitVector3;

use super::{selections, Projected};

pub(super) fn direction(
    value: cadmpeg_ir::features::geometry::FeatureDirection3,
) -> Projected<Construction<kinds::Direction>> {
    Ok(Construction {
        resolved: Some(
            UnitVector3::normalized_nonzero(value.into()).ok_or("the direction is degenerate")?,
        ),
        inputs: Vec::new(),
        after: Vec::new(),
        solution: None,
        mode: None,
        resolved_in: BTreeMap::default(),
    })
}

pub(super) fn axis(
    origin: cadmpeg_ir::features::geometry::FinitePoint3,
    value: cadmpeg_ir::features::geometry::FeatureDirection3,
) -> Projected<Construction<kinds::Axis>> {
    Ok(Construction {
        resolved: Some(AxisValue {
            origin,
            direction: direction(value)?.resolved.ok_or("the axis is degenerate")?,
        }),
        inputs: Vec::new(),
        after: Vec::new(),
        solution: None,
        mode: None,
        resolved_in: BTreeMap::default(),
    })
}

pub(super) fn reference(
    reference: cadmpeg_ir::design::selection::OrderedSelection,
) -> ConstructionInput {
    ConstructionInput {
        role: Some(InputRole {
            subject: Subject::Whole,
            relation: Relation::Coincident,
        }),
        reference: Some(InputReference::Picks(reference)),
        value: None,
        reversed: None,
    }
}

pub(super) fn datum_plane(id: &old::FeatureId) -> Projected<Construction<kinds::Plane>> {
    Ok(Construction {
        inputs: vec![reference(selections::selection(vec![selections::picked(
            cadmpeg_ir::design::selection::PickTarget::Datum {
                feature: id.clone(),
                part: None,
            },
        )])?)],
        after: Vec::new(),
        solution: None,
        mode: None,
        resolved: None,
        resolved_in: BTreeMap::default(),
    })
}

fn plane(
    value: cadmpeg_ir::features::geometry::FeatureSupportPlaneFrame,
    native: &str,
    source: Option<cadmpeg_ir::features::extents::PlaneTargetSource>,
) -> Projected<Construction<kinds::Plane>> {
    Ok(Construction {
        inputs: vec![reference(selections::selection(vec![selections::named(
            native, None,
        )?])?)],
        mode: source.map(|source| {
            source_mode(match source {
                cadmpeg_ir::features::extents::PlaneTargetSource::WorkPlane => {
                    cadmpeg_core::nonblank_literal!("work_plane")
                }
                cadmpeg_ir::features::extents::PlaneTargetSource::WorkPoint => {
                    cadmpeg_core::nonblank_literal!("work_point")
                }
                cadmpeg_ir::features::extents::PlaneTargetSource::FaceSurface => {
                    cadmpeg_core::nonblank_literal!("face_surface")
                }
            })
        }),
        resolved: Some(PlaneValue {
            origin: value.origin(),
            normal: UnitVector3::normalized_nonzero(value.normal().into())
                .ok_or("the plane normal is degenerate")?,
            x_axis: UnitVector3::normalized_nonzero(value.u_axis().into())
                .ok_or("the plane x axis is degenerate")?,
        }),
        after: Vec::new(),
        solution: None,
        resolved_in: BTreeMap::default(),
    })
}

fn source_mode(
    method: cadmpeg_core::text::NonBlankString,
) -> cadmpeg_ir::design::construction::SourceMode {
    cadmpeg_ir::design::construction::SourceMode {
        codec: cadmpeg_core::nonblank_literal!("inventor").into(),
        method: cadmpeg_ir::design::names::NativeNameValue::Text(method),
        native_options: BTreeMap::default(),
        source_settings: BTreeMap::default(),
    }
}

pub(super) fn boolean(op: cadmpeg_ir::features::operation_settings::BooleanOp) -> Option<Boolean> {
    Some(match op {
        cadmpeg_ir::features::operation_settings::BooleanOp::NewBody => Boolean::NewBody,
        cadmpeg_ir::features::operation_settings::BooleanOp::Unresolved => return None,
        cadmpeg_ir::features::operation_settings::BooleanOp::Join => Boolean::Apply {
            op: BooleanOp::Join,
            targets: None,
        },
        cadmpeg_ir::features::operation_settings::BooleanOp::Cut => Boolean::Apply {
            op: BooleanOp::Cut,
            targets: None,
        },
        cadmpeg_ir::features::operation_settings::BooleanOp::Intersect => Boolean::Apply {
            op: BooleanOp::Intersect,
            targets: None,
        },
    })
}

pub(super) fn output(
    op: cadmpeg_ir::features::operation_settings::BooleanOp,
    solid: Option<bool>,
) -> SolidOutput {
    SolidOutput {
        body: solid.map(|value| {
            if value {
                BodyForm::Solid
            } else {
                BodyForm::Sheet
            }
        }),
        boolean: boolean(op),
    }
}

pub(super) fn linear(
    value: &cadmpeg_ir::features::extents::LinearTermination,
) -> Projected<Limit<Length>> {
    Ok(match value {
        cadmpeg_ir::features::extents::LinearTermination::Blind { length } => Limit::Amount(
            Dimension::Literal(Length::new(length.get()).ok_or("the length is invalid")?),
        ),
        cadmpeg_ir::features::extents::LinearTermination::ThroughAll {} => Limit::ThroughAll,
        cadmpeg_ir::features::extents::LinearTermination::ToFirst {} => Limit::Next,
        cadmpeg_ir::features::extents::LinearTermination::ThroughNext {} => {
            return Err("the next-material exit has no equivalent first-face limit")
        }
        cadmpeg_ir::features::extents::LinearTermination::ToLast {} => Limit::Last,
        cadmpeg_ir::features::extents::LinearTermination::ToPlane {
            plane: value,
            native,
            source,
        } => Limit::ToPlane(plane(*value, native.as_str(), *source)?),
        cadmpeg_ir::features::extents::LinearTermination::ToFace { face, offset }
            if offset.is_none_or(|value| value.get() == 0.0) =>
        {
            Limit::ToGeometry {
                target: selections::face(face)?,
                offset: offset.map(|value| {
                    cadmpeg_ir::design::options::LimitOffset::Distance(Dimension::Literal(value))
                }),
                extend: None,
                far: None,
            }
        }
        _ => return Err("the linear termination has operands outside the v11 limit contract"),
    })
}

fn angular(value: &cadmpeg_ir::features::extents::AngularTermination) -> Projected<Limit<Angle>> {
    Ok(match value {
        cadmpeg_ir::features::extents::AngularTermination::Angle { angle } => Limit::Amount(
            Dimension::Literal(Angle::new(angle.get()).ok_or("the angle is invalid")?),
        ),
        cadmpeg_ir::features::extents::AngularTermination::ThroughAll {} => Limit::ThroughAll,
        cadmpeg_ir::features::extents::AngularTermination::ToFirst {} => Limit::Next,
        cadmpeg_ir::features::extents::AngularTermination::ThroughNext {} => {
            return Err("the next-material exit has no equivalent first-face limit")
        }
        cadmpeg_ir::features::extents::AngularTermination::ToLast {} => Limit::Last,
        cadmpeg_ir::features::extents::AngularTermination::ToPlane {
            plane: value,
            native,
            source,
        } => Limit::ToPlane(plane(*value, native.as_str(), *source)?),
        _ => return Err("the angular termination has operands outside the v11 limit contract"),
    })
}

fn opposite<Q>(limit: Limit<Q>, negate: impl FnOnce(Q) -> Projected<Q>) -> Projected<Limit<Q>> {
    Ok(match limit {
        Limit::Amount(Dimension::Literal(value)) => {
            Limit::Amount(Dimension::Literal(negate(value)?))
        }
        Limit::Amount(_) => return Err("the opposite limit has a bound amount"),
        value => value,
    })
}

pub(super) fn extent(
    value: &cadmpeg_ir::features::extents::ExtrudeExtent,
) -> Projected<Span<Length>> {
    let negative = |value: Length| Length::new(-value.get()).ok_or("the reverse length is invalid");
    Ok(match value {
        cadmpeg_ir::features::extents::ExtrudeExtent::OneSided { side } => Span::Between {
            start: Box::new(Limit::AtOrigin),
            end: Box::new(linear(&side.termination)?),
        },
        cadmpeg_ir::features::extents::ExtrudeExtent::TwoSided { first, second } => Span::Between {
            start: Box::new(opposite(linear(&second.termination)?, negative)?),
            end: Box::new(linear(&first.termination)?),
        },
        cadmpeg_ir::features::extents::ExtrudeExtent::Symmetric { side } => {
            match linear(&side.termination)? {
                Limit::Amount(amount) => Span::Symmetric {
                    amount,
                    measure: Some(SymmetricMeasure::Total),
                },
                Limit::ThroughAll => Span::Between {
                    start: Box::new(Limit::ThroughAll),
                    end: Box::new(Limit::ThroughAll),
                },
                _ => return Err("the symmetric termination has no symmetric v11 limit"),
            }
        }
    })
}

fn revolve_span(value: &cadmpeg_ir::features::revolve::RevolveExtent) -> Projected<Span<Angle>> {
    Ok(match value {
        cadmpeg_ir::features::revolve::RevolveExtent::OneSided { termination } => Span::Between {
            start: Box::new(Limit::AtOrigin),
            end: Box::new(angular(termination)?),
        },
        cadmpeg_ir::features::revolve::RevolveExtent::TwoSided { first, second } => Span::Between {
            start: Box::new(opposite(angular(second)?, |value| {
                Angle::new(-value.get()).ok_or("the reverse angle is invalid")
            })?),
            end: Box::new(angular(first)?),
        },
        cadmpeg_ir::features::revolve::RevolveExtent::Symmetric { termination } => {
            match angular(termination)? {
                Limit::Amount(amount) => Span::Symmetric {
                    amount,
                    measure: Some(SymmetricMeasure::Total),
                },
                _ => return Err("the symmetric revolve termination has no symmetric v11 limit"),
            }
        }
    })
}

pub(super) fn setting(
    settings: &mut NativeOptions,
    name: cadmpeg_core::text::NonBlankString,
    value: Option<bool>,
) {
    if let Some(value) = value {
        settings.insert(name, NativeOption::Bool(Dimension::Literal(value)));
    }
}

pub(super) fn operation(
    value: &LegacyOperation,
    sketches: &[Sketch],
) -> Option<Projected<FeatureOperation>> {
    if !matches!(
        value,
        LegacyOperation::Extrude { .. }
            | LegacyOperation::Revolve { .. }
            | LegacyOperation::Sweep { .. }
            | LegacyOperation::Loft { .. }
    ) {
        return None;
    }
    Some((|| {
        Ok(match value {
            LegacyOperation::Extrude {
                profile,
                direction: travel,
                start,
                extent: span,
                op,
                solid,
                face_maker,
                inner_wire_taper,
                length_along_profile_normal,
                allow_multi_profile_faces,
            } => {
                if face_maker.is_some() {
                    return Err("the extrude face-maker has no v11 field");
                }
                let travel = match travel {
                    cadmpeg_ir::features::extents::ExtrudeDirection::ProfileNormal {} => {
                        Some(new::ExtrudeDirection::ProfileNormal {
                            reversed: Some(Dimension::Literal(false)),
                        })
                    }
                    cadmpeg_ir::features::extents::ExtrudeDirection::ReversedProfileNormal {} => {
                        Some(new::ExtrudeDirection::ProfileNormal {
                            reversed: Some(Dimension::Literal(true)),
                        })
                    }
                    cadmpeg_ir::features::extents::ExtrudeDirection::Explicit {
                        vector,
                        source,
                    } => {
                        let mut axis = direction(*vector)?;
                        match source {
                            Some(cadmpeg_ir::features::extents::ExtrusionDirectionSource::Edge { reference: path }) => {
                                let mut input = reference(selections::path(path)?);
                                input.role = Some(InputRole {
                                    subject: Subject::Whole,
                                    relation: Relation::Parallel,
                                });
                                axis.inputs.push(input);
                                axis.mode =
                                    Some(source_mode(cadmpeg_core::nonblank_literal!("edge")));
                            }
                            Some(cadmpeg_ir::features::extents::ExtrusionDirectionSource::Custom {}) => {}
                            Some(cadmpeg_ir::features::extents::ExtrusionDirectionSource::ProfileNormal {}) => {
                                axis.mode = Some(source_mode(cadmpeg_core::nonblank_literal!(
                                    "profile_normal"
                                )));
                            }
                            None => {}
                        }
                        Some(new::ExtrudeDirection::Along {
                            direction: Box::new(axis),
                            lengths_along_normal: length_along_profile_normal
                                .map(Dimension::Literal),
                        })
                    }
                    cadmpeg_ir::features::extents::ExtrudeDirection::Unresolved {} => {
                        return Err("the extrusion travel sense is unresolved")
                    }
                };
                if !matches!(travel, Some(new::ExtrudeDirection::Along { .. }))
                    && length_along_profile_normal.is_some()
                {
                    return Err("the normal-length control requires an explicit direction");
                }
                let origin = match start {
                    cadmpeg_ir::features::extents::ExtrudeStart::ProfilePlane {} => {
                        Some(new::ExtrudeOrigin::ProfilePlane)
                    }
                    cadmpeg_ir::features::extents::ExtrudeStart::OffsetProfilePlane { .. } => {
                        return Err("the start offset is measured along the profile normal")
                    }
                    cadmpeg_ir::features::extents::ExtrudeStart::FromPlane {
                        plane: value,
                        native,
                        source,
                    } => Some(new::ExtrudeOrigin::Plane(plane(
                        *value,
                        native.as_str(),
                        *source,
                    )?)),
                    cadmpeg_ir::features::extents::ExtrudeStart::FromFace { face, offset }
                        if offset.is_none_or(|value| value.get() == 0.0) =>
                    {
                        Some(new::ExtrudeOrigin::Faces {
                            faces: selections::face(face)?,
                            offset: offset.map(Dimension::Literal),
                        })
                    }
                    cadmpeg_ir::features::extents::ExtrudeStart::FromFace { .. } => {
                        return Err(
                            "the start-face offset direction exceeds the v11 origin contract",
                        )
                    }
                    cadmpeg_ir::features::extents::ExtrudeStart::Unresolved {} => {
                        return Err("the extrusion span origin is unresolved")
                    }
                };
                let has_draft = match span {
                    cadmpeg_ir::features::extents::ExtrudeExtent::OneSided { side }
                    | cadmpeg_ir::features::extents::ExtrudeExtent::Symmetric { side } => {
                        side.draft.is_some()
                    }
                    cadmpeg_ir::features::extents::ExtrudeExtent::TwoSided { first, second } => {
                        first.draft.is_some() || second.draft.is_some()
                    }
                };
                if has_draft || inner_wire_taper.is_some() {
                    return Err(
                        "the extrusion draft sign and neutral plane require a source reading",
                    );
                }
                let mut settings = BTreeMap::default();
                setting(
                    &mut settings,
                    cadmpeg_core::nonblank_literal!("inventor:allow_multi_profile_faces"),
                    *allow_multi_profile_faces,
                );
                FeatureOperation::Extrude(Box::new(new::Extrude {
                    profile: selections::profile(profile, sketches)?,
                    loops: None,
                    direction: travel,
                    origin,
                    span: Some(extent(span)?),
                    draft: None,
                    fit: None,
                    outside: None,
                    sheet_cut: None,
                    output: output(*op, *solid),
                    native_options: BTreeMap::default(),
                    source_settings: settings,
                }))
            }
            LegacyOperation::Revolve { construction, op } => {
                let cadmpeg_ir::features::revolve::RevolveConstruction::Resolved {
                    profile,
                    axis: about,
                    extent,
                    solid,
                    face_maker,
                    fuse_order,
                    allow_multi_profile_faces,
                } = construction
                else {
                    return Err("the revolve construction is partial");
                };
                if face_maker.is_some() || fuse_order.is_some() {
                    return Err("the revolve construction settings have no v11 field");
                }
                let mut about_axis = axis(about.origin, about.direction)?;
                if let Some(path) = &about.reference {
                    about_axis.inputs.push(reference(selections::path(path)?));
                }
                let mut settings = BTreeMap::default();
                setting(
                    &mut settings,
                    cadmpeg_core::nonblank_literal!("inventor:allow_multi_profile_faces"),
                    *allow_multi_profile_faces,
                );
                FeatureOperation::Revolve(Box::new(new::Revolve {
                    profile: selections::planar(profile, sketches)?,
                    loops: None,
                    axis: Some(about_axis),
                    span: Some(revolve_span(extent)?),
                    fit: None,
                    output: output(*op, *solid),
                    native_options: BTreeMap::default(),
                    source_settings: settings,
                }))
            }
            LegacyOperation::Sweep {
                shape,
                path,
                orientation,
                transition,
                transformation,
                path_tangent,
                linearize,
                twist,
                path_extent,
                guide_rail,
                taper,
                scale,
                allow_multi_profile_faces,
            } => {
                if *path_tangent || *linearize || transformation.is_some() || scale.is_some() {
                    return Err("the sweep controls require additional v11 laws");
                }
                let (section, output) = match shape {
                    cadmpeg_ir::features::sweeps::SweepShape::Solid {
                        op,
                        section: cadmpeg_ir::features::sweeps::SweepSection::Profile(profile),
                        sections,
                    } if sections.is_empty() => (
                        profile,
                        output(
                            match op {
                                cadmpeg_ir::features::sweeps::SolidSweepOperation::NewBody => {
                                    cadmpeg_ir::features::operation_settings::BooleanOp::NewBody
                                }
                                cadmpeg_ir::features::sweeps::SolidSweepOperation::Join => {
                                    cadmpeg_ir::features::operation_settings::BooleanOp::Join
                                }
                                cadmpeg_ir::features::sweeps::SolidSweepOperation::Cut => {
                                    cadmpeg_ir::features::operation_settings::BooleanOp::Cut
                                }
                                cadmpeg_ir::features::sweeps::SolidSweepOperation::Intersect => {
                                    cadmpeg_ir::features::operation_settings::BooleanOp::Intersect
                                }
                            },
                            Some(true),
                        ),
                    ),
                    cadmpeg_ir::features::sweeps::SweepShape::Surface {
                        section: cadmpeg_ir::features::sweeps::SweepSection::Profile(profile),
                        sections,
                    } if sections.is_empty() => (
                        profile,
                        output(
                            cadmpeg_ir::features::operation_settings::BooleanOp::Unresolved,
                            Some(false),
                        ),
                    ),
                    _ => return Err("the sweep section has no single v11 profile"),
                };
                let orient = match orientation {
                    None => None,
                    Some(cadmpeg_ir::features::sweeps::SweepOrientation::CorrectedFrenet {}) => {
                        Some(new::SweepOrientation::MinimumTwist)
                    }
                    Some(cadmpeg_ir::features::sweeps::SweepOrientation::Fixed {}) => {
                        Some(new::SweepOrientation::Fixed)
                    }
                    Some(cadmpeg_ir::features::sweeps::SweepOrientation::Frenet {}) => {
                        Some(new::SweepOrientation::Frenet)
                    }
                    _ => return Err("the sweep orientation requires additional v11 construction"),
                };
                let path_extent_value =
                    |value: &cadmpeg_ir::features::sweeps::SweepPathExtent| new::PathExtent {
                        along: Some(Dimension::Literal(value.along_fraction)),
                        against: Some(Dimension::Literal(value.against_fraction)),
                    };
                let mut settings = BTreeMap::default();
                setting(
                    &mut settings,
                    cadmpeg_core::nonblank_literal!("inventor:allow_multi_profile_faces"),
                    *allow_multi_profile_faces,
                );
                FeatureOperation::Sweep(Box::new(new::Sweep {
                    section: new::SweepSection::Profile(selections::planar(section, sketches)?),
                    path: selections::path(path.as_ref().ok_or("the sweep path is absent")?)?,
                    controls: new::SweepControls {
                        orientation: orient,
                        twist: twist.map(|value| new::Twist::Angle(Dimension::Literal(value))),
                        scaling: taper
                            .map(|value| new::ScalingLaw::Taper(Dimension::Literal(value))),
                    },
                    guides: guide_rail
                        .as_ref()
                        .map(|value| -> Projected<_> {
                            Ok(new::SweepGuide {
                                path: selections::path(&value.path)?,
                                extent: Some(path_extent_value(&value.extent)),
                            })
                        })
                        .transpose()?
                        .into_iter()
                        .collect(),
                    extent: path_extent.as_ref().map(path_extent_value),
                    placement: None,
                    limits: new::SpineLimits {
                        start: None,
                        end: None,
                    },
                    transition: transition.map(|value| match value {
                        cadmpeg_ir::features::sweeps::SweepTransition::Transformed => {
                            new::CornerTransition::Carried
                        }
                        cadmpeg_ir::features::sweeps::SweepTransition::RightCorner => {
                            new::CornerTransition::Mitre
                        }
                        cadmpeg_ir::features::sweeps::SweepTransition::RoundCorner => {
                            new::CornerTransition::Round
                        }
                    }),
                    output,
                    native_options: BTreeMap::default(),
                    source_settings: settings,
                }))
            }
            LegacyOperation::Loft {
                sections,
                guidance,
                op,
                closed,
                solid,
                ruled,
                linearize,
                max_degree,
                allow_multi_profile_faces,
            } => {
                if *closed || *ruled || *linearize {
                    return Err(
                        "the loft closure, approximation, and ruled controls require a source reading",
                    );
                }
                let mut profiles = Vec::new();
                for section in sections {
                    let cadmpeg_ir::features::profiles::LoftSection::Profile(profile) = section
                    else {
                        return Err("the loft point section requires a v11 end construction");
                    };
                    profiles.push(new::Section {
                        form: new::SectionForm::Profile(selections::profile(profile, sketches)?),
                        flip: None,
                        seam: None,
                        condition: None,
                        trim_support: None,
                        spine_point: None,
                    });
                }
                let (spine, guides) = match guidance {
                    old::LoftGuidance::Guides(paths) => (
                        None,
                        paths
                            .iter()
                            .map(|path| {
                                Ok(new::Guide {
                                    path: selections::path(path)?,
                                    tangency: None,
                                    influence: None,
                                })
                            })
                            .collect::<Projected<_>>()?,
                    ),
                    old::LoftGuidance::Centerline(path) => (
                        Some(new::Spine::Centerline {
                            path: selections::path(path)?,
                            sections: None,
                            stations: Vec::new(),
                        }),
                        Vec::new(),
                    ),
                };
                let mut settings = BTreeMap::default();
                setting(
                    &mut settings,
                    cadmpeg_core::nonblank_literal!("inventor:allow_multi_profile_faces"),
                    *allow_multi_profile_faces,
                );
                FeatureOperation::Loft(Box::new(new::Loft {
                    start: None,
                    sections: profiles,
                    end: None,
                    cross: Vec::new(),
                    spine,
                    guides,
                    correspondence: None,
                    shape: new::LoftShape {
                        style: None,
                        max_degree: max_degree.map(Dimension::Literal),
                        rebuild: None,
                        ruled: None,
                        closed: None,
                        relimit: None,
                        merge_tangent_faces: None,
                        smoothing: None,
                        section_law: None,
                    },
                    output: output(*op, Some(*solid)),
                    native_options: BTreeMap::default(),
                    source_settings: settings,
                }))
            }
            _ => return Err("unhandled"),
        })
    })())
}
