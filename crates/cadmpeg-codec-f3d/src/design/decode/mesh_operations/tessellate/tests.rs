use super::*;

fn reference(target: u32) -> [u8; 11] {
    let mut bytes = [0; 11];
    bytes[0] = 1;
    bytes[1..5].copy_from_slice(&target.to_le_bytes());
    bytes
}

fn scope(owners: [u32; 4], operands: &[u32]) -> Vec<u8> {
    let mut bytes = vec![0; 21];
    for owner in owners {
        bytes.extend_from_slice(&reference(owner));
    }
    bytes.extend_from_slice(&u32::try_from(operands.len()).unwrap().to_le_bytes());
    for operand in operands {
        bytes.extend_from_slice(&reference(*operand));
    }
    bytes.extend_from_slice(&TAIL);
    bytes.extend_from_slice(&36u32.to_le_bytes());
    for word in "00000000-0000-0000-0000-000000000000".encode_utf16() {
        bytes.extend_from_slice(&word.to_le_bytes());
    }
    bytes
}

fn operand_record(group: u32, body: u32) -> Vec<u8> {
    let mut bytes = vec![0; 21];
    bytes.extend_from_slice(&reference(group));
    bytes.extend_from_slice(&reference(body));
    bytes.extend_from_slice(&TAIL);
    bytes
}

fn read(bytes: &[u8]) -> Result<MeshTessellateScope, MeshOperationFraming> {
    crate::test_support::with_decode_context(|ctx| read_scope(ctx, bytes)).unwrap()
}

#[test]
fn tessellate_scope_reads_owner_slots_and_operand_run() {
    let parsed = read(&scope([30, 31, 32, 33], &[40])).unwrap();
    assert_eq!(parsed.owners, [Some(30), Some(31), Some(32), Some(33)]);
    assert_eq!(parsed.operand_count, Some(1));
    assert_eq!(parsed.operands, [40]);
    assert_eq!(parsed.tail_error, None);
    assert!(parsed.deviations.is_empty());
}

#[test]
fn tessellate_scope_reads_the_operand_word_as_a_count() {
    let parsed = read(&scope([30, 31, 32, 33], &[40, 41])).unwrap();
    assert_eq!(parsed.operand_count, Some(2));
    assert_eq!(parsed.operands, [40, 41]);
    assert_eq!(parsed.tail_error, None);
}

#[test]
fn tessellate_scope_keeps_owners_before_an_unreadable_operand_run() {
    let mut bytes = scope([30, 31, 32, 33], &[40]);
    bytes.truncate(prefix::OPERAND_COUNT + 4);
    bytes[prefix::OPERAND_COUNT..].copy_from_slice(&9u32.to_le_bytes());
    let parsed = read(&bytes).unwrap();
    assert_eq!(parsed.owners, [Some(30), Some(31), Some(32), Some(33)]);
    assert_eq!(parsed.operand_count, Some(9));
    assert!(parsed.operands.is_empty());
    assert_eq!(
        parsed.tail_error,
        Some(MeshOperationFraming("tessellate operand count"))
    );
}

#[test]
fn tessellate_scope_reports_a_changed_tail_byte() {
    let mut bytes = scope([30, 31, 32, 33], &[40]);
    bytes[prefix::LEN + 11 + 1] = 7;
    let parsed = read(&bytes).unwrap();
    assert_eq!(parsed.deviations, [("tessellate-tail-byte:1", 7)]);
}

#[test]
fn tessellate_scope_rejects_a_set_flag_or_nonzero_prefix() {
    let mut flagged = scope([30, 31, 32, 33], &[40]);
    flagged[prefix::FLAG] = 1;
    assert_eq!(
        read(&flagged),
        Err(MeshOperationFraming("tessellate scope flag"))
    );
    let mut prefixed = scope([30, 31, 32, 33], &[40]);
    prefixed[prefix::ZERO_RUN_9] = 1;
    assert_eq!(
        read(&prefixed),
        Err(MeshOperationFraming("tessellate scope prefix"))
    );
}

#[test]
fn tessellation_operand_names_its_group_and_produced_body() {
    let bytes = operand_record(50, 51);
    assert_eq!(bytes.len(), operand::LEN);
    let parsed = read_operand(&bytes).unwrap();
    assert_eq!(parsed.group, 50);
    assert_eq!(parsed.body, 51);
    assert!(parsed.deviations.is_empty());
}

#[test]
fn tessellation_operand_ends_at_its_tail() {
    let mut bytes = operand_record(50, 51);
    bytes.extend_from_slice(&[3, 0, 0, 0]);
    assert_eq!(read_operand(&bytes).unwrap().end, operand::LEN);
}
