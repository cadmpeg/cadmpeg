// SPDX-License-Identifier: Apache-2.0
//! Resource scaling of topology carrier lookups.

use std::fmt::Write;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::document::CadIr;

#[test]
fn pcurve_index_is_shared_across_shells_and_coedges() {
    let mut shells = String::new();
    let mut records = String::new();
    for id in 1_000..1_100 {
        if !shells.is_empty() {
            shells.push(',');
        }
        write!(shells, "#{id}").expect("write fixture shell");
        writeln!(records, "#{id}=OPEN_SHELL('',(#54));").expect("write fixture shell");
    }
    for id in 10_000..11_000 {
        writeln!(records, "#{id}=CARTESIAN_POINT('',(0.,0.,0.));").expect("write fixture point");
    }
    let source = include_str!("data/tp12_two_admissions.p21")
        .replace(
            "#56=SHELL_BASED_SURFACE_MODEL('',(#55));",
            &format!("#56=SHELL_BASED_SURFACE_MODEL('',({shells}));"),
        )
        .replace(
            "ENDSEC;\nEND-ISO-10303-21;",
            &format!("{records}ENDSEC;\nEND-ISO-10303-21;"),
        );
    let mut ir = CadIr::empty();
    let (exchange, _) =
        crate::test_support::with_service_context(source.as_bytes(), |bytes, ctx| {
            let (exchange, diagnostics) =
                crate::parse::parse_inner(bytes, ctx).expect("authored exchange parses");
            crate::reader::geometry::decode(&exchange, &mut ir, ctx)
                .expect("authored geometry fits the preparation policy");
            (exchange, diagnostics)
        });
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // One shared carrier index and all 100 drafts fit; 200 complete model
    // indexes would consume more than this allowance.
    policy.limits.max_collection_items = 100_000;
    let (ctx, _) = DecodeContext::from_root_bytes(source.as_bytes(), &arena, &policy)
        .expect("source fits policy");
    let carriers =
        crate::reader::index::CarrierIndex::from_ir(&ir, &ctx).expect("carrier lookup fits policy");
    super::super::decode(&exchange, &mut ir, &carriers, &ctx)
        .expect("all shells share the admitted pcurve index");
    assert_eq!(ir.model.bodies.len(), 100);
    assert_eq!(ir.model.faces.len(), 100);
    assert_eq!(ir.model.coedges.len(), 400);
    assert_eq!(
        ir.model
            .coedges
            .iter()
            .filter(|coedge| !coedge.pcurves.is_empty())
            .count(),
        200
    );
}
