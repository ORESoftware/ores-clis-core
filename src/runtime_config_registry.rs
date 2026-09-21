#![forbid(unsafe_code)]

/// Canonical repository-root CLI flag contract.
pub const CLI_FLAGS_CONFIG: &str = ".cli-flags.toml";
/// Canonical repository-root OpenTelemetry contract.
pub const ORES_OTEL_CONFIG: &str = ".ores-otel.toml";
/// Canonical repository-root Opto Sync contract.
pub const OPTO_SYNC_CONFIG: &str = ".opto-sync.toml";
/// Canonical repository-root middleware contract.
pub const ORES_MW_CONFIG: &str = ".ores-mw.toml";
/// Canonical repository-root rate-limit contract.
pub const ORES_RL_CONFIG: &str = ".ores-rl.toml";
/// Canonical repository-root LRU/cache contract.
pub const ORES_LRU_CONFIG: &str = ".ores-lru.toml";
/// Canonical repository-root SOPS / encrypted configuration binding.
pub const ORES_SOPS_CONFIG: &str = ".ores-sops.toml";
/// Canonical repository-root drag-and-drop contract binding.
pub const ORES_DND_CONFIG: &str = ".ores-dnd.toml";
/// Canonical repository-root WebSocket contract binding.
pub const ORES_WS_CONFIG: &str = ".ores-ws.toml";
/// Canonical repository-root service-worker contract binding.
pub const ORES_SW_CONFIG: &str = ".ores-sw.toml";
/// Canonical repository-root sidecar contract.
pub const ORES_SIDECAR_CONFIG: &str = ".ores-sidecar.toml";
/// Canonical repository-root Shared Auth binding.
pub const SHARED_AUTH_CONFIG: &str = ".shared-auth.toml";
/// Compatibility-only Shared Auth spelling. It must not coexist with the canonical file.
pub const SHARED_AUTH_COMPAT_CONFIG: &str = ".auth-shared.toml";

/// Reviewed runtime-TOML names currently admitted by `ores-cli` portfolio policy.
///
/// This list intentionally preserves the operational registry exactly while the
/// public policy document catches up on older registered extensions. Consumers
/// should import this list instead of hand-maintaining another spelling table.
pub const REGISTERED_RUNTIME_CONFIGS: &[&str] = &[
    ORES_OTEL_CONFIG,
    ".ores-chat.toml",
    ".ores-forms.toml",
    OPTO_SYNC_CONFIG,
    ".fanwaave-cfg.toml",
    ORES_MW_CONFIG,
    ORES_RL_CONFIG,
    ORES_LRU_CONFIG,
    ORES_SOPS_CONFIG,
    ORES_DND_CONFIG,
    ORES_WS_CONFIG,
    ORES_SW_CONFIG,
    ".ores-lock.toml",
    SHARED_AUTH_CONFIG,
    SHARED_AUTH_COMPAT_CONFIG,
    ".ores-rpc.toml",
    ".ores-legal.toml",
    ".ores-wasm.toml",
    ORES_SIDECAR_CONFIG,
    ".ores-infra.toml",
    ".indiebuild.toml",
    ".canonical-cfg.toml",
];

/// A repository-root configuration identity whose canonical spelling and aliases
/// must remain consistent across ORES CLIs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RootConfigIdentity {
    /// Stable concern identifier.
    pub concern: &'static str,
    /// Canonical repository-root filename.
    pub canonical: &'static str,
    /// Compatibility-only spellings accepted during migration.
    pub compatibility_aliases: &'static [&'static str],
}

/// An executable owner for a runtime config whose checked-in TOML is consumed by
/// code in the owning repository rather than serving as a presence-only binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExecutableRuntimeConfigOwner {
    /// Stable concern identifier matching [`RootConfigIdentity::concern`].
    pub concern: &'static str,
    /// Canonical repository-root filename.
    pub canonical: &'static str,
    /// Repository that owns both the contract and executable policy.
    pub repository: &'static str,
    /// Immutable merged revision containing the executable config policy.
    pub runtime_revision: &'static str,
    /// Human-authored TypeSpec authority path in the owner repository.
    pub typespec: &'static str,
    /// Human-authored JSON Schema authority path in the owner repository.
    pub json_schema: &'static str,
    /// Rust packages that expose the executable server/runtime policy.
    pub rust_packages: &'static [&'static str],
}

/// Cross-CLI filename identities that currently have more than one consumer.
pub const ROOT_CONFIG_IDENTITIES: &[RootConfigIdentity] = &[
    RootConfigIdentity {
        concern: "cli-flags",
        canonical: CLI_FLAGS_CONFIG,
        compatibility_aliases: &[],
    },
    RootConfigIdentity {
        concern: "ores-otel",
        canonical: ORES_OTEL_CONFIG,
        compatibility_aliases: &[],
    },
    RootConfigIdentity {
        concern: "opto-sync",
        canonical: OPTO_SYNC_CONFIG,
        compatibility_aliases: &[],
    },
    RootConfigIdentity {
        concern: "ores-middleware",
        canonical: ORES_MW_CONFIG,
        compatibility_aliases: &[],
    },
    RootConfigIdentity {
        concern: "ores-rate-limit",
        canonical: ORES_RL_CONFIG,
        compatibility_aliases: &[],
    },
    RootConfigIdentity {
        concern: "ores-lru",
        canonical: ORES_LRU_CONFIG,
        compatibility_aliases: &[],
    },
    RootConfigIdentity {
        concern: "ores-sops",
        canonical: ORES_SOPS_CONFIG,
        compatibility_aliases: &[],
    },
    RootConfigIdentity {
        concern: "ores-dnd",
        canonical: ORES_DND_CONFIG,
        compatibility_aliases: &[],
    },
    RootConfigIdentity {
        concern: "ores-websocket",
        canonical: ORES_WS_CONFIG,
        compatibility_aliases: &[],
    },
    RootConfigIdentity {
        concern: "ores-service-worker",
        canonical: ORES_SW_CONFIG,
        compatibility_aliases: &[],
    },
    RootConfigIdentity {
        concern: "ores-sidecar",
        canonical: ORES_SIDECAR_CONFIG,
        compatibility_aliases: &[],
    },
    RootConfigIdentity {
        concern: "shared-auth",
        canonical: SHARED_AUTH_CONFIG,
        compatibility_aliases: &[SHARED_AUTH_COMPAT_CONFIG],
    },
];

/// Configs whose canonical owners now expose executable parsing/evaluation and
/// server/runtime enforcement. This is intentionally a smaller set than
/// [`REGISTERED_RUNTIME_CONFIGS`]; absence here must not be interpreted as an
/// implementation claim for other filenames.
pub const EXECUTABLE_RUNTIME_CONFIG_OWNERS: &[ExecutableRuntimeConfigOwner] = &[
    ExecutableRuntimeConfigOwner {
        concern: "ores-websocket",
        canonical: ORES_WS_CONFIG,
        repository: "ORESoftware/ores-websocket",
        runtime_revision: "11e9493b10e499cd463f0f768364a1096f780cc7",
        typespec: "contracts/runtime-config/main.tsp",
        json_schema: "contracts/runtime-config/authored.schema.json",
        rust_packages: &["ores-websocket"],
    },
    ExecutableRuntimeConfigOwner {
        concern: "ores-service-worker",
        canonical: ORES_SW_CONFIG,
        repository: "ORESoftware/ores-sw.js",
        runtime_revision: "af40988742570008d3b7cc573b673fca97b28112",
        typespec: "contracts/config/main.tsp",
        json_schema: "contracts/config/authored.schema.json",
        rust_packages: &["ores-sw-assets", "ores-sw-axum07", "ores-sw-axum08"],
    },
];

/// Return the reviewed identity for a concern.
#[must_use]
pub fn root_config_identity(concern: &str) -> Option<&'static RootConfigIdentity> {
    ROOT_CONFIG_IDENTITIES
        .iter()
        .find(|identity| identity.concern == concern)
}

/// Return whether a filename is admitted by the current portfolio runtime-TOML registry.
#[must_use]
pub fn is_registered_runtime_config(name: &str) -> bool {
    REGISTERED_RUNTIME_CONFIGS.contains(&name)
}

/// Return the executable owner for a canonical runtime config, when one has been
/// explicitly reviewed and registered.
#[must_use]
pub fn executable_runtime_config_owner(
    canonical: &str,
) -> Option<&'static ExecutableRuntimeConfigOwner> {
    EXECUTABLE_RUNTIME_CONFIG_OWNERS
        .iter()
        .find(|owner| owner.canonical == canonical)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn registered_runtime_names_are_unique() {
        let names = REGISTERED_RUNTIME_CONFIGS
            .iter()
            .copied()
            .collect::<BTreeSet<_>>();
        assert_eq!(names.len(), REGISTERED_RUNTIME_CONFIGS.len());
    }

    #[test]
    fn canonical_shared_auth_and_opto_sync_match_portfolio_policy() {
        let shared = root_config_identity("shared-auth").expect("shared-auth identity");
        assert_eq!(shared.canonical, ".shared-auth.toml");
        assert_eq!(shared.compatibility_aliases, &[".auth-shared.toml"]);
        assert_eq!(
            root_config_identity("opto-sync")
                .expect("opto identity")
                .canonical,
            ".opto-sync.toml"
        );
    }

    #[test]
    fn server_runtime_config_names_are_canonical_and_registered() {
        for (concern, expected) in [
            ("ores-middleware", ".ores-mw.toml"),
            ("ores-sops", ".ores-sops.toml"),
            ("ores-dnd", ".ores-dnd.toml"),
            ("ores-websocket", ".ores-ws.toml"),
            ("ores-service-worker", ".ores-sw.toml"),
            ("ores-sidecar", ".ores-sidecar.toml"),
        ] {
            let identity = root_config_identity(concern).expect("runtime config identity");
            assert_eq!(identity.canonical, expected);
            assert!(
                is_registered_runtime_config(expected),
                "{expected} must be in REGISTERED_RUNTIME_CONFIGS"
            );
        }
    }

    #[test]
    fn websocket_and_service_worker_have_executable_immutable_owners() {
        for (canonical, repository, revision, package) in [
            (
                ORES_WS_CONFIG,
                "ORESoftware/ores-websocket",
                "11e9493b10e499cd463f0f768364a1096f780cc7",
                "ores-websocket",
            ),
            (
                ORES_SW_CONFIG,
                "ORESoftware/ores-sw.js",
                "af40988742570008d3b7cc573b673fca97b28112",
                "ores-sw-axum08",
            ),
        ] {
            let owner = executable_runtime_config_owner(canonical).expect("executable owner");
            assert_eq!(owner.repository, repository);
            assert_eq!(owner.runtime_revision, revision);
            assert_eq!(owner.runtime_revision.len(), 40);
            assert!(
                owner
                    .runtime_revision
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
            );
            assert!(owner.rust_packages.contains(&package));
            assert!(is_registered_runtime_config(owner.canonical));
            assert_eq!(
                root_config_identity(owner.concern)
                    .expect("matching filename identity")
                    .canonical,
                owner.canonical
            );
        }
    }

    #[test]
    fn compatibility_alias_is_registered_but_not_canonical() {
        assert!(is_registered_runtime_config(SHARED_AUTH_COMPAT_CONFIG));
        assert_ne!(SHARED_AUTH_CONFIG, SHARED_AUTH_COMPAT_CONFIG);
    }
}
