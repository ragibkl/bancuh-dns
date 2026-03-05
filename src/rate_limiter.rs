use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::num::NonZeroU32;

use governor::{DefaultKeyedRateLimiter, Quota, RateLimiter as GovRateLimiter};

pub type RateLimiter = DefaultKeyedRateLimiter<IpAddr>;

/// Mask an IP address to the given prefix length for rate-limiting grouping.
/// e.g. mask_ip(192.168.1.100, 24, 48) => 192.168.1.0
/// e.g. mask_ip(2001:db8:1:2:3:4:5:6, 32, 48) => 2001:db8:1:2::
pub fn mask_ip(ip: IpAddr, ipv4_prefix: u8, ipv6_prefix: u8) -> IpAddr {
    match ip {
        IpAddr::V4(v4) => {
            if ipv4_prefix >= 32 {
                return ip;
            }
            let bits = u32::from(v4);
            let mask = u32::MAX << (32 - ipv4_prefix);
            IpAddr::V4(Ipv4Addr::from(bits & mask))
        }
        IpAddr::V6(v6) => {
            if ipv6_prefix >= 128 {
                return ip;
            }
            let bits = u128::from(v6);
            let mask = u128::MAX << (128 - ipv6_prefix);
            IpAddr::V6(Ipv6Addr::from(bits & mask))
        }
    }
}

pub fn new_rate_limiter(requests_per_second: u32) -> Option<RateLimiter> {
    let rps = NonZeroU32::new(requests_per_second)?;
    let quota = Quota::per_second(rps);
    Some(GovRateLimiter::keyed(quota))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mask_ipv4() {
        let ip: IpAddr = "192.168.1.100".parse().unwrap();
        assert_eq!(mask_ip(ip, 32, 48), "192.168.1.100".parse::<IpAddr>().unwrap());
        assert_eq!(mask_ip(ip, 24, 48), "192.168.1.0".parse::<IpAddr>().unwrap());
        assert_eq!(mask_ip(ip, 16, 48), "192.168.0.0".parse::<IpAddr>().unwrap());
    }

    #[test]
    fn test_mask_ipv6() {
        let ip: IpAddr = "2001:db8:1:2:3:4:5:6".parse().unwrap();
        assert_eq!(mask_ip(ip, 32, 128), "2001:db8:1:2:3:4:5:6".parse::<IpAddr>().unwrap());
        assert_eq!(mask_ip(ip, 32, 48), "2001:db8:1::".parse::<IpAddr>().unwrap());
        assert_eq!(mask_ip(ip, 32, 64), "2001:db8:1:2::".parse::<IpAddr>().unwrap());
    }
}
