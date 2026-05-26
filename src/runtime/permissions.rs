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
    AllowNetTargets(Vec<NetTargetRule>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetTargetRule {
    pub host: String,
    pub port: Option<u16>,
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
        check_permission(self.can_read_fs(), "fs.read", None, span)
    }

    pub fn check_fs_write(&self, span: Span) -> IcooResult<()> {
        check_permission(self.can_write_fs(), "fs.write", None, span)
    }

    pub fn check_fs_list(&self, span: Span) -> IcooResult<()> {
        check_permission(self.can_list_fs(), "fs.list", None, span)
    }

    pub fn check_env_read(&self, span: Span) -> IcooResult<()> {
        check_permission(self.can_read_env(), "env.read", None, span)
    }

    pub fn check_os_info(&self, span: Span) -> IcooResult<()> {
        check_permission(self.can_read_os_info(), "os.info", None, span)
    }

    pub fn check_net_connect(&self, span: Span) -> IcooResult<()> {
        check_permission(self.can_connect_net(), "net.connect", None, span)
    }

    pub fn check_net_listen(&self, span: Span) -> IcooResult<()> {
        check_permission(self.can_listen_net(), "net.listen", None, span)
    }

    pub fn check_fs_read_path(&self, path: impl AsRef<Path>, span: Span) -> IcooResult<()> {
        self.fs_read.check_path("fs.read", path.as_ref(), span)
    }

    pub fn check_fs_write_path(&self, path: impl AsRef<Path>, span: Span) -> IcooResult<()> {
        self.fs_write.check_path("fs.write", path.as_ref(), span)
    }

    pub fn check_fs_list_path(&self, path: impl AsRef<Path>, span: Span) -> IcooResult<()> {
        self.fs_list.check_path("fs.list", path.as_ref(), span)
    }

    pub fn check_env_key(&self, key: &str, span: Span) -> IcooResult<()> {
        self.env_read.check_env_key("env.read", key, span)
    }

    pub fn check_net_connect_target(&self, host: &str, port: u16, span: Span) -> IcooResult<()> {
        self.net_connect
            .check_net_target("net.connect", host, port, span)
    }

    pub fn check_net_listen_target(&self, host: &str, port: u16, span: Span) -> IcooResult<()> {
        self.net_listen
            .check_net_target("net.listen", host, port, span)
    }
}

impl Default for RuntimePermissions {
    fn default() -> Self {
        Self::allow_all()
    }
}

impl PermissionRule {
    pub fn allow_paths<I, P>(paths: I) -> Self
    where
        I: IntoIterator<Item = P>,
        P: Into<PathBuf>,
    {
        Self::AllowPaths(paths.into_iter().map(Into::into).collect())
    }

    pub fn allow_env_keys<I, S>(keys: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self::AllowEnvKeys(keys.into_iter().map(Into::into).collect())
    }

    pub fn allow_net_targets<I>(targets: I) -> Self
    where
        I: IntoIterator<Item = NetTargetRule>,
    {
        Self::AllowNetTargets(targets.into_iter().collect())
    }

    pub fn allows(&self) -> bool {
        !matches!(self, Self::DenyAll)
    }

    fn check_path(&self, capability: &str, path: &Path, span: Span) -> IcooResult<()> {
        let target = normalize_path(path);
        let allowed = match self {
            Self::AllowAll => true,
            Self::AllowPaths(paths) => paths
                .iter()
                .map(|path| normalize_path(path))
                .any(|allowed_path| target.starts_with(allowed_path)),
            Self::DenyAll | Self::AllowEnvKeys(_) | Self::AllowNetTargets(_) => false,
        };
        check_permission(
            allowed,
            capability,
            Some(target.to_string_lossy().into_owned()),
            span,
        )
    }

    fn check_env_key(&self, capability: &str, key: &str, span: Span) -> IcooResult<()> {
        let allowed = match self {
            Self::AllowAll => true,
            Self::AllowEnvKeys(keys) => keys.iter().any(|allowed| env_key_matches(allowed, key)),
            Self::DenyAll | Self::AllowPaths(_) | Self::AllowNetTargets(_) => false,
        };
        check_permission(allowed, capability, Some(key.to_string()), span)
    }

    fn check_net_target(
        &self,
        capability: &str,
        host: &str,
        port: u16,
        span: Span,
    ) -> IcooResult<()> {
        let allowed = match self {
            Self::AllowAll => true,
            Self::AllowNetTargets(targets) => {
                targets.iter().any(|target| target.matches(host, port))
            }
            Self::DenyAll | Self::AllowPaths(_) | Self::AllowEnvKeys(_) => false,
        };
        check_permission(
            allowed,
            capability,
            Some(format!("{}:{}", host, port)),
            span,
        )
    }
}

impl NetTargetRule {
    pub fn new(host: impl Into<String>, port: Option<u16>) -> Self {
        Self {
            host: host.into(),
            port,
        }
    }

    pub fn host(host: impl Into<String>) -> Self {
        Self::new(host, None)
    }

    pub fn host_port(host: impl Into<String>, port: u16) -> Self {
        Self::new(host, Some(port))
    }

    fn matches(&self, host: &str, port: u16) -> bool {
        self.host.eq_ignore_ascii_case(host) && self.port.map_or(true, |allowed| allowed == port)
    }
}

fn check_permission(
    allowed: bool,
    capability: &str,
    resource: Option<String>,
    span: Span,
) -> IcooResult<()> {
    if allowed {
        Ok(())
    } else {
        let message = if let Some(resource) = resource {
            format!("permission denied: {} {}", capability, resource)
        } else {
            format!("permission denied: {}", capability)
        };
        Err(IcooError::runtime(message, Some(span)))
    }
}

fn normalize_path(path: &Path) -> PathBuf {
    if let Ok(path) = path.canonicalize() {
        return path;
    }

    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map(|cwd| cwd.join(path))
            .unwrap_or_else(|_| path.to_path_buf())
    };

    normalize_existing_prefix(&absolute).unwrap_or_else(|| normalize_lexically(&absolute))
}

fn normalize_existing_prefix(path: &Path) -> Option<PathBuf> {
    let mut suffix = Vec::new();
    let mut current = path;
    loop {
        if let Ok(mut prefix) = current.canonicalize() {
            for component in suffix.iter().rev() {
                prefix.push(component);
            }
            return Some(normalize_lexically(&prefix));
        }
        let name = current.file_name()?.to_os_string();
        suffix.push(name);
        current = current.parent()?;
    }
}

fn normalize_lexically(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            _ => normalized.push(component.as_os_str()),
        }
    }
    normalized
}

fn env_key_matches(allowed: &str, key: &str) -> bool {
    if cfg!(windows) {
        allowed.eq_ignore_ascii_case(key)
    } else {
        allowed == key
    }
}
