use alloc::{vec, boxed::Box};

use crate::{mm, font};
use crate::lock::Lock;
use crate::interrupts::PRINT_IN_INTERRUPTS;

use boot_block::{FramebufferInfo, PixelFormat};
use page_table::PhysAddr;

pub const DEFAULT_FOREGROUND_COLOR: u32 = 0xffffff;

static FRAMEBUFFER: Lock<Option<TextFramebuffer>> = {
    if PRINT_IN_INTERRUPTS {
        Lock::new_non_preemptible(None)
    } else {
        Lock::new(None)
    }
};

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
        assert!(info.pixels_per_scanline >= (32 / 4), "Too small amount of pixels per scanline.");

        let mmio = {
            // Use write-combining memory type for MMIO.
            let aligned_size = (info.fb_size + 0xfff) & !0xfff;
            let virt_addr    = mm::map_mmio(PhysAddr(info.fb_base), aligned_size, mm::PAGE_WC);

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
    data:          &'static [Option<&'static [u8]>],
    width:         usize,
    height:        usize,
    visual_width:  usize,
    visual_height: usize,
    x_padding:     usize,
    y_padding:     usize,
    x_scale:       usize,
    y_scale:       usize,
}

impl Font {
    fn new(
        data:     &'static [Option<&'static [u8]>],
        width:    usize,
        height:   usize,
        x_padding:usize,
        y_padding:usize,
        x_scale:  usize,
        y_scale:  usize,
    ) -> Self {
        assert!(width  >  0, "Font width cannot be 0.");
        assert!(height >  0, "Font height cannot be 0.");

        assert!(x_scale > 0, "X scale cannot be 0.");
        assert!(y_scale > 0, "Y scale cannot be 0.");

        Self {
            data,
            width,
            height,
            visual_width:  width  * x_scale + x_padding * 2,
            visual_height: height * y_scale + y_padding * 2,
            x_padding,
            y_padding,
            x_scale,
            y_scale,
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn draw(
        &self,
        ch:          u8,
        x:           usize,
        y:           usize,
        foreground:  u32,
        background:  u32,
        line_buffer: &mut [u32],
        framebuffer: &mut Framebuffer,
    ) {
        let char_index = ch as usize;
        if  char_index >= self.data.len() {
            return;
        }

        if let Some(char_data) = self.data[ch as usize] {
            let mut index = 0;
            let mut bit   = 0;

            for oy in 0..self.height {
                for ox in 0..self.width {
                    let set   = char_data[index] & (1 << bit) != 0;
                    let color = if set { foreground } else { background };

                    for index in 0..self.x_scale {
                        line_buffer[ox * self.x_scale + index] = color;
                    }

                    bit += 1;

                    if bit >= 8 {
                        bit    = 0;
                        index += 1;
                    }
                }

                if bit != 0 {
                    bit    = 0;
                    index += 1;
                }

                if !line_buffer.iter().all(|&color| color == background) {
                    for index in 0..self.y_scale {
                        let x = x + self.x_padding;
                        let y = y + self.y_padding + oy * self.y_scale + index;

                        framebuffer.set_pixels_in_line(x, y, line_buffer);
                    }
                }
            }
        }
    }

    fn line_size(&self) -> usize {
        self.width * self.x_scale
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

    line_buffer: Box<[u32]>,
    text:        Box<[u32]>,
}

impl TextFramebuffer {
    fn encode_colored_character(ch: u8, mut color: u32) -> u32 {
        assert!(color & 0xff00_0000 == 0, "Reserved color bits aren't zero.");

        // Ignore color for empty characters.
        if ch == b'\0' || ch == b' ' {
            color = 0;
        }

        color | (ch as u32) << 24
    }

    fn new(framebuffer: Framebuffer) -> Self {
        let font = Font::new(&font::DATA, font::WIDTH, font::HEIGHT, 0, 2, 1, 1);

        let width  = framebuffer.width  / font.visual_width;
        let height = framebuffer.height / font.visual_height;
        
        assert!(width  > 1, "Text framebuffer width must > 1.");
        assert!(height > 1, "Text framebuffer height must > 1.");

        let background         = framebuffer.convert_color(0x000000);
        let default_foreground = framebuffer.convert_color(DEFAULT_FOREGROUND_COLOR);
        let empty_char         = Self::encode_colored_character(b' ', default_foreground);

        let mut text_fb = Self {
            line_buffer: vec![0u32;       font.line_size()].into_boxed_slice(),
            text:        vec![empty_char; width * height].into_boxed_slice(),

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

    fn split_text_line(&self) -> LineSplit {
        // TODO: Pick some good value here (heuristics?).
        // The higher framebuffer width the higher this value should be.
        // The higher console width the higher this value should be.
        // 16, 32, 64 seem to work well.
        LineSplit::new(self.width, 32)
    }

    fn scroll(&mut self) {
        // Split text characters.
        let split = self.split_text_line();

        // We can use `copy_32` on text only if difference between `from` and `to` is >=
        // than block copy size (32 bytes).
        let vectorized_copy_text = self.width >= (32 / 4);

        // Move every line one line up (except the first one).
        // As there is overlap and source > destination we need to copy forwards.
        for text_y in 0..(self.height - 1) {
            let from_text_y = text_y + 1;
            let to_text_y   = text_y;

            // Divide the text line into parts and copy only parts where
            // `from` is different than `to`.
            split.split(|text_x, text_size| {
                {
                    // Calculate text indices in the buffer.
                    let from_text_index = from_text_y * self.width + text_x;
                    let to_text_index   = to_text_y   * self.width + text_x;

                    let from = self.text[from_text_index..][..text_size].as_mut_ptr();
                    let to   = self.text[to_text_index  ..][..text_size].as_mut_ptr();

                    unsafe {
                        // If all characters in `from` are the same as in `to` then we don't need
                        // to copy anything and we can just return.
                        if compare_32(from, to, text_size) {
                            return;
                        }

                        if vectorized_copy_text {
                            copy_32(from, to, text_size);
                        } else {
                            for index in 0..text_size {
                                *to.add(index) = *from.add(index);
                            }
                        }
                    }
                }

                let graphics_x    = text_x    * self.font.visual_width;
                let graphics_size = text_size * self.font.visual_width;

                // We know that text is different in this line. We need to copy the pixel buffer.
                // Go through every graphics line and check if it is different. If it is then
                // copy it.
                for graphics_y in ((text_y + 0) * self.font.visual_height)..
                                  ((text_y + 1) * self.font.visual_height) {
                    let from_y = graphics_y + self.font.visual_height;
                    let to_y   = graphics_y;

                    let from_index = from_y * self.framebuffer.pixels_per_scanline + graphics_x;
                    let to_index   = to_y   * self.framebuffer.pixels_per_scanline + graphics_x;

                    let equal = unsafe {
                        compare_32(self.framebuffer.buffer[from_index..].as_ptr(),
                                   self.framebuffer.buffer[to_index..].as_ptr(),
                                   graphics_size)
                    };

                    // If parts are equal we don't need to copy anything.
                    if !equal {
                        let c_from      = self.framebuffer.buffer[from_index..].as_ptr();
                        let c_to        = self.framebuffer.buffer[to_index..].as_mut_ptr();
                        let c_to_mmio   = self.framebuffer.mmio[to_index..].as_mut_ptr();

                        unsafe {
                            dual_copy_32(c_from, c_to_mmio, c_to, graphics_size);
                        }
                    }
                }
            });
        }
    }

    fn clear_line(&mut self, text_y: usize) {
        // Split text characters.
        let split = self.split_text_line();

        let color     = self.background;
        let character = Self::encode_colored_character(b' ', color);

        // Divide the text line into parts and clear only parts which are not clear.
        split.split(|text_x, text_size| {
            {
                let text_index = text_y * self.width + text_x;
                let text_ptr   = self.text[text_index..][..text_size].as_mut_ptr();

                unsafe {
                    // If all characters are clear then we don't need to copy anything and
                    // we can just return.
                    if compare_single_32(text_ptr, character, text_size) {
                        return;
                    }

                    // Clear this text line.
                    set_32(text_ptr, character, text_size);
                }
            }

            let graphics_x    = text_x    * self.font.visual_width;
            let graphics_size = text_size * self.font.visual_width;

            // We know that text is not clear in this part. We need to clear the pixel buffer.
            // Go through every graphics line and check if it is not clear. If it is not then
            // clear it.
            for graphics_y in ((text_y + 0) * self.font.visual_height)..
                              ((text_y + 1) * self.font.visual_height) {
                let graphics_index = graphics_y * self.framebuffer.pixels_per_scanline +
                    graphics_x;

                let equal = unsafe {
                    compare_single_32(self.framebuffer.buffer[graphics_index..].as_ptr(),
                                      color, graphics_size)
                };

                // If parts are equal we don't need to copy anything.
                if !equal {
                    let c_mmio   = self.framebuffer.mmio[graphics_index..].as_mut_ptr();
                    let c_buffer = self.framebuffer.buffer[graphics_index..].as_mut_ptr();

                    unsafe {
                        dual_set_32(c_mmio, c_buffer, color, graphics_size);
                    }
                }
            }
        });
    }

    fn newline(&mut self) {
        // Go to the new line.
        self.x  = 0;
        self.y += 1;

        if self.y >= self.height {
            // We are out of vertical space and we need to scroll and clear last line.
            self.scroll();
            self.clear_line(self.height - 1);
            
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
            // Update colored character that we are drawing now.
            let character = Self::encode_colored_character(byte, self.foreground);
            self.text[self.x + self.y * self.width] = character;

            let x = self.x * self.font.visual_width;
            let y = self.y * self.font.visual_height;

            self.font.draw(byte, x, y, self.foreground, self.background,
                           &mut self.line_buffer, &mut self.framebuffer);
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
    let framebuffer_info: Option<FramebufferInfo> = *core!().boot_block.framebuffer.lock();

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

unsafe fn copy_32(from: *const u32, to: *mut u32, size: usize) {
    let vectorized_copy_size = size & !ALIGN_MASK;
    if  vectorized_copy_size > 0 {
        asm!(
            r#"
            2:
                vmovups ymm0, [rsi]
                vmovups [rdi], ymm0
                add rsi, 32
                add rdi, 32
                sub rcx, 32
                jnz 2b
            "#,
            inout("rsi") from                     => _,
            inout("rdi") to                       => _,
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

            *to.add(index) = value;
        }
    }
}

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

unsafe fn set_32(target: *mut u32, value: u32, size: usize) {
    let vectorized_set_size = size & !ALIGN_MASK;
    if  vectorized_set_size > 0 {
        let values = [value; ELEMENTS_PER_VECTOR];

        asm!(
            r#"
                vmovups ymm0, [rsi]
            2:
                vmovups [rdi], ymm0
                add rdi, 32
                sub rcx, 32
                jnz 2b
            "#,
            inout("rsi") values.as_ptr()         => _,
            inout("rdi") target                  => _,
            inout("rcx") vectorized_set_size * 4 => _,
            out("ymm0") _,
        );
    }

    let left_to_set = size - vectorized_set_size;
    if  left_to_set > 0 {
        let start_index = vectorized_set_size;
        for index in 0..left_to_set {
            let index = start_index + index;

            *target.add(index) = value;
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

unsafe fn compare_single_32(buffer: *const u32, value: u32, size: usize) -> bool {
    let vectorized_cmp_size = size & !ALIGN_MASK;
    if  vectorized_cmp_size > 0 {
        let values = [value; ELEMENTS_PER_VECTOR];
        let result: u32;

        asm!(
            r#"
                vmovups ymm0, [rsi]

            2:
                vpcmpeqd  ymm1, ymm0, [rdi]
                vpmovmskb eax, ymm1

                cmp eax, -1
                jne 1f

                add rdi, 32
                sub rcx, 32
                jnz 2b
            1:
            "#,
            inout("rsi") values.as_ptr()         => _,
            inout("rdi") buffer                  => _,
            inout("rcx") vectorized_cmp_size * 4 => _,
            out("eax")   result,
            out("ymm0") _,
            out("ymm1") _,
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
            let read  = *buffer.add(index);

            if read != value {
                return false;
            }
        }
    }

    true
}

struct LineSplit {
    part_count: usize,
    part_size:  usize,
    last_add:   usize,
}

impl LineSplit {
    fn new(width: usize, part_count: usize) -> Self {
        let part_size = width / part_count;
        let last_add  = width - (part_count * part_size);

        Self {
            part_count,
            part_size,
            last_add,
        }
    }

    fn split(&self, mut callback: impl FnMut(usize, usize)) {
        for part_index in 0..self.part_count {
            let start_x = part_index * self.part_size;

            let size = if part_index + 1 == self.part_count {
                self.last_add
            } else {
                0
            } + self.part_size;

            callback(start_x, size);
        }
    }
}
