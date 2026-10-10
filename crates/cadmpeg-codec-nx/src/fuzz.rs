// SPDX-License-Identifier: Apache-2.0
//! `()`-returning wrappers over internal parsers for the `cadmpeg-fuzz` targets.
//!
//! Each wrapper feeds arbitrary bytes to one internal parser and discards the
//! result. The contract is that no input may panic.
#![doc(hidden)]

use crate::framing::node_kind::NodeKind;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

/// Desktop salvage ceilings for fuzz wrappers.
///
/// `DecodePolicy::service()` tightens collection and entity limits 8–16× and
/// would silently shrink coverage. Wrappers must not copy that profile.
fn fuzz_policy() -> DecodePolicy {
    DecodePolicy::default()
}

/// Exercise the NX deltas walker.
pub fn deltas(data: &[u8]) {
    let arena = DecodeArena::new();
    let policy = fuzz_policy();
    if let Ok((ctx, _)) = DecodeContext::from_root_bytes(data, &arena, &policy) {
        let census_resource_refused = match ctx.with_scoped_storage("NX fuzz deltas census", || {
            Ok::<_, cadmpeg_core::CodecError>(crate::deltas::census::walk(&ctx, data))
        }) {
            Ok((census, storage)) => {
                let resource_refused =
                    matches!(&census, Err(cadmpeg_core::CodecError::ResourceLimit(_)));
                drop(census);
                drop(storage);
                resource_refused
            }
            Err(_) => true,
        };
        if census_resource_refused {
            return;
        }
        let mid = data.len() / 2;
        drop(crate::deltas::unmatched_terminal_tombstones(
            &ctx,
            &data[..mid],
            &data[mid..],
        ));
    }
}

/// Exercise NX object-model indexed section framing.
pub fn om(data: &[u8]) -> Result<(), cadmpeg_core::CodecError> {
    let arena = DecodeArena::new();
    let policy = fuzz_policy();
    let (ctx, _) = DecodeContext::from_root_bytes(data, &arena, &policy)?;
    // discarded-value: marker constants and unit classification are fuzz probes.
    let _ = (
        crate::om_tokens::ROOT_MARKER,
        crate::om_tokens::HOST_GLOBALS,
        crate::om_tokens::CLASS_NAME_PREFIX,
        crate::om_tokens::NUMBER_PREFIX,
        // `unit_for` reads text. Bytes that state no text are the refusal this
        // wrapper passes through: the lookup does not run for them and nothing
        // stands in for the text they do not state. The walkers below still
        // see the whole input.
        ctx.validate_utf8(data, "NX fuzz token UTF-8")?
            .ok()
            .map(|token| crate::om_tokens::unit_for(&ctx, token))
            .transpose()?,
    );
    let mut at = 0;
    loop {
        ctx.charge_work(1, "NX fuzz compact tokens")?;
        let Some(token) = crate::om::compact::NullableCompactIndex::read(data, at) else {
            break;
        };
        at += token.raw().len();
    }
    let (_, indexed_sections_storage) =
        ctx.with_scoped_storage("NX fuzz indexed section output", || {
            let sections = crate::om::indexed_sections(&ctx, data)?;
            let mut remaining_sections = sections.iter();
            while !remaining_sections.as_slice().is_empty() {
                let Some(section) =
                    ctx.next_charged(&mut remaining_sections, "NX fuzz indexed sections")?
                else {
                    break;
                };
                drop(ctx.with_scoped_storage("NX fuzz numeric expressions", || {
                    section.numeric_expressions(&ctx)
                })?);
            }
            Ok::<_, cadmpeg_core::CodecError>(())
        })?;
    drop(indexed_sections_storage);
    let (_, sections_storage) = ctx.with_scoped_storage("NX fuzz framed section output", || {
        let sections = crate::om::sections(&ctx, data)?;
        let mut remaining_sections = sections.iter();
        while !remaining_sections.as_slice().is_empty() {
            let Some(section) =
                ctx.next_charged(&mut remaining_sections, "NX fuzz framed sections")?
            else {
                break;
            };
            drop(
                ctx.with_scoped_storage("NX fuzz operation body references", || {
                    section.operation_body_references(&ctx)
                })?,
            );
        }
        Ok::<_, cadmpeg_core::CodecError>(())
    })?;
    drop(sections_storage);
    Ok(())
}

/// Exercise NX analytic point extraction.
pub fn geometry_points(data: &[u8]) {
    with_geometry_context(data, |ctx| {
        drop(ctx.with_scoped_storage("NX fuzz geometry points", || {
            crate::geometry::points(ctx, data)
        }));
    });
}

/// Exercise NX analytic curve extraction.
pub fn geometry_curves(data: &[u8]) {
    with_geometry_context(data, |ctx| {
        drop(ctx.with_scoped_storage("NX fuzz geometry curves", || {
            crate::geometry::curves(ctx, data)
        }));
    });
}

/// Exercise NX analytic surface extraction.
pub fn geometry_surfaces(data: &[u8]) {
    with_geometry_context(data, |ctx| {
        drop(ctx.with_scoped_storage("NX fuzz geometry surfaces", || {
            crate::geometry::surfaces(ctx, data)
        }));
    });
}

fn with_geometry_context(data: &[u8], parse: impl FnOnce(&DecodeContext<'_>)) {
    let arena = DecodeArena::new();
    if let Ok((ctx, _)) = DecodeContext::from_root_bytes(data, &arena, &fuzz_policy()) {
        parse(&ctx);
    }
}

/// Exercise NX surface-intersection chart decoding.
pub fn intersection(data: &[u8]) {
    let arena = DecodeArena::new();
    let policy = fuzz_policy();
    let Ok((ctx, _)) = DecodeContext::from_root_bytes(data, &arena, &policy) else {
        return;
    };
    let _ = ctx.with_scoped_storage("NX fuzz intersection scan", || {
        if let Ok(scan) =
            crate::intersection::scan(&ctx, data, crate::intersection::ChartPointLayout::Xyz3)
        {
            let Ok(curves) = ctx.admit_iter(scan.curves, "NX fuzz intersection curves") else {
                return Ok(());
            };
            for curve in curves {
                // discarded-value: fuzz decoded curve fields without using their values.
                let _ = (curve.references, curve.pos);
            }
        }
        Ok::<_, cadmpeg_core::CodecError>(())
    });
}

/// Exercise NX NURBS curve extraction.
pub fn nurbs_curves(data: &[u8]) {
    let arena = DecodeArena::new();
    let policy = fuzz_policy();
    if let Ok((ctx, _)) = DecodeContext::from_root_bytes(data, &arena, &policy) {
        let _ = ctx.with_scoped_storage("NX fuzz NURBS curves", || {
            drop(crate::nurbs::curves(&ctx, data));
            Ok::<_, cadmpeg_core::CodecError>(())
        });
    }
}

/// Exercise NX NURBS surface extraction.
pub fn nurbs_surfaces(data: &[u8]) {
    let arena = DecodeArena::new();
    let policy = fuzz_policy();
    if let Ok((ctx, _)) = DecodeContext::from_root_bytes(data, &arena, &policy) {
        let _ = ctx.with_scoped_storage("NX fuzz NURBS surfaces", || {
            drop(crate::nurbs::surfaces(&ctx, data));
            Ok::<_, cadmpeg_core::CodecError>(())
        });
    }
}

/// Exercise NX Parasolid topology parsing.
pub fn topology(data: &[u8]) {
    let arena = DecodeArena::new();
    let policy = fuzz_policy();
    let Ok((ctx, _)) = DecodeContext::from_root_bytes(data, &arena, &policy) else {
        return;
    };
    let graph_stopped = match ctx.with_scoped_storage("NX fuzz topology graph", || {
        let graph = crate::topology::Graph::parse(&ctx, data);
        let mut stop = matches!(&graph, Err(cadmpeg_core::CodecError::ResourceLimit(_)));
        if let Ok(graph) = &graph {
            if let Ok(nodes) = ctx.admit_iter(graph.of_kind(NodeKind::Body), "NX fuzz body nodes") {
                for node in nodes {
                    // discarded-value: fuzz this bounded node read without using its value.
                    let _ = node.byte_at(0);
                    // discarded-value: fuzz this bounded scalar read without using its value.
                    let _ = node.f64_at(0);
                }
            } else {
                stop = true;
            }
        }
        Ok::<_, cadmpeg_core::CodecError>((stop, graph))
    }) {
        Ok(((stop, graph), storage)) => {
            drop(graph);
            drop(storage);
            stop
        }
        Err(_) => true,
    };
    if graph_stopped {
        return;
    }
    let Ok((result, storage)) = ctx.with_scoped_storage("NX fuzz composite curves", || {
        Ok::<_, cadmpeg_core::CodecError>(crate::topology::composite_curves(&ctx, data))
    }) else {
        return;
    };
    let resource_refused = matches!(&result, Err(cadmpeg_core::CodecError::ResourceLimit(_)));
    drop(result);
    drop(storage);
    if resource_refused {
        return;
    }
    let Ok((result, storage)) = ctx.with_scoped_storage("NX fuzz intersection curves", || {
        Ok::<_, cadmpeg_core::CodecError>(crate::topology::intersection_data_curves(&ctx, data))
    }) else {
        return;
    };
    let resource_refused = matches!(&result, Err(cadmpeg_core::CodecError::ResourceLimit(_)));
    drop(result);
    drop(storage);
    if resource_refused {
        return;
    }
    let Ok((result, storage)) = ctx.with_scoped_storage("NX fuzz blend surfaces", || {
        Ok::<_, cadmpeg_core::CodecError>(crate::topology::blend_surfaces(&ctx, data))
    }) else {
        return;
    };
    let resource_refused = matches!(&result, Err(cadmpeg_core::CodecError::ResourceLimit(_)));
    drop(result);
    drop(storage);
    if resource_refused {
        return;
    }
    let Ok((result, storage)) = ctx.with_scoped_storage("NX fuzz offset surfaces", || {
        Ok::<_, cadmpeg_core::CodecError>(crate::topology::offset_surfaces(&ctx, data))
    }) else {
        return;
    };
    let resource_refused = matches!(&result, Err(cadmpeg_core::CodecError::ResourceLimit(_)));
    drop(result);
    drop(storage);
    if resource_refused {
        return;
    }
    let Ok((result, storage)) = ctx.with_scoped_storage("NX fuzz surface curves", || {
        Ok::<_, cadmpeg_core::CodecError>(crate::topology::surface_curves(&ctx, data))
    }) else {
        return;
    };
    let resource_refused = matches!(&result, Err(cadmpeg_core::CodecError::ResourceLimit(_)));
    drop(result);
    drop(storage);
    if resource_refused {
        return;
    }
    let _ = ctx.with_scoped_storage("NX fuzz trimmed curves", || {
        drop(crate::topology::trimmed_curves(&ctx, data));
        Ok::<_, cadmpeg_core::CodecError>(())
    });
}

/// Exercise NX Parasolid stream extraction.
pub fn parasolid(data: &[u8]) {
    let arena = DecodeArena::new();
    let Ok((ctx, root)) = DecodeContext::from_root_bytes(data, &arena, &fuzz_policy()) else {
        return;
    };
    let _ = ctx.with_scoped_storage("NX fuzz Parasolid stream output", || {
        let Ok(container) = crate::container::scan_bytes(&ctx, data) else {
            return Ok(());
        };
        if let Ok(streams) = crate::parasolid::extract_streams(&ctx, root, &container) {
            let Ok(streams) = ctx.admit_iter(streams, "NX fuzz stream fields") else {
                return Ok(());
            };
            for stream in streams {
                let _ = stream.consumed;
            }
        }
        Ok::<_, cadmpeg_core::CodecError>(())
    });
}

#[cfg(test)]
mod tests {
    #[test]
    fn wrappers_accept_empty() {
        super::deltas(&[]);
        drop(super::om(&[]));
        super::geometry_points(&[]);
        super::geometry_curves(&[]);
        super::geometry_surfaces(&[]);
        super::intersection(&[]);
        super::nurbs_curves(&[]);
        super::nurbs_surfaces(&[]);
        super::topology(&[]);
        super::parasolid(&[]);
    }

    #[test]
    fn deltas_wrapper_accepts_fixture() {
        let stream = crate::test_support::test_deltas::status_framed_deltas_stream();
        super::deltas(&stream);
    }

    #[test]
    fn om_wrapper_accepts_fixture() {
        drop(super::om(
            &crate::test_support::test_om::indexed_om_section(),
        ));
        drop(super::om(
            &crate::test_support::test_om::size_framed_om_section(),
        ));
    }

    #[test]
    fn geometry_wrappers_accept_fixture() {
        let stream = crate::test_support::test_streams::partition_stream();
        super::geometry_points(&stream);
        super::geometry_curves(&stream);
        super::geometry_surfaces(&stream);
    }

    #[test]
    fn intersection_wrapper_accepts_fixture() {
        let stream =
            crate::test_support::test_streams::charted_intersection_curve_topology_partition_stream(
            );
        super::intersection(&stream);
    }

    #[test]
    fn nurbs_wrappers_accept_fixture() {
        let stream = crate::test_support::test_deltas::bspline_partition_stream();
        super::nurbs_curves(&stream);
        super::nurbs_surfaces(&stream);
    }

    #[test]
    fn topology_wrapper_accepts_fixture() {
        let stream = crate::test_support::test_streams::topology_partition_stream();
        super::topology(&stream);
    }

    #[test]
    fn parasolid_wrapper_accepts_fixture() {
        let bytes = crate::test_support::test_prt::single_part_prt();
        super::parasolid(&bytes);
    }

    /// Bytes that state no text are an input the wrapper carries to every
    /// parser below it. Only the token lookup that reads text is skipped, and
    /// nothing stands in for the text the input does not state.
    #[test]
    fn om_accepts_bytes_that_state_no_text() {
        let bytes: Vec<u8> = vec![0xff, 0xfe, 0x80, 0x41, 0x00, 0xc3, 0x28];
        assert!(
            std::str::from_utf8(&bytes).is_err(),
            "the fixture must state no text"
        );
        drop(super::om(&bytes));
    }
}
