use std::sync::OnceLock;

use argon2::password_hash::{PasswordHasher, PasswordVerifier};
use argon2::Argon2;

use crate::error::AppError;

pub const MIN_PASSWORD_CHARS: usize = 8;
/// 给 argon2 的输入设上限，免得有人拿超长密码打 CPU。
pub const MAX_PASSWORD_CHARS: usize = 256;

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

/// 输出 PHC 字符串（`$argon2id$...`），盐与参数都在里面。CPU 密集，异步调用方走 `spawn_blocking`。
pub fn hash(password: &str) -> Result<String, AppError> {
    Argon2::default()
        .hash_password(password.as_bytes())
        .map(|hash| hash.to_string())
        .map_err(AppError::internal)
}

/// 异步入口：argon2 吃 CPU，放到阻塞线程池，别卡住 tokio 工作线程。
pub async fn hash_async(password: &str) -> Result<String, AppError> {
    let owned = password.to_owned();
    tokio::task::spawn_blocking(move || hash(&owned))
        .await
        .map_err(AppError::internal)?
}

/// 密码超长直接判不匹配，不进 argon2：登录接口不需要登录就能打到这里。
pub async fn verify_async(password: &str, stored_hash: &str) -> Result<bool, AppError> {
    if password.chars().count() > MAX_PASSWORD_CHARS {
        return Ok(false);
    }
    let (owned, stored) = (password.to_owned(), stored_hash.to_owned());
    tokio::task::spawn_blocking(move || verify(&owned, &stored))
        .await
        .map_err(AppError::internal)
}

/// 账号不存在时也做一次等价的 argon2 校验，让响应时间不暴露账号是否存在。
pub async fn verify_against_dummy_async(password: &str) -> Result<(), AppError> {
    if password.chars().count() > MAX_PASSWORD_CHARS {
        return Ok(());
    }
    let owned = password.to_owned();
    tokio::task::spawn_blocking(move || verify_against_dummy(&owned))
        .await
        .map_err(AppError::internal)
}

pub fn verify(password: &str, stored_hash: &str) -> bool {
    Argon2::default()
        .verify_password(password.as_bytes(), stored_hash)
        .is_ok()
}

fn verify_against_dummy(password: &str) {
    static DUMMY: OnceLock<String> = OnceLock::new();
    // 默认参数下哈希只会因系统随机数失败；宁可让这次请求报 500，也不能退化成空哈希而泄露计时。
    let dummy = DUMMY.get_or_init(|| hash("dummy-password-for-timing").expect("生成对照哈希"));
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
}
