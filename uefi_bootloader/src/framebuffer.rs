use crate::{efi, framebuffer_resolutions, BOOT_BLOCK};
use efi::EfiGuid;

pub unsafe fn initialize(system_table: *mut efi::EfiSystemTable) {
    const EFI_GOP_GUID: EfiGuid =
        EfiGuid(0x9042a9de, 0x23dc, 0x4a38, [0x96, 0xfb, 0x7a, 0xde, 0xd0, 0x80, 0x51, 0x6a]);

    fn is_pixel_format_usable(format: efi::EfiGraphicsPixelFormat) -> bool {
        matches!(format, efi::PIXEL_RGB | efi::PIXEL_BGR)
    }

    let mut protocol = 0;
    let status = ((*(*system_table).boot_services).locate_protocol)(&EFI_GOP_GUID,
                                                                    core::ptr::null_mut(),
                                                                    &mut protocol);
    if status != 0 {
        println!("WARNING: Getting EFI graphic output protocol failed with status {:x}.",
                 status);
        return;
    }

    let protocol = &mut *(protocol as *mut efi::EfiGraphicsOutputProtocol);
    let max_mode = (*protocol.mode).max_mode;

    let preferred_resolutions = framebuffer_resolutions::PREFERRED_RESOLUTIONS;

    let mut best_mode  = None;
    let mut dense_mode = None;

    let mut supported_modes = boot_block::SupportedModes {
        modes:    [(0, 0); boot_block::MAX_SUPPORTED_MODES],
        count:    0,
        overflow: false,
    };

    for mode in 0..max_mode {
        let mut info         = core::ptr::null_mut();
        let mut size_of_info = 0;

        let status = (protocol.query_mode)(protocol, mode, &mut size_of_info, &mut info);
        if  status != 0 {
            println!("WARNING: Failed to query display mode {}.", mode);
            continue;
        }

        assert!(size_of_info >= core::mem::size_of::<efi::EfiGraphicsOutputModeInformation>(),
                "EFI returned too small output mode information.");

        let info = &*info;

        if !is_pixel_format_usable(info.pixel_format) {
            continue;
        }

        let resolution = (info.horizontal_res, info.vertical_res);

        if let Some(index) = preferred_resolutions.iter().position(|r| *r == resolution) {
            // Pick one with higher priority (lower index).
            let is_better = match best_mode {
                Some((other_index, _)) => index < other_index,
                None                   => true,
            };

            if is_better {
                best_mode = Some((index, mode));
            }
        } else {
            let pixels = resolution.0 * resolution.1;

            // Pick one with higher pixel count.
            let is_better = match dense_mode {
                Some((other_pixels, _)) => pixels > other_pixels,
                None                    => true,
            };

            if is_better {
                dense_mode = Some((pixels, mode));
            }
        }

        if (supported_modes.count as usize) >= boot_block::MAX_SUPPORTED_MODES {
            supported_modes.overflow = true;
        } else {
            let index = supported_modes.count as usize;

            supported_modes.modes[index] = resolution;

            supported_modes.count += 1;
        }

        if false {
            println!("{}x{}; pixel format {}.", info.horizontal_res, info.vertical_res,
                     info.pixel_format);
        }
    }

    // Inform the kernel about supported framebuffer modes.
    *BOOT_BLOCK.supported_modes.lock() = Some(supported_modes);

    // If we haven't found any of the preferred modes than pick one with highest pixel count.
    if best_mode.is_none() {
        best_mode = dense_mode.map(|(pixels, mode)| (pixels as usize, mode));
    }

    if let Some((_, best_mode)) = best_mode {
        assert!((protocol.set_mode)(protocol, best_mode) == 0,
                "Failed to switch to preferred framebuffer mode.");
    }

    let mode      = &(*protocol.mode);
    let mode_info = &(*mode.info);

    if !is_pixel_format_usable(mode_info.pixel_format) {
        println!("WARNING: Selected EFI output mode is not usable as a framebuffer.");
        return;
    }
    
    let mut format = boot_block::PixelFormat {
        red:   0,
        green: 0,
        blue:  0,
    };

    match mode_info.pixel_format {
        efi::PIXEL_RGB => {
            format.red   = 0x0000ff;
            format.green = 0x00ff00;
            format.blue  = 0xff0000;
        }
        efi::PIXEL_BGR => {
            format.red   = 0xff0000;
            format.green = 0x00ff00;
            format.blue  = 0x0000ff;
        }
        _ => unreachable!(),
    }

    let framebuffer_info = boot_block::FramebufferInfo {
        width:               mode_info.horizontal_res,
        height:              mode_info.vertical_res,
        pixel_format:        format,
        pixels_per_scanline: mode_info.pixels_per_scanline,
        fb_base:             mode.fb_base as u64,
        fb_size:             mode.fb_size as u64,
    };

    *BOOT_BLOCK.framebuffer.lock() = Some(framebuffer_info);
}
