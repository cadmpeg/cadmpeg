// SPDX-License-Identifier: Apache-2.0
//! The part's sketches and their constraints (§6.1).

use std::collections::BTreeMap;

use cadmpeg_core::serde_erase::json;
use cadmpeg_core::text::NonBlankString;
use cadmpeg_core::CodecError;
use cadmpeg_ir::design::construction::{
    Construction, ConstructionInput, FrameValue, InputReference,
};
use cadmpeg_ir::design::dimension::Dimension;
use cadmpeg_ir::design::evaluation::EvaluationStatus;
use cadmpeg_ir::design::ops::sketch::{
    Planar, Sketch, SketchConstraint, SketchPlane, SketchRelation, SketchSolve,
};
use cadmpeg_ir::design::options::NativeOption;
use cadmpeg_ir::design::selection::{Anchor, Picks, Selection};
use cadmpeg_ir::features::FeatureId;
use cadmpeg_ir::sketches::{
    Sketch as LegacySketch, SketchConstraint as LegacyConstraint, SketchConstraintDefinitionInput,
    SketchId,
};
use cadmpeg_ir::units::UnitVector3;

/// A sketch owned by `owner`. Its plane is the frame the decoder reads
/// (`frame`), or one input whose picks are unread, anchored `at` (§5.8).
pub(super) fn sketch(
    id: SketchId,
    owner: FeatureId,
    native_ref: Option<String>,
    frame: Option<FrameValue>,
    at: Anchor,
) -> Sketch {
    let inputs = if frame.is_some() {
        Vec::new()
    } else {
        vec![ConstructionInput {
            role: None,
            reference: Some(InputReference::Picks(Selection {
                at,
                picks: Picks::Unread,
                native: None,
            })),
            value: None,
            reversed: None,
        }]
    };
    Sketch {
        id,
        owner,
        name: None,
        plane: SketchPlane::Placed(Box::new(Construction {
            inputs,
            after: Vec::new(),
            solution: None,
            mode: None,
            resolved: frame,
            resolved_in: BTreeMap::default(),
        })),
        solve: SketchSolve {
            state: None,
            status: EvaluationStatus::Current,
        },
        native_options: BTreeMap::new(),
        source_settings: BTreeMap::new(),
        native_ref,
    }
}

/// A constraint the decoder reads as a native kind: a `Native` relation of
/// that kind. Its native properties and operands are text options.
pub(super) fn constraint(
    legacy: &LegacyConstraint,
) -> Result<Option<SketchConstraint<Planar>>, CodecError> {
    let SketchConstraintDefinitionInput::Native {
        native_kind,
        native_properties,
        operands,
        ..
    } = legacy.definition.kind()
    else {
        return Ok(None);
    };
    let kind = NonBlankString::new(format!("catia:{}", native_kind.as_str()))
        .ok_or_else(|| CodecError::malformed("CATIA sketch constraint kind is blank"))?;
    let mut native_options = BTreeMap::new();
    for (name, text) in [
        (
            cadmpeg_ir::codec_name!("catia:properties"),
            json::to_string(native_properties),
        ),
        (
            cadmpeg_ir::codec_name!("catia:operands"),
            json::to_string(operands),
        ),
    ] {
        let text = text.map_err(|error| CodecError::malformed(error.to_string()))?;
        let text = NonBlankString::new(text)
            .ok_or_else(|| CodecError::malformed("CATIA sketch constraint text is blank"))?;
        native_options.insert(name, NativeOption::Text(Dimension::Literal(text)));
    }
    Ok(Some(SketchConstraint {
        id: legacy.id.clone(),
        sketch: legacy.sketch.clone(),
        relation: SketchRelation::Native {
            kind,
            operands: Vec::new(),
            values: Vec::new(),
        },
        name: legacy.name.clone(),
        active: legacy.active.map(Dimension::Literal),
        native_options,
        source_settings: BTreeMap::new(),
        native_ref: legacy.native_ref.clone(),
    }))
}

/// The frame of a sketch whose placement the decoder resolves: its origin,
/// its unit u axis as x, and its unit normal as z.
pub(super) fn frame(sketch: &LegacySketch) -> Option<FrameValue> {
    let (origin, normal, u_axis) = sketch.resolved_placement()?;
    let x_axis = UnitVector3::normalized_nonzero(u_axis)?;
    let z_axis = UnitVector3::normalized_nonzero(normal)?;
    Some(FrameValue {
        origin,
        x_axis,
        z_axis,
        y_axis: UnitVector3::normalized(z_axis.as_raw().cross(*x_axis.as_raw()))?,
    })
}
