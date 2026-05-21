use super::NativeModuleSpec;

pub const SPEC: NativeModuleSpec = NativeModuleSpec {
    import_path: "std.yaml",
    kind: "yaml",
    type_name: "Yaml",
    methods: &["stringify", "parse"],
};
