// All the boilerplate code that defines interrupt handlers.

global_asm!(r#"
    .intel_syntax
    .extern handle_interrupt

    call_handler:
        // Save the register state.
        push rax
        push rbx
        push rcx
        push qword ptr [r15 + 0x00] // RDX
        push qword ptr [r15 + 0x08] // RSI
        push qword ptr [r15 + 0x10] // RDI
        push rbp
        push r8
        push r9
        push r10
        push r11
        push r12
        push r13
        push r14
        push qword ptr [r15 + 0x18] // R15

        // Save the current stack pointer for the 4th argument (register state).
        mov rcx, rsp

        // Allocate shadow space and align the stack to 16 byte boundary.
        mov rbp, rsp
        sub rsp, 0x20
        and rsp, ~0xf

        call handle_interrupt

        // Restore the stack pointer.
        mov rsp, rbp

        // Restore the register state.
        pop  qword ptr [r15 + 0x18] // R15
        pop  r14
        pop  r13
        pop  r12
        pop  r11
        pop  r10
        pop  r9
        pop  r8
        pop  rbp
        pop  qword ptr [r15 + 0x10] // RDI
        pop  qword ptr [r15 + 0x08] // RSI
        pop  qword ptr [r15 + 0x00] // RDX
        pop  rcx
        pop  rbx
        pop  rax

        ret

    .macro make_handler id, has_error_code
        .global interrupt_\id

        interrupt_\id:
            // Save registers which we will clobber.
            push r15
            push rdi
            push rsi
            push rdx

            // Save the current stack frame.
            mov r15, rsp

            // Get the interrupt ID.
            mov edi, \id

            .if \has_error_code
                // Get the interrupt frame and error code.
                lea rsi, [rsp+0x28]
                mov rdx, [rsp+0x20]

                // Align the stack to 16 byte boundary.
                sub rsp, 8
            .else
                // Get the interrupt frame and zero out error code.
                lea rsi, [rsp+0x20]
                mov rdx, 0

                // Stack is already 16 byte aligned.
            .endif

            call call_handler

            .if \has_error_code
                // Remove stack alignment from before.
                add rsp, 8
            .endif

            // Restore clobbered registers.
            pop rdx
            pop rsi
            pop rdi
            pop r15

            .if \has_error_code
                // Pop the error code.
                add rsp, 8
            .endif

            iretq

    .endm

    make_handler 0,   0
    make_handler 1,   0
    make_handler 2,   0
    make_handler 3,   0
    make_handler 4,   0
    make_handler 5,   0
    make_handler 6,   0
    make_handler 7,   0
    make_handler 8,   1
    make_handler 9,   0
    make_handler 10,  1
    make_handler 11,  1
    make_handler 12,  1
    make_handler 13,  1
    make_handler 14,  1
    make_handler 15,  0
    make_handler 16,  0
    make_handler 17,  1
    make_handler 18,  0
    make_handler 19,  0
    make_handler 20,  0
    make_handler 21,  0
    make_handler 22,  0
    make_handler 23,  0
    make_handler 24,  0
    make_handler 25,  0
    make_handler 26,  0
    make_handler 27,  0
    make_handler 28,  0
    make_handler 29,  0
    make_handler 30,  0
    make_handler 31,  0
    make_handler 32,  0
    make_handler 33,  0
    make_handler 34,  0
    make_handler 35,  0
    make_handler 36,  0
    make_handler 37,  0
    make_handler 38,  0
    make_handler 39,  0
    make_handler 40,  0
    make_handler 41,  0
    make_handler 42,  0
    make_handler 43,  0
    make_handler 44,  0
    make_handler 45,  0
    make_handler 46,  0
    make_handler 47,  0
    make_handler 48,  0
    make_handler 49,  0
    make_handler 50,  0
    make_handler 51,  0
    make_handler 52,  0
    make_handler 53,  0
    make_handler 54,  0
    make_handler 55,  0
    make_handler 56,  0
    make_handler 57,  0
    make_handler 58,  0
    make_handler 59,  0
    make_handler 60,  0
    make_handler 61,  0
    make_handler 62,  0
    make_handler 63,  0
    make_handler 64,  0
    make_handler 65,  0
    make_handler 66,  0
    make_handler 67,  0
    make_handler 68,  0
    make_handler 69,  0
    make_handler 70,  0
    make_handler 71,  0
    make_handler 72,  0
    make_handler 73,  0
    make_handler 74,  0
    make_handler 75,  0
    make_handler 76,  0
    make_handler 77,  0
    make_handler 78,  0
    make_handler 79,  0
    make_handler 80,  0
    make_handler 81,  0
    make_handler 82,  0
    make_handler 83,  0
    make_handler 84,  0
    make_handler 85,  0
    make_handler 86,  0
    make_handler 87,  0
    make_handler 88,  0
    make_handler 89,  0
    make_handler 90,  0
    make_handler 91,  0
    make_handler 92,  0
    make_handler 93,  0
    make_handler 94,  0
    make_handler 95,  0
    make_handler 96,  0
    make_handler 97,  0
    make_handler 98,  0
    make_handler 99,  0
    make_handler 100, 0
    make_handler 101, 0
    make_handler 102, 0
    make_handler 103, 0
    make_handler 104, 0
    make_handler 105, 0
    make_handler 106, 0
    make_handler 107, 0
    make_handler 108, 0
    make_handler 109, 0
    make_handler 110, 0
    make_handler 111, 0
    make_handler 112, 0
    make_handler 113, 0
    make_handler 114, 0
    make_handler 115, 0
    make_handler 116, 0
    make_handler 117, 0
    make_handler 118, 0
    make_handler 119, 0
    make_handler 120, 0
    make_handler 121, 0
    make_handler 122, 0
    make_handler 123, 0
    make_handler 124, 0
    make_handler 125, 0
    make_handler 126, 0
    make_handler 127, 0
    make_handler 128, 0
    make_handler 129, 0
    make_handler 130, 0
    make_handler 131, 0
    make_handler 132, 0
    make_handler 133, 0
    make_handler 134, 0
    make_handler 135, 0
    make_handler 136, 0
    make_handler 137, 0
    make_handler 138, 0
    make_handler 139, 0
    make_handler 140, 0
    make_handler 141, 0
    make_handler 142, 0
    make_handler 143, 0
    make_handler 144, 0
    make_handler 145, 0
    make_handler 146, 0
    make_handler 147, 0
    make_handler 148, 0
    make_handler 149, 0
    make_handler 150, 0
    make_handler 151, 0
    make_handler 152, 0
    make_handler 153, 0
    make_handler 154, 0
    make_handler 155, 0
    make_handler 156, 0
    make_handler 157, 0
    make_handler 158, 0
    make_handler 159, 0
    make_handler 160, 0
    make_handler 161, 0
    make_handler 162, 0
    make_handler 163, 0
    make_handler 164, 0
    make_handler 165, 0
    make_handler 166, 0
    make_handler 167, 0
    make_handler 168, 0
    make_handler 169, 0
    make_handler 170, 0
    make_handler 171, 0
    make_handler 172, 0
    make_handler 173, 0
    make_handler 174, 0
    make_handler 175, 0
    make_handler 176, 0
    make_handler 177, 0
    make_handler 178, 0
    make_handler 179, 0
    make_handler 180, 0
    make_handler 181, 0
    make_handler 182, 0
    make_handler 183, 0
    make_handler 184, 0
    make_handler 185, 0
    make_handler 186, 0
    make_handler 187, 0
    make_handler 188, 0
    make_handler 189, 0
    make_handler 190, 0
    make_handler 191, 0
    make_handler 192, 0
    make_handler 193, 0
    make_handler 194, 0
    make_handler 195, 0
    make_handler 196, 0
    make_handler 197, 0
    make_handler 198, 0
    make_handler 199, 0
    make_handler 200, 0
    make_handler 201, 0
    make_handler 202, 0
    make_handler 203, 0
    make_handler 204, 0
    make_handler 205, 0
    make_handler 206, 0
    make_handler 207, 0
    make_handler 208, 0
    make_handler 209, 0
    make_handler 210, 0
    make_handler 211, 0
    make_handler 212, 0
    make_handler 213, 0
    make_handler 214, 0
    make_handler 215, 0
    make_handler 216, 0
    make_handler 217, 0
    make_handler 218, 0
    make_handler 219, 0
    make_handler 220, 0
    make_handler 221, 0
    make_handler 222, 0
    make_handler 223, 0
    make_handler 224, 0
    make_handler 225, 0
    make_handler 226, 0
    make_handler 227, 0
    make_handler 228, 0
    make_handler 229, 0
    make_handler 230, 0
    make_handler 231, 0
    make_handler 232, 0
    make_handler 233, 0
    make_handler 234, 0
    make_handler 235, 0
    make_handler 236, 0
    make_handler 237, 0
    make_handler 238, 0
    make_handler 239, 0
    make_handler 240, 0
    make_handler 241, 0
    make_handler 242, 0
    make_handler 243, 0
    make_handler 244, 0
    make_handler 245, 0
    make_handler 246, 0
    make_handler 247, 0
    make_handler 248, 0
    make_handler 249, 0
    make_handler 250, 0
    make_handler 251, 0
    make_handler 252, 0
    make_handler 253, 0
    make_handler 254, 0
    make_handler 255, 0

    .att_syntax
"#);

extern {
    fn interrupt_0();
    fn interrupt_1();
    fn interrupt_2();
    fn interrupt_3();
    fn interrupt_4();
    fn interrupt_5();
    fn interrupt_6();
    fn interrupt_7();
    fn interrupt_8();
    fn interrupt_9();
    fn interrupt_10();
    fn interrupt_11();
    fn interrupt_12();
    fn interrupt_13();
    fn interrupt_14();
    fn interrupt_15();
    fn interrupt_16();
    fn interrupt_17();
    fn interrupt_18();
    fn interrupt_19();
    fn interrupt_20();
    fn interrupt_21();
    fn interrupt_22();
    fn interrupt_23();
    fn interrupt_24();
    fn interrupt_25();
    fn interrupt_26();
    fn interrupt_27();
    fn interrupt_28();
    fn interrupt_29();
    fn interrupt_30();
    fn interrupt_31();
    fn interrupt_32();
    fn interrupt_33();
    fn interrupt_34();
    fn interrupt_35();
    fn interrupt_36();
    fn interrupt_37();
    fn interrupt_38();
    fn interrupt_39();
    fn interrupt_40();
    fn interrupt_41();
    fn interrupt_42();
    fn interrupt_43();
    fn interrupt_44();
    fn interrupt_45();
    fn interrupt_46();
    fn interrupt_47();
    fn interrupt_48();
    fn interrupt_49();
    fn interrupt_50();
    fn interrupt_51();
    fn interrupt_52();
    fn interrupt_53();
    fn interrupt_54();
    fn interrupt_55();
    fn interrupt_56();
    fn interrupt_57();
    fn interrupt_58();
    fn interrupt_59();
    fn interrupt_60();
    fn interrupt_61();
    fn interrupt_62();
    fn interrupt_63();
    fn interrupt_64();
    fn interrupt_65();
    fn interrupt_66();
    fn interrupt_67();
    fn interrupt_68();
    fn interrupt_69();
    fn interrupt_70();
    fn interrupt_71();
    fn interrupt_72();
    fn interrupt_73();
    fn interrupt_74();
    fn interrupt_75();
    fn interrupt_76();
    fn interrupt_77();
    fn interrupt_78();
    fn interrupt_79();
    fn interrupt_80();
    fn interrupt_81();
    fn interrupt_82();
    fn interrupt_83();
    fn interrupt_84();
    fn interrupt_85();
    fn interrupt_86();
    fn interrupt_87();
    fn interrupt_88();
    fn interrupt_89();
    fn interrupt_90();
    fn interrupt_91();
    fn interrupt_92();
    fn interrupt_93();
    fn interrupt_94();
    fn interrupt_95();
    fn interrupt_96();
    fn interrupt_97();
    fn interrupt_98();
    fn interrupt_99();
    fn interrupt_100();
    fn interrupt_101();
    fn interrupt_102();
    fn interrupt_103();
    fn interrupt_104();
    fn interrupt_105();
    fn interrupt_106();
    fn interrupt_107();
    fn interrupt_108();
    fn interrupt_109();
    fn interrupt_110();
    fn interrupt_111();
    fn interrupt_112();
    fn interrupt_113();
    fn interrupt_114();
    fn interrupt_115();
    fn interrupt_116();
    fn interrupt_117();
    fn interrupt_118();
    fn interrupt_119();
    fn interrupt_120();
    fn interrupt_121();
    fn interrupt_122();
    fn interrupt_123();
    fn interrupt_124();
    fn interrupt_125();
    fn interrupt_126();
    fn interrupt_127();
    fn interrupt_128();
    fn interrupt_129();
    fn interrupt_130();
    fn interrupt_131();
    fn interrupt_132();
    fn interrupt_133();
    fn interrupt_134();
    fn interrupt_135();
    fn interrupt_136();
    fn interrupt_137();
    fn interrupt_138();
    fn interrupt_139();
    fn interrupt_140();
    fn interrupt_141();
    fn interrupt_142();
    fn interrupt_143();
    fn interrupt_144();
    fn interrupt_145();
    fn interrupt_146();
    fn interrupt_147();
    fn interrupt_148();
    fn interrupt_149();
    fn interrupt_150();
    fn interrupt_151();
    fn interrupt_152();
    fn interrupt_153();
    fn interrupt_154();
    fn interrupt_155();
    fn interrupt_156();
    fn interrupt_157();
    fn interrupt_158();
    fn interrupt_159();
    fn interrupt_160();
    fn interrupt_161();
    fn interrupt_162();
    fn interrupt_163();
    fn interrupt_164();
    fn interrupt_165();
    fn interrupt_166();
    fn interrupt_167();
    fn interrupt_168();
    fn interrupt_169();
    fn interrupt_170();
    fn interrupt_171();
    fn interrupt_172();
    fn interrupt_173();
    fn interrupt_174();
    fn interrupt_175();
    fn interrupt_176();
    fn interrupt_177();
    fn interrupt_178();
    fn interrupt_179();
    fn interrupt_180();
    fn interrupt_181();
    fn interrupt_182();
    fn interrupt_183();
    fn interrupt_184();
    fn interrupt_185();
    fn interrupt_186();
    fn interrupt_187();
    fn interrupt_188();
    fn interrupt_189();
    fn interrupt_190();
    fn interrupt_191();
    fn interrupt_192();
    fn interrupt_193();
    fn interrupt_194();
    fn interrupt_195();
    fn interrupt_196();
    fn interrupt_197();
    fn interrupt_198();
    fn interrupt_199();
    fn interrupt_200();
    fn interrupt_201();
    fn interrupt_202();
    fn interrupt_203();
    fn interrupt_204();
    fn interrupt_205();
    fn interrupt_206();
    fn interrupt_207();
    fn interrupt_208();
    fn interrupt_209();
    fn interrupt_210();
    fn interrupt_211();
    fn interrupt_212();
    fn interrupt_213();
    fn interrupt_214();
    fn interrupt_215();
    fn interrupt_216();
    fn interrupt_217();
    fn interrupt_218();
    fn interrupt_219();
    fn interrupt_220();
    fn interrupt_221();
    fn interrupt_222();
    fn interrupt_223();
    fn interrupt_224();
    fn interrupt_225();
    fn interrupt_226();
    fn interrupt_227();
    fn interrupt_228();
    fn interrupt_229();
    fn interrupt_230();
    fn interrupt_231();
    fn interrupt_232();
    fn interrupt_233();
    fn interrupt_234();
    fn interrupt_235();
    fn interrupt_236();
    fn interrupt_237();
    fn interrupt_238();
    fn interrupt_239();
    fn interrupt_240();
    fn interrupt_241();
    fn interrupt_242();
    fn interrupt_243();
    fn interrupt_244();
    fn interrupt_245();
    fn interrupt_246();
    fn interrupt_247();
    fn interrupt_248();
    fn interrupt_249();
    fn interrupt_250();
    fn interrupt_251();
    fn interrupt_252();
    fn interrupt_253();
    fn interrupt_254();
    fn interrupt_255();
}

pub const INTERRUPT_HANDLERS: [unsafe extern fn(); 256] = [
    interrupt_0,    interrupt_1,    interrupt_2,
    interrupt_3,    interrupt_4,    interrupt_5,
    interrupt_6,    interrupt_7,    interrupt_8,
    interrupt_9,    interrupt_10,   interrupt_11,
    interrupt_12,   interrupt_13,   interrupt_14,
    interrupt_15,   interrupt_16,   interrupt_17,
    interrupt_18,   interrupt_19,   interrupt_20,
    interrupt_21,   interrupt_22,   interrupt_23,
    interrupt_24,   interrupt_25,   interrupt_26,
    interrupt_27,   interrupt_28,   interrupt_29,
    interrupt_30,   interrupt_31,   interrupt_32,
    interrupt_33,   interrupt_34,   interrupt_35,
    interrupt_36,   interrupt_37,   interrupt_38,
    interrupt_39,   interrupt_40,   interrupt_41,
    interrupt_42,   interrupt_43,   interrupt_44,
    interrupt_45,   interrupt_46,   interrupt_47,
    interrupt_48,   interrupt_49,   interrupt_50,
    interrupt_51,   interrupt_52,   interrupt_53,
    interrupt_54,   interrupt_55,   interrupt_56,
    interrupt_57,   interrupt_58,   interrupt_59,
    interrupt_60,   interrupt_61,   interrupt_62,
    interrupt_63,   interrupt_64,   interrupt_65,
    interrupt_66,   interrupt_67,   interrupt_68,
    interrupt_69,   interrupt_70,   interrupt_71,
    interrupt_72,   interrupt_73,   interrupt_74,
    interrupt_75,   interrupt_76,   interrupt_77,
    interrupt_78,   interrupt_79,   interrupt_80,
    interrupt_81,   interrupt_82,   interrupt_83,
    interrupt_84,   interrupt_85,   interrupt_86,
    interrupt_87,   interrupt_88,   interrupt_89,
    interrupt_90,   interrupt_91,   interrupt_92,
    interrupt_93,   interrupt_94,   interrupt_95,
    interrupt_96,   interrupt_97,   interrupt_98,
    interrupt_99,   interrupt_100,  interrupt_101,
    interrupt_102,  interrupt_103,  interrupt_104,
    interrupt_105,  interrupt_106,  interrupt_107,
    interrupt_108,  interrupt_109,  interrupt_110,
    interrupt_111,  interrupt_112,  interrupt_113,
    interrupt_114,  interrupt_115,  interrupt_116,
    interrupt_117,  interrupt_118,  interrupt_119,
    interrupt_120,  interrupt_121,  interrupt_122,
    interrupt_123,  interrupt_124,  interrupt_125,
    interrupt_126,  interrupt_127,  interrupt_128,
    interrupt_129,  interrupt_130,  interrupt_131,
    interrupt_132,  interrupt_133,  interrupt_134,
    interrupt_135,  interrupt_136,  interrupt_137,
    interrupt_138,  interrupt_139,  interrupt_140,
    interrupt_141,  interrupt_142,  interrupt_143,
    interrupt_144,  interrupt_145,  interrupt_146,
    interrupt_147,  interrupt_148,  interrupt_149,
    interrupt_150,  interrupt_151,  interrupt_152,
    interrupt_153,  interrupt_154,  interrupt_155,
    interrupt_156,  interrupt_157,  interrupt_158,
    interrupt_159,  interrupt_160,  interrupt_161,
    interrupt_162,  interrupt_163,  interrupt_164,
    interrupt_165,  interrupt_166,  interrupt_167,
    interrupt_168,  interrupt_169,  interrupt_170,
    interrupt_171,  interrupt_172,  interrupt_173,
    interrupt_174,  interrupt_175,  interrupt_176,
    interrupt_177,  interrupt_178,  interrupt_179,
    interrupt_180,  interrupt_181,  interrupt_182,
    interrupt_183,  interrupt_184,  interrupt_185,
    interrupt_186,  interrupt_187,  interrupt_188,
    interrupt_189,  interrupt_190,  interrupt_191,
    interrupt_192,  interrupt_193,  interrupt_194,
    interrupt_195,  interrupt_196,  interrupt_197,
    interrupt_198,  interrupt_199,  interrupt_200,
    interrupt_201,  interrupt_202,  interrupt_203,
    interrupt_204,  interrupt_205,  interrupt_206,
    interrupt_207,  interrupt_208,  interrupt_209,
    interrupt_210,  interrupt_211,  interrupt_212,
    interrupt_213,  interrupt_214,  interrupt_215,
    interrupt_216,  interrupt_217,  interrupt_218,
    interrupt_219,  interrupt_220,  interrupt_221,
    interrupt_222,  interrupt_223,  interrupt_224,
    interrupt_225,  interrupt_226,  interrupt_227,
    interrupt_228,  interrupt_229,  interrupt_230,
    interrupt_231,  interrupt_232,  interrupt_233,
    interrupt_234,  interrupt_235,  interrupt_236,
    interrupt_237,  interrupt_238,  interrupt_239,
    interrupt_240,  interrupt_241,  interrupt_242,
    interrupt_243,  interrupt_244,  interrupt_245,
    interrupt_246,  interrupt_247,  interrupt_248,
    interrupt_249,  interrupt_250,  interrupt_251,
    interrupt_252,  interrupt_253,  interrupt_254,
    interrupt_255,
];
