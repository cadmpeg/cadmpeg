// SPDX-License-Identifier: Apache-2.0
//! Pattern seeds, runs, and omissions.

use std::collections::BTreeMap;

use cadmpeg_ir::design::construction::{kinds, Construction, PlaneValue};
use cadmpeg_ir::design::dimension::Dimension;
use cadmpeg_ir::design::names::InstanceIndex;
use cadmpeg_ir::design::ops::{
    body as new,
    shared::{LinearRun, PatternLayout, PatternRun, RunSides, Spacing},
};
use cadmpeg_ir::features::geometry::{FeatureDirection3, FinitePoint3};
use cadmpeg_ir::features::patterns::{PatternKind, PatternSeed, PatternTransform};
use cadmpeg_ir::math::Vector3;
use cadmpeg_ir::scalar::Length;
use cadmpeg_ir::units::UnitVector3;

use super::super::{selections, swept, Projected};

pub(super) fn plane(
    origin: FinitePoint3,
    normal: FeatureDirection3,
) -> Projected<Construction<kinds::Plane>> {
    let normal = swept::direction(normal)?
        .resolved
        .ok_or("the mirror normal is absent")?;
    let n = normal.as_raw();
    let basis = if n.x.abs() <= n.y.abs() && n.x.abs() <= n.z.abs() {
        Vector3::new(1.0, 0.0, 0.0)
    } else if n.y.abs() <= n.z.abs() {
        Vector3::new(0.0, 1.0, 0.0)
    } else {
        Vector3::new(0.0, 0.0, 1.0)
    };
    Ok(Construction {
        resolved: Some(PlaneValue {
            origin,
            normal,
            x_axis: UnitVector3::normalized(n.cross(basis))
                .ok_or("the mirror frame is degenerate")?,
        }),
        inputs: Vec::new(),
        after: Vec::new(),
        solution: None,
        mode: None,
        resolved_in: BTreeMap::default(),
    })
}

fn run(
    spacing: cadmpeg_ir::scalar::PositiveLength,
    count: u32,
    symmetric: bool,
) -> Projected<PatternRun<Length>> {
    if symmetric {
        return Err("the alternating symmetric pattern indices have no equivalent v11 run");
    }
    Ok(PatternRun {
        spacing: Spacing::Pitch {
            count: Dimension::Literal(count),
            pitch: Dimension::Literal(
                Length::new(spacing.get()).ok_or("the pattern pitch is invalid")?,
            ),
        },
        sides: Some(RunSides::One),
        seed: None,
        increments: Vec::new(),
    })
}

pub(super) fn project(seeds: &[PatternSeed], pattern: &PatternKind) -> Projected<new::Pattern> {
    let seeds = if seeds
        .iter()
        .all(|seed| matches!(seed, PatternSeed::Feature(_)))
    {
        new::PatternSeeds::Features {
            features: selections::selection(
                seeds
                    .iter()
                    .map(|seed| match seed {
                        PatternSeed::Feature(id) => Ok(selections::picked(
                            cadmpeg_ir::design::selection::PickTarget::FeatureResult(id.clone()),
                        )),
                        _ => Err("the pattern mixes seed kinds"),
                    })
                    .collect::<Projected<_>>()?,
            )?,
            method: None,
        }
    } else if let [PatternSeed::Bodies(bodies)] = seeds {
        new::PatternSeeds::Bodies {
            bodies: selections::set(selections::bodies(bodies)?)?,
            join: None,
        }
    } else if let [PatternSeed::Faces(faces)] = seeds {
        new::PatternSeeds::Faces(selections::face(faces)?)
    } else {
        return Err("the pattern mixes seed kinds or has untyped seeds");
    };
    let mut overrides = Vec::new();
    let layout = match pattern.definition() {
        PatternTransform::Linear {
            direction,
            spacing,
            count,
            second,
            omitted_instances,
            symmetric,
        } => {
            for index in omitted_instances {
                overrides.push(new::InstanceOverride {
                    index: InstanceIndex(
                        index
                            .iter()
                            .map(|value| {
                                i32::try_from(*value)
                                    .map_err(|_| "the pattern index is outside the v11 range")
                            })
                            .collect::<Projected<_>>()?,
                    ),
                    change: new::InstanceChange::Skipped,
                });
            }
            PatternLayout::Linear {
                first: LinearRun {
                    direction: swept::direction(
                        direction.ok_or("the pattern direction is absent")?,
                    )?,
                    run: run(*spacing, *count, *symmetric)?,
                },
                second: second
                    .as_ref()
                    .map(|value| -> Projected<_> {
                        Ok(LinearRun {
                            direction: swept::direction(value.direction)?,
                            run: run(value.spacing, value.count, value.symmetric)?,
                        })
                    })
                    .transpose()?,
                seed_only: second.as_ref().map(|_| Dimension::Literal(false)),
            }
        }
        PatternTransform::Circular {
            axis_origin,
            axis_dir,
            angle,
            count,
        } => PatternLayout::Circular {
            axis: swept::axis(*axis_origin, *axis_dir)?,
            angular: PatternRun {
                spacing: Spacing::Span {
                    count: Dimension::Literal(*count),
                    span: Dimension::Literal(
                        cadmpeg_ir::scalar::Angle::new(angle.get())
                            .ok_or("the pattern angle is invalid")?,
                    ),
                },
                sides: Some(RunSides::One),
                seed: None,
                increments: Vec::new(),
            },
            radial: None,
            orientation: None,
        },
        PatternTransform::Mirror {
            plane_origin,
            plane_normal,
        } => PatternLayout::Mirror {
            plane: plane(*plane_origin, *plane_normal)?,
            stitch_tolerance: None,
        },
        _ => return Err("the pattern layout requires additional v11 operands"),
    };
    Ok(new::Pattern {
        seeds,
        layout,
        instances: new::PatternInstances::Implicit(overrides),
        native_options: BTreeMap::default(),
        source_settings: BTreeMap::default(),
    })
}
