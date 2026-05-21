use super::NativeModuleSpec;

pub const SPEC: NativeModuleSpec = NativeModuleSpec {
    import_path: "std.io.fs",
    kind: "io.fs",
    type_name: "IoFs",
    methods: &[
        "exists",
        "is_file",
        "is_dir",
        "read_text",
        "write_text",
        "append_text",
        "list_dir",
    ],
};
