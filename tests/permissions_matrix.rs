use icoo_lang_r::{PermissionRule, RuntimePermissions};

#[test]
fn allow_all_enables_every_runtime_capability() {
    let permissions = RuntimePermissions::allow_all();

    assert!(permissions.can_read_fs());
    assert!(permissions.can_write_fs());
    assert!(permissions.can_list_fs());
    assert!(permissions.can_read_env());
    assert!(permissions.can_read_os_info());
    assert!(permissions.can_connect_net());
    assert!(permissions.can_listen_net());
}

#[test]
fn deny_all_disables_every_runtime_capability() {
    let permissions = RuntimePermissions::deny_all();

    assert!(!permissions.can_read_fs());
    assert!(!permissions.can_write_fs());
    assert!(!permissions.can_list_fs());
    assert!(!permissions.can_read_env());
    assert!(!permissions.can_read_os_info());
    assert!(!permissions.can_connect_net());
    assert!(!permissions.can_listen_net());
}

#[test]
fn individual_rules_drive_individual_capability_queries() {
    let permissions = RuntimePermissions {
        fs_read: PermissionRule::AllowAll,
        fs_write: PermissionRule::DenyAll,
        fs_list: PermissionRule::AllowAll,
        env_read: PermissionRule::DenyAll,
        os_info: PermissionRule::AllowAll,
        net_connect: PermissionRule::DenyAll,
        net_listen: PermissionRule::AllowAll,
    };

    assert!(permissions.can_read_fs());
    assert!(!permissions.can_write_fs());
    assert!(permissions.can_list_fs());
    assert!(!permissions.can_read_env());
    assert!(permissions.can_read_os_info());
    assert!(!permissions.can_connect_net());
    assert!(permissions.can_listen_net());
}

#[test]
fn default_permissions_preserve_current_allowing_behavior() {
    assert_eq!(
        RuntimePermissions::default(),
        RuntimePermissions::allow_all()
    );
}

#[test]
fn unrestricted_permission_entry_still_runs_existing_pipeline() {
    icoo_lang_r::run_source_with_permissions("let value = 1 + 1", RuntimePermissions::allow_all())
        .unwrap();
}

#[test]
fn restricted_permission_entry_runs_until_module_checks_are_wired() {
    icoo_lang_r::run_source_with_permissions("let value = 1 + 1", RuntimePermissions::deny_all())
        .unwrap();
}
