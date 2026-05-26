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
    pub process_exec: PermissionRule,
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
            process_exec: PermissionRule::AllowAll,
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
            process_exec: PermissionRule::DenyAll,
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

    pub fn can_exec_process(&self) -> bool {
        self.process_exec.allows()
    }

    pub fn check_fs_read(&self, span: Span) -> IcooResult<()> {
        check_permission(self.can_read_fs(), "fs.read", None, span)
    }

    pub fn check_fs_read_path(&self, path: impl AsRef<Path>, span: Span) -> IcooResult<()> {
        check_permission(
            self.fs_read.allows_path(path.as_ref()),
            "fs.read",
            Some(format!("path '{}'", path.as_ref().display())),
            span,
        )
    }

    pub fn check_fs_write(&self, span: Span) -> IcooResult<()> {
        check_permission(self.can_write_fs(), "fs.write", None, span)
    }

    pub fn check_fs_write_path(&self, path: impl AsRef<Path>, span: Span) -> IcooResult<()> {
        check_permission(
            self.fs_write.allows_path(path.as_ref()),
            "fs.write",
            Some(format!("path '{}'", path.as_ref().display())),
            span,
        )
    }

    pub fn check_fs_list(&self, span: Span) -> IcooResult<()> {
        check_permission(self.can_list_fs(), "fs.list", None, span)
    }

    pub fn check_fs_list_path(&self, path: impl AsRef<Path>, span: Span) -> IcooResult<()> {
        check_permission(
            self.fs_list.allows_path(path.as_ref()),
            "fs.list",
            Some(format!("path '{}'", path.as_ref().display())),
            span,
        )
    }

    pub fn check_env_read(&self, span: Span) -> IcooResult<()> {
        check_permission(self.can_read_env(), "env.read", None, span)
    }

    pub fn check_env_read_key(&self, key: &str, span: Span) -> IcooResult<()> {
        check_permission(
            self.env_read.allows_env_key(key),
            "env.read",
            Some(format!("key '{}'", key)),
            span,
        )
    }

    pub fn check_env_key(&self, key: &str, span: Span) -> IcooResult<()> {
        self.check_env_read_key(key, span)
    }

    pub fn check_os_info(&self, span: Span) -> IcooResult<()> {
        check_permission(self.can_read_os_info(), "os.info", None, span)
    }

    pub fn check_net_connect(&self, span: Span) -> IcooResult<()> {
        check_permission(self.can_connect_net(), "net.connect", None, span)
    }

    pub fn check_net_connect_endpoint(&self, host: &str, port: u16, span: Span) -> IcooResult<()> {
        check_permission(
            self.net_connect.allows_net_target(host, port),
            "net.connect",
            Some(format!("endpoint '{}'", format_endpoint(host, port))),
            span,
        )
    }

    pub fn check_net_connect_target(&self, host: &str, port: u16, span: Span) -> IcooResult<()> {
        self.check_net_connect_endpoint(host, port, span)
    }

    pub fn check_net_listen(&self, span: Span) -> IcooResult<()> {
        check_permission(self.can_listen_net(), "net.listen", None, span)
    }

    pub fn check_net_listen_endpoint(&self, host: &str, port: u16, span: Span) -> IcooResult<()> {
        check_permission(
            self.net_listen.allows_net_target(host, port),
            "net.listen",
            Some(format!("endpoint '{}'", format_endpoint(host, port))),
            span,
        )
    }

    pub fn check_net_listen_target(&self, host: &str, port: u16, span: Span) -> IcooResult<()> {
        self.check_net_listen_endpoint(host, port, span)
    }

    pub fn check_process_exec(&self, span: Span) -> IcooResult<()> {
        check_permission(self.can_exec_process(), "process.exec", None, span)
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

    pub fn allow_net_endpoints<I, H>(endpoints: I) -> Self
    where
        I: IntoIterator<Item = (H, u16)>,
        H: Into<String>,
    {
        Self::AllowNetTargets(
            endpoints
                .into_iter()
                .map(|(host, port)| NetTargetRule::host_port(host, port))
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
            Self::DenyAll | Self::AllowEnvKeys(_) | Self::AllowNetTargets(_) => false,
        }
    }

    fn allows_env_key(&self, key: &str) -> bool {
        match self {
            Self::AllowAll => true,
            Self::AllowEnvKeys(keys) => keys.iter().any(|allowed| env_keys_equal(allowed, key)),
            Self::DenyAll | Self::AllowPaths(_) | Self::AllowNetTargets(_) => false,
        }
    }

    fn allows_net_target(&self, host: &str, port: u16) -> bool {
        match self {
            Self::AllowAll => true,
            Self::AllowNetTargets(targets) => {
                targets.iter().any(|target| target.matches(host, port))
            }
            Self::DenyAll | Self::AllowPaths(_) | Self::AllowEnvKeys(_) => false,
        }
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
