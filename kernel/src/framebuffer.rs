use boot_block::{FramebufferInfo, PixelFormat};
use page_table::{PageType, PAGE_PRESENT, PAGE_WRITE, PAGE_CACHE_DISABLE, PAGE_NX, VirtAddr};
use crate::{mm, font};

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
            let row_data = char_data[oy];

            for ox in 0..self.width {
                let set   = row_data & (1 << (7 - ox)) != 0;
                let color = if set { foreground } else { background };

                framebuffer.set_pixel(x + ox + self.x_padding, y + oy + self.y_padding, color);
            }
        }
    }
}

struct TextFramebuffer {
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
    fn new(framebuffer: Framebuffer) -> Self {
        let font = Font::new(&font::FONT, 8, font::HEIGHT, 1, 1);

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
            let pixels_per_y = self.font.visual_height * self.framebuffer.pixels_per_scanline;

            // Move every line one line up (except the first one).
            for y in 1..self.height {
                let from_y = y;
                let to_y   = y - 1;

                let from_start = from_y * pixels_per_y;
                let to_start   = to_y   * pixels_per_y;

                let copy_size = pixels_per_y;

                // Copy every pixel from `from_y` line to `to_y` line.
                for index in 0..copy_size {
                    let from = from_start + index;
                    let to   = to_start   + index;

                    unsafe {
                        let value = core::ptr::read_volatile(&self.framebuffer.buffer[from]);
                        core::ptr::write_volatile(&mut self.framebuffer.buffer[to], value);
                    }
                }
            }

            // Clear the last line.
            let clear_size = pixels_per_y;
            let clear_y    = self.height - 1;

            for index in 0..clear_size {
                let index = clear_y * pixels_per_y + index;

                unsafe {
                    core::ptr::write_volatile(&mut self.framebuffer.buffer[index],
                                              self.background);
                }
            }

            // Move cursor one line up.
            self.y = self.height - 1;
        }
    }

    fn write_char(&mut self, ch: char) {
        // Handle special characters.
        match ch {
            '\r' => { self.x = 0;     return; },
            '\n' => { self.newline(); return; },
            _    => (),
        }

        // Convert character to ASCII. Use '?' when cannot convert.
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

    fn test(&mut self) {
        for index in 0..39 {
            let name = alloc::format!("{}", index);
            for ch in name.chars() {
                self.write_char(ch);
            }

            self.write_char('\n');
        }
    }
}

fn test() {
    let framebuffer_info: Option<FramebufferInfo> =
        core!().boot_block.framebuffer.lock().clone();

    if let Some(framebuffer_info) = framebuffer_info {
        println!("Got framebuffer device.");

        let framebuffer = unsafe {
            Framebuffer::new(&framebuffer_info)
        };

        let mut framebuffer = TextFramebuffer::new(framebuffer);

        framebuffer.test();
    }
}

pub unsafe fn initialize() {
    test();
}
