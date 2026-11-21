// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use cadmpeg_ir::design::construction::{kinds, Construction};
use cadmpeg_ir::design::dimension::Dimension;
use cadmpeg_ir::design::feature::FeatureOperation;
use cadmpeg_ir::design::ops::dressup::{
    Bore, DepthTo, Hole, HoleBottom, HolePositions, HoleShape, ThreadSpec,
};
use cadmpeg_ir::design::ops::shared::Limit;
use cadmpeg_ir::design::options::NativeOptions;
use cadmpeg_ir::design::selection::{Anchor, OrderedSelection, Picks};
use cadmpeg_ir::features::holes::{
    HoleBottom as LegacyBottom, HoleConstruction, HoleKind, HolePlacement,
};
use cadmpeg_ir::features::{
    Feature, FeatureOperation as LegacyOperation, LinearTermination, NativeSelections,
};
use cadmpeg_ir::scalar::Length;
use cadmpeg_ir::units::UnitVector3;

pub(super) fn operation(
    source: &Feature,
    thread: ThreadSpec,
    native_options: NativeOptions,
) -> Option<FeatureOperation> {
    let LegacyOperation::Hole {
        shape,
        taper_angle,
        placements,
        face,
        direction,
        extent,
        bottom,
        ..
    } = source.evaluation.definition().operation()
    else {
        return None;
    };
    let point_angle = match shape.construction() {
        HoleConstruction::Form {
            kind: HoleKind::Simple,
            ..
        } => None,
        HoleConstruction::Form {
            kind: HoleKind::SimpleDrilled { drill_point_angle },
            ..
        } => Some(*drill_point_angle),
        _ => return None,
    };
    if shape.exit_kind().is_some() || taper_angle.is_some() {
        return None;
    }
    let native = NativeSelections::try_from(vec![source.native_ref.clone()?]).ok()?;
    let placed = placements.as_ref().and_then(|placements| {
        placements
            .iter()
            .map(|placement| match placement {
                HolePlacement::Directed { position, .. } => Some(Construction::<kinds::Point> {
                    resolved: Some(*position),
                    ..Default::default()
                }),
                HolePlacement::Axis { .. } => None,
            })
            .collect::<Option<Vec<_>>>()
    });
    let positions = placed.map_or_else(
        || HolePositions::Points {
            points: OrderedSelection {
                at: Anchor::Input,
                picks: Picks::Unread,
                native: Some(native),
            },
            centers: None,
        },
        HolePositions::Placed,
    );
    let extent = match extent {
        Some(LinearTermination::Blind { length }) => Some(Limit::Amount(Dimension::Literal(
            Length::new(length.get())?,
        ))),
        Some(LinearTermination::ThroughAll {}) => Some(Limit::ThroughAll),
        None => None,
        _ => return None,
    };
    let bottom = bottom
        .as_ref()
        .map(|bottom| match bottom {
            LegacyBottom::Flat => HoleBottom::Flat,
            LegacyBottom::Angled {
                included_angle,
                depth_to_tip,
            } => HoleBottom::DrillPoint {
                angle: Some(Dimension::Literal(*included_angle)),
                depth_to: Some(if *depth_to_tip {
                    DepthTo::Tip
                } else {
                    DepthTo::Shoulder
                }),
            },
        })
        .or_else(|| {
            point_angle.map(|angle| HoleBottom::DrillPoint {
                angle: Some(Dimension::Literal(angle)),
                depth_to: None,
            })
        });
    let placement_direction = placements.as_ref().and_then(|placements| {
        let HolePlacement::Directed { direction, .. } = placements.first()? else { return None; };
        placements.iter().all(|placement| matches!(placement, HolePlacement::Directed { direction: candidate, .. } if candidate == direction)).then_some(*direction)
    });
    let direction = direction
        .or(placement_direction)
        .and_then(|direction| UnitVector3::normalized(direction.get()))
        .map(|direction| Construction {
            resolved: Some(direction),
            ..Default::default()
        });
    Some(FeatureOperation::Hole(Box::new(Hole {
        positions,
        start: face
            .as_ref()
            .and_then(|face| super::selection(face, source.native_ref.as_deref()?)),
        direction,
        shape: HoleShape::Bore(Bore {
            diameter: thread.minor_diameter.clone(),
            head: None,
            taper: None,
            bottom,
            near_chamfer: None,
            neck_chamfer: None,
            far_chamfer: None,
        }),
        extent,
        standard: None,
        thread: Some(thread),
        targets: None,
        series: Vec::new(),
        native_options,
        source_settings: BTreeMap::new(),
    })))
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadmpeg_ir::features::{
        DistinctMembers, FeatureContent, FeatureDefinition, FeatureEvaluation, FeatureId,
    };
    use cadmpeg_ir::scalar::{NonZeroLength, PositiveLength};

    fn source() -> Feature {
        Feature {
            product_definition: None,
            id: FeatureId::mint("f3d:model:feature#hole").unwrap(),
            ordinal: 0,
            name: None,
            suppressed: None,
            dependencies: DistinctMembers::default(),
            source_properties: BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: FeatureContent::default(),
            evaluation: FeatureEvaluation::from_definition(FeatureDefinition::Operation(
                LegacyOperation::Hole {
                    profile: None,
                    profile_filter: None,
                    face: None,
                    direction: None,
                    placements: None,
                    shape: cadmpeg_ir::features::holes::HoleShape::new(
                        HoleConstruction::form(HoleKind::Simple),
                        None,
                        PositiveLength::new(10.0),
                    )
                    .unwrap(),
                    extent: Some(LinearTermination::Blind {
                        length: NonZeroLength::new(12.0).unwrap(),
                    }),
                    bottom: Some(LegacyBottom::Flat),
                    taper_angle: None,
                    allow_multi_profile_faces: None,
                },
            )),
            native_ref: Some("f3d:test:design-parameter-scope#1".to_owned()),
        }
    }

    #[test]
    fn a_hole_owned_thread_uses_hole_thread_and_the_minor_bore_diameter() {
        let construction = super::super::tests::construction();
        let thread = super::super::spec(
            &construction,
            Some(&cadmpeg_ir::features::CosmeticThreadExtent::Through {}),
        );
        let projected = operation(&source(), thread.clone(), BTreeMap::new()).unwrap();
        let FeatureOperation::Hole(hole) = projected else {
            panic!("Hole");
        };
        assert_eq!(hole.thread, Some(thread));
        assert_eq!(hole.standard, None);
        assert_eq!(
            hole.extent,
            Some(Limit::Amount(Dimension::Literal(
                Length::new(12.0).unwrap()
            )))
        );
        let HoleShape::Bore(bore) = hole.shape else {
            panic!("Bore");
        };
        assert_eq!(
            bore.diameter,
            Some(Dimension::Literal(PositiveLength::new(8.0).unwrap()))
        );
        assert_eq!(bore.bottom, Some(HoleBottom::Flat));
        let HolePositions::Points { points, .. } = hole.positions else {
            panic!("native positions");
        };
        assert_eq!(points.at, Anchor::Input);
        assert!(matches!(points.picks, Picks::Unread));
    }
}
