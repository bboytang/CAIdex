use crate::{Error, Result};
use std::time::Duration;
use tokio::{sync::Semaphore, time::Instant};

#[derive(Clone, Debug)]
pub struct Limits {
    pub request_bytes: usize,
    pub frame_bytes: usize,
    pub response_bytes: usize,
    pub in_flight: usize,
    pub connect_timeout: Duration,
    pub header_timeout: Duration,
    pub idle_timeout: Duration,
    pub total_timeout: Duration,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            request_bytes: 8 * 1024 * 1024,
            frame_bytes: 2 * 1024 * 1024,
            response_bytes: 16 * 1024 * 1024,
            in_flight: 16,
            connect_timeout: Duration::from_secs(10),
            header_timeout: Duration::from_secs(30),
            idle_timeout: Duration::from_secs(90),
            total_timeout: Duration::from_secs(600),
        }
    }
}
impl Limits {
    pub fn validate(&self) -> Result<()> {
        if self.request_bytes == 0
            || self.frame_bytes == 0
            || self.response_bytes == 0
            || self.in_flight == 0
            || self.in_flight > Semaphore::MAX_PERMITS
            || [
                self.connect_timeout,
                self.header_timeout,
                self.idle_timeout,
                self.total_timeout,
            ]
            .iter()
            .any(|duration| duration.is_zero() || Instant::now().checked_add(*duration).is_none())
        {
            return Err(Error::InvalidLimits);
        }
        Ok(())
    }
}
