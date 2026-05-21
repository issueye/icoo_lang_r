use super::NativeModuleSpec;

pub const SPEC: NativeModuleSpec = NativeModuleSpec {
    import_path: "std.net.http.server",
    kind: "net.http.server",
    type_name: "NetHttpServer",
    methods: &["serve_once"],
};
