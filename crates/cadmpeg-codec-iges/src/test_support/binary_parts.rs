// SPDX-License-Identifier: Apache-2.0
//! Counted Binary Directory fixtures with actual offset-linked point data.

fn integer(value: i32) -> Vec<u8> {
    let mut bytes = vec![1];
    bytes.extend_from_slice(&value.to_be_bytes());
    bytes
}

fn pointer(value: u32) -> Vec<u8> {
    let mut bytes = vec![5];
    bytes.extend_from_slice(&value.to_be_bytes());
    bytes
}

fn text(value: &[u8]) -> Vec<u8> {
    let mut bytes = vec![6];
    bytes.extend_from_slice(
        &u32::try_from(value.len())
            .expect("test text length")
            .to_be_bytes(),
    );
    bytes.extend_from_slice(value);
    bytes
}

fn counted(body: &[u8]) -> Vec<u8> {
    let mut bytes = u32::try_from(body.len())
        .expect("test record length")
        .to_be_bytes()
        .to_vec();
    bytes.extend_from_slice(body);
    bytes
}

fn section(marker: u8, body: &[u8]) -> Vec<u8> {
    let mut bytes = vec![marker];
    bytes.extend_from_slice(&counted(body));
    bytes
}

pub(crate) fn point_file(prefix: &[u8], directory_pointer: u32) -> Vec<u8> {
    assemble(prefix, 1, &point_parameters(directory_pointer))
}

pub(crate) fn point_after_void(corrupt_first: bool) -> Vec<u8> {
    let mut prefix = directory_record(0, 0);
    if corrupt_first {
        prefix[4] = 7;
    }
    let next = u32::try_from(prefix.len() + 1).expect("test Directory offset");
    point_file(&prefix, next)
}

pub(crate) fn point_pair(corrupt_first: bool) -> Vec<u8> {
    let mut prefix = directory_record(116, 1);
    if corrupt_first {
        prefix[4] = 7;
    }
    let next = u32::try_from(prefix.len() + 1).expect("test Directory offset");
    let mut parameters = point_parameters(1);
    let target = u32::try_from(parameters.len() + 1).expect("test Parameter offset");
    parameters.extend_from_slice(&point_parameters(next));
    assemble(&prefix, target, &parameters)
}

fn directory_record(entity_type: i32, parameter_pointer: u32) -> Vec<u8> {
    let mut directory = vec![integer(entity_type), pointer(parameter_pointer)];
    directory.extend(std::iter::repeat_n(vec![0], 14));
    counted(&directory.concat())
}

fn point_parameters(directory_pointer: u32) -> Vec<u8> {
    point_parameters_with_symbol(directory_pointer, 0)
}

fn point_parameters_with_symbol(directory_pointer: u32, symbol: u32) -> Vec<u8> {
    counted(
        &[
            integer(116),
            pointer(directory_pointer),
            integer(1),
            integer(2),
            integer(3),
            pointer(symbol),
        ]
        .concat(),
    )
}

fn assemble(prefix: &[u8], parameter_pointer: u32, parameters: &[u8]) -> Vec<u8> {
    let mut directory_bytes = prefix.to_vec();
    directory_bytes.extend_from_slice(&directory_record(116, parameter_pointer));
    let global = [
        vec![0],
        vec![0],
        text(b"product"),
        text(b"part.igs"),
        text(b"cadmpeg"),
        text(b"test"),
        integer(32),
        integer(38),
        integer(6),
        integer(308),
        integer(15),
        vec![0],
        integer(1),
        integer(2),
        text(b"MM"),
        integer(1),
        integer(1),
        text(b"260714.000000"),
        integer(1),
        integer(1000),
        text(b"author"),
        text(b"org"),
        integer(6),
        integer(0),
    ]
    .concat();
    let sections = [
        section(b'S', &text(b"Binary parts")),
        section(b'G', &global),
        section(b'D', &directory_bytes),
        section(b'P', parameters),
    ];
    let mut counts = Vec::new();
    counts.push(b'B');
    counts.extend_from_slice(&80_u32.to_be_bytes());
    for (marker, source) in b"SGDP".iter().zip(&sections) {
        counts.push(*marker);
        counts.extend_from_slice(
            &u32::try_from(source.len())
                .expect("test section length")
                .to_be_bytes(),
        );
    }
    let terminate = section(b'T', &counts);
    let mut bytes = vec![0; 80];
    bytes[0] = b'B';
    bytes[1..5].copy_from_slice(&75_u32.to_be_bytes());
    bytes[5..11].copy_from_slice(&[32, 32, 8, 23, 11, 52]);
    for (index, (marker, size)) in [
        (b'B', 80),
        (b'S', sections[0].len()),
        (b'G', sections[1].len()),
        (b'D', sections[2].len()),
        (b'P', sections[3].len()),
        (b'T', terminate.len()),
    ]
    .into_iter()
    .enumerate()
    {
        let start = 11 + index * 5;
        bytes[start] = marker;
        bytes[start + 1..start + 5].copy_from_slice(
            &u32::try_from(size)
                .expect("test displacement")
                .to_be_bytes(),
        );
    }
    bytes[72] = b'B';
    bytes[73..79].fill(b'0');
    bytes[79] = b'1';
    for section in &sections {
        bytes.extend_from_slice(section);
    }
    bytes.extend_from_slice(&terminate);
    bytes.push(b'E');
    bytes
}

pub(crate) fn malformed_directory() -> Vec<u8> {
    let mut bytes = directory_record(0, 0);
    bytes[4] = 7;
    bytes
}

pub(crate) fn broken_parameter_reference(broken: bool) -> Vec<u8> {
    let prefix = directory_record(116, 1);
    let next = u32::try_from(prefix.len() + 1).expect("test Directory offset");
    let mut parameters = point_parameters_with_symbol(1, if broken { next + 1 } else { 0 });
    let target = u32::try_from(parameters.len() + 1).expect("test Parameter offset");
    parameters.extend_from_slice(&point_parameters(next));
    assemble(&prefix, target, &parameters)
}

fn transformed_directory(entity_type: i32, parameter: u32, transform: u32) -> Vec<u8> {
    let mut fields = vec![integer(entity_type), pointer(parameter)];
    fields.extend(std::iter::repeat_n(vec![0], 14));
    fields[6] = pointer(transform);
    counted(&fields.concat())
}

pub(crate) fn broken_transform_chain(broken: bool) -> Vec<u8> {
    transform_chain(broken, false)
}

pub(crate) fn broken_transform_parameter() -> Vec<u8> {
    transform_chain(false, true)
}

fn transform_chain(broken: bool, malformed_parameter: bool) -> Vec<u8> {
    let mut last_value = integer(0);
    if malformed_parameter {
        last_value[0] = 7;
    }
    let transform = counted(
        &[
            integer(124),
            pointer(1),
            integer(1),
            integer(0),
            integer(0),
            integer(0),
            integer(0),
            integer(1),
            integer(0),
            integer(0),
            integer(0),
            integer(0),
            integer(1),
            last_value,
        ]
        .concat(),
    );
    let first = transformed_directory(124, 1, if broken { 2 } else { 0 });
    let second_offset = u32::try_from(first.len() + 1).expect("test Directory offset");
    let second_parameter = u32::try_from(transform.len() + 1).expect("test Parameter offset");
    let second = transformed_directory(116, second_parameter, 1);
    let peer_offset = u32::try_from(first.len() + second.len() + 1).expect("test Directory offset");
    let mut prefix = first;
    prefix.extend_from_slice(&second);
    let mut parameters = transform;
    parameters.extend_from_slice(&point_parameters(second_offset));
    let peer_parameter = u32::try_from(parameters.len() + 1).expect("test Parameter offset");
    parameters.extend_from_slice(&point_parameters(peer_offset));
    assemble(&prefix, peer_parameter, &parameters)
}

#[derive(Clone, Copy)]
pub(crate) enum DirectoryAdmissionFault {
    Structure,
    Form,
    Status,
    EntityType,
}

pub(crate) fn directory_admission_fault(fault: DirectoryAdmissionFault, broken: bool) -> Vec<u8> {
    let mut fields = vec![integer(116), pointer(1)];
    fields.extend(std::iter::repeat_n(vec![0], 14));
    match fault {
        DirectoryAdmissionFault::Structure => fields[2] = text(if broken { b"x" } else { b"0" }),
        DirectoryAdmissionFault::Form => fields[11] = text(if broken { b"x" } else { b"0" }),
        DirectoryAdmissionFault::Status => {
            fields[8] = integer(if broken { -1 } else { 0x00ff_ffff });
        }
        DirectoryAdmissionFault::EntityType => {
            if broken {
                fields[0][0] = 3;
            }
        }
    }
    let prefix = counted(&fields.concat());
    let peer = u32::try_from(prefix.len() + 1).expect("test Directory offset");
    let mut parameters = point_parameters(1);
    let target = u32::try_from(parameters.len() + 1).expect("test Parameter offset");
    parameters.extend_from_slice(&point_parameters(peer));
    assemble(&prefix, target, &parameters)
}
