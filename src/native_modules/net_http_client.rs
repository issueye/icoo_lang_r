use super::NativeModuleSpec;

pub const SPEC: NativeModuleSpec = NativeModuleSpec {
    import_path: "std.net.http.client",
    kind: "net.http.client",
    type_name: "NetHttpClient",
    methods: &[
        "get",
        "post",
        "put",
        "delete",
        "options",
        "stream_get",
        "stream_post",
        "stream_put",
        "stream_delete",
        "stream_options",
    ],
};
