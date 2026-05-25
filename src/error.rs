/// Application error types are now handled inline in each handler,
/// returning `server::Response` directly without needing a trait impl.
/// This module is kept for shared error formatting helpers.

pub fn json_error(status: u16, message: &str) -> Vec<u8> {
    format!(
        r#"{{"error":{{"message":"{message}","type":"api_error","code":{status}}}}}"#
    )
    .into_bytes()
}
