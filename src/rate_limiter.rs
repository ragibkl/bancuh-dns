use std::net::IpAddr;
use std::num::NonZeroU32;

use governor::{DefaultKeyedRateLimiter, Quota, RateLimiter as GovRateLimiter};

pub type RateLimiter = DefaultKeyedRateLimiter<IpAddr>;

pub fn new_rate_limiter(requests_per_second: u32) -> Option<RateLimiter> {
    let rps = NonZeroU32::new(requests_per_second)?;
    let quota = Quota::per_second(rps);
    Some(GovRateLimiter::keyed(quota))
}
