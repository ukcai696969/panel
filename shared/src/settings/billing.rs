// Billing / credits settings.
//
// Money rule for this module: 1 credit = 1 minor unit of the configured
// currency (1 cent when currency is USD/GBP/EUR). All balances, prices and
// charges are integer credits (`i64`) — never floats — so rounding can only
// happen once, at the Stripe checkout boundary, where the amount in minor
// units is exactly the number of credits bought.
//
// Stripe credentials are stored in settings (see below) and may be
// overridden by environment variables, which take precedence — see
// `backend::billing::StripeConfig::resolve` for the precedence rules:
//   STRIPE_SECRET_KEY        (test mode only in this first version)
//   STRIPE_WEBHOOK_SECRET    (required for the webhook to accept anything)
//   STRIPE_PUBLISHABLE_KEY   (public key, sent to the browser)
//   BILLING_CHECKOUT_URL_TEMPLATE
// The secret key and webhook secret are encrypted at rest (base32 +
// `Database::encrypt`, the same pattern as the S3/mail/updates secrets).
// The publishable key and checkout URL template are not secret and are
// stored in plaintext.

use super::{
    ExtensionSettings, SettingsDeserializeExt, SettingsDeserializer, SettingsSerializeExt,
    SettingsSerializer,
};
use crate::{
    censor::{CENSORED_PLACEHOLDER, Censor},
    prelude::{AsyncOptionExt, StringExt},
};
use compact_str::ToCompactString;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Price of one hour of a server, in credits.
///
/// `cpu` is priced per 100 CPU units (100 = 1 core in this panel, see
/// `Server.cpu`), `memory` / `disk` per GiB. Server memory/disk are stored
/// in MiB, so the caller divides by 1024 and rounds up to the next whole
/// GiB for each resource (a 512 MiB server still pays for 1 GiB).
#[derive(Clone, ToSchema, Serialize, Deserialize)]
pub struct AppSettingsBilling {
    /// Master switch. When false, no hourly charging runs, no checkout
    /// sessions are created, and the client billing pages show billing as
    /// disabled. Existing balances/transactions are kept.
    pub enabled: bool,
    /// ISO 4217 currency code, e.g. `USD`. Used for display and for the
    /// Stripe checkout session currency.
    pub currency: compact_str::CompactString,
    /// Credits charged per GiB of RAM per hour.
    pub price_per_gb_ram_per_hour_credits: i64,
    /// Credits charged per 100 CPU units per hour.
    pub price_per_100_cpu_per_hour_credits: i64,
    /// Credits charged per GiB of disk per hour.
    pub price_per_gb_disk_per_hour_credits: i64,
    /// When true, the hourly task suspends a server (via the existing
    /// `servers.suspended` flag) once its owner's balance goes below zero.
    pub suspend_on_negative_balance: bool,
    /// Smallest purchase, in credits (= minor currency units). Stripe
    /// rejects very small charges; 50 keeps test checkouts valid.
    pub minimum_purchase_credits: i64,
    /// Largest single purchase, in credits. Caps accidental / abusive
    /// checkouts; an admin can raise it.
    pub maximum_purchase_credits: i64,
    /// Stripe secret key (`sk_test_…`), encrypted at rest. `None` means
    /// no key is stored in settings (an environment variable may still
    /// supply one — see `backend::billing::StripeConfig::resolve`).
    pub stripe_secret_key: Option<compact_str::CompactString>,
    /// Stripe webhook signing secret (`whsec_…`), encrypted at rest.
    /// `None` means no secret is stored in settings; without a webhook
    /// secret (from settings or the environment) the webhook route
    /// rejects every request.
    pub stripe_webhook_secret: Option<compact_str::CompactString>,
    /// Stripe publishable key (`pk_test_…`). Public by design — the
    /// client billing route sends it to the browser — so it is stored
    /// in plaintext, not encrypted.
    pub stripe_publishable_key: Option<compact_str::CompactString>,
    /// Checkout URL template. Supports `{session_id}`, `{credits}` and
    /// `{app_url}`; see `DEFAULT_CHECKOUT_URL_TEMPLATE` for the default.
    pub checkout_url_template: compact_str::CompactString,
}

impl AppSettingsBilling {
    /// Hourly cost of a server in credits, from its allocated resources.
    /// `memory_mib` / `disk_mib` are MiB, `cpu` is in panel CPU units.
    pub fn hourly_cost_credits(&self, memory_mib: i64, cpu: i32, disk_mib: i64) -> i64 {
        if !self.enabled {
            return 0;
        }

        // Round each resource up to the next whole unit: a server never
        // pays 0 for a resource it actually holds.
        // `i64::div_ceil` is still unstable; do the ceiling division
        // by hand (inputs are resource sizes, far from i64::MAX).
        let ram_gb = ((memory_mib + 1023) / 1024).max(0);
        let cpu_units = ((i64::from(cpu) + 99) / 100).max(0);
        let disk_gb = ((disk_mib + 1023) / 1024).max(0);

        ram_gb
            .saturating_mul(self.price_per_gb_ram_per_hour_credits)
            .saturating_add(cpu_units.saturating_mul(self.price_per_100_cpu_per_hour_credits))
            .saturating_add(disk_gb.saturating_mul(self.price_per_gb_disk_per_hour_credits))
    }

    /// Human-readable price, e.g. `12 credits/hour` for a server.
    pub fn hourly_cost_label(&self, memory_mib: i64, cpu: i32, disk_mib: i64) -> String {
        format!(
            "{} credits/hour",
            self.hourly_cost_credits(memory_mib, cpu, disk_mib)
        )
    }

    /// Default checkout URL template: sends the user back to their
    /// billing page with the checkout session id attached.
    pub const DEFAULT_CHECKOUT_URL_TEMPLATE: &str =
        "{app_url}/account/billing?checkout_session={session_id}";

    /// Mask a secret for display to an admin, e.g. `sk_test_…1234`:
    /// the first 8 characters, an ellipsis, and the last 4. Values of
    /// 12 characters or fewer show only an ellipsis and the last 4
    /// (or nothing at all for very short values). The masked form is
    /// what `GET /api/admin/billing/stripe` returns, and sending it
    /// back is treated as "leave unchanged" — see the `PUT /stripe`
    /// route. Only ever call this on a secret, never on a value that
    /// is about to be stored.
    pub fn mask_secret(value: &str) -> String {
        let chars: Vec<char> = value.chars().collect();
        if chars.len() <= 4 {
            return "••••".to_string();
        }
        if chars.len() <= 12 {
            return format!("…{}", chars[chars.len() - 4..].iter().collect::<String>());
        }
        format!(
            "{}…{}",
            chars[..8].iter().collect::<String>(),
            chars[chars.len() - 4..].iter().collect::<String>()
        )
    }

    /// True when `value` looks like a masked secret as produced by
    /// [`Self::mask_secret`] — i.e. the admin UI sending back exactly
    /// what it was shown. Stripe keys are ASCII, so a real key can
    /// never contain the ellipsis this checks for.
    pub fn is_masked_secret(value: &str) -> bool {
        value.contains('…')
    }

    /// Masked form of the stored secret key, if one is stored.
    pub fn stripe_secret_key_masked(&self) -> Option<String> {
        self.stripe_secret_key.as_deref().map(Self::mask_secret)
    }

    /// Masked form of the stored webhook secret, if one is stored.
    pub fn stripe_webhook_secret_masked(&self) -> Option<String> {
        self.stripe_webhook_secret.as_deref().map(Self::mask_secret)
    }
}

impl Censor for AppSettingsBilling {
    /// Blank the two Stripe secrets. The publishable key is public by
    /// design and the checkout URL template is not secret, so neither
    /// is censored.
    fn censor(&mut self) {
        if let Some(secret_key) = &mut self.stripe_secret_key {
            *secret_key = CENSORED_PLACEHOLDER.into();
        }
        if let Some(webhook_secret) = &mut self.stripe_webhook_secret {
            *webhook_secret = CENSORED_PLACEHOLDER.into();
        }
    }
}

#[async_trait::async_trait]
impl SettingsSerializeExt for AppSettingsBilling {
    async fn serialize(
        &self,
        serializer: SettingsSerializer,
    ) -> Result<SettingsSerializer, anyhow::Error> {
        let database = serializer.database.clone();

        Ok(serializer
            .write_raw_setting("billing_enabled", self.enabled.to_compact_string())
            .write_raw_setting("billing_currency", &*self.currency)
            .write_raw_setting(
                "billing_price_per_gb_ram_per_hour_credits",
                self.price_per_gb_ram_per_hour_credits.to_compact_string(),
            )
            .write_raw_setting(
                "billing_price_per_100_cpu_per_hour_credits",
                self.price_per_100_cpu_per_hour_credits
                    .to_compact_string(),
            )
            .write_raw_setting(
                "billing_price_per_gb_disk_per_hour_credits",
                self.price_per_gb_disk_per_hour_credits.to_compact_string(),
            )
            .write_raw_setting(
                "billing_suspend_on_negative_balance",
                self.suspend_on_negative_balance.to_compact_string(),
            )
            .write_raw_setting(
                "billing_minimum_purchase_credits",
                self.minimum_purchase_credits.to_compact_string(),
            )
            .write_raw_setting(
                "billing_maximum_purchase_credits",
                self.maximum_purchase_credits.to_compact_string(),
            )
            .write_raw_setting(
                "billing_stripe_secret_key",
                if let Some(secret_key) = &self.stripe_secret_key {
                    base32::encode(
                        base32::Alphabet::Z,
                        &database.encrypt(secret_key.clone()).await?,
                    )
                } else {
                    "".into()
                },
            )
            .write_raw_setting(
                "billing_stripe_webhook_secret",
                if let Some(webhook_secret) = &self.stripe_webhook_secret {
                    base32::encode(
                        base32::Alphabet::Z,
                        &database.encrypt(webhook_secret.clone()).await?,
                    )
                } else {
                    "".into()
                },
            )
            .write_raw_setting(
                "billing_stripe_publishable_key",
                self.stripe_publishable_key.clone().unwrap_or_default(),
            )
            .write_raw_setting(
                "billing_checkout_url_template",
                &*self.checkout_url_template,
            ))
    }
}

impl AppSettingsBilling {
    #[cfg(test)]
    pub fn mock() -> Self {
        Self {
            enabled: false,
            currency: "USD".into(),
            price_per_gb_ram_per_hour_credits: 5,
            price_per_100_cpu_per_hour_credits: 2,
            price_per_gb_disk_per_hour_credits: 1,
            suspend_on_negative_balance: true,
            minimum_purchase_credits: 50,
            maximum_purchase_credits: 100_000,
            stripe_secret_key: None,
            stripe_webhook_secret: None,
            stripe_publishable_key: None,
            checkout_url_template: Self::DEFAULT_CHECKOUT_URL_TEMPLATE.into(),
        }
    }
}

pub struct AppSettingsBillingDeserializer;

#[async_trait::async_trait]
impl SettingsDeserializeExt for AppSettingsBillingDeserializer {
    async fn deserialize_boxed(
        &self,
        mut deserializer: SettingsDeserializer<'_>,
    ) -> Result<ExtensionSettings, anyhow::Error> {
        Ok(Box::new(AppSettingsBilling {
            enabled: deserializer
                .take_raw_setting("billing_enabled")
                .map(|s| s == "true")
                .unwrap_or(false),
            currency: deserializer
                .take_raw_setting("billing_currency")
                .filter(|s| s.len() == 3 && s.chars().all(|c| c.is_ascii_alphabetic()))
                .map(|s| s.to_ascii_uppercase().into())
                .unwrap_or_else(|| "USD".into()),
            price_per_gb_ram_per_hour_credits: deserializer
                .take_raw_setting("billing_price_per_gb_ram_per_hour_credits")
                .and_then(|s| s.parse().ok())
                .unwrap_or(5),
            price_per_100_cpu_per_hour_credits: deserializer
                .take_raw_setting("billing_price_per_100_cpu_per_hour_credits")
                .and_then(|s| s.parse().ok())
                .unwrap_or(2),
            price_per_gb_disk_per_hour_credits: deserializer
                .take_raw_setting("billing_price_per_gb_disk_per_hour_credits")
                .and_then(|s| s.parse().ok())
                .unwrap_or(1),
            suspend_on_negative_balance: deserializer
                .take_raw_setting("billing_suspend_on_negative_balance")
                .map(|s| s == "true")
                .unwrap_or(true),
            minimum_purchase_credits: deserializer
                .take_raw_setting("billing_minimum_purchase_credits")
                .and_then(|s| s.parse().ok())
                .unwrap_or(50),
            maximum_purchase_credits: deserializer
                .take_raw_setting("billing_maximum_purchase_credits")
                .and_then(|s| s.parse().ok())
                .unwrap_or(100_000),
            stripe_secret_key: if let Some(secret_key) = deserializer
                .take_raw_setting("billing_stripe_secret_key")
                .and_then(|s| s.into_optional())
            {
                base32::decode(base32::Alphabet::Z, &secret_key)
                    .map(|encrypted| deserializer.database.decrypt(encrypted))
                    .awaited()
                    .await
                    .transpose()?
            } else {
                None
            },
            stripe_webhook_secret: if let Some(webhook_secret) = deserializer
                .take_raw_setting("billing_stripe_webhook_secret")
                .and_then(|s| s.into_optional())
            {
                base32::decode(base32::Alphabet::Z, &webhook_secret)
                    .map(|encrypted| deserializer.database.decrypt(encrypted))
                    .awaited()
                    .await
                    .transpose()?
            } else {
                None
            },
            stripe_publishable_key: deserializer
                .take_raw_setting("billing_stripe_publishable_key")
                .and_then(|s| s.into_optional()),
            checkout_url_template: deserializer
                .take_raw_setting("billing_checkout_url_template")
                .and_then(|s| s.into_optional())
                .unwrap_or_else(|| AppSettingsBilling::DEFAULT_CHECKOUT_URL_TEMPLATE.into()),
        }))
    }
}
