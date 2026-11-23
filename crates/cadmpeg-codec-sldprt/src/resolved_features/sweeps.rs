//! Owned sweep path references and generated circular sections.

mod paths;
mod read;

use super::component_paths::component_feature_precedes;
use super::scalars::FeatureObjectNames;
use crate::loss::SldprtLossCode;
use crate::records::{FeatureHistory, FeatureInputLane};
use cadmpeg_ir::features::profiles::PathRef;
use cadmpeg_ir::features::sweeps::{
    GeneratedSweepSection, SolidSweepOperation, SweepCircularRegion, SweepSection, SweepShape,
};
use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureOperation, SketchFeatureBinding};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::scalar::PositiveLength;
use cadmpeg_ir::sketches::{
    Sketch, SketchEntity, SketchEntityId, SpatialSketchEntity, SpatialSketchEntityId,
};

pub(crate) struct Geometry<'a> {
    pub(crate) sketches: &'a [Sketch],
    pub(crate) entities: &'a mut Vec<SketchEntity>,
    pub(crate) spatial_entities: &'a mut Vec<SpatialSketchEntity>,
}

fn producer<'a>(
    source: u32,
    consumer: &crate::records::Feature,
    histories: &'a [FeatureHistory],
) -> Option<&'a crate::records::Feature> {
    let mut candidates = histories
        .iter()
        .flat_map(|history| &history.features)
        .filter(|feature| {
            feature.source_value() == Some(source) && feature.parent == consumer.parent
        });
    let feature = candidates.next()?;
    (candidates.next().is_none() && component_feature_precedes(feature.into(), consumer.into()))
        .then_some(feature)
}

fn bind_path(
    producer: &Feature,
    native: &str,
    lane: &FeatureInputLane,
    geometry: &mut Geometry<'_>,
) -> Option<(PathRef, Vec<paths::Assumption>)> {
    match producer.evaluation.definition() {
        FeatureDefinition::Operation(FeatureOperation::Sketch {
            sketch: SketchFeatureBinding::Planar(Some(sketch)),
        }) => {
            geometry
                .sketches
                .iter()
                .find(|value| &value.id == sketch)?
                .resolved_placement()?;
            let curves = paths::planar(lane, native)?;
            let assumed = assumptions(&curves);
            let mut ids = Vec::with_capacity(curves.len());
            let mut pending = Vec::new();
            for curve in curves {
                let mut represented = geometry.entities.iter().filter(|entity| {
                    entity.sketch == *sketch
                        && !entity.construction
                        && entity.native_ref.as_deref() == Some(curve.native.as_str())
                        && entity.geometry == curve.geometry
                });
                if let Some(existing) = represented.next() {
                    if represented.next().is_some() {
                        return None;
                    }
                    ids.push(existing.id().clone());
                    continue;
                }
                let id = SketchEntityId::mint(format!(
                    "{}:sweep-path:{}",
                    sketch.as_str(),
                    curve.offset
                ))
                .ok()?;
                if let Some(existing) = geometry.entities.iter().find(|entity| entity.id() == &id) {
                    if existing.geometry != curve.geometry || existing.sketch != *sketch {
                        return None;
                    }
                } else {
                    pending.push(
                        SketchEntity::new(id.clone(), sketch.clone(), curve.geometry)
                            .with_native_ref(Some(curve.native)),
                    );
                }
                ids.push(id);
            }
            let path = PathRef::sketch_curves(sketch.clone(), ids).ok()?;
            geometry.entities.extend(pending);
            Some((path, assumed))
        }
        FeatureDefinition::Operation(FeatureOperation::SpatialSketch {
            sketch: Some(sketch),
        }) => {
            let curves = paths::spatial(lane, native)?;
            let assumed = assumptions(&curves);
            let mut ids = Vec::with_capacity(curves.len());
            let mut pending = Vec::new();
            for curve in curves {
                let mut represented = geometry.spatial_entities.iter().filter(|entity| {
                    entity.sketch == *sketch
                        && !entity.construction
                        && entity.native_ref.as_deref() == Some(curve.native.as_str())
                        && entity.geometry == curve.geometry
                });
                if let Some(existing) = represented.next() {
                    if represented.next().is_some() {
                        return None;
                    }
                    ids.push(existing.id().clone());
                    continue;
                }
                let id = SpatialSketchEntityId::mint(format!(
                    "{}:sweep-path:{}",
                    sketch.as_str(),
                    curve.offset
                ))
                .ok()?;
                if let Some(existing) = geometry
                    .spatial_entities
                    .iter()
                    .find(|entity| entity.id() == &id)
                {
                    if existing.geometry != curve.geometry || existing.sketch != *sketch {
                        return None;
                    }
                } else {
                    pending.push(
                        SpatialSketchEntity::new(id.clone(), sketch.clone(), curve.geometry)
                            .with_native_ref(Some(curve.native)),
                    );
                }
                ids.push(id);
            }
            let path = PathRef::spatial_sketch_curves(sketch.clone(), ids).ok()?;
            geometry.spatial_entities.extend(pending);
            Some((path, assumed))
        }
        _ => None,
    }
}

fn assumptions<G>(curves: &[paths::Curve<G>]) -> Vec<paths::Assumption> {
    let mut assumed = curves
        .iter()
        .filter_map(|curve| curve.assumed)
        .collect::<Vec<_>>();
    assumed.sort_unstable();
    assumed.dedup();
    assumed
}

fn same_curves<G: PartialEq>(
    left: Option<Vec<paths::Curve<G>>>,
    right: Option<Vec<paths::Curve<G>>>,
) -> bool {
    match (left, right) {
        (Some(left), Some(right)) => left
            .iter()
            .map(|curve| &curve.geometry)
            .eq(right.iter().map(|curve| &curve.geometry)),
        _ => false,
    }
}

fn agrees(
    producer: &Feature,
    native: &str,
    first: &FeatureInputLane,
    other: &FeatureInputLane,
) -> bool {
    match producer.evaluation.definition() {
        FeatureDefinition::Operation(FeatureOperation::Sketch { .. }) => {
            same_curves(paths::planar(first, native), paths::planar(other, native))
        }
        FeatureDefinition::Operation(FeatureOperation::SpatialSketch { .. }) => {
            same_curves(paths::spatial(first, native), paths::spatial(other, native))
        }
        _ => false,
    }
}

pub(crate) fn bind(
    features: &mut [Feature],
    histories: &[FeatureHistory],
    lanes: &[FeatureInputLane],
    mut geometry: Geometry<'_>,
    losses: &mut Vec<LossNote>,
) {
    let records = crate::records::HistoryFeatures::new(histories);
    for index in 0..features.len() {
        let feature = &features[index];
        let FeatureDefinition::Operation(FeatureOperation::Sweep { path, .. }) =
            feature.evaluation.definition()
        else {
            continue;
        };
        let has_path = path.is_some();
        let Some(record) = feature
            .native_ref
            .as_deref()
            .and_then(|native| records.get(native))
        else {
            continue;
        };
        if !matches!(
            record.input_class.as_deref(),
            Some("moSweep_c" | "moSweepCut_c" | "moSweepRefSurface_c")
        ) {
            continue;
        }
        let mut votes = Vec::new();
        let mut unreadable = false;
        for lane in lanes {
            let names = FeatureObjectNames::new(&lane.names);
            if !names.has_candidate(record) {
                continue;
            }
            let Some(name) = names.of(record) else {
                votes.push((lane, None));
                continue;
            };
            let Ok(start) = usize::try_from(name.offset) else {
                votes.push((lane, None));
                continue;
            };
            let end = histories
                .iter()
                .flat_map(|history| &history.features)
                .filter_map(|candidate| names.of(candidate))
                .filter(|candidate| candidate.offset > name.offset)
                .filter_map(|candidate| usize::try_from(candidate.offset).ok())
                .min()
                .unwrap_or(lane.native_payload.len());
            let operand = read::path(lane, start, end);
            unreadable |= operand.is_none()
                && lane.classes.iter().any(|class| {
                    class.name == "moWireContact_c"
                        && usize::try_from(class.offset)
                            .is_ok_and(|offset| (start..end).contains(&offset))
                });
            votes.push((lane, operand));
        }
        let Some((lane, Some(operand))) = votes.first() else {
            if unreadable {
                losses.push(SldprtLossCode::FeatureSweepPathRetained.note(format!(
                    "{} retains its unreadable sweep path records.",
                    feature.id
                )));
            }
            continue;
        };
        if votes
            .iter()
            .any(|(_, candidate)| candidate.as_ref() != Some(operand))
        {
            losses.push(SldprtLossCode::FeatureSweepPathRetained.note(format!(
                "{} retains disagreeing sweep path records.",
                feature.id
            )));
            continue;
        }
        let lane_key = lane
            .id
            .rsplit_once('#')
            .map_or(lane.id.as_str(), |(_, key)| key);
        let bound = if has_path {
            None
        } else {
            let native = format!(
                "sldprt:feature-input:sweep-path:{lane_key}:{}",
                operand.offset
            );
            let selected = operand
                .source
                .and_then(|source| producer(source, record, histories));
            let selected = selected.and_then(|selected| {
                let mut matches = features
                    .iter()
                    .filter(|feature| feature.native_ref.as_deref() == Some(selected.id.as_str()));
                let feature = matches.next()?;
                matches
                    .next()
                    .is_none()
                    .then_some((feature, selected.id.as_str()))
            });
            let resolved = selected.and_then(|(feature, native)| {
                if votes
                    .iter()
                    .skip(1)
                    .any(|(other, _)| !agrees(feature, native, lane, other))
                {
                    return None;
                }
                bind_path(feature, native, lane, &mut geometry)
            });
            let dependency = selected.map(|(feature, _)| feature.id.clone());
            let (path, assumed) =
                resolved.unwrap_or_else(|| (PathRef::Native(native.clone()), Vec::new()));
            if matches!(path, PathRef::Native(_)) {
                losses.push(
                    SldprtLossCode::FeatureSweepPathRetained
                        .note(format!("{} retains its path record {native}.", feature.id)),
                );
            }
            for assumption in assumed {
                losses.push(match assumption {
                    paths::Assumption::IndexBasis => {
                        SldprtLossCode::SketchPointRecordIndexBasisAssumed.note(format!(
                            "{} reads path curve records on locus 05 00 01 00 by assumed zero-based point-record positions in their sgSketch.",
                            feature.id
                        ))
                    }
                    paths::Assumption::Roster => {
                        SldprtLossCode::FeatureSweepPathRosterAssumed.note(format!(
                            "{} reads path-line endpoints as positions in its sketch point roster.",
                            feature.id
                        ))
                    }
                });
            }
            if operand.header_assumed {
                losses.push(SldprtLossCode::FeatureSweepPathHeaderAssumed.note(format!(
                    "{} reads its path component source after a u32 header word.",
                    feature.id
                )));
            }
            Some((path, dependency))
        };
        let circular = operand
            .radius_mm
            .filter(|_| record.input_class.as_deref() != Some("moSweepRefSurface_c"))
            .and_then(PositiveLength::new)
            .and_then(|radius| SweepCircularRegion::new(radius, None).ok());
        let feature = &mut features[index];
        feature.evaluation.edit(|definition, _| {
            if let FeatureDefinition::Operation(FeatureOperation::Sweep {
                shape, path: slot, ..
            }) = definition
            {
                if let Some((path, _)) = bound.as_ref() {
                    *slot = Some(path.clone());
                }
                if let Some(region) = circular {
                    let op = match shape {
                        SweepShape::Solid { op, .. } => *op,
                        _ => SolidSweepOperation::Join,
                    };
                    let reading = if op == SolidSweepOperation::Join {
                        "with Merge enabled"
                    } else {
                        "without a Merge reading and keeps its recorded Boolean operation"
                    };
                    losses.push(SldprtLossCode::FeatureSweepMergeAssumed.note(format!(
                        "{} reads the generated solid sweep {reading}.",
                        record.id
                    )));
                    *shape = SweepShape::Solid {
                        op,
                        section: SweepSection::Generated(GeneratedSweepSection::CircularRegion {
                            region,
                        }),
                        sections: Vec::new(),
                    };
                }
            }
        });
        if let Some((_, Some(dependency))) = bound {
            feature.dependencies.insert(dependency);
        }
    }
}

#[cfg(test)]
mod tests;
