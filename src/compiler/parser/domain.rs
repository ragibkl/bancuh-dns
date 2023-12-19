use addr::parse_domain_name;
use lazy_static::lazy_static;
use regex::Regex;
use thiserror::Error;

#[derive(Debug, Eq, Hash, PartialEq)]
pub struct Domain(pub String);

fn parse_domain(value: &str) -> Option<Domain> {
    lazy_static! {
        static ref RE: Regex = Regex::new(r"(?P<domain>.{2,200}\.[a-z]{2,6})").unwrap();
    }

    RE.captures(value)
        .and_then(|cap| cap.name("domain"))
        .and_then(|d| {
            if d.as_str().starts_with("*.") {
                let as_str = d.as_str().replace("*.", "");
                parse_domain_name(&as_str)
                    .ok()
                    .map(|v| "*.".to_string() + v.as_str())
            } else {
                parse_domain_name(d.as_str())
                    .ok()
                    .map(|v| v.as_str().trim().to_string())
            }
        })
        .map(|d| d.as_str().trim().to_string())
        .and_then(|d| idna::domain_to_ascii(&d).ok())
        .map(Domain)
}

impl Domain {
    pub fn parse(value: &str) -> Option<Domain> {
        parse_domain(value)
    }
}

#[derive(Error, Debug)]
#[error("ParseDomainError")]
pub struct ParseDomainError;

impl TryFrom<&str> for Domain {
    type Error = ParseDomainError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match parse_domain(value) {
            Some(d) => Ok(d),
            None => Err(ParseDomainError),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_parse_domain(input: &str, expected: &str) {
        let domain = Domain(expected.to_string());
        let output = Domain::parse(input);
        assert_eq!(output, Some(domain));
    }

    fn test_parse_domain_none(input: &str) {
        let output = Domain::parse(input);
        assert_eq!(output, None);
    }

    #[test]
    fn it_extract_domain() {
        test_parse_domain("abc.example.com", "abc.example.com");
        test_parse_domain("Bücher.example.com", "xn--bcher-kva.example.com");
        test_parse_domain_none("");
        test_parse_domain_none("# abc.example.com");
    }
}
