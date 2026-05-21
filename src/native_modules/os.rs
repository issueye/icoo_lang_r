use super::NativeModuleSpec;

pub const SPEC: NativeModuleSpec = NativeModuleSpec {
    import_path: "std.os",
    kind: "os",
    type_name: "Os",
    methods: &[
        "name", "family", "arch", "pid", "cwd", "args", "exe_path", "get_env", "has_env",
    ],
};
