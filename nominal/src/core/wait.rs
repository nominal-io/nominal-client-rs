use std::time::Duration;

/// Polling policy for image readiness and dataset-file completion.
#[derive(Debug, Clone, Copy)]
pub struct WaitOptions {
    interval: Duration,
    timeout: Option<Duration>,
}

impl Default for WaitOptions {
    fn default() -> Self {
        Self {
            interval: Duration::from_secs(1),
            timeout: None,
        }
    }
}

impl WaitOptions {
    #[must_use]
    pub fn interval(mut self, interval: Duration) -> Self {
        self.interval = interval;
        self
    }
    #[must_use]
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }
    pub fn poll_interval(&self) -> Duration {
        self.interval
    }
    pub fn timeout_duration(&self) -> Option<Duration> {
        self.timeout
    }
    pub(crate) fn validate(&self) -> crate::Result<()> {
        if self.interval.is_zero() || self.timeout.is_some_and(|t| t.is_zero()) {
            return Err(crate::Error::Ingest {
                details: "poll interval and timeout must be positive".into(),
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn rejects_zero_wait_intervals_and_deadlines() {
        assert!(
            WaitOptions::default()
                .interval(Duration::ZERO)
                .validate()
                .is_err()
        );
        assert!(
            WaitOptions::default()
                .timeout(Duration::ZERO)
                .validate()
                .is_err()
        );
        assert!(WaitOptions::default().validate().is_ok());
    }
}
