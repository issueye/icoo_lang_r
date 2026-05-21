use super::NativeModuleSpec;

pub const SPEC: NativeModuleSpec = NativeModuleSpec {
    import_path: "std.time",
    kind: "time",
    type_name: "Time",
    methods: &["now_ms", "now_sec"],
};
