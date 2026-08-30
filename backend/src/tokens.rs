use rand::{rngs::OsRng, RngCore};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

pub const JOB_ID_BYTES: usize = 16;
pub const OWNER_TOKEN_BYTES: usize = 32;

pub fn new_job_id() -> String {
    random_hex(JOB_ID_BYTES)
}

pub fn new_owner_token() -> String {
    random_hex(OWNER_TOKEN_BYTES)
}

pub fn token_hash(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}

pub fn token_matches(token: &str, expected_hash: &str) -> bool {
    let actual = Sha256::digest(token.as_bytes());
    let Ok(expected) = hex::decode(expected_hash) else {
        return false;
    };
    actual.as_slice().ct_eq(expected.as_slice()).into()
}

pub fn valid_job_id(value: &str) -> bool {
    value.len() == JOB_ID_BYTES * 2 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn random_hex(bytes: usize) -> String {
    let mut value = vec![0_u8; bytes];
    OsRng.fill_bytes(&mut value);
    hex::encode(value)
}

#[cfg(test)]
mod tests {
    use super::{new_job_id, new_owner_token, token_hash, token_matches, valid_job_id};

    #[test]
    fn creates_independent_high_entropy_tokens() {
        let first = new_owner_token();
        let second = new_owner_token();
        assert_eq!(first.len(), 64);
        assert_ne!(first, second);
        assert!(token_matches(&first, &token_hash(&first)));
        assert!(!token_matches(&second, &token_hash(&first)));
    }

    #[test]
    fn accepts_only_fixed_hex_job_ids() {
        assert!(valid_job_id(&new_job_id()));
        assert!(!valid_job_id("../another-job"));
        assert!(!valid_job_id("abcd"));
    }
}
