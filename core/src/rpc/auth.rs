use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Validation, Header};
use serde::{Deserialize, Serialize};
use warp::{Filter, Rejection};

use super::types::{AuthConfig, AccessLevel};
use super::error::RpcError;

#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String,
    pub exp: usize,
    pub role: String,
}

pub struct AuthManager {
    config: AuthConfig,
}

impl AuthManager {
    pub fn new(config: AuthConfig) -> Self {
        Self { config }
    }
    
    pub fn create_jwt(&self, role: &str) -> Result<String, String> {
        let expiration = chrono::Utc::now()
            .checked_add_signed(chrono::Duration::seconds(self.config.token_expiry.as_secs() as i64))
            .expect("valid timestamp")
            .timestamp();

        let claims = Claims {
            sub: role.to_owned(),
            role: role.to_owned(),
            exp: expiration as usize,
        };

        encode(
            &Header::default(),
            &claims,
            &EncodingKey::from_secret(self.config.jwt_secret.as_bytes()),
        )
        .map_err(|_| "Failed to create token".to_string())
    }
    
    pub fn verify_api_key(&self, api_key: &str) -> bool {
        api_key == self.config.admin_api_key
    }
    
    /// Create authentication filter for a specific access level
    pub fn auth_filter(
        &self,
        required_level: AccessLevel,
    ) -> impl Filter<Extract = (), Error = Rejection> + Clone {
        let auth_config = self.config.clone();
        warp::header::optional::<String>("authorization")
            .and_then(move |auth_header: Option<String>| {
                let auth_config = auth_config.clone();
                async move {
                    if !auth_config.require_auth {
                        return Ok(());
                    }
                    
                    let token_str = auth_header
                        .and_then(|h| h.strip_prefix("Bearer ").map(str::to_string))
                        .ok_or_else(|| warp::reject::custom(RpcError("Missing or invalid authorization header".to_string())))?;
                    
                    let token_data = decode::<Claims>(
                        &token_str,
                        &DecodingKey::from_secret(auth_config.jwt_secret.as_bytes()),
                        &Validation::default(),
                    )
                    .map_err(|_| warp::reject::custom(RpcError("Invalid JWT token".to_string())))?;
                    
                    if required_level == AccessLevel::Admin && token_data.claims.role != "admin" {
                        return Err(warp::reject::custom(RpcError("Insufficient permissions".to_string())));
                    }
                    Ok(())
                }
            })
            .untuple_one()
    }
} 