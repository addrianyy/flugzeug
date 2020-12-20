#![no_std]

#[derive(Clone, Copy, Debug)]
#[repr(C, packed)]
pub struct Rsdp {
    pub signature: [u8; 8],
    pub checksum:  u8,
    pub oem_id:    [u8; 6],
    pub revision:  u8,
    pub rsdt_addr: u32,
}

#[derive(Clone, Copy, Debug)]
#[repr(C, packed)]
pub struct RsdpExtended {
    pub rsdp:              Rsdp,
    pub length:            u32,
    pub xsdt_addr:         u64,
    pub extended_checksum: u8,
    pub reserved:          [u8; 3],
}

#[derive(Clone, Copy, Debug)]
#[repr(C, packed)]
pub struct Header {
    pub signature:        [u8; 4],
    pub length:           u32,
    pub revision:         u8,
    pub checksum:         u8,
    pub oem_id:           [u8; 6],
    pub oem_table_id:     u64,
    pub oem_revision:     u32,
    pub creator_id:       u32,
    pub creator_revision: u32,
}
