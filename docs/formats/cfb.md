# Compound File Binary directory: Format Specification

Record offsets and widths are maintained in [the checked layout table](../layouts/cfb.md), generated from `docs/layouts/cfb.toml`.

## 1. Directory entry

Each directory entry occupies 128 bytes. Integer fields use little-endian order, as specified by [MS-CFB §2.2](https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-cfb/05060311-bfce-4b12-874d-71fd4ce63aea). [MS-CFB §2.6.1](https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-cfb/60fe8611-66c3-496b-b70d-a504c94c9ace) defines this field sequence:

| Field | Width | Meaning |
| --- | ---: | --- |
| `name` | 64 bytes | UTF-16 name storage. |
| `name_length` | 2 bytes | Even byte count including the null terminator; at most 64. |
| `object_type` | 1 byte | Unallocated=0, storage=1, stream=2, root=5. |
| `color` | 1 byte | Red=0 or black=1. |
| `left` | 4 bytes | Left sibling ordinal. |
| `right` | 4 bytes | Right sibling ordinal. |
| `child` | 4 bytes | Child ordinal. |
| `clsid` | 16 bytes | Storage class identifier. |
| `state_bits` | 4 bytes | Application flags. |
| `creation_time` | 8 bytes | UTC FILETIME. |
| `modified_time` | 8 bytes | UTC FILETIME. |
| `start_sector` | 4 bytes | First stream sector. |
| `stream_size` | 8 bytes | Stream byte length. |

Absent links use NOSTREAM (`0xffffffff`). Streams require NOSTREAM children and zero class/timestamps; nonzero state bits remain permitted. Storages require zero start and effective size. Root creation time is zero; its class, flags and modified time remain meaningful. Root start and size describe the mini stream. Version 3 stream and mini-stream sizes do not exceed `0x80000000`.

## 2. Unallocated entries and versioned size

Unallocated entries contain zero bytes except their three links, which contain NOSTREAM. For version 3, readers ignore the size's high DWORD, including storage entries; writers zero it. Version 4 uses all 64 bits. These rules follow [MS-CFB §2.6.3](https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-cfb/b37413bb-f3ef-4adc-b18e-29bddd62c26e).
