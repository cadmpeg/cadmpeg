// SPDX-License-Identifier: Apache-2.0
//! Views, drawings, and view-dependent presentation relationships.

use super::geometry::{entity_loss, resolve_transform, Projection};
use crate::directory::DirectoryEntry;
use crate::global::Global;
use crate::parameter::{trailing_pointer_groups, ParameterRecord};
use cadmpeg_ir::CadIr;
use std::collections::{BTreeMap, BTreeSet};

fn finite_vector(record: &ParameterRecord, start: usize) -> Option<[f64; 3]> {
    let values = [
        record.number(start)?,
        record.number(start + 1)?,
        record.number(start + 2)?,
    ];
    values
        .iter()
        .all(|value| value.is_finite())
        .then_some(values)
}

fn norm_squared(vector: [f64; 3]) -> f64 {
    vector.iter().map(|value| value * value).sum()
}

pub(super) fn project(
    _ir: &mut CadIr,
    directory: &[DirectoryEntry],
    parameters: &[ParameterRecord],
    global: &Global,
) -> Projection {
    let records = parameters
        .iter()
        .map(|record| (record.directory_sequence, record))
        .collect::<BTreeMap<_, _>>();
    let entries = directory
        .iter()
        .map(|entry| (entry.sequence, entry))
        .collect::<BTreeMap<_, _>>();
    let mut handled = BTreeSet::new();
    let mut decoded = BTreeSet::new();
    let mut losses = Vec::new();

    for entry in directory
        .iter()
        .filter(|entry| entry.entity_type == 406 && matches!(entry.form, 16 | 17))
    {
        handled.insert(entry.sequence);
        let Some(record) = records.get(&entry.sequence).copied() else {
            losses.push(entity_loss(entry, "Parameter Data record is missing"));
            continue;
        };
        let valid = if entry.form == 16 {
            record.integer(1) == Some(2)
                && (2..=3).all(|index| {
                    record
                        .number(index)
                        .is_some_and(|value| value.is_finite() && value > 0.0)
                })
        } else {
            record.integer(1) == Some(2)
                && record
                    .integer(2)
                    .is_some_and(|value| matches!(value, 1..=11))
                && record.string(3).is_some_and(|value| !value.is_empty())
        };
        if valid {
            decoded.insert(entry.sequence);
        } else {
            losses.push(entity_loss(
                entry,
                "drawing size or unit property fields are invalid",
            ));
        }
    }

    for entry in directory
        .iter()
        .filter(|entry| entry.entity_type == 404 && matches!(entry.form, 0 | 1))
    {
        handled.insert(entry.sequence);
        let Some(record) = records.get(&entry.sequence).copied() else {
            losses.push(entity_loss(entry, "Parameter Data record is missing"));
            continue;
        };
        let view_count = record.count(1);
        let width = if entry.form == 0 { 3 } else { 4 };
        let views_valid = view_count.is_some_and(|count| {
            (0..count).all(|index| {
                let start = 2 + index * width;
                record
                    .integer(start)
                    .and_then(|value| u32::try_from(value).ok())
                    .and_then(|sequence| entries.get(&sequence).copied())
                    .is_some_and(|view| view.entity_type == 410 && view.status.subordinate == 2)
                    && (start + 1..=start + 2)
                        .all(|index| record.number(index).is_some_and(f64::is_finite))
                    && (entry.form == 0
                        || match record.tokens.get(start + 3).map(|token| &token.value) {
                            None | Some(crate::parameter::TokenValue::Omitted) => true,
                            _ => record.number(start + 3).is_some_and(f64::is_finite),
                        })
            })
        });
        let annotation_count_index = 2 + view_count.unwrap_or_default() * width;
        let annotation_count = record.count(annotation_count_index);
        let annotations_valid = annotation_count.is_some_and(|count| {
            (0..count).all(|index| {
                record
                    .integer(annotation_count_index + 1 + index)
                    .and_then(|value| u32::try_from(value).ok())
                    .and_then(|sequence| entries.get(&sequence).copied())
                    .is_some_and(|annotation| {
                        annotation.status.use_flag == 1 && annotation.status.subordinate == 1
                    })
            })
        });
        if views_valid && annotations_valid {
            decoded.insert(entry.sequence);
        } else {
            losses.push(entity_loss(
                entry,
                "drawing view placements or drawing-space annotations are invalid",
            ));
        }
    }

    for entry in directory
        .iter()
        .filter(|entry| entry.entity_type == 410 && matches!(entry.form, 0 | 1))
    {
        handled.insert(entry.sequence);
        let Some(record) = records.get(&entry.sequence).copied() else {
            losses.push(entity_loss(entry, "Parameter Data record is missing"));
            continue;
        };
        let view_number_valid = record.integer(1).is_some();
        let scale_valid = match record.tokens.get(2).map(|token| &token.value) {
            None | Some(crate::parameter::TokenValue::Omitted) => entry.form == 0,
            _ => record
                .number(2)
                .is_some_and(|value| value.is_finite() && value > 0.0),
        };
        let form_valid = if entry.form == 0 {
            let transform_valid = if entry.transform == 0 {
                true
            } else {
                u32::try_from(entry.transform).ok().is_some_and(|sequence| {
                    entries.get(&sequence).is_some_and(|target| {
                        target.entity_type == 124
                            && target.form == 0
                            && global.length_factor_mm().is_some_and(|factor| {
                                resolve_transform(
                                    entry.transform,
                                    &entries,
                                    &records,
                                    factor,
                                    &mut BTreeSet::new(),
                                )
                                .is_ok()
                            })
                    })
                })
            };
            let clipping_valid = (3..=8).all(|index| {
                record.integer(index).is_some_and(|value| {
                    value == 0
                        || u32::try_from(value).ok().is_some_and(|sequence| {
                            entries
                                .get(&sequence)
                                .is_some_and(|target| target.entity_type == 108)
                        })
                })
            });
            transform_valid && clipping_valid
        } else {
            let normal = finite_vector(record, 3);
            let reference = finite_vector(record, 6);
            let center = finite_vector(record, 9);
            let up = finite_vector(record, 12);
            let vectors_valid = normal.zip(up).is_some_and(|(normal, up)| {
                let normal_norm = norm_squared(normal);
                let up_norm = norm_squared(up);
                let dot = (0..3).map(|index| normal[index] * up[index]).sum::<f64>();
                normal_norm > 0.0 && up_norm > 0.0 && up_norm - dot * dot / normal_norm > 1.0e-20
            });
            let window_valid = (15..=19)
                .all(|index| record.number(index).is_some_and(f64::is_finite))
                && record
                    .number(16)
                    .zip(record.number(17))
                    .is_some_and(|(min, max)| min < max)
                && record
                    .number(18)
                    .zip(record.number(19))
                    .is_some_and(|(min, max)| min < max);
            let depth = record.integer(20).filter(|value| matches!(value, 0..=3));
            let depth_values_valid = (21..=22)
                .all(|index| record.number(index).is_some_and(f64::is_finite))
                && (depth != Some(3)
                    || record
                        .number(21)
                        .zip(record.number(22))
                        .is_some_and(|(min, max)| min < max));
            entry.transform == 0
                && reference.is_some()
                && center.is_some()
                && vectors_valid
                && window_valid
                && depth.is_some()
                && depth_values_valid
        };
        if view_number_valid && scale_valid && form_valid {
            decoded.insert(entry.sequence);
        } else {
            losses.push(entity_loss(
                entry,
                "view number, projection, transform, scale, or clipping fields are invalid",
            ));
        }
    }

    for entry in directory
        .iter()
        .filter(|entry| entry.entity_type == 402 && entry.form == 19)
    {
        handled.insert(entry.sequence);
        let Some(record) = records.get(&entry.sequence).copied() else {
            losses.push(entity_loss(entry, "Parameter Data record is missing"));
            continue;
        };
        let count = record.count(1).filter(|count| *count > 0);
        let mut last_view = None;
        let mut closed_views = BTreeSet::new();
        let mut last_breakpoint = None;
        let blocks_valid = count.is_some_and(|count| {
            (0..count).all(|index| {
                let start = 2 + index * 6;
                let view = record
                    .integer(start)
                    .and_then(|value| u32::try_from(value).ok())
                    .filter(|sequence| {
                        entries
                            .get(sequence)
                            .is_some_and(|target| target.entity_type == 410)
                    });
                if view != last_view {
                    if let Some(previous) = last_view {
                        closed_views.insert(previous);
                    }
                    last_breakpoint = None;
                }
                let view_order_valid = view.is_some_and(|view| !closed_views.contains(&view));
                let breakpoint = record.number(start + 1).filter(|value| value.is_finite());
                let breakpoint_order_valid = breakpoint
                    .is_some_and(|value| last_breakpoint.is_none_or(|previous| value > previous));
                last_view = view;
                last_breakpoint = breakpoint;
                let display_valid = record
                    .integer(start + 2)
                    .is_some_and(|value| matches!(value, 0..=1));
                let color_valid = match record.tokens.get(start + 3).map(|token| &token.value) {
                    None | Some(crate::parameter::TokenValue::Omitted) => true,
                    _ => record.integer(start + 3).is_some_and(|value| {
                        matches!(value, 0..=8)
                            || value
                                .checked_neg()
                                .and_then(|value| {
                                    u32::try_from(value).ok().filter(|sequence| {
                                        entries
                                            .get(sequence)
                                            .is_some_and(|target| target.entity_type == 314)
                                    })
                                })
                                .is_some()
                    }),
                };
                let font_valid = match record.tokens.get(start + 4).map(|token| &token.value) {
                    None | Some(crate::parameter::TokenValue::Omitted) => true,
                    _ => record.integer(start + 4).is_some_and(|value| {
                        matches!(value, 0..=5)
                            || value
                                .checked_neg()
                                .and_then(|value| {
                                    u32::try_from(value).ok().filter(|sequence| {
                                        entries
                                            .get(sequence)
                                            .is_some_and(|target| target.entity_type == 304)
                                    })
                                })
                                .is_some()
                    }),
                };
                let weight_valid = match record.tokens.get(start + 5).map(|token| &token.value) {
                    None | Some(crate::parameter::TokenValue::Omitted) => true,
                    _ => record.integer(start + 5).is_some_and(|value| value >= 0),
                };
                view_order_valid
                    && breakpoint_order_valid
                    && display_valid
                    && color_valid
                    && font_valid
                    && weight_valid
            })
        });
        if blocks_valid {
            decoded.insert(entry.sequence);
        } else {
            losses.push(entity_loss(
                entry,
                "segmented-view blocks, grouping, breakpoints, or display fields are invalid",
            ));
        }
    }

    for entry in directory
        .iter()
        .filter(|entry| entry.entity_type == 402 && matches!(entry.form, 3 | 4))
    {
        handled.insert(entry.sequence);
        let Some(record) = records.get(&entry.sequence).copied() else {
            losses.push(entity_loss(entry, "Parameter Data record is missing"));
            continue;
        };
        let view_count = record.count(1).filter(|count| *count > 0);
        let entity_count = record.count(2);
        let block_width = if entry.form == 3 { 1 } else { 5 };
        let views_valid = view_count.is_some_and(|count| {
            (0..count).all(|index| {
                let start = 3 + index * block_width;
                record
                    .integer(start)
                    .and_then(|value| u32::try_from(value).ok())
                    .and_then(|sequence| entries.get(&sequence).copied())
                    .is_some_and(|view| {
                        view.entity_type == 410
                            && records.get(&view.sequence).is_some_and(|view_record| {
                                trailing_pointer_groups(view_record, &entries).is_some_and(
                                    |groups| groups.associations.contains(&entry.sequence),
                                )
                            })
                    })
                    && (entry.form == 3 || {
                        let line_font = record.integer(start + 1);
                        let definition = record.integer(start + 2);
                        let color = record.integer(start + 3);
                        let weight = record.integer(start + 4);
                        line_font.is_some_and(|value| matches!(value, 0..=5))
                            && definition.is_some_and(|value| {
                                if line_font == Some(0) {
                                    u32::try_from(value).ok().is_some_and(|sequence| {
                                        entries
                                            .get(&sequence)
                                            .is_some_and(|target| target.entity_type == 304)
                                    })
                                } else {
                                    value == 0
                                }
                            })
                            && color.is_some_and(|value| {
                                matches!(value, 0..=8)
                                    || value
                                        .checked_neg()
                                        .and_then(|value| {
                                            u32::try_from(value).ok().filter(|sequence| {
                                                entries
                                                    .get(sequence)
                                                    .is_some_and(|target| target.entity_type == 314)
                                            })
                                        })
                                        .is_some()
                            })
                            && weight.is_some_and(|value| value >= 0)
                    })
            })
        });
        let entities_valid = view_count.zip(entity_count).is_some_and(|(views, count)| {
            (0..count).all(|index| {
                record
                    .integer(3 + views * block_width + index)
                    .and_then(|value| u32::try_from(value).ok())
                    .filter(|sequence| sequence % 2 == 1)
                    .is_some_and(|sequence| entries.contains_key(&sequence))
            })
        });
        if views_valid && entities_valid {
            decoded.insert(entry.sequence);
        } else {
            losses.push(entity_loss(
                entry,
                "view-visibility blocks, display overrides, entities, or back pointers are invalid",
            ));
        }
    }

    Projection {
        handled,
        decoded,
        losses,
    }
}
