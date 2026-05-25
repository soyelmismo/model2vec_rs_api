/// Application error types are now handled inline in each handler,
/// returning `server::Response` directly without needing a trait impl.
/// This module is kept for shared error formatting helpers.

pub fn json_error(status: u16, message: &str) -> Vec<u8> {
    format!(
        r#"{{"error":{{"message":"{message}","type":"api_error","code":{status}}}}}"#
    )
    .into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_401() {
        let body = json_error(401, "unauthorized");
        assert_eq!(
            std::str::from_utf8(&body).unwrap(),
            r#"{"error":{"message":"unauthorized","type":"api_error","code":401}}"#
        );
    }

    #[test]
    fn format_404() {
        let body = json_error(404, "not found");
        assert_eq!(
            std::str::from_utf8(&body).unwrap(),
            r#"{"error":{"message":"not found","type":"api_error","code":404}}"#
        );
    }

    #[test]
    fn format_500() {
        let body = json_error(500, "server error");
        assert_eq!(
            std::str::from_utf8(&body).unwrap(),
            r#"{"error":{"message":"server error","type":"api_error","code":500}}"#
        );
    }

    #[test]
    fn message_contains_special_chars() {
        let body = json_error(400, "invalid 'input'");
        assert!(std::str::from_utf8(&body).unwrap().contains("invalid 'input'"));
    }
}
