// SPDX-License-Identifier: Apache-2.0

use std::collections::HashSet;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::geometry::pcurve::{LinePcurve, Pcurve, PcurveGeometry};
use cadmpeg_ir::geometry::{ProceduralSurface, ProceduralSurfaceDefinition};
use cadmpeg_ir::ids::{PcurveId, ProceduralSurfaceId, SurfaceId};
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::CadIr;

#[test]
fn protected_pcurve_root_filter_preserves_lookup_refusal() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=PCURVE('',#3,#4);#2=PCURVE('',#3,#4);#3=ITEM();#4=ITEM();ENDSEC;END-ISO-10303-21;";
    let (exchange, _) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("pcurve ownership exchange");
    let owned_id = PcurveId::try_from("step:data:pcurve#1").unwrap();
    let mut model = CadIr::empty();
    model.model.pcurves.push(Pcurve {
        id: owned_id.clone(),
        geometry: PcurveGeometry::Line(
            LinePcurve::try_new(Point2::new(0.0, 0.0), Point2::new(1.0, 0.0)).unwrap(),
        ),
        metadata: Default::default(),
    });
    model.model.procedural_surfaces.push(ProceduralSurface::new(
        ProceduralSurfaceId::try_from("step:data:bounded#5").unwrap(),
        ProceduralSurfaceDefinition::CurveBounded {
            support: SurfaceId::try_from("step:data:surface#3").unwrap(),
            boundaries: Vec::new(),
            boundary_pcurves: vec![owned_id],
            implicit_outer: false,
        },
        None,
    ));
    let mut limit = 0;
    loop {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(source, &arena, &policy).unwrap();
        let result = super::super::retain_unowned_carriers(
            &exchange,
            &mut model.clone(),
            &mut HashSet::new(),
            (&mut Vec::new(), &std::cell::RefCell::new(ctx.reserve_scoped(0, "report fixture").expect("scope"))),
            &ctx,
        );
        match result {
            Err(CodecError::ResourceLimit(refusal))
                if refusal.operation == "STEP protected pcurve root lookup" =>
            {
                assert_eq!(refusal.dimension, ResourceDimension::WorkUnits);
                assert_eq!(ctx.resource_refusal(), Some(refusal));
                break;
            }
            Err(CodecError::ResourceLimit(refusal)) => {
                limit = refusal
                    .used
                    .checked_add(refusal.additional)
                    .expect("bounded setup charge");
                assert!(limit <= DecodePolicy::service().limits.max_work_units);
            }
            _ => panic!("the fallible pcurve filter must propagate its lookup refusal"),
        }
    }
}
