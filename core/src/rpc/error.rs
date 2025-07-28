use warp::{Reply, Rejection, http::StatusCode};
use super::types::ApiResponse;

#[derive(Debug)]
pub struct RpcError(pub String);

impl warp::reject::Reject for RpcError {}

/// Global error handler for rejections
pub async fn handle_rejection(err: Rejection) -> std::result::Result<impl Reply, std::convert::Infallible> {
    let (code, message) = if err.is_not_found() {
        (StatusCode::NOT_FOUND, "Endpoint not found".to_string())
    } else if let Some(rpc_error) = err.find::<RpcError>() {
        match rpc_error.0.as_str() {
            "Rate limit exceeded" | "IP temporarily blocked" => (StatusCode::TOO_MANY_REQUESTS, rpc_error.0.clone()),
            "Missing or invalid authorization header" | "Invalid JWT token" => (StatusCode::UNAUTHORIZED, rpc_error.0.clone()),
            "Insufficient permissions" => (StatusCode::FORBIDDEN, rpc_error.0.clone()),
            _ => (StatusCode::BAD_REQUEST, rpc_error.0.clone()),
        }
    } else if err.find::<warp::reject::PayloadTooLarge>().is_some() {
        (StatusCode::PAYLOAD_TOO_LARGE, "Request body too large".to_string())
    } else if err.find::<warp::reject::InvalidHeader>().is_some() {
        (StatusCode::BAD_REQUEST, "Invalid headers".to_string())  
    } else if let Some(e) = err.find::<warp::body::BodyDeserializeError>() {
        (StatusCode::BAD_REQUEST, format!("Invalid request body: {e}"))
    } else {
        log::error!("Unhandled rejection: {err:?}");
        (StatusCode::INTERNAL_SERVER_ERROR, "Internal server error".to_string())
    };
    
    let response = ApiResponse::<()>::error(message);
    Ok(warp::reply::with_status(
        warp::reply::json(&response),
        code,
    ))
} 