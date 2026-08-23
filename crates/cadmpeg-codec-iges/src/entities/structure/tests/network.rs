// SPDX-License-Identifier: Apache-2.0

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::loss::IgesLossCode;
use crate::test_support::{owned_test_file_with_global, OwnedTestEntity};
use crate::IgesCodec;

#[test]
fn reference_designator_and_template_default_to_null_in_v4_and_v5() {
    let global_v4 = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,13H260714.000000,0.001,1000.0,6Hauthor,3Horg,6,0;";
    let global_v5 = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,13H260714.000000,0.001,1000.0,6Hauthor,3Horg,8,0,0H;";

    for global in [&global_v4[..], &global_v5[..]] {
        let result = IgesCodec
            .decode(
                &mut Cursor::new(owned_test_file_with_global(
                    &[
                        OwnedTestEntity {
                            entity_type: 320,
                            form: 0,
                            label: "NETWORK".into(),
                            status: "00000200",
                            parameters: "320,0,3HNET,0,1,,,0;".into(),
                        },
                        OwnedTestEntity {
                            entity_type: 420,
                            form: 0,
                            label: "INSTANCE".into(),
                            status: "00000000",
                            parameters: "420,1,0,0,0,1,,,1,,,0;".into(),
                        },
                    ],
                    global,
                )),
                &DecodeOptions::default(),
            )
            .unwrap();
        let native = result.ir().native.namespace("iges").unwrap();

        assert_eq!(native.arenas["network_definitions"].len(), 1);
        assert_eq!(native.arenas["network_instances"].len(), 1);
        let definition = &native.arenas["network_definitions"][0];
        assert!(definition.fields()["primary_reference_designator"].is_null());
        assert!(definition.fields()["display_template"].is_null());
        let instance = &native.arenas["network_instances"][0];
        assert_eq!(instance.fields()["type_flag"], 1);
        assert!(instance.fields()["primary_reference_designator"].is_null());
        assert!(instance.fields()["display_template"].is_null());
        assert!(!result
            .report()
            .losses
            .iter()
            .any(|loss| loss.code == IgesLossCode::EntityNotProjected.kind()));
    }
}
