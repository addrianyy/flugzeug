#![no_std]

use core::convert::TryInto;
use core::fmt;

macro_rules! implement_readable {
    ($type: ty) => {
        impl Readable for $type {
            fn read(bytes: &[u8], endianness: Endianness) -> Option<Self> {
                match endianness {
                    Endianness::Little => Some(Self::from_le_bytes(bytes.try_into().ok()?)),
                    Endianness::Big    => Some(Self::from_be_bytes(bytes.try_into().ok()?)),
                }
            }
        }
    };

    ($($type: ty),*) => {
        $( implement_readable! { $type } )*
    };
}

implement_readable! { u8, u16, u32, u64 }
implement_readable! { i8, i16, i32, i64 }

fn byte_slice(bytes: &[u8], offset: u64, size: u64) -> Option<&[u8]> {
    let start: usize = offset.try_into().ok()?;
    let end:   usize = start.checked_add(size.try_into().ok()?)?;

    bytes.get(start..end)
}

trait Readable: Sized {
    fn read(bytes: &[u8], endianness: Endianness) -> Option<Self>;
}

struct Reader<'a> {
    bytes:      &'a [u8],
    endianness: Endianness,
}

impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8], endianness: Endianness) -> Self {
        Self {
            bytes,
            endianness,
        }
    }

    fn partial(bytes: &'a [u8], offset: u64, size: u64, endianness: Endianness) -> Option<Self> {
        let slice = byte_slice(bytes, offset, size)?;

        Some(Self {
            bytes: slice,
            endianness,
        })
    }

    fn read<T: Readable>(&self, offset: u64) -> Option<T> {
        let slice = byte_slice(self.bytes, offset, core::mem::size_of::<T>() as u64)?;

        T::read(slice, self.endianness)
    }
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Bitness {
    Bits32 = 32,
    Bits64 = 64,
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Endianness {
    Little,
    Big,
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Machine {
    None,
    X86,
    Mips,
    PowerPC,
    PowerPC64,
    Arm,
    Ia64,
    Amd64,
    Arm64,
    RiscV,
    Other(u16),
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum SegmentType {
    Null,
    Load,
    Dynamic,
    Interp,
    Note,
    Shlib,
    Phdr,
    Tls,
    Other(u32),
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum SectionType {
    Null,
    Progbits,
    Symtab,
    Strtab,
    Rela,
    Hash,
    Dynamic,
    Note,
    Nobits,
    Rel,
    Shlib,
    Dynsym,
    InitArray,
    FiniArray,
    PreinitArray,
    Group,
    SymtabShndx,
    Num,
    Other(u32),
}

#[derive(Clone)]
pub struct Elf<'a> {
    bytes:   &'a [u8],
    strings: Option<&'a [u8]>,

    segment_table:      u64,
    segment_count:      u64,
    segment_entry_size: u64,

    section_table:      u64,
    section_count:      u64,
    section_entry_size: u64,

    base_address: u64,
    entrypoint:   u64,

    bitness:    Bitness,
    endianness: Endianness,
    machine:    Machine,
}

#[derive(Clone)]
pub struct Segment<'a> {
    raw_offset: u64,
    raw_size:   u64,

    pub bytes: &'a [u8],

    pub seg_type: SegmentType,

    pub virt_addr: u64,
    pub virt_size: u64,
    pub align:     Option<u64>,

    pub read:    bool,
    pub write:   bool,
    pub execute: bool,
}

#[derive(Clone)]
pub struct Section<'a> {
    raw_offset: u64,
    raw_size:   u64,

    pub bytes: Option<&'a [u8]>,

    pub name:     Option<&'a str>,
    pub sec_type: SectionType,

    pub virt_addr: u64,
    pub align:     u64,

    pub flags:   u64,
    pub alloc:   bool,
    pub write:   bool,
    pub execute: bool,

    pub info:       u32,
    pub link:       Option<u64>,
    pub entry_size: Option<u64>,
}

impl<'a> Elf<'a> {
    fn get_string(&self, offset: u64) -> Option<&str> {
        let start: usize = offset.try_into().ok()?;

        self.strings.and_then(|strings| {
            let string = strings.get(start..)?;
            let null   = string.iter().position(|x| *x == 0)?;
            let string = string.get(..null)?;

            core::str::from_utf8(string).ok()
        })
    }

    fn segment_reader(&self, index: u64) -> Option<Reader> {
        if index >= self.segment_count {
            return None;
        }
        
        let entry = index * self.segment_entry_size + self.segment_table;

        Reader::partial(self.bytes, entry, self.segment_entry_size, self.endianness)
    }

    fn section_reader(&self, index: u64) -> Option<Reader> {
        if index >= self.section_count {
            return None;
        }
        
        let entry = index * self.section_entry_size + self.section_table;

        Reader::partial(self.bytes, entry, self.section_entry_size, self.endianness)
    }

    pub fn parse(bytes: &'a [u8]) -> Option<Self> {
        if bytes.get(0x00..0x04)? != b"\x7fELF" {
            return None;
        }

        let bitness = match bytes.get(0x04)? {
            1 => Bitness::Bits32,
            2 => Bitness::Bits64,
            _ => return None,
        };

        let endianness = match bytes.get(0x05)? {
            1 => Endianness::Little,
            2 => Endianness::Big,
            _ => return None,
        };

        let reader = Reader::new(bytes, endianness);

        let machine = match reader.read::<u16>(0x12)? {
            0x00 => Machine::None,
            0x03 => Machine::X86,
            0x08 => Machine::Mips,
            0x14 => Machine::PowerPC,
            0x15 => Machine::PowerPC64,
            0x28 => Machine::Arm,
            0x32 => Machine::Ia64,
            0x3e => Machine::Amd64,
            0xb7 => Machine::Arm64,
            0xf3 => Machine::RiscV,
            x    => Machine::Other(x),
        };

        let (entrypoint, segment_table, segment_entry_size, segment_count,
             section_table, section_entry_size, section_count, shstrndx) = match bitness {
            Bitness::Bits32 => {
                let entry = reader.read::<u32>(0x18)?;

                let phoff      = reader.read::<u32>(0x1c)?;
                let phent_size = reader.read::<u16>(0x2a)?;
                let phnum      = reader.read::<u16>(0x2c)?;

                let shoff      = reader.read::<u32>(0x20)?;
                let shent_size = reader.read::<u16>(0x2e)?;
                let shnum      = reader.read::<u16>(0x30)?;

                let shstrndx = reader.read::<u16>(0x32)?;

                (entry as u64, phoff as u64, phent_size as u64, phnum as u64,
                 shoff as u64, shent_size as u64, shnum as u64, shstrndx as u64)
            }
            Bitness::Bits64 => {
                let entry = reader.read::<u64>(0x18)?;

                let phoff      = reader.read::<u64>(0x20)?;
                let phent_size = reader.read::<u16>(0x36)?;
                let phnum      = reader.read::<u16>(0x38)?;

                let shoff      = reader.read::<u64>(0x28)?;
                let shent_size = reader.read::<u16>(0x3a)?;
                let shnum      = reader.read::<u16>(0x3c)?;

                let shstrndx = reader.read::<u16>(0x3e)?;

                (entry, phoff, phent_size as u64, phnum as u64, shoff, shent_size as u64,
                 shnum as u64, shstrndx as u64)
            }
        };

        let mut elf = Elf {
            bytes,
            strings: None,

            segment_table,
            segment_count,
            segment_entry_size,

            section_table,
            section_entry_size,
            section_count,

            base_address: 0,
            entrypoint,

            bitness,
            endianness,
            machine,
        };

        if let Some(string_section) = elf.section_by_index(shstrndx) {
            let strings = byte_slice(bytes, string_section.raw_offset,
                                     string_section.raw_size);

            elf.strings = strings;
        }

        let mut base_address = None;

        elf.segments(|segment| {
            if segment.seg_type != SegmentType::Load {
                return;
            }

            let new_base = match base_address {
                Some(base) => core::cmp::min(base, segment.virt_addr),
                None       => segment.virt_addr,
            };

            base_address = Some(new_base);
        })?;

        elf.base_address = base_address?;

        Some(elf)
    }

    pub fn segment_by_index(&self, index: u64) -> Option<Segment> {
        let reader = self.segment_reader(index)?;

        let seg_type = match reader.read::<u32>(0x00)? {
            0 => SegmentType::Null,
            1 => SegmentType::Load,
            2 => SegmentType::Dynamic,
            3 => SegmentType::Interp,
            4 => SegmentType::Note,
            5 => SegmentType::Shlib,
            6 => SegmentType::Phdr,
            7 => SegmentType::Tls,
            x => SegmentType::Other(x),
        };

        let (virt_addr, virt_size, raw_offset, raw_size, flags, align) = match self.bitness {
            Bitness::Bits32 => {
                let virt_addr  = reader.read::<u32>(0x08)?;
                let virt_size  = reader.read::<u32>(0x14)?;

                let raw_offset = reader.read::<u32>(0x04)?;
                let raw_size   = reader.read::<u32>(0x10)?;

                let flags = reader.read::<u32>(0x18)?;
                let align = reader.read::<u32>(0x1c)?;

                (virt_addr as u64, virt_size as u64, raw_offset as u64, raw_size as u64,
                 flags, align as u64)
            }
            Bitness::Bits64 => {
                let virt_addr  = reader.read::<u64>(0x10)?;
                let virt_size  = reader.read::<u64>(0x28)?;

                let raw_offset = reader.read::<u64>(0x08)?;
                let raw_size   = reader.read::<u64>(0x20)?;

                let flags = reader.read::<u32>(0x04)?;
                let align = reader.read::<u64>(0x30)?;

                (virt_addr, virt_size, raw_offset, raw_size, flags, align)
            }
        };

        let execute = flags & 1 != 0;
        let write   = flags & 2 != 0;
        let read    = flags & 4 != 0;

        let align = match align {
            0 | 1 => None,
            x     => Some(x),
        };

        Some(Segment {
            raw_offset,
            raw_size,

            bytes: byte_slice(self.bytes, raw_offset, raw_size)?,

            seg_type,

            virt_addr,
            virt_size,
            align,

            read,
            write,
            execute,
        })
    }

    pub fn section_by_index(&self, index: u64) -> Option<Section> {
        let reader = self.section_reader(index)?;
        let name   = self.get_string(reader.read::<u32>(0x00)? as u64);

        let sec_type = match reader.read::<u32>(0x04)? {
            0x00 => SectionType::Null,
            0x01 => SectionType::Progbits,
            0x02 => SectionType::Symtab,
            0x03 => SectionType::Strtab,
            0x04 => SectionType::Rela,
            0x05 => SectionType::Hash,
            0x06 => SectionType::Dynamic,
            0x07 => SectionType::Note,
            0x08 => SectionType::Nobits,
            0x09 => SectionType::Rel,
            0x0a => SectionType::Shlib,
            0x0b => SectionType::Dynsym,
            0x0e => SectionType::InitArray,
            0x0f => SectionType::FiniArray,
            0x10 => SectionType::PreinitArray,
            0x11 => SectionType::Group,
            0x12 => SectionType::SymtabShndx,
            0x13 => SectionType::Num,
            x    => SectionType::Other(x),
        };

        let (virt_addr, raw_offset, raw_size, link, info,
             entry_size, flags, align) = match self.bitness {
            Bitness::Bits32 => {
                let virt_addr = reader.read::<u32>(0x0c)?;

                let raw_offset = reader.read::<u32>(0x10)?;
                let raw_size   = reader.read::<u32>(0x14)?;

                let link    = reader.read::<u32>(0x18)?;
                let info    = reader.read::<u32>(0x1c)?;
                let entsize = reader.read::<u32>(0x24)?;

                let flags = reader.read::<u32>(0x08)?;
                let align = reader.read::<u32>(0x20)?;

                (virt_addr as u64, raw_offset as u64, raw_size as u64, 
                 link as u64, info, entsize as u64, flags as u64, align as u64)
            }
            Bitness::Bits64 => {
                let virt_addr = reader.read::<u64>(0x10)?;

                let raw_offset = reader.read::<u64>(0x18)?;
                let raw_size   = reader.read::<u64>(0x20)?;

                let link    = reader.read::<u32>(0x28)?;
                let info    = reader.read::<u32>(0x2c)?;
                let entsize = reader.read::<u64>(0x38)?;

                let flags = reader.read::<u64>(0x08)?;
                let align = reader.read::<u64>(0x30)?;

                (virt_addr, raw_offset, raw_size, link as u64, info, entsize, flags, align)
            }
        };

        let alloc   = flags & 2 != 0;
        let write   = flags & 1 != 0;
        let execute = flags & 4 != 0;

        let link = match link {
            0 => None,
            x => Some(x),
        };

        let entry_size = match entry_size {
            0 => None,
            x => Some(x),
        };

        Some(Section {
            raw_offset,
            raw_size,

            bytes: byte_slice(self.bytes, raw_offset, raw_size),

            name,
            sec_type,

            virt_addr,
            align,

            flags,
            alloc,
            write,
            execute,

            info,
            link,
            entry_size,
        })
    }

    pub fn section_by_name(&self, name: &str) -> Option<Section> {
        for index in 0..self.section_count {
            let reader       = self.section_reader(index)?;
            let section_name = self.get_string(reader.read::<u32>(0x00)? as u64);

            if section_name == Some(name) {
                return self.section_by_index(index);
            }
        }

        None
    }

    pub fn segments(&self, mut callback: impl FnMut(&Segment)) -> Option<()> {
        for index in 0..self.segment_count {
            callback(&self.segment_by_index(index)?);
        }

        Some(())
    }

    pub fn sections(&self, mut callback: impl FnMut(&Section)) -> Option<()> {
        for index in 0..self.section_count {
            callback(&self.section_by_index(index)?);
        }

        Some(())
    }

    pub fn base_address(&self) -> u64 {
        self.base_address
    }

    pub fn entrypoint(&self) -> u64 {
        self.entrypoint
    }

    pub fn bitness(&self) -> Bitness {
        self.bitness
    }

    pub fn endianness(&self) -> Endianness {
        self.endianness
    }

    pub fn machine(&self) -> Machine {
        self.machine
    }
}

impl fmt::Debug for Elf<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Elf")
         .field("base_address",&self.base_address)
         .field("entrypoint",  &self.entrypoint)
         .field("bitness",     &self.bitness)
         .field("endianness",  &self.endianness)
         .field("machine",     &self.machine)
         .finish()
    }
}

impl fmt::Debug for Segment<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Segment")
         .field("type",      &self.seg_type)
         .field("virt_addr", &self.virt_addr)
         .field("virt_size", &self.virt_size)
         .field("align",     &self.align)
         .field("read",      &self.read)
         .field("write",     &self.write)
         .field("execute",   &self.execute)
         .finish()
    }
}

impl fmt::Debug for Section<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Section")
         .field("name",       &self.name)
         .field("type",       &self.sec_type)
         .field("virt_addr",  &self.virt_addr)
         .field("align",      &self.align)
         .field("flags",      &self.flags)
         .field("alloc",      &self.alloc)
         .field("write",      &self.write)
         .field("execute",    &self.execute)
         .field("info",       &self.info)
         .field("link",       &self.link)
         .field("entry_size", &self.entry_size)
         .field("contents",   &self.bytes.is_some())
         .finish()
    }
}

#[cfg(test)]
mod tests {
    extern crate std;

    use super::*;

    #[test]
    fn test() {
        let bytes = std::fs::read("/usr/bin/sh").unwrap();
        let elf   = Elf::parse(&bytes).unwrap();

        std::println!("{:#x?}\n", elf);

        elf.segments(|segment| {
            std::println!("{:#x?}", segment);
        }).unwrap();

        std::println!();

        elf.sections(|section| {
            std::println!("{:#x?}", section);
        }).unwrap();

        panic!("Done!");
    }
}
