use super::NativeModuleSpec;

pub const SPEC: NativeModuleSpec = NativeModuleSpec {
    import_path: "std.web.ino",
    kind: "web.ino",
    type_name: "WebIno",
    methods: &["App", "create"],
};
