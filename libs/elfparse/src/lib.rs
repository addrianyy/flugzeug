#![no_std]

// Everything here must be exactly the same in 32 bit mode and 64 bit mode.

use core::convert::TryInto;

macro_rules! read {
    ($bytes: expr, $offset: expr, $type: ty) => {{
        let start: usize = $offset.try_into().ok()?;
        let end:   usize = start.checked_add(core::mem::size_of::<$type>())?;
        <$type>::from_le_bytes($bytes.get(start..end)?.try_into().ok()?)
    }}
}

#[derive(Copy, Clone, PartialEq, Eq)]
pub enum Bitness {
    Bits32 = 32,
    Bits64 = 64,
}

#[derive(Clone)]
pub struct Elf<'a> {
    bytes:            &'a [u8],
    segment_table:    u64,
    segment_count:    u64,
    table_entry_size: u64,
    base_address:     u64,
    entrypoint:       u64,
    bitness:          Bitness,
}

#[derive(Clone, Debug)]
pub struct Segment<'a> {
    pub bytes:     &'a [u8],
    pub virt_addr: u64,
    pub virt_size: u64,
    pub read:      bool,
    pub write:     bool,
    pub execute:   bool,
}

impl<'a> Elf<'a> {
    pub fn parse(bytes: &'a [u8]) -> Option<Self> {
        if bytes.get(0x00..0x04)? != b"\x7fELF" {
            return None;
        }

        let bitness = match bytes.get(0x04)? {
            1 => Bitness::Bits32,
            2 => Bitness::Bits64,
            _ => return None,
        };

        let (entrypoint, segment_table, table_entry_size, segment_count) = match bitness {
            Bitness::Bits32 => {
                let entry       = read!(bytes, 0x18, u32);
                let phoff       = read!(bytes, 0x1c, u32);
                let phent_size  = read!(bytes, 0x2a, u16);
                let phnum       = read!(bytes, 0x2c, u16);

                (entry as u64, phoff as u64, phent_size as u64, phnum as u64)
            }
            Bitness::Bits64 => {
                let entry       = read!(bytes, 0x18, u64);
                let phoff       = read!(bytes, 0x20, u64);
                let phent_size  = read!(bytes, 0x36, u16);
                let phnum       = read!(bytes, 0x38, u16);

                (entry, phoff, phent_size as u64, phnum as u64)
            }
        };

        let mut elf = Elf {
            bytes,
            segment_table,
            segment_count,
            table_entry_size,
            base_address: 0,
            entrypoint,
            bitness,
        };

        let mut base_address = None;

        elf.loadable_segments(|segment| {
            let new_base = match base_address {
                Some(base) => core::cmp::min(base, segment.virt_addr),
                None       => segment.virt_addr,
            };

            base_address = Some(new_base);
        })?;

        elf.base_address = base_address?;

        Some(elf)
    }

    pub fn loadable_segments(&self, mut callback: impl FnMut(&Segment)) -> Option<()> {
        for segment in 0..self.segment_count {
            let entry = segment * self.table_entry_size + self.segment_table;

            let segment_type = read!(self.bytes, entry + 0x00, u32);

            let (virt_addr, virt_size, raw_offset, raw_size, flags) = match self.bitness {
                Bitness::Bits32 => {
                    let virt_addr  = read!(self.bytes, entry + 0x08, u32);
                    let virt_size  = read!(self.bytes, entry + 0x14, u32);
                    let raw_offset = read!(self.bytes, entry + 0x04, u32);
                    let raw_size   = read!(self.bytes, entry + 0x10, u32);
                    let flags      = read!(self.bytes, entry + 0x18, u32);

                    (virt_addr as u64, virt_size as u64, raw_offset as u64, raw_size as u64, flags)
                }
                Bitness::Bits64 => {
                    let virt_addr  = read!(self.bytes, entry + 0x10, u64);
                    let virt_size  = read!(self.bytes, entry + 0x28, u64);
                    let raw_offset = read!(self.bytes, entry + 0x08, u64);
                    let raw_size   = read!(self.bytes, entry + 0x20, u64);
                    let flags      = read!(self.bytes, entry + 0x04, u32);

                    (virt_addr, virt_size, raw_offset, raw_size, flags)
                }
            };

            let load = segment_type == 1;
            if !load {
                continue;
            }

            let execute = flags & 1 != 0;
            let write   = flags & 2 != 0;
            let read    = flags & 4 != 0;

            let start: usize = raw_offset.try_into().ok()?;
            let end:   usize = start.checked_add(raw_size.try_into().ok()?)?;

            let segment = Segment {
                read,
                write,
                execute,
                virt_addr,
                virt_size,
                bytes: self.bytes.get(start..end)?,
            };

            callback(&segment);
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
}

#[cfg(test)]
mod tests {
    extern crate std;

    use super::*;
    use std::println;

    #[test]
    fn test() {
        let bytes = std::fs::read("/usr/bin/sh").unwrap();
        let elf   = Elf::parse(&bytes).unwrap();

        println!("Bitness:      {}.",   elf.bitness() as usize);
        println!("Base address: {:x}.", elf.base_address());
        println!("Entrypoint:   {:x}.", elf.entrypoint());

        elf.loadable_segments(|segment| {
            let mut r = '-';
            let mut w = '-';
            let mut x = '-';

            if segment.read    { r = 'r'; }
            if segment.write   { w = 'w'; }
            if segment.execute { x = 'x'; }

            std::println!("{:016x} - {:016x} {}{}{}",
                          segment.virt_addr, segment.virt_size, r, w, x);
        });

        panic!("Done!");
    }
}
