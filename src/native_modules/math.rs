use super::NativeModuleSpec;

pub const SPEC: NativeModuleSpec = NativeModuleSpec {
    import_path: "std.math",
    kind: "math",
    type_name: "Math",
    methods: &["abs", "floor", "ceil", "round", "min", "max", "random"],
};
