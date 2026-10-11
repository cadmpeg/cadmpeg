// SPDX-License-Identifier: Apache-2.0
//! Shared raw associations across installed topology roots.

use std::fmt::Write as _;

use cadmpeg_core::decode::ResourceDimension;
use cadmpeg_core::CodecError;

fn shared_association_work(count: u64, seam: bool) -> u64 {
    let fixture = include_str!("../../../../tests/fixtures/ap214_sheet.p21");
    let (header, _) = fixture.split_once("DATA;\n").expect("fixture DATA section");
    let mut source = format!("{header}DATA;\n");
    for record in fixture.lines().filter(|line| line.starts_with('#')) {
        if ["#30=", "#31=", "#32=", "#33=", "#57="]
            .iter()
            .any(|prefix| record.starts_with(prefix))
        {
            continue;
        }
        if seam && record.starts_with("#22=") {
            writeln!(
                source,
                "#22=SEAM_EDGE('',*,*,#19,.T.,#{});",
                10_000 + count - 1
            )
            .expect("append explicit seam");
        } else {
            writeln!(source, "{record}").expect("append carrier");
        }
    }
    let mut pcurves = Vec::new();
    for index in 0..count {
        let pcurve = 10_000 + index;
        writeln!(source, "#{pcurve}=PCURVE('',#28,#55);").expect("append associated pcurve");
        pcurves.push(format!("#{pcurve}"));
        let root = 100_000 + index;
        let shell = 200_000 + index;
        writeln!(
            source,
            "#{root}=SHELL_BASED_SURFACE_MODEL('',(#{shell}));\n#{shell}=OPEN_SHELL('',(#29));"
        )
        .expect("append root");
    }
    writeln!(
        source,
        "#57=SURFACE_CURVE('',#16,({}),.PCURVE_S1.);\nENDSEC;\nEND-ISO-10303-21;",
        pcurves.join(",")
    )
    .expect("append shared associations");
    let (exchange, _) =
        crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner)
            .expect("associated roots parse");
    let setup = cadmpeg_test_support::service_decode_context();
    let mut ir = cadmpeg_ir::CadIr::empty();
    crate::reader::geometry::decode(&exchange, &mut ir, &setup)
        .expect("association carrier geometry");
    let carriers =
        crate::reader::index::CarrierIndex::from_ir(&ir, &setup).expect("predecoded carriers");
    let ctx = cadmpeg_test_support::service_decode_context();
    let outcome =
        super::super::decode(&exchange, &mut ir, &carriers, &ctx).expect("ambiguous roots commit");
    let roots = usize::try_from(count).expect("root count fits");
    assert_eq!(ir.model.bodies.len(), roots);
    assert_eq!(ir.model.faces.len(), roots);
    if seam {
        assert_eq!(
            ir.model
                .coedges
                .iter()
                .filter(|coedge| !coedge.pcurves.is_empty())
                .count(),
            roots
        );
        let expected = format!("step:data:pcurve#{}", 10_000 + count - 1);
        assert!(ir
            .model
            .coedges
            .iter()
            .flat_map(|coedge| &coedge.pcurves)
            .all(|use_| use_.pcurve.as_str() == expected));
        assert!(outcome.losses.is_empty());
    } else {
        assert!(ir
            .model
            .coedges
            .iter()
            .all(|coedge| coedge.pcurves.is_empty()));
        assert_eq!(
            outcome
                .losses
                .iter()
                .filter(|loss| loss.code
                    == crate::loss::StepLossCode::PcurveAssociationAmbiguous.kind())
                .count(),
            roots
        );
        assert!(outcome.losses.iter().all(|loss| loss.code==crate::loss::StepLossCode::PcurveAssociationAmbiguous.kind()
        && loss.message==format!("curve #57 associates {count} pcurves with surface #28; Part 42 provides no non-seam selector, so the coedge has no pcurve")));
    }
    let CodecError::ResourceLimit(limit) = ctx
        .charge_work(u64::MAX, "shared association root work probe")
        .expect_err("work probe")
    else {
        panic!("work refusal");
    };
    assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
    limit.used
}

#[test]
fn topology_roots_share_association_lists_without_rescanning_candidates() {
    let small = shared_association_work(128, false);
    let large = shared_association_work(256, false);
    assert!(
        large < 3 * small,
        "association work grew from {small} to {large}"
    );
}

#[test]
fn topology_roots_share_explicit_seam_membership_without_rescanning_lists() {
    let small = shared_association_work(128, true);
    let large = shared_association_work(256, true);
    assert!(
        large < 3 * small,
        "explicit seam work grew from {small} to {large}"
    );
}
