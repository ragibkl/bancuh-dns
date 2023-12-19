use lazy_static::lazy_static;
use regex::Regex;

use super::Domain;

#[derive(Debug, PartialEq, Eq)]
pub struct Host {
    ip: String,
    domain: Domain,
}

pub fn parse_host(value: &str) -> Option<Host> {
    lazy_static! {
        static ref RE: Regex = Regex::new(
            r"(?P<ip>\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3})\s+(?P<domain>.{2,200}\.[a-z]{2,6})"
        )
        .unwrap();
    }

    let captures = match RE.captures(value) {
        Some(c) => c,
        None => return None,
    };

    match (captures.name("ip"), captures.name("domain")) {
        (Some(ip), Some(domain)) => {
            let ip = ip.as_str().trim().to_string();
            let domain = domain.as_str().trim().to_string();

            let domain = match Domain::parse(&domain) {
                Some(domain) => domain,
                None => return None,
            };

            Some(Host { ip, domain })
        }
        _ => None,
    }
}

impl Host {
    pub fn parse(value: &str) -> Option<Self> {
        parse_host(value)
    }

    pub fn into_domain(self) -> Domain {
        self.domain
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_parse_host_some(input: &str, expected_ip: &str, expected_domain: &str) {
        let expected = Some(Host {
            ip: expected_ip.to_string(),
            domain: Domain(expected_domain.to_string()),
        });
        let output = parse_host(input);
        assert_eq!(output, expected);
    }

    #[test]
    fn it_extract_domain() {
        test_parse_host_some("127.0.0.1 abc.example.com", "127.0.0.1", "abc.example.com");
        test_parse_host_some("0.0.0.0 abc.example.com", "0.0.0.0", "abc.example.com");
        test_parse_host_some(
            "127.0.0.1 Bücher.example.com",
            "127.0.0.1",
            "xn--bcher-kva.example.com",
        );
    }
}
