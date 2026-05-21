use super::NativeModuleSpec;

pub const SPEC: NativeModuleSpec = NativeModuleSpec {
    import_path: "std.toml",
    kind: "toml",
    type_name: "Toml",
    methods: &["stringify", "parse"],
};
