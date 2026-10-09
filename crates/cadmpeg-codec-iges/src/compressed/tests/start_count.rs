// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn compressed_terminate_counts_all_start_cards() {
    for start_count in [1_usize, 2, 3] {
        let original = compressed_points_file();
        let lines = source_lines(&original);
        assert_eq!(lines.iter().filter(|line| line.get(72) == Some(&b'S')).count(), 1);
        let global_count = lines.iter().filter(|line| line.get(72) == Some(&b'G')).count();
        let mut source = Vec::new();
        for mut line in lines {
            if line.get(72) == Some(&b'T') {
                // Each point has two Directory cards and one Parameter card.
                for (index, (marker, count)) in [
                    (b'S', start_count), (b'G', global_count), (b'D', 4), (b'P', 2),
                ].into_iter().enumerate() {
                    let field = format!("{}{:7}", char::from(marker), count);
                    line[index * 8..(index + 1) * 8].copy_from_slice(field.as_bytes());
                }
            }
            source.extend_from_slice(&line);
            source.extend_from_slice(b"\r\n");
            if line.get(72) == Some(&b'S') {
                for sequence in 2..=start_count {
                    let field = format!("S{sequence:7}");
                    line[72..80].copy_from_slice(field.as_bytes());
                    source.extend_from_slice(&line);
                    source.extend_from_slice(b"\r\n");
                }
            }
        }
        let normalized = normalize_for_test(&source).unwrap();
        let normalized_lines = source_lines(&normalized);
        assert_eq!(normalized_lines.iter().filter(|line| line.get(72) == Some(&b'S')).count(), start_count);
        let terminate = normalized_lines.iter().find(|line| line.get(72) == Some(&b'T')).unwrap();
        let expected = format!("S{start_count:7}");
        assert_eq!(&terminate[..8], expected.as_bytes());
        assert_eq!(normalized.len(), (start_count + global_count + 4 + 2 + 1) * 81);
        let decoded = IgesCodec.decode(&mut Cursor::new(source), &DecodeOptions::default()).unwrap();
        assert_eq!(decoded.ir().model.points.len(), 2);
        assert!(!decoded.report().losses.iter().any(|loss| loss.code == IgesLossCode::CardFramingRecovered.kind()));
    }
}
