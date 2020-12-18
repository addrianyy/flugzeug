pub type Handle          = usize;
pub type EfiMemoryType   = u32;
pub type EfiStatus       = usize;

#[repr(C)]
pub struct EfiMemoryDescriptor {
    pub typ:            u32,
    pub physical_start: u64,
    pub virtual_start:  u64,
    pub pages:          u64,
    pub attribute:      u64,
}

pub type AllocatePool = unsafe extern "efiapi" fn(
    pool:   EfiMemoryType,
    size:   usize,
    buffer: &mut *mut u8,
) -> EfiStatus;

pub type FreePool = unsafe extern "efiapi" fn(
    buffer: *mut u8,
) -> EfiStatus;

pub type GetMemoryMap = unsafe extern "efiapi" fn(
    size:               &mut usize,
    map:                *mut EfiMemoryDescriptor,
    key:                &mut usize,
    descriptor_size:    &mut usize,
    descriptor_version: &mut u32
) -> EfiStatus;

pub type ExitBootServices = unsafe extern "efiapi" fn(
    image_handle: usize,
    map_key:      usize,
) -> EfiStatus;

#[repr(C)]
pub struct EfiTableHeader {
    pub signature:   u64,
    pub revision:    u32,
    pub header_size: u32,
    pub crc32:       u32,
    pub reserved:    u32,
}

#[repr(C)]
pub struct EfiRuntimeServices {
    pub header: EfiTableHeader,
}

#[repr(C)]
pub struct EfiBootServices {
    pub header:                                 EfiTableHeader,
    pub raise_tpl:                              usize,
    pub restore_tpl:                            usize,
    pub allocate_pages:                         usize,
    pub free_pages:                             usize,
    pub get_memory_map:                         GetMemoryMap,
    pub allocate_pool:                          AllocatePool,
    pub free_pool:                              FreePool,
    pub create_event:                           usize,
    pub set_timer:                              usize,
    pub wait_for_event:                         usize,
    pub signal_event:                           usize,
    pub close_event:                            usize,
    pub check_event:                            usize,
    pub install_protocol_interface:             usize,
    pub reinstall_protocol_interface:           usize,
    pub uninstall_protocol_interface:           usize,
    pub handle_protocol:                        usize,
    pub reserved:                               usize,
    pub register_protocol_notify:               usize,
    pub locate_handle:                          usize,
    pub locate_device_path:                     usize,
    pub install_configuration_table:            usize,
    pub load_image:                             usize,
    pub start_image:                            usize,
    pub exit:                                   usize,
    pub unload_image:                           usize,
    pub exit_boot_services:                     ExitBootServices,
    pub get_next_monotonic_count:               usize,
    pub stall:                                  usize,
    pub set_watchdog_timer:                     usize,
    pub connect_controller:                     usize,
    pub disconnect_controller:                  usize,
    pub open_protocol:                          usize,
    pub close_protocol:                         usize,
    pub open_protocol_information:              usize,
    pub protocols_per_handle:                   usize,
    pub locate_handle_buffer:                   usize,
    pub locate_protocol:                        usize,
    pub install_multiple_protocol_interfaces:   usize,
    pub uninstall_multiple_protocol_interfaces: usize,
    pub calculate_crc32:                        usize,
    pub copy_mem:                               usize,
    pub set_mem:                                usize,
    pub create_event_ex:                        usize,
}

#[derive(PartialEq, Eq)]
#[repr(C)]
pub struct EfiGuid(pub u32, pub u16, pub u16, pub [u8; 8]);

#[repr(C)]
pub struct EfiConfigurationTable {
    pub guid:  EfiGuid,
    pub table: usize,
}

#[repr(C)]
pub struct EfiSystemTable {
    pub header:            EfiTableHeader,
    pub firmware_vendor:   *const u16,
    pub firmware_revision: u32,

    pub stdin_handle: Handle,
    pub stdin:        usize,

    pub stdout_handle: Handle,
    pub stdout:        usize,

    pub stderr_handle: Handle,
    pub stderr:        usize,

    pub runtime_services: *mut EfiRuntimeServices,
    pub boot_services:    *mut EfiBootServices,

    pub table_entries:       usize,
    pub configuration_table: *mut EfiConfigurationTable,
}

pub const EFI_LOADER_CODE:          u32 = 1;
pub const EFI_LOADER_DATA:          u32 = 2;
pub const EFI_BOOT_SERVICES_CODE:   u32 = 3;
pub const EFI_BOOT_SERVICES_DATA:   u32 = 4;
pub const EFI_CONVENTIONAL_MEMORY:  u32 = 7;
