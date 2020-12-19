use boot_block::{FramebufferInfo, PixelFormat};
use page_table::{PageType, PAGE_PRESENT, PAGE_WRITE, PAGE_CACHE_DISABLE, PAGE_NX, VirtAddr};
use crate::{mm, font};
use lock::Lock;

static FRAMEBUFFER: Lock<Option<TextFramebuffer>> = Lock::new(None);

struct Framebuffer {
    width:               usize,
    height:              usize,
    pixels_per_scanline: usize,

    black:   u32,
    white:   u32,
    _format: PixelFormat,

    buffer: &'static mut [u32],
}

impl Framebuffer {
    unsafe fn new(info: &FramebufferInfo) -> Self {
        assert!(info.fb_size % 4 == 0, "Framebuffer size is not 4 byte aligned.");
        assert!(info.fb_base     != 0, "Framebuffer is null.");
        assert!(info.width > 0 && info.height > 0, "Framebuffer is empty");

        let mut page_table = core!().boot_block.page_table.lock();
        let page_table     = page_table.as_mut().unwrap();

        let aligned_size = (info.fb_size + 0xfff) & !0xfff;
        let virt_addr    = mm::reserve_virt_addr(aligned_size as usize);

        // Map framebuffer into virtual memory.
        for offset in (0..aligned_size).step_by(4096) {
            let backing   = info.fb_base + offset;
            let backing   = PAGE_PRESENT | PAGE_WRITE | PAGE_CACHE_DISABLE | PAGE_NX | backing;
            let virt_addr = VirtAddr(virt_addr.0 + offset);

            page_table.map_raw(&mut mm::PhysicalMemory, virt_addr, PageType::Page4K,
                               backing, true, false)
                .expect("Failed to map framebuffer to the virtual memory.");
        }

        let buffer_ptr  = virt_addr.0 as *mut u32;
        let buffer_size = (info.fb_size as usize) / core::mem::size_of::<u32>();
        let buffer      = core::slice::from_raw_parts_mut(buffer_ptr, buffer_size);

        let mut white = 0;

        for &mask in &[info.pixel_format.red, info.pixel_format.green,
                       info.pixel_format.blue] {
            white |= mask;
        }

        Self {
            width:               info.width  as usize,
            height:              info.height as usize,
            pixels_per_scanline: info.pixels_per_scanline as usize,

            black: 0,
            white,
            _format: info.pixel_format,

            buffer,
        }
    }

    fn clear(&mut self, color: u32) {
        for index in 0..self.buffer.len() {
            unsafe {
                core::ptr::write_volatile(&mut self.buffer[index], color);
            }
        }
    }

    fn set_pixel(&mut self, x: usize, y: usize, color: u32) {
        assert!(x < self.width,  "X coordinate is out of bounds.");
        assert!(y < self.height, "Y coordinate is out of bounds.");

        let index = x + self.pixels_per_scanline * y;

        unsafe {
            core::ptr::write_volatile(&mut self.buffer[index], color);
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
            let line_data = char_data[oy];

            for ox in 0..self.width {
                let set   = line_data & (1 << (7 - ox)) != 0;
                let color = if set { foreground } else { background };

                framebuffer.set_pixel(x + ox + self.x_padding, y + oy + self.y_padding, color);
            }
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

    background: u32,
    foreground: u32,
}

impl TextFramebuffer {
    fn new(framebuffer: Framebuffer, font: Font) -> Self {

        let width  = framebuffer.width  / font.visual_width;
        let height = framebuffer.height / font.visual_height;
        
        assert!(width  > 1, "Text framebuffer width must > 1.");
        assert!(height > 1, "Text framebuffer height must > 1.");

        let mut text_fb = Self {
            font,
            x: 0,
            y: 0,
            width,
            height,
            foreground: framebuffer.white,
            background: framebuffer.black,
            framebuffer,
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
            // Because we don't have optimized memcpy we will manually copy pixels in framebuffer.
            // As there is overlap and source > destination we need to copy forwards.
            unsafe {
                let from_ptr: *mut u32 = &mut self.framebuffer.buffer[from_start];
                let to_ptr:   *mut u32 = &mut self.framebuffer.buffer[to_start];

                if copy_size & 1 == 0 {
                    asm!(
                        "rep movsq",
                        inout("rsi") from_ptr      => _,
                        inout("rdi") to_ptr        => _,
                        inout("rcx") copy_size / 2 => _,
                    );
                } else {
                    asm!(
                        "rep movsd",
                        inout("rsi") from_ptr  => _,
                        inout("rdi") to_ptr    => _,
                        inout("rcx") copy_size => _,
                    );
                }
            }

            // Clear the last line.
            let clear_y     = self.height - 1;
            let clear_start = clear_y * pixels_per_line;
            let clear_size  = pixels_per_line;
            let clear_value = (self.background as u64) << 32 | (self.background as u64);

            unsafe {
                let clear_ptr: *mut u32 = &mut self.framebuffer.buffer[clear_start];

                if clear_size & 1 == 0 {
                    asm!(
                        "rep stosq",
                        in("rax")    clear_value,
                        inout("rdi") clear_ptr      => _,
                        inout("rcx") clear_size / 2 => _,
                    );
                } else {
                    asm!(
                        "rep stosd",
                        in("rax")    clear_value,
                        inout("rdi") clear_ptr  => _,
                        inout("rcx") clear_size => _,
                    );
                }
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

        println!("Initialized framebuffer device with resolution {}x{}.",
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
