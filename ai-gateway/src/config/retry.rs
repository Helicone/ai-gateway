use std::time::Duration;

use backon::{BackoffBuilder, ConstantBuilder, ExponentialBuilder};
use rust_decimal::{Decimal, prelude::ToPrimitive};
use serde::{Deserialize, Serialize};

pub(crate) const DEFAULT_RETRY_FACTOR: f32 = 2.0;

#[derive(Debug, Clone, Deserialize, Eq, PartialEq, Hash, Serialize)]
#[serde(rename_all = "kebab-case", tag = "strategy")]
pub enum RetryConfig {
    Exponential {
        #[serde(
            with = "humantime_serde",
            rename = "min-delay",
            default = "default_min_delay"
        )]
        min_delay: Duration,
        #[serde(
            with = "humantime_serde",
            rename = "max-delay",
            default = "default_max_delay"
        )]
        max_delay: Duration,
        #[serde(rename = "max-retries", default = "default_max_retries")]
        max_retries: u8,
        #[serde(default = "default_factor")]
        factor: Decimal,
    },
    Constant {
        #[serde(with = "humantime_serde", default = "default_min_delay")]
        delay: Duration,
        #[serde(rename = "max-retries", default = "default_max_retries")]
        max_retries: u8,
    },
}

impl RetryConfig {
    #[must_use]
    pub fn as_iterator(
        &self,
    ) -> Box<dyn Iterator<Item = Duration> + Send + Sync> {
        match self {
            Self::Exponential {
                min_delay,
                max_delay,
                max_retries,
                factor,
            } => {
                let backoff = ExponentialBuilder::default()
                    .with_min_delay(*min_delay)
                    .with_max_delay(*max_delay)
                    .with_max_times(usize::from(*max_retries))
                    .with_factor(
                        factor.to_f32().unwrap_or(DEFAULT_RETRY_FACTOR),
                    )
                    .with_jitter()
                    .build();
                Box::new(backoff)
            }
            Self::Constant { delay, max_retries } => {
                let backoff = ConstantBuilder::default()
                    .with_delay(*delay)
                    .with_max_times(usize::from(*max_retries))
                    .build();
                Box::new(backoff)
            }
        }
    }
}

fn default_factor() -> Decimal {
    Decimal::try_from(DEFAULT_RETRY_FACTOR).expect("always valid if tests pass")
}

fn default_max_retries() -> u8 {
    2
}

fn default_min_delay() -> Duration {
    Duration::from_secs(1)
}

fn default_max_delay() -> Duration {
    Duration::from_secs(30)
}

#[cfg(feature = "testing")]
impl crate::tests::TestDefault for RetryConfig {
    fn test_default() -> Self {
        Self::Constant {
            delay: Duration::from_millis(5),
            max_retries: 2,
        }
    }
}
