use crate::error::{IcooError, IcooResult};
use crate::lexer::token::Span;
use std::path::{Component, Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimePermissions {
    pub fs_read: PermissionRule,
    pub fs_write: PermissionRule,
    pub fs_list: PermissionRule,
    pub env_read: PermissionRule,
    pub os_info: PermissionRule,
    pub net_connect: PermissionRule,
    pub net_listen: PermissionRule,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PermissionRule {
    AllowAll,
    DenyAll,
    AllowPaths(Vec<PathBuf>),
    AllowEnvKeys(Vec<String>),
    AllowNetEndpoints(Vec<(String, u16)>),
}

impl RuntimePermissions {
    pub fn allow_all() -> Self {
        Self {
            fs_read: PermissionRule::AllowAll,
            fs_write: PermissionRule::AllowAll,
            fs_list: PermissionRule::AllowAll,
            env_read: PermissionRule::AllowAll,
            os_info: PermissionRule::AllowAll,
            net_connect: PermissionRule::AllowAll,
            net_listen: PermissionRule::AllowAll,
        }
    }

    pub fn deny_all() -> Self {
        Self {
            fs_read: PermissionRule::DenyAll,
            fs_write: PermissionRule::DenyAll,
            fs_list: PermissionRule::DenyAll,
            env_read: PermissionRule::DenyAll,
            os_info: PermissionRule::DenyAll,
            net_connect: PermissionRule::DenyAll,
            net_listen: PermissionRule::DenyAll,
        }
    }

    pub fn can_read_fs(&self) -> bool {
        self.fs_read.allows()
    }

    pub fn can_write_fs(&self) -> bool {
        self.fs_write.allows()
    }

    pub fn can_list_fs(&self) -> bool {
        self.fs_list.allows()
    }

    pub fn can_read_env(&self) -> bool {
        self.env_read.allows()
    }

    pub fn can_read_os_info(&self) -> bool {
        self.os_info.allows()
    }

    pub fn can_connect_net(&self) -> bool {
        self.net_connect.allows()
    }

    pub fn can_listen_net(&self) -> bool {
        self.net_listen.allows()
    }

    pub fn check_fs_read(&self, span: Span) -> IcooResult<()> {
        check_permission(self.can_read_fs(), "fs.read", span)
    }

    pub fn check_fs_read_path(&self, path: impl AsRef<Path>, span: Span) -> IcooResult<()> {
        check_resource_permission(
            self.fs_read.allows_path(path.as_ref()),
            "fs.read",
            format!("path '{}'", path.as_ref().display()),
            span,
        )
    }

    pub fn check_fs_write(&self, span: Span) -> IcooResult<()> {
        check_permission(self.can_write_fs(), "fs.write", span)
    }

    pub fn check_fs_write_path(&self, path: impl AsRef<Path>, span: Span) -> IcooResult<()> {
        check_resource_permission(
            self.fs_write.allows_path(path.as_ref()),
            "fs.write",
            format!("path '{}'", path.as_ref().display()),
            span,
        )
    }

    pub fn check_fs_list(&self, span: Span) -> IcooResult<()> {
        check_permission(self.can_list_fs(), "fs.list", span)
    }

    pub fn check_fs_list_path(&self, path: impl AsRef<Path>, span: Span) -> IcooResult<()> {
        check_resource_permission(
            self.fs_list.allows_path(path.as_ref()),
            "fs.list",
            format!("path '{}'", path.as_ref().display()),
            span,
        )
    }

    pub fn check_env_read(&self, span: Span) -> IcooResult<()> {
        check_permission(self.can_read_env(), "env.read", span)
    }

    pub fn check_env_read_key(&self, key: &str, span: Span) -> IcooResult<()> {
        check_resource_permission(
            self.env_read.allows_env_key(key),
            "env.read",
            format!("key '{}'", key),
            span,
        )
    }

    pub fn check_os_info(&self, span: Span) -> IcooResult<()> {
        check_permission(self.can_read_os_info(), "os.info", span)
    }

    pub fn check_net_connect(&self, span: Span) -> IcooResult<()> {
        check_permission(self.can_connect_net(), "net.connect", span)
    }

    pub fn check_net_connect_endpoint(&self, host: &str, port: u16, span: Span) -> IcooResult<()> {
        check_resource_permission(
            self.net_connect.allows_net_endpoint(host, port),
            "net.connect",
            format!("endpoint '{}'", format_endpoint(host, port)),
            span,
        )
    }

    pub fn check_net_listen(&self, span: Span) -> IcooResult<()> {
        check_permission(self.can_listen_net(), "net.listen", span)
    }

    pub fn check_net_listen_endpoint(&self, host: &str, port: u16, span: Span) -> IcooResult<()> {
        check_resource_permission(
            self.net_listen.allows_net_endpoint(host, port),
            "net.listen",
            format!("endpoint '{}'", format_endpoint(host, port)),
            span,
        )
    }
}

impl Default for RuntimePermissions {
    fn default() -> Self {
        Self::allow_all()
    }
}

impl PermissionRule {
    pub fn allow_paths(paths: impl IntoIterator<Item = impl Into<PathBuf>>) -> Self {
        Self::AllowPaths(paths.into_iter().map(Into::into).collect())
    }

    pub fn allow_env_keys(keys: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self::AllowEnvKeys(keys.into_iter().map(Into::into).collect())
    }

    pub fn allow_net_endpoints(
        endpoints: impl IntoIterator<Item = (impl Into<String>, u16)>,
    ) -> Self {
        Self::AllowNetEndpoints(
            endpoints
                .into_iter()
                .map(|(host, port)| (host.into(), port))
                .collect(),
        )
    }

    pub fn allows(&self) -> bool {
        matches!(self, Self::AllowAll)
    }

    fn allows_path(&self, path: &Path) -> bool {
        match self {
            Self::AllowAll => true,
            Self::AllowPaths(allowed_paths) => {
                let requested = normalize_path(path);
                !requested.as_os_str().is_empty()
                    && allowed_paths.iter().any(|allowed| {
                        let allowed = normalize_path(allowed);
                        !allowed.as_os_str().is_empty() && requested.starts_with(allowed)
                    })
            }
            Self::DenyAll | Self::AllowEnvKeys(_) | Self::AllowNetEndpoints(_) => false,
        }
    }

    fn allows_env_key(&self, key: &str) -> bool {
        match self {
            Self::AllowAll => true,
            Self::AllowEnvKeys(keys) => keys.iter().any(|allowed| env_keys_equal(allowed, key)),
            Self::DenyAll | Self::AllowPaths(_) | Self::AllowNetEndpoints(_) => false,
        }
    }

    fn allows_net_endpoint(&self, host: &str, port: u16) -> bool {
        match self {
            Self::AllowAll => true,
            Self::AllowNetEndpoints(endpoints) => {
                endpoints.iter().any(|(allowed_host, allowed_port)| {
                    *allowed_port == port && allowed_host.eq_ignore_ascii_case(host)
                })
            }
            Self::DenyAll | Self::AllowPaths(_) | Self::AllowEnvKeys(_) => false,
        }
    }
}

fn check_permission(allowed: bool, capability: &str, span: Span) -> IcooResult<()> {
    if allowed {
        Ok(())
    } else {
        Err(IcooError::runtime(
            format!("permission denied: {}", capability),
            Some(span),
        ))
    }
}

fn check_resource_permission(
    allowed: bool,
    capability: &str,
    resource: String,
    span: Span,
) -> IcooResult<()> {
    if allowed {
        Ok(())
    } else {
        Err(IcooError::runtime(
            format!("permission denied: {} {}", capability, resource),
            Some(span),
        ))
    }
}

fn normalize_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !normalized.pop() {
                    normalized.push(component.as_os_str());
                }
            }
            Component::Prefix(_) | Component::RootDir | Component::Normal(_) => {
                normalized.push(component.as_os_str());
            }
        }
    }
    normalized
}

fn env_keys_equal(left: &str, right: &str) -> bool {
    if cfg!(windows) {
        left.eq_ignore_ascii_case(right)
    } else {
        left == right
    }
}

fn format_endpoint(host: &str, port: u16) -> String {
    if host.contains(':') && !host.starts_with('[') {
        format!("[{}]:{}", host, port)
    } else {
        format!("{}:{}", host, port)
    }
}
