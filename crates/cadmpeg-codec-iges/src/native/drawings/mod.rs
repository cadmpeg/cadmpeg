//! Neutral drawing sheets, views, and visibility from admitted IGES drawings.
//!
//! A 404 drawing becomes a Page drawing that links its views. A 410 view
//! becomes a View drawing positioned by its first drawing-sheet slot; the
//! slot origin scales by the model length factor, and the slot rotation is
//! read as radians. View visibility associativities list their displayed
//! entities on the viewed drawing. Segmented visibility blocks become view
//! presentations that retain the viewed record, breakpoint, and display
//! flag; per-segment line styles stay in the native block record.

use super::{NativeDrawing, NativeSegmentedVisibility, NativeView, NativeViewVisibility};
use crate::ids::{self, Stem};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::CadIr;
use std::collections::{BTreeMap, BTreeSet};

fn view_sequence(value: &str) -> Option<u32> {
    value.strip_prefix("iges:presentation:view#D")?.parse().ok()
}

pub(super) struct DrawingSources<'a> {
    pub drawings: &'a [NativeDrawing],
    pub views: &'a [NativeView],
    pub visibility: &'a [NativeViewVisibility],
    pub segmented: &'a [NativeSegmentedVisibility],
    pub admitted: &'a BTreeSet<u32>,
    pub length_factor: Option<f64>,
}

pub(super) fn project(
    ir: &mut CadIr,
    sources: &DrawingSources<'_>,
    ctx: &DecodeContext<'_>,
) -> Result<Vec<LossNote>, CodecError> {
    use cadmpeg_core::text::NonBlankString;
    use cadmpeg_ir::drawings::{Drawing, DrawingKind};
    use cadmpeg_ir::references::{ReferenceSelection, ReferenceTarget};

    let losses = Vec::new();
    let DrawingSources {
        drawings,
        views,
        visibility,
        segmented,
        admitted,
        length_factor,
    } = sources;
    let mut pages: BTreeMap<u32, &NativeDrawing> = BTreeMap::new();
    for record in *drawings {
        let Ok(sequence) = super::products::source_sequence(&record.source_entity) else {
            continue;
        };
        if admitted.contains(&sequence) {
            pages.insert(sequence, record);
        }
    }
    let mut ordered_views: BTreeMap<u32, &NativeView> = BTreeMap::new();
    for record in *views {
        let Ok(sequence) = super::products::source_sequence(&record.source_entity) else {
            continue;
        };
        if admitted.contains(&sequence) {
            ordered_views.insert(sequence, record);
        }
    }
    let mut placements: BTreeMap<u32, ([Option<f64>; 2], Option<f64>)> = BTreeMap::new();
    for record in pages.values() {
        for slot in &record.views {
            let Some(target) = slot.view.as_ref().and_then(|view| view_sequence(view)) else {
                continue;
            };
            placements
                .entry(target)
                .or_insert((slot.origin, slot.rotation));
        }
    }
    let mut view_drawings: BTreeMap<u32, cadmpeg_ir::drawings::DrawingId> = BTreeMap::new();
    let mut ordered: Vec<(u32, bool)> = pages
        .keys()
        .map(|sequence| (*sequence, true))
        .chain(ordered_views.keys().map(|sequence| (*sequence, false)))
        .collect();
    ordered.sort();
    let mut order = 0_u32;
    for (sequence, is_page) in ordered {
        let id = ids::drawing(&Stem::directory(sequence));
        if !is_page {
            view_drawings.insert(sequence, id.clone());
        }
        let record_id = if is_page {
            pages.get(&sequence).map(|record| record.id.clone())
        } else {
            ordered_views.get(&sequence).map(|record| record.id.clone())
        };
        let Some(native_ref) = record_id else {
            continue;
        };
        let mut drawing = Drawing::new(
            id,
            format!("iges:entity:directory#{sequence}"),
            if is_page {
                DrawingKind::Page
            } else {
                DrawingKind::View
            },
            if is_page {
                "iges:404".to_owned()
            } else {
                "iges:410".to_owned()
            },
            order,
            Some(native_ref),
        );
        order = order
            .checked_add(1)
            .ok_or_else(|| CodecError::malformed("IGES drawing order overflows"))?;
        if !is_page {
            if let Some(record) = ordered_views.get(&sequence) {
                if let Some(scale) = record
                    .scale
                    .and_then(cadmpeg_ir::scalar::FiniteReal::new)
                    .and_then(cadmpeg_ir::scalar::PositiveReal::from_finite)
                {
                    drawing.scale = Some(scale);
                }
                if let Some((origin, rotation)) = placements.get(&sequence) {
                    if let (Some(length_factor), [Some(x), Some(y)]) = (length_factor, *origin) {
                        if let Some(position) =
                            cadmpeg_ir::scalar::FiniteReal::new(x * length_factor)
                                .zip(cadmpeg_ir::scalar::FiniteReal::new(y * length_factor))
                                .and_then(|(x, y)| {
                                    cadmpeg_ir::units::FiniteVector::new([x.get(), y.get()])
                                })
                        {
                            drawing.position = Some(position);
                        }
                    }
                    if let Some(rotation) = *rotation {
                        if rotation.is_finite() {
                            drawing.rotation_degrees = cadmpeg_ir::scalar::FiniteReal::new(
                                rotation * 180.0 / std::f64::consts::PI,
                            );
                        }
                    }
                }
            }
        }
        ctx.charge_entities(1, "iges_drawings")?;
        ir.model.drawings.push(drawing);
    }
    let role =
        NonBlankString::new("views").ok_or_else(|| CodecError::malformed("views role is blank"))?;
    for record in pages.values() {
        let Some(drawing) = ir
            .model
            .drawings
            .iter_mut()
            .find(|drawing| drawing.native_ref.as_deref() == Some(record.id.as_str()))
        else {
            continue;
        };
        for slot in &record.views {
            let Some(target) = slot
                .view
                .as_ref()
                .and_then(|view| view_sequence(view))
                .and_then(|target| view_drawings.get(&target))
            else {
                continue;
            };
            drawing
                .relationships
                .entry(role.clone())
                .or_default()
                .push(ReferenceSelection::new(
                    ReferenceTarget::Local(target.as_str().to_owned()),
                    Vec::new(),
                ));
        }
    }
    let displays_role = NonBlankString::new("displays")
        .ok_or_else(|| CodecError::malformed("displays role is blank"))?;
    for record in *visibility {
        for display in &record.displays {
            let Some(target) = display
                .view
                .as_ref()
                .and_then(|view| view_sequence(view))
                .and_then(|target| view_drawings.get(&target))
            else {
                continue;
            };
            let Some(drawing) = ir
                .model
                .drawings
                .iter_mut()
                .find(|drawing| drawing.id == *target)
            else {
                continue;
            };
            for entity in record.entities.iter().flatten() {
                drawing
                    .relationships
                    .entry(displays_role.clone())
                    .or_default()
                    .push(ReferenceSelection::new(
                        ReferenceTarget::Local(entity.clone()),
                        Vec::new(),
                    ));
            }
        }
    }
    let mut presentation_order = 0_u32;
    for record in *segmented {
        let Ok(sequence) = super::products::source_sequence(&record.source_entity) else {
            continue;
        };
        if !admitted.contains(&sequence) {
            continue;
        }
        for (index, block) in record.blocks.iter().enumerate() {
            let Some(view) = block.view.as_ref() else {
                continue;
            };
            let mut properties = BTreeMap::new();
            if let Some(breakpoint) = block.breakpoint.filter(|value| value.is_finite()) {
                if let Some(name) = NonBlankString::new("breakpoint") {
                    properties.insert(name, format!("{breakpoint:?}"));
                }
            }
            if let Some(flag) = block.display_flag {
                if let Some(name) = NonBlankString::new("display_flag") {
                    properties.insert(name, format!("{flag}"));
                }
            }
            let id = cadmpeg_ir::presentation::PresentationId::mint(format!(
                "iges:model:view-presentation#D{sequence}-{index:03}"
            ))
            .map_err(|_| CodecError::malformed("IGES view presentation identity is invalid"))?;
            ctx.charge_entities(1, "iges_view_presentations")?;
            ir.model
                .view_presentations
                .push(cadmpeg_ir::presentation::ViewPresentation {
                    id,
                    object: Some(view.clone()),
                    order: presentation_order,
                    expanded: None,
                    visible: None,
                    display_mode: None,
                    selection_style: None,
                    line_width: None,
                    point_size: None,
                    properties,
                    native_ref: Some(record.id.clone()),
                });
            presentation_order = presentation_order
                .checked_add(1)
                .ok_or_else(|| CodecError::malformed("IGES view presentation order overflows"))?;
        }
    }
    Ok(losses)
}

#[cfg(test)]
mod tests;
