use boot_block::{FramebufferInfo, PixelFormat};
use page_table::{PageType, PAGE_PRESENT, PAGE_WRITE, PAGE_NX, VirtAddr};
use crate::{mm, font};
use lock::Lock;
use alloc::{vec, boxed::Box};

pub const DEFAULT_FOREGROUND_COLOR: u32 = 0xffffff;

static FRAMEBUFFER: Lock<Option<TextFramebuffer>> = Lock::new(None);

unsafe fn copy_32(from: *const u32, to: *mut u32, size: usize) {
    if size & 1 == 0 {
        asm!(
            "rep movsq",
            inout("rsi") from     => _,
            inout("rdi") to       => _,
            inout("rcx") size / 2 => _,
        );
    } else {
        asm!(
            "rep movsd",
            inout("rsi") from => _,
            inout("rdi") to   => _,
            inout("rcx") size => _,
        );
    }
}

unsafe fn set_32(target: *mut u32, value: u32, size: usize) {
    let value = (value as u64) << 32 | (value as u64);

    if size & 1 == 0 {
        asm!(
            "rep stosq",
            inout("rdi") target   => _,
            inout("rcx") size / 2 => _,
            inout("rax") value    => _,
        );
    } else {
        asm!(
            "rep stosd",
            inout("rdi") target => _,
            inout("rcx") size   => _,
            inout("rax") value  => _,
        );
    }
}

enum ColorMode {
    RGB,
    BGR,
    Custom,
}

struct Framebuffer {
    width:               usize,
    height:              usize,
    pixels_per_scanline: usize,

    black:      u32,
    white:      u32,
    _format:    PixelFormat,
    color_mode: ColorMode,

    mmio:   &'static mut [u32],
    buffer: Box<[u32]>,
}

impl Framebuffer {
    unsafe fn new(info: &FramebufferInfo) -> Self {
        assert!(info.fb_size % 4 == 0, "Framebuffer size is not 4 byte aligned.");
        assert!(info.fb_base     != 0, "Framebuffer is null.");
        assert!(info.width > 0 && info.height > 0, "Framebuffer is empty");

        let mmio = {
            let mut page_table = core!().boot_block.page_table.lock();
            let page_table     = page_table.as_mut().unwrap();

            let aligned_size = (info.fb_size + 0xfff) & !0xfff;
            let virt_addr    = mm::reserve_virt_addr(aligned_size as usize);

            // Map framebuffer into virtual memory.
            for offset in (0..aligned_size).step_by(4096) {
                // PAGE_CACHE_DISABLE doesn't seem to be needed here and enabling it causes
                // huge slowdown on VM.
                let fb_phys   = info.fb_base + offset;
                let backing   = PAGE_PRESENT | PAGE_WRITE | PAGE_NX | fb_phys;
                let virt_addr = VirtAddr(virt_addr.0 + offset);

                page_table.map_raw(&mut mm::PhysicalMemory, virt_addr, PageType::Page4K,
                                   backing, true, false)
                    .expect("Failed to map framebuffer to the virtual memory.");
            }

            let buffer_ptr  = virt_addr.0 as *mut u32;
            let buffer_size = (info.fb_size as usize) / core::mem::size_of::<u32>();

            core::slice::from_raw_parts_mut(buffer_ptr, buffer_size)
        };

        let mut white = 0;

        for &mask in &[info.pixel_format.red, info.pixel_format.green,
                       info.pixel_format.blue] {
            white |= mask;
        }

        // This is different than UEFI values.
        let color_mode = match info.pixel_format {
            PixelFormat { red: 0xff0000, green: 0x00ff00, blue: 0x0000ff } => {
                ColorMode::RGB
            }
            PixelFormat { red: 0x0000ff, green: 0x00ff00, blue: 0xff0000 } => {
                ColorMode::BGR
            }
            _ => ColorMode::Custom,
        };

        let buffer_size = mmio.len();

        Self {
            width:               info.width  as usize,
            height:              info.height as usize,
            pixels_per_scanline: info.pixels_per_scanline as usize,

            black: 0,
            white,
            _format: info.pixel_format,
            color_mode,

            mmio,
            buffer: vec![0u32; buffer_size].into_boxed_slice(),
        }
    }

    fn clear(&mut self, color: u32) {
        let clear_size = self.buffer.len();

        unsafe {
            set_32(self.mmio.as_mut_ptr(),   color, clear_size);
            set_32(self.buffer.as_mut_ptr(), color, clear_size);
        }
    }

    fn set_pixels_in_line(&mut self, x: usize, y: usize, colors: &[u32]) {
        assert!(x + colors.len() < self.width,  "X coordinate is out of bounds.");
        assert!(y                < self.height, "Y coordinate is out of bounds.");

        let index = self.pixels_per_scanline * y + x;

        unsafe {
            copy_32(colors.as_ptr(), self.mmio[index..].as_mut_ptr(),   colors.len());
            copy_32(colors.as_ptr(), self.buffer[index..].as_mut_ptr(), colors.len());
        }
    }

    fn convert_color(&self, color: u32) -> u32 {
        assert!(color & 0xff00_0000 == 0, "Invalid RGB color {:x} passed \
                to `convert_color`.", color);

        match self.color_mode {
            ColorMode::RGB => color,
            ColorMode::BGR => {
                let r = (color >> 16) & 0xff;
                let g = (color >> 8)  & 0xff;
                let b = (color >> 0)  & 0xff;

                (r << 0) | (g << 8) | (b << 16)
            }
            _ => panic!("Custom color mode framebuffers are not fully supported yet."),
        }
    }
}

struct Font {
    data:          &'static [u8],
    width:         usize,
    height:        usize,
    visual_width:  usize,
    visual_height: usize,
    x_padding:     usize,
    y_padding:     usize,
}

impl Font {
    fn new(data: &'static [u8], width: usize, height: usize,
           x_padding: usize, y_padding: usize) -> Self {
        assert!(width  == 8, "Font width must be equal to 8.");
        assert!(height >  0, "Font height cannot be 0.");

        Self {
            data,
            width,
            height,
            visual_width:  width  + x_padding * 2,
            visual_height: height + y_padding * 2,
            x_padding,
            y_padding,
        }
    }

    fn draw(&self, ch: u8, x: usize, y: usize, foreground: u32, background: u32,
            framebuffer: &mut Framebuffer) {
        let index     = (ch as usize) * self.height;
        let char_data = &self.data[index..][..self.height];

        for oy in 0..self.height {
            let line_data  = char_data[oy];
            let mut colors = [0u32; 8];

            for ox in 0..self.width {
                let set   = line_data & (1 << (7 - ox)) != 0;
                let color = if set { foreground } else { background };

                colors[ox] = color;
            }

            framebuffer.set_pixels_in_line(x + self.x_padding, y + oy + self.y_padding, &colors);
        }
    }
}

pub struct TextFramebuffer {
    framebuffer: Framebuffer,

    font:   Font,
    x:      usize,
    y:      usize,
    width:  usize,
    height: usize,

    background:         u32,
    foreground:         u32,
    default_foreground: u32,
}

impl TextFramebuffer {
    fn new(framebuffer: Framebuffer, font: Font) -> Self {
        let width  = framebuffer.width  / font.visual_width;
        let height = framebuffer.height / font.visual_height;
        
        assert!(width  > 1, "Text framebuffer width must > 1.");
        assert!(height > 1, "Text framebuffer height must > 1.");

        let background         = framebuffer.black;
        let default_foreground = if DEFAULT_FOREGROUND_COLOR == 0xffffff {
            // To support framebuffers with weird color formats.
            framebuffer.white
        } else {
            framebuffer.convert_color(DEFAULT_FOREGROUND_COLOR)
        };

        let mut text_fb = Self {
            font,
            x: 0,
            y: 0,
            width,
            height,

            framebuffer,

            default_foreground,
            foreground: default_foreground,
            background,
        };

        text_fb.framebuffer.clear(text_fb.background);

        text_fb
    }

    fn newline(&mut self) {
        // Go to the new line.
        self.x  = 0;
        self.y += 1;

        if self.y >= self.height {
            // We are out of vertical space and we need to scroll.

            // Calculate number of pixels in one text line.
            let pixels_per_line = self.font.visual_height * self.framebuffer.pixels_per_scanline;

            let from_y = 1;
            let to_y   = 0;

            let from_start = from_y * pixels_per_line;
            let to_start   = to_y   * pixels_per_line;
            let copy_size  = (self.height - 1) * pixels_per_line;
            
            // Move every line one line up (except the first one).
            // As there is overlap and source > destination we need to copy forwards.
            unsafe {
                let _from_ptr: *mut u32 = &mut self.framebuffer.mmio[from_start];
                let to_ptr:    *mut u32 = &mut self.framebuffer.mmio[to_start];

                let from_ptr_buffer: *mut u32 = &mut self.framebuffer.buffer[from_start];
                let to_ptr_buffer:   *mut u32 = &mut self.framebuffer.buffer[to_start];

                // Move lines up in both MMIO and buffer. For MMIO we use buffer as
                // a source to avoid expensive MMIO read.
                copy_32(from_ptr_buffer, to_ptr,        copy_size);
                copy_32(from_ptr_buffer, to_ptr_buffer, copy_size);
            }

            let clear_y     = self.height - 1;
            let clear_start = clear_y * pixels_per_line;
            let clear_size  = pixels_per_line;
            let clear_value = self.background;

            // Clear the last line.
            unsafe {
                let clear_ptr:        *mut u32 = &mut self.framebuffer.mmio[clear_start];
                let clear_ptr_buffer: *mut u32 = &mut self.framebuffer.buffer[clear_start];

                // Clear both MMIO and buffer.
                set_32(clear_ptr,        clear_value, clear_size);
                set_32(clear_ptr_buffer, clear_value, clear_size);
            }

            // Move cursor one line up.
            self.y = self.height - 1;
        }
    }

    pub fn write_char(&mut self, ch: char) {
        // Handle special characters.
        match ch {
            '\r' => { self.x = 0;     return; },
            '\n' => { self.newline(); return; },
            _    => (),
        }

        // Convert character to ASCII. Use '?' when conversion is impossible.
        let byte: u8 = if ch.is_ascii() {
            ch as u8
        } else {
            b'?'
        };

        // Draw the character.
        {
            let x = self.x * self.font.visual_width;
            let y = self.y * self.font.visual_height;

            self.font.draw(byte, x, y, self.foreground, self.background, &mut self.framebuffer);
        }

        // Move cursor one position to the right.
        self.x += 1;

        // Wrap text.
        if self.x >= self.width {
            self.newline();
        }
    }

    pub fn write_string(&mut self, string: &str) {
        for ch in string.chars() {
            self.write_char(ch);
        }
    }

    pub fn reset_color(&mut self) {
        self.foreground = self.default_foreground;
    }

    pub fn set_color(&mut self, color: u32) {
        if color == DEFAULT_FOREGROUND_COLOR {
            self.reset_color();
        } else {
            self.foreground = self.framebuffer.convert_color(color);
        }
    }
}

impl core::fmt::Write for TextFramebuffer {
    fn write_str(&mut self, string: &str) -> core::fmt::Result {
        self.write_string(string);

        Ok(())
    }
}

pub unsafe fn initialize() {
    let mut kernel_framebuffer = FRAMEBUFFER.lock();

    assert!(kernel_framebuffer.is_none(), "Framebuffer was already initialized.");

    // Get framebuffer information from the bootloader.
    let framebuffer_info: Option<FramebufferInfo> =
        core!().boot_block.framebuffer.lock().clone();

    if let Some(framebuffer_info) = framebuffer_info {
        // Create graphics framebuffer.
        let framebuffer = Framebuffer::new(&framebuffer_info);

        let font = Font::new(&font::FONT, 8, font::HEIGHT, 1, 1);

        // Create text framebuffer which uses graphics framebuffer and our simple bitmap
        // font.
        let framebuffer = TextFramebuffer::new(framebuffer, font);

        // Set global kernel text framebuffer.
        *kernel_framebuffer = Some(framebuffer);

        drop(kernel_framebuffer);

        println!("Initialized framebuffer device with {}x{} resolution.",
                 framebuffer_info.width, framebuffer_info.height);

        if true {
            if let Some(supported_modes) = core!().boot_block.supported_modes.lock().as_ref() {
                println!("Supported modes:");

                let count = supported_modes.count as usize;

                for (width, height) in &supported_modes.modes[..count] {
                    println!("  {}x{}", width, height);
                }

                println!();

                if supported_modes.overflow {
                    println!("WARNING: Detected supported mode list overflow.");
                }
            }
        }
    }
}

pub fn get() -> &'static Lock<Option<TextFramebuffer>> {
    &FRAMEBUFFER
}
