import {
  fixCodecSize,
  getBytesCodec,
  getStructCodec,
  getU8Codec,
  getU16Codec,
  getU32Codec,
  getU64Codec,
} from "@solana/codecs";

const bytes = (size: number) => fixCodecSize(getBytesCodec(), size);

export const ELF_IDENT_CODEC = getStructCodec([
  ["magic", bytes(4)],
  ["class", getU8Codec()],
  ["endianness", getU8Codec()],
  ["version", getU8Codec()],
  ["osAbi", getU8Codec()],
  ["abiVersion", getU8Codec()],
  ["padding", bytes(7)],
]);

export const ELF_HEADER_CODEC = getStructCodec([
  ["ident", ELF_IDENT_CODEC],
  ["type", getU16Codec()],
  ["machine", getU16Codec()],
  ["version", getU32Codec()],
  ["entry", getU64Codec()],
  ["programOffset", getU64Codec()],
  ["sectionOffset", getU64Codec()],
  ["flags", getU32Codec()],
  ["headerSize", getU16Codec()],
  ["programEntrySize", getU16Codec()],
  ["programCount", getU16Codec()],
  ["sectionEntrySize", getU16Codec()],
  ["sectionCount", getU16Codec()],
  ["sectionNames", getU16Codec()],
]);

export const ELF_PROGRAM_HEADER_CODEC = getStructCodec([
  ["type", getU32Codec()],
  ["flags", getU32Codec()],
  ["offset", getU64Codec()],
  ["virtualAddress", getU64Codec()],
  ["physicalAddress", getU64Codec()],
  ["fileSize", getU64Codec()],
  ["memorySize", getU64Codec()],
  ["alignment", getU64Codec()],
]);

export const ELF_SECTION_HEADER_CODEC = getStructCodec([
  ["name", getU32Codec()],
  ["type", getU32Codec()],
  ["flags", getU64Codec()],
  ["address", getU64Codec()],
  ["offset", getU64Codec()],
  ["size", getU64Codec()],
  ["link", getU32Codec()],
  ["info", getU32Codec()],
  ["alignment", getU64Codec()],
  ["entrySize", getU64Codec()],
]);

export const LOADER_PROGRAM_CODEC = getStructCodec([
  ["state", getU32Codec()],
  ["programData", bytes(32)],
]);

export const LOADER_PROGRAM_DATA_CODEC = getStructCodec([
  ["state", getU32Codec()],
  ["slot", getU64Codec()],
  ["hasAuthority", getU8Codec()],
  ["authority", bytes(32)],
]);

export const LOADER_BUFFER_CODEC = getStructCodec([
  ["state", getU32Codec()],
  ["hasAuthority", getU8Codec()],
  ["authority", bytes(32)],
]);

export type ElfHeader = ReturnType<typeof ELF_HEADER_CODEC.decode>;
export type ElfProgramHeader = ReturnType<typeof ELF_PROGRAM_HEADER_CODEC.decode>;
export type ElfSectionHeader = ReturnType<typeof ELF_SECTION_HEADER_CODEC.decode>;
