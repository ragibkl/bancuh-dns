use lazy_static::lazy_static;
use regex::Regex;

use super::Domain;

#[derive(Debug, PartialEq, Eq)]
pub struct CName {
    pub domain: Domain,
    pub alias: Domain,
}

fn parse_cname(input: &str) -> Option<CName> {
    lazy_static! {
        static ref RE: Regex = Regex::new(
            r"(?P<domain>.{2,200}\.[a-z]{2,6})\s+(CNAME|cname)\s+(?P<alias>.{2,200}\.[a-z]{2,6})\."
        )
        .unwrap();
    }

    let captures = match RE.captures(input) {
        Some(c) => c,
        None => return None,
    };

    match (captures.name("domain"), captures.name("alias")) {
        (Some(domain), Some(alias)) => {
            let domain = domain.as_str().trim().to_string();
            let alias = alias.as_str().trim().to_string();

            let domain = Domain::parse(&domain);
            let alias = Domain::parse(&alias);

            match (domain, alias) {
                (Some(domain), Some(alias)) => Some(CName { domain, alias }),
                _ => None,
            }
        }
        _ => None,
    }
}

impl CName {
    pub fn parse(input: &str) -> Option<Self> {
        parse_cname(input)
    }

    pub fn into_domain(self) -> Domain {
        self.domain
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_parse_cname_some(input: &str, expected_domain: &str, expected_alias: &str) {
        let domain = Domain(expected_domain.to_string());
        let alias = Domain(expected_alias.to_string());
        let cname = CName { domain, alias };

        let output = CName::parse(input);
        assert_eq!(output, Some(cname));
    }

    #[test]
    fn it_extract_domain() {
        test_parse_cname_some(
            "www.bing.com    CNAME   strict.bing.com.",
            "www.bing.com",
            "strict.bing.com",
        );
        test_parse_cname_some(
            "www.google.com.my    CNAME   forcesafesearch.google.com.",
            "www.google.com.my",
            "forcesafesearch.google.com",
        );
        // todo: idna domain
    }
}
