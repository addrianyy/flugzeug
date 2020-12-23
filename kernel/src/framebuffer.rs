use boot_block::{FramebufferInfo, PixelFormat};
use page_table::{PageType, PAGE_PRESENT, PAGE_WRITE, PAGE_NX, VirtAddr};
use crate::{mm, font};
use lock::Lock;
use alloc::{vec, boxed::Box};

pub const DEFAULT_FOREGROUND_COLOR: u32 = 0xffffff;

static FRAMEBUFFER: Lock<Option<TextFramebuffer>> = Lock::new(None);

enum ColorMode {
    RGB,
    BGR,
    Custom,
}

struct Framebuffer {
    width:               usize,
    height:              usize,
    pixels_per_scanline: usize,

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

            _format: info.pixel_format,
            color_mode,

            mmio,
            buffer: vec![0u32; buffer_size].into_boxed_slice(),
        }
    }

    fn clear(&mut self, color: u32) {
        let clear_size = self.buffer.len();

        unsafe {
            dual_set_32(self.mmio.as_mut_ptr(),
                        self.buffer.as_mut_ptr(),
                        color,
                        clear_size);
        }
    }

    fn set_pixels_in_line(&mut self, x: usize, y: usize, colors: &[u32]) {
        assert!(x + colors.len() <= self.width,  "X coordinate is out of bounds.");
        assert!(y                <  self.height, "Y coordinate is out of bounds.");

        let index = self.pixels_per_scanline * y + x;

        unsafe {
            dual_copy_32(colors.as_ptr(),
                         self.mmio[index..].as_mut_ptr(),
                         self.buffer[index..].as_mut_ptr(),
                         colors.len());
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

            if line_data == 0 {
                continue;
            }

            for ox in 0..self.width {
                let set   = line_data & (1 << (7 - ox)) != 0;
                let color = if set { foreground } else { background };

                colors[ox] = color;
            }

            framebuffer.set_pixels_in_line(x + self.x_padding,
                                           y + oy + self.y_padding,
                                           &colors);
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
    fn new(framebuffer: Framebuffer) -> Self {
        let font = Font::new(&font::FONT, 8, font::HEIGHT, 1, 1);

        let width  = framebuffer.width  / font.visual_width;
        let height = framebuffer.height / font.visual_height;
        
        assert!(width  > 1, "Text framebuffer width must > 1.");
        assert!(height > 1, "Text framebuffer height must > 1.");

        let background         = framebuffer.convert_color(0x000000);
        let default_foreground = framebuffer.convert_color(DEFAULT_FOREGROUND_COLOR);

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

    fn scroll(&mut self) {
        // Move every line one line up (except the first one).
        // As there is overlap and source > destination we need to copy forwards.

        let part_count = 8;
        let part_size  = self.framebuffer.width / part_count;
        let last_add   = self.framebuffer.width - (part_count * part_size);

        // We move up every text line except the first one.
        let graphics_lines_to_copy = (self.height - 1) * self.font.visual_height;

        for line_index in 0..graphics_lines_to_copy {
            // Copy from the next line to the current one.
            let from_y = line_index + self.font.visual_height;
            let to_y   = line_index;

            // Calculate buffer indices from graphics Y position.
            let from_index = from_y * self.framebuffer.pixels_per_scanline;
            let to_index   = to_y   * self.framebuffer.pixels_per_scanline;

            // Divide the line into parts and copy only parts where `from` is different than `to`.
            for part_index in 0..part_count {
                // Get the starting X position of this part.
                let start_x = part_index * part_size;

                let copy_size = if part_index + 1 == part_count {
                    // If framebuffer width is not divisible by part count then last part
                    // will have more elements than others.
                    last_add
                } else {
                    0
                } + part_size;

                let from_index = from_index + start_x;
                let to_index   = to_index   + start_x;

                let equal = unsafe {
                    compare_32(self.framebuffer.buffer[from_index..].as_ptr(),
                               self.framebuffer.buffer[to_index..].as_ptr(),
                               copy_size)
                };

                // If parts are equal we don't need to copy anything.
                if equal {
                    continue;
                }

                let c_from      = self.framebuffer.buffer[from_index..].as_ptr();
                let c_to        = self.framebuffer.buffer[to_index..].as_mut_ptr();
                let c_to_mmio   = self.framebuffer.mmio[to_index..].as_mut_ptr();

                unsafe {
                    dual_copy_32(c_from, c_to_mmio, c_to, copy_size);
                }
            }
        }
    }

    fn newline(&mut self) {
        // Go to the new line.
        self.x  = 0;
        self.y += 1;

        if self.y >= self.height {
            // We are out of vertical space and we need to scroll.
            self.scroll();

            // Calulate number of pixels per text line.
            let pixels_per_text_line = self.font.visual_height *
                self.framebuffer.pixels_per_scanline;

            let clear_y     = self.height - 1;
            let clear_start = clear_y * pixels_per_text_line;
            let clear_size  = pixels_per_text_line;
            let clear_value = self.background;

            let c_mmio   = self.framebuffer.mmio[clear_start..].as_mut_ptr();
            let c_buffer = self.framebuffer.buffer[clear_start..].as_mut_ptr();

            // Clear the last line.
            unsafe {
                dual_set_32(c_mmio, c_buffer, clear_value, clear_size);
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
        let framebuffer = TextFramebuffer::new(Framebuffer::new(&framebuffer_info));

        // Set global kernel text framebuffer.
        *kernel_framebuffer = Some(framebuffer);

        drop(kernel_framebuffer);

        println!("Initialized framebuffer device with {}x{} resolution. (video buffer 0x{:x})",
                 framebuffer_info.width, framebuffer_info.height, framebuffer_info.fb_base);

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

const ELEMENTS_PER_VECTOR: usize = 32 / 4;
const ALIGN_MASK:          usize = ELEMENTS_PER_VECTOR - 1;

unsafe fn dual_copy_32(from: *const u32, to1: *mut u32, to2: *mut u32, size: usize) {
    let vectorized_copy_size = size & !ALIGN_MASK;
    if  vectorized_copy_size > 0 {
        asm!(
            r#"
            2:
                vmovups ymm0, [rsi]
                vmovups [rdi], ymm0
                vmovups [rbx], ymm0
                add rsi, 32
                add rdi, 32
                add rbx, 32
                sub rcx, 32
                jnz 2b
            "#,
            inout("rsi") from                     => _,
            inout("rdi") to1                      => _,
            inout("rbx") to2                      => _,
            inout("rcx") vectorized_copy_size * 4 => _,
            out("ymm0") _,
        );
    }

    let left_to_copy = size - vectorized_copy_size;
    if  left_to_copy > 0 {
        let start_index = vectorized_copy_size;
        for index in 0..left_to_copy {
            let index = start_index + index;
            let value = *from.add(index);

            *to1.add(index) = value;
            *to2.add(index) = value;
        }
    }
}

unsafe fn dual_set_32(target1: *mut u32, target2: *mut u32, value: u32, size: usize) {
    let vectorized_set_size = size & !ALIGN_MASK;
    if  vectorized_set_size > 0 {
        let values = [value; ELEMENTS_PER_VECTOR];

        asm!(
            r#"
                vmovups ymm0, [rsi]
            2:
                vmovups [rdi], ymm0
                vmovups [rbx], ymm0
                add rdi, 32
                add rbx, 32
                sub rcx, 32
                jnz 2b
            "#,
            inout("rsi") values.as_ptr()         => _,
            inout("rdi") target1                 => _,
            inout("rbx") target2                 => _,
            inout("rcx") vectorized_set_size * 4 => _,
            out("ymm0") _,
        );
    }

    let left_to_set = size - vectorized_set_size;
    if  left_to_set > 0 {
        let start_index = vectorized_set_size;
        for index in 0..left_to_set {
            let index = start_index + index;

            *target1.add(index) = value;
            *target2.add(index) = value;
        }
    }
}

unsafe fn compare_32(a: *const u32, b: *const u32, size: usize) -> bool {
    let vectorized_cmp_size = size & !ALIGN_MASK;
    if  vectorized_cmp_size > 0 {
        let result: u32;

        asm!(
            r#"
            2:
                vmovups   ymm0, [rsi]
                vpcmpeqd  ymm0, ymm0, [rdi]
                vpmovmskb eax, ymm0

                cmp eax, -1
                jne 1f

                add rsi, 32
                add rdi, 32
                sub rcx, 32
                jnz 2b
            1:
            "#,
            inout("rsi") a                       => _,
            inout("rdi") b                       => _,
            inout("rcx") vectorized_cmp_size * 4 => _,
            out("eax")   result,
            out("ymm0") _,
        );

        if result != 0xffff_ffff {
            return false;
        }
    }

    let left_to_cmp = size - vectorized_cmp_size;
    if  left_to_cmp > 0 {
        let start_index = vectorized_cmp_size;
        for index in 0..left_to_cmp {
            let index = start_index + index;

            let a = *a.add(index);
            let b = *b.add(index);

            if a != b {
                return false;
            }
        }
    }

    true
}
