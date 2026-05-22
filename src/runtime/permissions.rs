use crate::error::{IcooError, IcooResult};
use crate::lexer::token::Span;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionRule {
    AllowAll,
    DenyAll,
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

    pub fn check_fs_write(&self, span: Span) -> IcooResult<()> {
        check_permission(self.can_write_fs(), "fs.write", span)
    }

    pub fn check_fs_list(&self, span: Span) -> IcooResult<()> {
        check_permission(self.can_list_fs(), "fs.list", span)
    }

    pub fn check_env_read(&self, span: Span) -> IcooResult<()> {
        check_permission(self.can_read_env(), "env.read", span)
    }

    pub fn check_os_info(&self, span: Span) -> IcooResult<()> {
        check_permission(self.can_read_os_info(), "os.info", span)
    }

    pub fn check_net_connect(&self, span: Span) -> IcooResult<()> {
        check_permission(self.can_connect_net(), "net.connect", span)
    }

    pub fn check_net_listen(&self, span: Span) -> IcooResult<()> {
        check_permission(self.can_listen_net(), "net.listen", span)
    }
}

impl Default for RuntimePermissions {
    fn default() -> Self {
        Self::allow_all()
    }
}

impl PermissionRule {
    pub fn allows(self) -> bool {
        matches!(self, Self::AllowAll)
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
