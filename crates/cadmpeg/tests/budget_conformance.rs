// SPDX-License-Identifier: Apache-2.0
//! Cross-codec resource-budget classification contracts.

use std::io::Cursor;

use cadmpeg_codec_catia::CatiaCodec;
use cadmpeg_codec_core::decode::{
    DecodeArena, DecodeContext, DecodePolicy, ResourceDimension, ResourceLimits,
};
use cadmpeg_codec_core::CodecError;
use cadmpeg_codec_creo::CreoCodec;
use cadmpeg_codec_freecad::FcstdCodec;
use cadmpeg_codec_nx::NxCodec;
use cadmpeg_codec_sldprt::SldprtCodec;
use cadmpeg_ir::codec::{CodecEntry, DecodeOptions};

const CATIA: &[u8] = include_bytes!("../../cadmpeg-fuzz/seeds/catia_container/standard_nested");
const CATIA_WORK: &[u8] = include_bytes!("fixtures/catia_tetrahedron_topology.CATPart");
const CREO: &[u8] = include_bytes!("../../cadmpeg-fuzz/seeds/creo_container/with_surface_rows");
const FREECAD: &[u8] =
    include_bytes!("../../cadmpeg-fuzz/seeds/fcstd_container/core_design_product.FCStd");
const NX: &[u8] = include_bytes!("fixtures/nx_topology_part.prt");
const SLDPRT: &[u8] = include_bytes!("fixtures/sldprt_triangle_body.sldprt");

#[derive(Clone, Copy)]
enum Starvation {
    Entities,
    CollectionItems,
    RecursionDepth,
    WorkUnits,
    DecompressedBytes,
    RetainedBytes,
}

impl Starvation {
    fn apply(self, limits: &mut ResourceLimits) {
        match self {
            Self::Entities => limits.max_entities = 0,
            Self::CollectionItems => limits.max_collection_items = 0,
            Self::RecursionDepth => limits.max_recursion_depth = 0,
            Self::WorkUnits => limits.max_work_units = 0,
            Self::DecompressedBytes => {
                limits.max_decompressed_bytes_total = 0;
                limits.max_decompressed_bytes_per_expand = 0;
            }
            Self::RetainedBytes => limits.max_retained_bytes = 0,
        }
    }

    const fn dimension(self) -> ResourceDimension {
        match self {
            Self::Entities => ResourceDimension::Entities,
            Self::CollectionItems => ResourceDimension::CollectionItems,
            Self::RecursionDepth => ResourceDimension::RecursionDepth,
            Self::WorkUnits => ResourceDimension::WorkUnits,
            Self::DecompressedBytes => ResourceDimension::DecompressedBytes,
            Self::RetainedBytes => ResourceDimension::RetainedBytes,
        }
    }
}

struct Case {
    name: &'static str,
    codec: &'static dyn CodecEntry,
    bytes: &'static [u8],
    starvation: Starvation,
}

static CATIA_CODEC: CatiaCodec = CatiaCodec;
static CREO_CODEC: CreoCodec = CreoCodec;
static FREECAD_CODEC: FcstdCodec = FcstdCodec;
static NX_CODEC: NxCodec = NxCodec;
static SLDPRT_CODEC: SldprtCodec = SldprtCodec;

fn cases() -> [Case; 8] {
    [
        Case {
            name: "catia/entities",
            codec: &CATIA_CODEC,
            bytes: CATIA,
            starvation: Starvation::Entities,
        },
        Case {
            name: "catia/work_units",
            codec: &CATIA_CODEC,
            bytes: CATIA_WORK,
            starvation: Starvation::WorkUnits,
        },
        Case {
            name: "creo/entities",
            codec: &CREO_CODEC,
            bytes: CREO,
            starvation: Starvation::Entities,
        },
        Case {
            name: "freecad/collection_items",
            codec: &FREECAD_CODEC,
            bytes: FREECAD,
            starvation: Starvation::CollectionItems,
        },
        Case {
            name: "freecad/recursion_depth",
            codec: &FREECAD_CODEC,
            bytes: FREECAD,
            starvation: Starvation::RecursionDepth,
        },
        Case {
            name: "freecad/retained_bytes",
            codec: &FREECAD_CODEC,
            bytes: FREECAD,
            starvation: Starvation::RetainedBytes,
        },
        Case {
            name: "nx/decompressed_bytes",
            codec: &NX_CODEC,
            bytes: NX,
            starvation: Starvation::DecompressedBytes,
        },
        Case {
            name: "sldprt/entities",
            codec: &SLDPRT_CODEC,
            bytes: SLDPRT,
            starvation: Starvation::Entities,
        },
    ]
}

#[test]
fn migrated_codec_paths_classify_starvation_identically_across_profiles() {
    for case in cases() {
        case.codec
            .decode(&mut Cursor::new(case.bytes), &DecodeOptions::default())
            .unwrap_or_else(|error| panic!("{} baseline failed: {error}", case.name));

        let mut observed = None;
        for mut policy in [DecodePolicy::desktop(), DecodePolicy::service()] {
            case.starvation.apply(&mut policy.limits);
            let error = case
                .codec
                .decode(
                    &mut Cursor::new(case.bytes),
                    &DecodeOptions {
                        container_only: false,
                        policy,
                    },
                )
                .expect_err("starved decode must fail");
            let CodecError::ResourceLimit(limit) = error else {
                panic!(
                    "{} returned the wrong error classification: {error}",
                    case.name
                );
            };
            assert_eq!(
                limit.dimension,
                case.starvation.dimension(),
                "{}",
                case.name
            );
            assert_eq!(observed.get_or_insert(limit.dimension), &limit.dimension);
        }
    }
}

#[test]
fn a_refusal_permanently_fuses_the_decode_session() {
    for policy in [DecodePolicy::desktop(), DecodePolicy::service()] {
        let arena = DecodeArena::new();
        let (ctx, _) =
            DecodeContext::from_root_bytes(b"root", &arena, &policy).expect("context construction");
        let requested = policy.limits.max_entities.saturating_add(1);
        assert!(matches!(
            ctx.charge_entities(requested, "force refusal"),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::Entities
        ));
        assert!(matches!(
            ctx.charge_entities(0, "post-refusal charge"),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::Entities
        ));
        assert!(matches!(
            ctx.finish_session(),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::Entities
        ));
    }
}
