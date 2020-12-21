#![no_std]

pub fn verify_cpu() {
    let features = cpu::get_features();

    macro_rules! verify_feature {
        ($name: ident) => {
            assert!(features.$name, "CPU feature \"{}\" is required but not \
                    supported by this CPU.", stringify!($name));
        }
    }

    verify_feature!(fpu);
    verify_feature!(tsc);
    verify_feature!(mmx);
    verify_feature!(sse);
    verify_feature!(sse2);
    verify_feature!(sse3);
    verify_feature!(ssse3);
    verify_feature!(sse4_1);
    verify_feature!(sse4_2);
    verify_feature!(xsave);
    verify_feature!(avx);
    verify_feature!(avx2);
    verify_feature!(fma);
    verify_feature!(apic);
    verify_feature!(xd);
    verify_feature!(bits64);
    verify_feature!(page2m);
}
