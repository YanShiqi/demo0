use rand_core::{OsRng, RngCore};
use sha2::{Digest, Sha256};

use crate::error::AppError;

const TOKEN_PREFIX: &str = "ZV1";
const RANDOM_BYTES: usize = 20;
const PAYLOAD_CHARACTERS: usize = 32;
const GROUP_CHARACTERS: usize = 4;
const CROCKFORD: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IssuedToken {
    pub plaintext: String,
    pub hash: String,
    pub mask: String,
}

/// 生成一个 160 位随机兑换码，并仅返回其可安全持久化的散列值与展示掩码。
pub fn issue() -> Result<IssuedToken, AppError> {
    let mut random = [0_u8; RANDOM_BYTES];
    OsRng
        .try_fill_bytes(&mut random)
        .map_err(|_| AppError::Internal("无法生成安全兑换码".to_owned()))?;

    let payload = encode_crockford(random);
    let plaintext = format_token(&payload);
    let normalized = normalize(&plaintext)?;
    let hash = hash_normalized(&normalized);
    let last_group = &payload[PAYLOAD_CHARACTERS - GROUP_CHARACTERS..];
    let mask = format!("{TOKEN_PREFIX}-****-****-****-****-****-****-****-{last_group}");

    Ok(IssuedToken {
        plaintext,
        hash,
        mask,
    })
}

/// 将用户输入转为唯一的紧凑大写表示，供比较和散列使用。
pub fn normalize(input: &str) -> Result<String, AppError> {
    let mut normalized = String::with_capacity(TOKEN_PREFIX.len() + PAYLOAD_CHARACTERS);

    for character in input.chars() {
        if character.is_ascii_whitespace() || character == '-' {
            continue;
        }

        if !character.is_ascii() {
            return Err(invalid_token_error());
        }
        normalized.push(character.to_ascii_uppercase());
    }

    // 在任何错误路径都不回显输入，防止兑换码意外进入日志或响应内容。
    if normalized.len() != TOKEN_PREFIX.len() + PAYLOAD_CHARACTERS
        || !normalized.starts_with(TOKEN_PREFIX)
        || !normalized[TOKEN_PREFIX.len()..]
            .bytes()
            .all(|character| CROCKFORD.contains(&character))
    {
        return Err(invalid_token_error());
    }

    Ok(normalized)
}

/// 对规范化兑换码计算 SHA-256 十六进制摘要，避免持久化明文。
pub fn hash_normalized(normalized: &str) -> String {
    hex::encode(Sha256::digest(normalized.as_bytes()))
}

fn encode_crockford(random: [u8; RANDOM_BYTES]) -> String {
    let mut payload = String::with_capacity(PAYLOAD_CHARACTERS);

    for chunk in random.chunks_exact(5) {
        let value = u64::from_be_bytes([0, 0, 0, chunk[0], chunk[1], chunk[2], chunk[3], chunk[4]]);
        for shift in (0..40).step_by(5).rev() {
            payload.push(CROCKFORD[((value >> shift) & 0b1_1111) as usize] as char);
        }
    }

    payload
}

fn format_token(payload: &str) -> String {
    let groups = payload
        .as_bytes()
        .chunks_exact(GROUP_CHARACTERS)
        .map(std::str::from_utf8)
        .collect::<Result<Vec<_>, _>>()
        .expect("Crockford Base32 字母表只包含 ASCII 字符");

    format!("{TOKEN_PREFIX}-{}", groups.join("-"))
}

fn invalid_token_error() -> AppError {
    AppError::BadRequest("兑换码格式无效".to_owned())
}

#[cfg(test)]
mod tests {
    use super::{hash_normalized, issue, normalize};

    #[test]
    fn issued_token_has_160_bits_and_round_trips_through_normalization() {
        let issued = issue().unwrap();

        assert_eq!(issued.plaintext.split('-').count(), 9);
        assert!(issued.plaintext.starts_with("ZV1-"));
        assert_eq!(
            normalize(&issued.plaintext.to_lowercase()).unwrap().len(),
            35
        );
        assert_eq!(
            hash_normalized(&normalize(&issued.plaintext).unwrap()),
            issued.hash
        );
        assert!(issued.mask.starts_with("ZV1-****-"));
        assert!(
            issued
                .mask
                .ends_with(issued.plaintext.rsplit('-').next().unwrap())
        );
    }

    #[test]
    fn normalization_accepts_spaces_and_hyphens_but_rejects_ambiguous_characters() {
        let issued = issue().unwrap();
        let spaced = issued.plaintext.replace('-', " ");

        assert_eq!(
            normalize(&spaced).unwrap(),
            normalize(&issued.plaintext).unwrap()
        );
        assert!(normalize("ZV1-OOOO-OOOO-OOOO-OOOO-OOOO-OOOO-OOOO-OOOO").is_err());
    }

    #[test]
    fn normalization_errors_do_not_echo_submitted_token() {
        let submitted = "ZV1-INVALID-SECRET";
        let error = normalize(submitted).unwrap_err();

        assert!(!error.to_string().contains(submitted));
    }
}
