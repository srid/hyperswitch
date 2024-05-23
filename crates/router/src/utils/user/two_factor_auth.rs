use std::sync::Arc;

use common_utils::pii;
use error_stack::ResultExt;
use masking::ExposeInterface;
use redis_interface::RedisConnectionPool;
use totp_rs::{Algorithm, TOTP};

use crate::{
    consts,
    core::errors::{UserErrors, UserResult},
    routes::SessionState,
};

pub fn generate_default_totp(
    email: pii::Email,
    secret: Option<masking::Secret<String>>,
) -> UserResult<TOTP> {
    let secret = secret
        .map(|sec| totp_rs::Secret::Encoded(sec.expose()))
        .unwrap_or_else(totp_rs::Secret::generate_secret)
        .to_bytes()
        .change_context(UserErrors::InternalServerError)?;

    TOTP::new(
        Algorithm::SHA1,
        consts::user::TOTP_DIGITS,
        consts::user::TOTP_TOLERANCE,
        consts::user::TOTP_VALIDITY_DURATION_IN_SECONDS,
        secret,
        Some(consts::user::TOTP_ISSUER_NAME.to_string()),
        email.expose().expose(),
    )
    .change_context(UserErrors::InternalServerError)
}

pub async fn check_totp_in_redis(state: &SessionState, user_id: &str) -> UserResult<bool> {
    let redis_conn = get_redis_connection(state)?;
    let key = format!("{}{}", consts::user::TOTP_PREFIX, user_id);
    redis_conn
        .exists::<()>(&key)
        .await
        .change_context(UserErrors::InternalServerError)
}

fn get_redis_connection(state: &SessionState) -> UserResult<Arc<RedisConnectionPool>> {
    state
        .store
        .get_redis_conn()
        .change_context(UserErrors::InternalServerError)
        .attach_printable("Failed to get redis connection")
}
