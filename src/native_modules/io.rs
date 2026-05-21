use super::NativeModuleSpec;

pub const SPEC: NativeModuleSpec = NativeModuleSpec {
    import_path: "std.io",
    kind: "io",
    type_name: "Io",
    methods: &["print"],
};
