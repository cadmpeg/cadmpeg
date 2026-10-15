// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::CodecBackend;
use cadmpeg_ir::geometry::ProceduralSurfaceDefinition;

use crate::test_support::test_owned::{owned_test_file, OwnedTestEntity};
use crate::IgesCodec;

const ATTACHMENTS: usize = 4096;

#[derive(Clone, Copy)]
enum Route {
    Ruled,
    TabulatedNurbs,
    TabulatedExact,
    RevolutionNurbs,
    RevolutionExact,
    Nurbs,
}

fn entity(kind: i64, form: i64, parameters: &str, subordinate: bool) -> OwnedTestEntity {
    OwnedTestEntity {
        entity_type: kind,
        form,
        label: "BATCH".into(),
        status: if subordinate { "00010000" } else { "00000000" },
        parameters: parameters.into(),
    }
}

impl Route {
    fn inputs(self, count: usize) -> Vec<u8> {
        let line = |parameters| entity(110, 0, parameters, true);
        let hyperbola = || {
            entity(
                104,
                2,
                "104,0.25,0,-0.1111111111111111,0,0,-1,0,2,0,3.086161269630487,3.525603580931404;",
                true,
            )
        };
        let (mut inputs, output) = match self {
            Self::Ruled => (vec![line("110,0,0,0,1,0,0;"), line("110,0,1,0,1,1,0;")],
                (118, 1, "118,1,3,0,1;")),
            Self::TabulatedNurbs => (vec![line("110,0,0,0,1,0,0;")],
                (122, 0, "122,1,0,0,2;")),
            Self::TabulatedExact => (vec![hyperbola()],
                (122, 0, "122,1,3.086161269630487,3.525603580931404,2;")),
            Self::RevolutionNurbs => (vec![line("110,0,0,0,0,0,2;"), line("110,1,0,0,1,0,2;")],
                (120, 0, "120,1,3,0,1.5707963267948966;")),
            Self::RevolutionExact => (vec![line("110,0,0,0,0,0,2;"), hyperbola()],
                (120, 0, "120,1,3,0,1.5707963267948966;")),
            Self::Nurbs => (Vec::new(),
                (128, 0, "128,1,1,1,1,0,0,1,0,0,0,0,1,1,0,0,1,1,1,1,1,1,0,0,0,1,0,0,0,1,0,1,1,0,0,1,0,1;")),
        };
        inputs.extend((0..count).map(|_| entity(output.0, output.1, output.2, false)));
        owned_test_file(&inputs)
    }

    fn work(self, count: usize) -> u64 {
        let bytes = self.inputs(count);
        let run = |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::default();
            policy.limits.max_work_units = cap;
            let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy)
                .expect("fixture fits the default input budget");
            let decoded = IgesCodec
                .decode_impl(&ctx, root)
                .expect("thousands of surfaces fit the decode budget");
            assert_eq!(decoded.ir.model.surfaces.len(), count);
            assert_eq!(decoded.ir.model.procedural_surfaces.len(), count);
            for (carrier, construction) in decoded
                .ir
                .model
                .surfaces
                .iter()
                .zip(&decoded.ir.model.procedural_surfaces)
            {
                assert_eq!(
                    carrier.geometry.procedural_construction(),
                    Some(&construction.id)
                );
                assert_eq!(
                    carrier.geometry.solved().is_some(),
                    !matches!(self, Self::TabulatedExact | Self::RevolutionExact)
                );
                assert!(matches!(
                    (self, construction.definition()),
                    (Self::Ruled, ProceduralSurfaceDefinition::Ruled { .. })
                        | (
                            Self::TabulatedNurbs | Self::TabulatedExact,
                            ProceduralSurfaceDefinition::Extrusion(_)
                        )
                        | (
                            Self::RevolutionNurbs | Self::RevolutionExact,
                            ProceduralSurfaceDefinition::Revolution(_)
                        )
                        | (Self::Nurbs, ProceduralSurfaceDefinition::Exact(_))
                ));
            }
            let result = ctx.charge_work(1, "test completed surface batch work");
            ctx.finish(result)
        };
        run(DecodePolicy::default().limits.max_work_units).expect("default-budget decode");
        let error = cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::WorkUnits,
            "test completed surface batch work",
            run,
        );
        let CodecError::ResourceLimit(limit) = error else {
            panic!("expected work measurement");
        };
        assert_eq!(limit.additional, 1);
        limit.used
    }

    fn assert_linear(self) {
        let small = self.work(ATTACHMENTS / 2);
        let large = self.work(ATTACHMENTS);
        // Tree heights and amortized vector growth can add overhead. Repeated
        // population scans multiply work by four when the input doubles.
        assert!(
            large <= small * 5 / 2,
            "work grows faster than the indexed route: {small} -> {large}"
        );
    }
}

#[test]
fn ruled_surface_batch_decodes_thousands_with_linear_work() {
    Route::Ruled.assert_linear();
}

#[test]
fn tabulated_nurbs_batch_decodes_thousands_with_linear_work() {
    Route::TabulatedNurbs.assert_linear();
}

#[test]
fn tabulated_exact_batch_decodes_thousands_with_linear_work() {
    Route::TabulatedExact.assert_linear();
}

#[test]
fn revolution_nurbs_batch_decodes_thousands_with_linear_work() {
    Route::RevolutionNurbs.assert_linear();
}

#[test]
fn revolution_exact_batch_decodes_thousands_with_linear_work() {
    Route::RevolutionExact.assert_linear();
}

#[test]
fn nurbs_surface_batch_decodes_thousands_with_linear_work() {
    Route::Nurbs.assert_linear();
}
