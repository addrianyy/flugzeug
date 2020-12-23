pub type Handle        = usize;
pub type EfiMemoryType = u32;
pub type EfiStatus     = usize;

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

pub type LocateProtocol = unsafe extern "efiapi" fn(
    protocol:      &EfiGuid,
    registration:  *mut usize,
    interface:     &mut usize,
) -> EfiStatus;

pub type ExitBootServices = unsafe extern "efiapi" fn(
    image_handle: usize,
    map_key:      usize,
) -> EfiStatus;

pub type OpenProtocol = unsafe extern "efiapi" fn (
    handle:     Handle,
    protocol:   &EfiGuid,
    interface:  &mut usize,
    agent:      Handle,
    controller: Handle,
    attributes: u32,
) -> EfiStatus;

pub type TextString = unsafe extern "efiapi" fn (
    this:       *mut EfiSimpleTextOutputProtocol,
    string:     *const u16,
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
    pub open_protocol:                          OpenProtocol,
    pub close_protocol:                         usize,
    pub open_protocol_information:              usize,
    pub protocols_per_handle:                   usize,
    pub locate_handle_buffer:                   usize,
    pub locate_protocol:                        LocateProtocol,
    pub install_multiple_protocol_interfaces:   usize,
    pub uninstall_multiple_protocol_interfaces: usize,
    pub calculate_crc32:                        usize,
    pub copy_mem:                               usize,
    pub set_mem:                                usize,
    pub create_event_ex:                        usize,
}

#[derive(PartialEq, Eq)]
#[repr(C, align(8))]
pub struct EfiGuid(pub u32, pub u16, pub u16, pub [u8; 8]);

#[repr(C)]
pub struct EfiConfigurationTable {
    pub guid:  EfiGuid,
    pub table: usize,
}

#[repr(C)]
pub struct EfiSimpleTextOutputProtocol {
    pub reset:         usize,
    pub output_string: TextString,
}

#[repr(C)]
pub struct EfiSystemTable {
    pub header:            EfiTableHeader,
    pub firmware_vendor:   *const u16,
    pub firmware_revision: u32,

    pub stdin_handle: Handle,
    pub stdin:        *mut EfiSimpleTextOutputProtocol,

    pub stdout_handle: Handle,
    pub stdout:        *mut EfiSimpleTextOutputProtocol,

    pub stderr_handle: Handle,
    pub stderr:        *mut EfiSimpleTextOutputProtocol,

    pub runtime_services: *mut EfiRuntimeServices,
    pub boot_services:    *mut EfiBootServices,

    pub table_entries:       usize,
    pub configuration_table: *mut EfiConfigurationTable,
}

#[repr(C)]
pub struct EfiLoadedImageProtocol {
    pub revision:        u32,
    pub parent:          Handle,
    pub system_table:    *mut EfiSystemTable,
    pub device:          Handle,
    pub file_path:       usize,
    pub reserved:        usize,
    pub load_opts_size:  u32,
    pub load_opts:       usize,
    pub image_base:      usize,
    pub image_size:      usize,
    pub image_code_type: EfiMemoryType,
    pub image_data_type: EfiMemoryType,
    pub unload:          usize,
}

pub const EFI_LOADER_CODE:          u32 = 1;
pub const EFI_LOADER_DATA:          u32 = 2;
pub const EFI_BOOT_SERVICES_CODE:   u32 = 3;
pub const EFI_BOOT_SERVICES_DATA:   u32 = 4;
pub const EFI_CONVENTIONAL_MEMORY:  u32 = 7;

pub type SetMode = unsafe extern "efiapi" fn(
    this: *mut EfiGraphicsOutputProtocol,
    mode: u32,
) -> EfiStatus;

pub type QueryMode = unsafe extern "efiapi" fn(
    this:         *mut EfiGraphicsOutputProtocol,
    mode:         u32,
    size_of_info: &mut usize,
    info:         &mut *mut EfiGraphicsOutputModeInformation,
) -> EfiStatus;

#[repr(C)]
pub struct EfiGraphicsOutputProtocolMode {
    pub max_mode:     u32,
    pub mode:         u32,
    pub info:         *mut EfiGraphicsOutputModeInformation,
    pub size_of_info: usize,
    pub fb_base:      usize,
    pub fb_size:      usize,
}

#[repr(C)]
pub struct EfiGraphicsOutputProtocol {
    pub query_mode: QueryMode,
    pub set_mode:   SetMode,
    pub blt:        usize,
    pub mode:       *mut EfiGraphicsOutputProtocolMode,
}

#[repr(C)]
pub struct EfiPixelBitmask {
    pub red:      u32,
    pub green:    u32,
    pub blue:     u32,
    pub reserved: u32,
}

pub type EfiGraphicsPixelFormat = u32;

pub const PIXEL_RGB: u32 = 0;
pub const PIXEL_BGR: u32 = 1;

#[repr(C)]
pub struct EfiGraphicsOutputModeInformation {
    pub version:             u32,
    pub horizontal_res:      u32,
    pub vertical_res:        u32,
    pub pixel_format:        EfiGraphicsPixelFormat,
    pub pixel_info:          EfiPixelBitmask,
    pub pixels_per_scanline: u32,
}
