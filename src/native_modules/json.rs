use super::NativeModuleSpec;

pub const SPEC: NativeModuleSpec = NativeModuleSpec {
    import_path: "std.json",
    kind: "json",
    type_name: "Json",
    methods: &["stringify", "parse"],
};
