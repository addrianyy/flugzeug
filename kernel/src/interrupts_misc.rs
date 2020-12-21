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
