use std::sync::OnceLock;

use argon2::password_hash::{PasswordHasher, PasswordVerifier};
use argon2::Argon2;

use crate::error::AppError;

pub const MIN_PASSWORD_CHARS: usize = 8;
/// 给 argon2 的输入设上限，免得有人拿超长密码打 CPU。
pub const MAX_PASSWORD_CHARS: usize = 256;
pub const MAX_ACCOUNT_CHARS: usize = 64;

pub fn validate_password(password: &str) -> Result<(), AppError> {
    let chars = password.chars().count();
    if chars < MIN_PASSWORD_CHARS {
        return Err(AppError::invalid(format!(
            "密码至少 {MIN_PASSWORD_CHARS} 个字符"
        )));
    }
    if chars > MAX_PASSWORD_CHARS {
        return Err(AppError::invalid(format!(
            "密码最多 {MAX_PASSWORD_CHARS} 个字符"
        )));
    }
    Ok(())
}

/// 账号名只许字母、数字与 `._@-`，登录与唯一性都不分大小写。
pub fn validate_account_name(account: &str) -> Result<(), AppError> {
    let valid_chars = account
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '@' | '-'));
    if account.is_empty() || account.len() > MAX_ACCOUNT_CHARS || !valid_chars {
        return Err(AppError::invalid(format!(
            "账号名为 1 到 {MAX_ACCOUNT_CHARS} 个字符，只能含字母、数字与 . _ @ -"
        )));
    }
    Ok(())
}

/// 输出 PHC 字符串（`$argon2id$...`），盐与参数都在里面。CPU 密集，异步调用方走 `spawn_blocking`。
pub fn hash(password: &str) -> Result<String, AppError> {
    Argon2::default()
        .hash_password(password.as_bytes())
        .map(|hash| hash.to_string())
        .map_err(AppError::internal)
}

pub fn verify(password: &str, stored_hash: &str) -> bool {
    Argon2::default()
        .verify_password(password.as_bytes(), stored_hash)
        .is_ok()
}

/// 账号不存在时也做一次等价的 argon2 校验，让响应时间不暴露账号是否存在。
pub fn verify_against_dummy(password: &str) {
    static DUMMY: OnceLock<String> = OnceLock::new();
    let dummy = DUMMY.get_or_init(|| hash("dummy-password-for-timing").unwrap_or_default());
    let _ = verify(password, dummy);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_is_phc_argon2id_and_salted() {
        let a = hash("correct horse").unwrap();
        let b = hash("correct horse").unwrap();
        assert!(a.starts_with("$argon2id$"));
        assert_ne!(a, b, "每次的盐应不同");
        assert!(!a.contains("correct horse"));
    }

    #[test]
    fn verify_accepts_only_the_right_password() {
        let stored = hash("correct horse").unwrap();
        assert!(verify("correct horse", &stored));
        assert!(!verify("wrong horse", &stored));
        assert!(!verify("correct horse", "not-a-phc-string"));
    }

    #[test]
    fn password_length_is_bounded() {
        assert!(validate_password("1234567").is_err());
        assert!(validate_password("12345678").is_ok());
        assert!(validate_password(&"a".repeat(MAX_PASSWORD_CHARS)).is_ok());
        assert!(validate_password(&"a".repeat(MAX_PASSWORD_CHARS + 1)).is_err());
        assert!(validate_password("密码密码密码密码").is_ok());
    }

    #[test]
    fn account_names_are_restricted() {
        for ok in ["alice", "Bob.Smith", "a_b-c@d", "x"] {
            assert!(validate_account_name(ok).is_ok(), "{ok}");
        }
        for bad in ["", "a b", "张三", "a/b", &"a".repeat(MAX_ACCOUNT_CHARS + 1)] {
            assert!(validate_account_name(bad).is_err(), "{bad}");
        }
    }
}
