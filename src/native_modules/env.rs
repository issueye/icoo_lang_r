use super::NativeModuleSpec;

pub const SPEC: NativeModuleSpec = NativeModuleSpec {
    import_path: "std.env",
    kind: "env",
    type_name: "Env",
    methods: &["cwd", "args", "get", "has"],
};
