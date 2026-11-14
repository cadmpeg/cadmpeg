use crate::layout::{move_face_marker_one_prologue as one, move_face_marker_zero_prologue as zero};
use crate::test_support::{indexed_header, lp_utf16, push_marked_reference};
use std::io::{Cursor, Write};

pub(crate) const TRANSFORM_GUID: &str = "84E6245F-1CDC-4FB8-8048-163A85FAD3F0";
pub(crate) const OTHER_GUID: &str = "00000000-0000-4000-8000-000000000001";
pub(crate) const TRANSLATION: [[f64; 4]; 4] = [
    [1., 0., 0., 0.25],
    [0., 1., 0., -0.5],
    [0., 0., 1., 0.],
    [0., 0., 0., 1.],
];

#[derive(Clone)]
pub(crate) struct MoveFaceFrame {
    pub(crate) class: [u8; 3],
    pub(crate) matrix: [[f64; 4]; 4],
    pub(crate) form: u32,
    pub(crate) malformed: bool,
}

impl Default for MoveFaceFrame {
    fn default() -> Self {
        Self {
            class: *b"257",
            matrix: TRANSLATION,
            form: 5,
            malformed: false,
        }
    }
}

pub(crate) struct MoveFaceFixture {
    pub(crate) references: Vec<u32>,
    pub(crate) marker_one: bool,
    pub(crate) group_mark: Option<u32>,
    pub(crate) group_role: Option<u64>,
    pub(crate) prefix: u8,
    pub(crate) with_metadata: bool,
    pub(crate) types: Vec<(&'static str, u32)>,
    pub(crate) frames: Vec<MoveFaceFrame>,
}

impl Default for MoveFaceFixture {
    fn default() -> Self {
        Self {
            references: vec![50, 60, 71],
            marker_one: false,
            group_mark: Some(50),
            group_role: Some(0x0000_0010_0000_0000),
            prefix: 0,
            with_metadata: true,
            types: vec![
                ("21B67709-045F-4903-B3FA-FD82E3DC3ADA", 1),
                (TRANSFORM_GUID, 2),
                (OTHER_GUID, 2),
            ],
            frames: vec![MoveFaceFrame::default()],
        }
    }
}

impl MoveFaceFixture {
    pub(crate) fn archive(&self) -> Vec<u8> {
        let mut bytes = Vec::new();
        indexed_header(&mut bytes, *b"256", 17);
        bytes.resize(zero::MARKER, 0);
        bytes[zero::PREFIX_FIELDS] = self.prefix;
        bytes.push(u8::from(self.marker_one));
        if self.marker_one {
            bytes.extend_from_slice(&0_u32.to_le_bytes());
        }
        bytes.push(1);
        push_marked_reference(&mut bytes, 71);
        bytes.extend_from_slice(&1_u32.to_le_bytes());
        if let Some(group) = self.group_mark {
            push_marked_reference(&mut bytes, group);
        } else {
            bytes.extend_from_slice(&[0; 11]);
        }
        bytes.resize(
            if self.marker_one {
                one::REFERENCE_COUNT
            } else {
                zero::REFERENCE_COUNT
            },
            0,
        );
        bytes.extend_from_slice(&u32::try_from(self.references.len()).unwrap().to_le_bytes());
        for &reference in &self.references {
            push_marked_reference(&mut bytes, reference);
        }
        bytes.extend_from_slice(&7_u32.to_le_bytes());
        lp_utf16(&mut bytes, "MoveFace");
        bytes.extend_from_slice(&1_u32.to_le_bytes());
        bytes.extend_from_slice(&[0; 73]);
        indexed_header(&mut bytes, *b"259", 17);
        for frame in &self.frames {
            let start = bytes.len();
            indexed_header(&mut bytes, frame.class, 71);
            bytes.extend_from_slice(&[0; 10]);
            bytes.extend_from_slice(&0_f64.to_le_bytes());
            for ordinal in 0..6_u32 {
                if ordinal == 5 {
                    bytes.extend_from_slice(&0_u32.to_le_bytes());
                }
                if frame.form == 2 && ordinal >= 2 {
                    push_marked_reference(&mut bytes, 90 + ordinal);
                } else {
                    bytes.push(0);
                }
            }
            bytes.extend_from_slice(&0_u32.to_le_bytes());
            bytes.extend_from_slice(&frame.form.to_le_bytes());
            bytes.push(if frame.malformed { 7 } else { 0 });
            for value in frame.matrix.into_iter().flatten() {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
            let length = match frame.form {
                0 => 283,
                1 => 273,
                2 => 293,
                _ => 253,
            };
            bytes.resize(start + length, 0);
            indexed_header(&mut bytes, *b"259", 71);
        }
        if let Some(role) = self.group_role {
            indexed_header(&mut bytes, *b"260", 50);
            bytes.extend_from_slice(&[0; 10]);
            bytes.extend_from_slice(&1_u32.to_le_bytes());
            push_marked_reference(&mut bytes, 60);
            bytes.extend_from_slice(&[0; 2]);
            bytes.extend_from_slice(&0_u32.to_le_bytes());
            bytes.extend_from_slice(&role.to_le_bytes());
            bytes.extend_from_slice(&[0; 10]);
            bytes.extend_from_slice(&1_u32.to_le_bytes());
            bytes.extend_from_slice(&0_f64.to_le_bytes());
            bytes.extend_from_slice(&1_u32.to_le_bytes());
            push_marked_reference(&mut bytes, 52);
            bytes.extend_from_slice(&[0, 1]);
            push_marked_reference(&mut bytes, 51);
            bytes.push(0);
            push_marked_reference(&mut bytes, 17);
            indexed_header(&mut bytes, *b"261", 50);
        }
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let options = crate::zip_write::file_options(zip::CompressionMethod::Stored);
        super::manifest_test::write_synthetic_manifests(&mut zip, options);
        zip.start_file("FusionAssetName[Active]/Design1/BulkStream.dat", options)
            .unwrap();
        zip.write_all(&bytes).unwrap();
        if self.with_metadata {
            let empty: &[u64] = &[];
            let types = self
                .types
                .iter()
                .map(|&(guid, version)| (guid, "", version, "Fusion", empty))
                .collect::<Vec<_>>();
            zip.start_file("FusionAssetName[Active]/Design1/MetaStream.dat", options)
                .unwrap();
            zip.write_all(&super::streams_test::design_metastream(&types))
                .unwrap();
        }
        zip.finish().unwrap().into_inner()
    }
}
