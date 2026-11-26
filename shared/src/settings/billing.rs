// Billing / credits settings.
//
// Money rule for this module: 1 credit = 1 minor unit of the configured
// currency (1 cent when currency is USD/GBP/EUR). All balances, prices and
// charges are integer credits (`i64`) — never floats — so rounding can only
// happen once, at the Stripe checkout boundary, where the amount in minor
// units is exactly the number of credits bought.
//
// Stripe secrets are NEVER stored in settings or the database. They come
// from the environment only:
//   STRIPE_SECRET_KEY        (test mode only in this first version)
//   STRIPE_WEBHOOK_SECRET    (required for the webhook to accept anything)
//   STRIPE_PUBLISHABLE_KEY   (public key, sent to the browser)
// The publishable key is read from the environment at request time by the
// client billing routes, not persisted here.

use super::{
    ExtensionSettings, SettingsDeserializeExt, SettingsDeserializer, SettingsSerializeExt,
    SettingsSerializer,
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
        let ram_gb = memory_mib.div_ceil(1024).max(0);
        let cpu_units = (i64::from(cpu)).div_ceil(100).max(0);
        let disk_gb = disk_mib.div_ceil(1024).max(0);

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
}

#[async_trait::async_trait]
impl SettingsSerializeExt for AppSettingsBilling {
    async fn serialize(
        &self,
        serializer: SettingsSerializer,
    ) -> Result<SettingsSerializer, anyhow::Error> {
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
        }))
    }
}
