// Advanced settings — values that today live in environment variables and are
// only read once, at process start (`shared/src/env.rs`, `Env::parse()`).
//
// Every field in this module is `Option<T>`:
//   * `None` means "not set in the settings database" — the environment
//     variable governs, exactly as it does on a panel that has never saved
//     this tab. This is what makes the env var the fallback the task calls
//     for, and what keeps a fresh install behaving exactly like today.
//   * `Some(v)` is the value an admin saved in Settings -> Advanced.
//
// LIVE vs RESTART — read this before assuming a saved value applies:
//
// The settings store itself is live (a save publishes a new buffer at once,
// stale readers catch up within the 60 s cache TTL — see
// `shared/src/settings/mod.rs`, `Settings::get`). But a live settings value
// only changes behaviour if its consumer re-reads settings. Of the fields
// below, only `request_log_limit` has such a consumer: the per-request
// middleware in `backend/src/lib.rs` (`handle_request`) resolves it from
// settings on every request, falling back to `Env.app_request_log_limit`.
// Every other consumer reads its value once at startup, from `Env` (or from
// `Database` / `Cache` / the Sentry client, which are all built from `Env`
// before settings are even loaded — `Settings` is constructed in
// `backend/src/lib.rs` after `Database::new` and `Cache::new`, and
// `Env::parse()` runs before the database exists, so settings cannot be
// consulted at parse time). For those fields the environment variable
// remains the source of truth at runtime; the saved settings value is the
// admin's recorded intent and is shown in the UI with an explicit
// restart-required warning. Making a stored value actually win at boot
// would need a startup-order change (read settings from a throwaway pool
// before `Database::new`, or interior-mutable `Env` + setters on
// `Database`/`Cache`) — deliberately NOT attempted here, because this
// change is staged uncompiled and that restructure must be reviewed on its
// own. See `MANIFEST.md` / `SUMMARY.md` in the staging directory.
//
// Per-field status (consumer, and what happens after a save + restart):
//   * `password_algorithm` / `password_bcrypt_cost` /
//     `password_argon2_memory_mib` / `password_argon2_iterations` —
//     RESTART-REQUIRED, env is truth. Consumer: `Env::parse()` builds a
//     `crypt::PasswordAlgorithm`, copied into `Database.password_algorithm`
//     (`shared/src/database.rs:66,153`). New hashes use it; existing hashes
//     are rewritten on next login (`PasswordHashString::needs_rehash`).
//     Changing the algorithm never locks anyone out.
//   * `blocked_cidrs` — RESTART-REQUIRED, env is truth. Consumer:
//     `shared/src/net.rs` (`BlockedIpResolver`, `is_blocked_ip`) reads
//     `env.app_blocked_cidrs` on every outbound resolve, but the list itself
//     is fixed at startup. Empty list disables the SSRF protection.
//   * `trusted_proxies` — RESTART-REQUIRED, env is truth. Consumer:
//     `Env::find_ip` / `find_host` (`shared/src/env.rs`) read
//     `env.app_trusted_proxies` per request, but the list is fixed at
//     startup. Wrong values here let clients spoof `X-Forwarded-For`.
//   * `request_log_limit` — LIVE. Consumer: `handle_request`
//     (`backend/src/lib.rs`) resolves
//     `settings.advanced.request_log_limit` per request, falling back to
//     `env.app_request_log_limit` when unset (or when settings cannot be
//     read). `0` means unlimited.
//   * `sentry_url` / `sentry_tracing_sample_rate` — RESTART-REQUIRED, env is
//     truth. Consumer: `sentry::init` in `backend/src/lib.rs` (before the
//     database/settings exist). The sample rate only applies to spans whose
//     operation does not start with `shared::cache::` (those are sampled
//     at 0.0).
//   * `enable_wings_proxy` — RESTART-REQUIRED, env is truth. Consumer: the
//     wings-proxy gate in `backend/src/lib.rs` reads
//     `env.app_enable_wings_proxy` per request, but the value is fixed at
//     startup.
//   * `use_decryption_cache` — RESTART-REQUIRED, env is truth. Consumer:
//     `Database::new` builds (or skips) the `moka` decryption cache from
//     `env.app_use_decryption_cache` (`shared/src/database.rs:67`).
//   * `use_internal_cache` — RESTART-REQUIRED, env is truth. Consumer:
//     `Cache::new` (`shared/src/cache.rs:258,331,355`) snapshots
//     `env.app_use_internal_cache` at construction; it decides the
//     effective in-memory TTL of cached values.
//
// Defaults below are the code defaults in `shared/src/env.rs` (and
// `shared/src/crypt.rs` for the password ranges). They are the *effective*
// defaults — the value in force when neither the setting nor the env var is
// set — not values written into the database: `mock()` and a fresh
// deserializer both return `None` everywhere, so a panel that never opens
// the Advanced tab behaves byte-for-byte like today.
//
// The Sentry DSN is stored in plaintext like the captcha secrets (see the
// audit §1): a Sentry DSN carries only the public key, and the GET settings
// route already returns the whole `AppSettings` uncensored to
// `settings.read` holders. It is NOT in `AppSettings::censored()` because
// that only blanks true secrets.

use super::{
    ExtensionSettings, SettingsDeserializeExt, SettingsDeserializer, SettingsSerializeExt,
    SettingsSerializer,
};
use crate::crypt::PasswordAlgorithm;
use crate::prelude::StringExt;
use compact_str::ToCompactString;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Which algorithm new password hashes are written with. Mirrors the
/// `APP_PASSWORD_ALGORITHM` env var (`bcrypt` | `argon2id`).
#[derive(Clone, Copy, ToSchema, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PasswordAlgorithmSetting {
    Bcrypt,
    Argon2id,
}

impl PasswordAlgorithmSetting {
    /// Build the `crypt::PasswordAlgorithm` this setting describes. The
    /// `PasswordAlgorithm::bcrypt` / `argon2id` constructors enforce the
    /// ranges (`crypt::PasswordAlgorithm::BCRYPT_COST`,
    /// `ARGON2_MEMORY_MIB`, `ARGON2_ITERATIONS`), so an out-of-range
    /// stored value fails here instead of being silently clamped.
    pub fn to_password_algorithm(
        self,
        bcrypt_cost: u32,
        argon2_memory_mib: u32,
        argon2_iterations: u32,
    ) -> Result<PasswordAlgorithm, anyhow::Error> {
        match self {
            Self::Bcrypt => PasswordAlgorithm::bcrypt(bcrypt_cost),
            Self::Argon2id => {
                PasswordAlgorithm::argon2id(argon2_memory_mib, argon2_iterations)
            }
        }
    }

    /// Split a `crypt::PasswordAlgorithm` back into this setting plus its
    /// parameters (used by the future startup reconciliation and by tests).
    pub fn from_password_algorithm(algorithm: PasswordAlgorithm) -> (Self, u32, u32, u32) {
        match algorithm {
            PasswordAlgorithm::Bcrypt { cost } => (Self::Bcrypt, cost, 64, 3),
            PasswordAlgorithm::Argon2id {
                memory_mib,
                iterations,
            } => (Self::Argon2id, 12, memory_mib, iterations),
        }
    }
}

/// The hardcoded SSRF block list from `Env::default_blocked_cidrs`
/// (`shared/src/env.rs`). This is the effective default for `blocked_cidrs`
/// when neither the setting nor `APP_BLOCKED_CIDRS` is set. It is also the
/// list `.env.example` documents. Keep the two in sync.
pub const DEFAULT_BLOCKED_CIDRS: [&str; 17] = [
    "0.0.0.0/8",
    "127.0.0.0/8",
    "10.0.0.0/8",
    "100.64.0.0/10",
    "172.16.0.0/12",
    "192.168.0.0/16",
    "169.254.0.0/16",
    "192.0.0.0/24",
    "198.18.0.0/15",
    "224.0.0.0/4",
    "240.0.0.0/4",
    "::/128",
    "::1/128",
    "fe80::/10",
    "fc00::/7",
    "2002::/16",
    "ff00::/8",
];

/// Effective (code) defaults, used by the UI help text and by
/// `AppSettingsAdvanced::effective_*` when a field is unset.
pub const DEFAULT_PASSWORD_BCRYPT_COST: u32 = 12;
pub const DEFAULT_PASSWORD_ARGON2_MEMORY_MIB: u32 = 64;
pub const DEFAULT_PASSWORD_ARGON2_ITERATIONS: u32 = 3;
pub const DEFAULT_REQUEST_LOG_LIMIT: usize = 250;
pub const DEFAULT_SENTRY_TRACING_SAMPLE_RATE: f32 = 1.0;

#[derive(Clone, ToSchema, Serialize, Deserialize)]
pub struct AppSettingsAdvanced {
    /// `APP_PASSWORD_ALGORITHM`. `None` = env governs.
    pub password_algorithm: Option<PasswordAlgorithmSetting>,
    /// `APP_PASSWORD_BCRYPT_COST`, 10–16. `None` = env governs.
    pub password_bcrypt_cost: Option<u32>,
    /// `APP_PASSWORD_ARGON2_MEMORY`, MiB, 19–1024. `None` = env governs.
    pub password_argon2_memory_mib: Option<u32>,
    /// `APP_PASSWORD_ARGON2_ITERATIONS`, 1–16. `None` = env governs.
    pub password_argon2_iterations: Option<u32>,
    /// `APP_BLOCKED_CIDRS`. `None` = env governs (or the 17-entry
    /// `DEFAULT_BLOCKED_CIDRS` when the env var is also unset). An
    /// explicitly saved empty list disables the protection — same as an
    /// empty env var.
    pub blocked_cidrs: Option<Vec<compact_str::CompactString>>,
    /// `APP_TRUSTED_PROXIES`. `None` = env governs (default: no proxy is
    /// trusted). An explicitly saved empty list trusts no proxy.
    pub trusted_proxies: Option<Vec<compact_str::CompactString>>,
    /// `APP_REQUEST_LOG_LIMIT`, requests/second written to the log.
    /// `0` = unlimited. `None` = env governs (default 250). LIVE.
    pub request_log_limit: Option<usize>,
    /// `SENTRY_URL` (Sentry DSN). `None` = env governs (default: Sentry
    /// disabled). Empty string is treated as unset on load.
    pub sentry_url: Option<compact_str::CompactString>,
    /// `SENTRY_TRACING_SAMPLE_RATE`, 0.0–1.0. `None` = env governs
    /// (default 1.0).
    pub sentry_tracing_sample_rate: Option<f32>,
    /// `APP_ENABLE_WINGS_PROXY`. `None` = env governs (default false).
    pub enable_wings_proxy: Option<bool>,
    /// `APP_USE_DECRYPTION_CACHE`. `None` = env governs (default false).
    pub use_decryption_cache: Option<bool>,
    /// `APP_USE_INTERNAL_CACHE`. `None` = env governs (default true).
    pub use_internal_cache: Option<bool>,
}

impl AppSettingsAdvanced {
    /// Resolve the password algorithm to hash new passwords with. Returns
    /// `None` when the setting is incomplete (algorithm set but its
    /// parameters missing) — the caller must then use the env value
    /// (`Database::password_algorithm`). A fully-set but out-of-range
    /// value also yields `None`: the PUT route rejects out-of-range values,
    /// so this only guards against a hand-edited database row, and a bad
    /// row must never break logins.
    pub fn resolve_password_algorithm(&self) -> Option<PasswordAlgorithm> {
        let algorithm = self.password_algorithm?;
        algorithm
            .to_password_algorithm(
                self.password_bcrypt_cost
                    .unwrap_or(DEFAULT_PASSWORD_BCRYPT_COST),
                self.password_argon2_memory_mib
                    .unwrap_or(DEFAULT_PASSWORD_ARGON2_MEMORY_MIB),
                self.password_argon2_iterations
                    .unwrap_or(DEFAULT_PASSWORD_ARGON2_ITERATIONS),
            )
            .ok()
    }

    /// Parse the stored blocked CIDRs. Returns `None` when unset (env
    /// governs) or when any entry fails to parse (a hand-edited bad row
    /// must not disable SSRF protection by accident — the PUT route
    /// validates every entry, so this is defence in depth only).
    pub fn resolve_blocked_cidrs(&self) -> Option<Vec<cidr::IpCidr>> {
        self.blocked_cidrs.as_ref().map(|cidrs| {
            cidrs
                .iter()
                .map(|cidr| cidr.parse())
                .collect::<Result<Vec<_>, _>>()
        })?
        .ok()
    }

    /// Parse the stored trusted proxies. Same `None`-on-unset / `None`-on-
    /// bad-row contract as `resolve_blocked_cidrs`.
    pub fn resolve_trusted_proxies(&self) -> Option<Vec<cidr::IpCidr>> {
        self.trusted_proxies.as_ref().map(|proxies| {
            proxies
                .iter()
                .map(|proxy| proxy.parse())
                .collect::<Result<Vec<_>, _>>()
        })?
        .ok()
    }

    /// The request-log limit in force: the saved setting when set, else the
    /// env value passed in. This is the one resolver with a live consumer
    /// (`handle_request` in `backend/src/lib.rs`).
    pub fn effective_request_log_limit(&self, env_limit: usize) -> usize {
        self.request_log_limit.unwrap_or(env_limit)
    }
}

#[async_trait::async_trait]
impl SettingsSerializeExt for AppSettingsAdvanced {
    async fn serialize(
        &self,
        serializer: SettingsSerializer,
    ) -> Result<SettingsSerializer, anyhow::Error> {
        // `None` is written as an empty raw value (same convention as the
        // `updates` tokens) or JSON `null` for lists, so "unset" round-trips
        // and the env var keeps governing.
        Ok(serializer
            .write_raw_setting(
                "password_algorithm",
                self.password_algorithm
                    .map(|algorithm| match algorithm {
                        PasswordAlgorithmSetting::Bcrypt => "bcrypt",
                        PasswordAlgorithmSetting::Argon2id => "argon2id",
                    })
                    .unwrap_or(""),
            )
            .write_raw_setting(
                "password_bcrypt_cost",
                self.password_bcrypt_cost
                    .map(|cost| cost.to_compact_string())
                    .unwrap_or_default(),
            )
            .write_raw_setting(
                "password_argon2_memory_mib",
                self.password_argon2_memory_mib
                    .map(|memory| memory.to_compact_string())
                    .unwrap_or_default(),
            )
            .write_raw_setting(
                "password_argon2_iterations",
                self.password_argon2_iterations
                    .map(|iterations| iterations.to_compact_string())
                    .unwrap_or_default(),
            )
            .write_serde_setting("blocked_cidrs", &self.blocked_cidrs)?
            .write_serde_setting("trusted_proxies", &self.trusted_proxies)?
            .write_raw_setting(
                "request_log_limit",
                self.request_log_limit
                    .map(|limit| limit.to_compact_string())
                    .unwrap_or_default(),
            )
            .write_raw_setting(
                "sentry_url",
                self.sentry_url.as_deref().unwrap_or(""),
            )
            .write_raw_setting(
                "sentry_tracing_sample_rate",
                self.sentry_tracing_sample_rate
                    .map(|rate| rate.to_compact_string())
                    .unwrap_or_default(),
            )
            .write_raw_setting(
                "enable_wings_proxy",
                self.enable_wings_proxy
                    .map(|enabled| enabled.to_compact_string())
                    .unwrap_or_default(),
            )
            .write_raw_setting(
                "use_decryption_cache",
                self.use_decryption_cache
                    .map(|enabled| enabled.to_compact_string())
                    .unwrap_or_default(),
            )
            .write_raw_setting(
                "use_internal_cache",
                self.use_internal_cache
                    .map(|enabled| enabled.to_compact_string())
                    .unwrap_or_default(),
            ))
    }
}

impl AppSettingsAdvanced {
    #[cfg(test)]
    pub fn mock() -> Self {
        // All `None`: a mock (and a fresh install) behaves exactly like
        // today's env-only panel — the env vars govern.
        Self {
            password_algorithm: None,
            password_bcrypt_cost: None,
            password_argon2_memory_mib: None,
            password_argon2_iterations: None,
            blocked_cidrs: None,
            trusted_proxies: None,
            request_log_limit: None,
            sentry_url: None,
            sentry_tracing_sample_rate: None,
            enable_wings_proxy: None,
            use_decryption_cache: None,
            use_internal_cache: None,
        }
    }
}

pub struct AppSettingsAdvancedDeserializer;

#[async_trait::async_trait]
impl SettingsDeserializeExt for AppSettingsAdvancedDeserializer {
    async fn deserialize_boxed(
        &self,
        mut deserializer: SettingsDeserializer<'_>,
    ) -> Result<ExtensionSettings, anyhow::Error> {
        // Every field degrades to `None` (env governs) on a missing,
        // empty, unparsable or out-of-range stored value. A bad row in this
        // module must never fail the whole settings load.
        let password_algorithm = deserializer
            .take_raw_setting("password_algorithm")
            .and_then(|s| s.into_optional())
            .and_then(|s| match s.as_str() {
                "bcrypt" => Some(PasswordAlgorithmSetting::Bcrypt),
                "argon2id" => Some(PasswordAlgorithmSetting::Argon2id),
                _ => None,
            });
        let password_bcrypt_cost = deserializer
            .take_raw_setting("password_bcrypt_cost")
            .and_then(|s| s.parse().ok())
            .filter(|cost| PasswordAlgorithm::BCRYPT_COST.contains(cost));
        let password_argon2_memory_mib = deserializer
            .take_raw_setting("password_argon2_memory_mib")
            .and_then(|s| s.parse().ok())
            .filter(|memory| PasswordAlgorithm::ARGON2_MEMORY_MIB.contains(memory));
        let password_argon2_iterations = deserializer
            .take_raw_setting("password_argon2_iterations")
            .and_then(|s| s.parse().ok())
            .filter(|iterations| PasswordAlgorithm::ARGON2_ITERATIONS.contains(iterations));
        let blocked_cidrs: Option<Vec<compact_str::CompactString>> = deserializer
            .read_serde_setting("blocked_cidrs")
            .ok()
            .flatten()
            .filter(|cidrs: &Vec<compact_str::CompactString>| {
                cidrs
                    .iter()
                    .all(|cidr| cidr.parse::<cidr::IpCidr>().is_ok())
            });
        let trusted_proxies: Option<Vec<compact_str::CompactString>> = deserializer
            .read_serde_setting("trusted_proxies")
            .ok()
            .flatten()
            .filter(|proxies: &Vec<compact_str::CompactString>| {
                proxies
                    .iter()
                    .all(|proxy| proxy.parse::<cidr::IpCidr>().is_ok())
            });
        let request_log_limit = deserializer
            .take_raw_setting("request_log_limit")
            .and_then(|s| s.parse().ok());
        let sentry_url = deserializer
            .take_raw_setting("sentry_url")
            .and_then(|s| s.into_optional());
        let sentry_tracing_sample_rate = deserializer
            .take_raw_setting("sentry_tracing_sample_rate")
            .and_then(|s| s.parse::<f32>().ok())
            .filter(|rate| rate.is_finite() && (0.0..=1.0).contains(rate));
        let bool_setting = |deserializer: &mut SettingsDeserializer<'_>, key: &str| {
            deserializer
                .take_raw_setting(key)
                .and_then(|s| s.into_optional())
                .and_then(|s| match s.as_str() {
                    "true" => Some(true),
                    "false" => Some(false),
                    _ => None,
                })
        };
        let enable_wings_proxy = bool_setting(&mut deserializer, "enable_wings_proxy");
        let use_decryption_cache = bool_setting(&mut deserializer, "use_decryption_cache");
        let use_internal_cache = bool_setting(&mut deserializer, "use_internal_cache");

        Ok(Box::new(AppSettingsAdvanced {
            password_algorithm,
            password_bcrypt_cost,
            password_argon2_memory_mib,
            password_argon2_iterations,
            blocked_cidrs,
            trusted_proxies,
            request_log_limit,
            sentry_url,
            sentry_tracing_sample_rate,
            enable_wings_proxy,
            use_decryption_cache,
            use_internal_cache,
        }))
    }
}
