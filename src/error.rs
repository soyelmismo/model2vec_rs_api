use serde::Serialize;

#[derive(Serialize)]
struct ApiError {
    error: ErrorBody,
}

#[derive(Serialize)]
struct ErrorBody {
    message: String,
    r#type: &'static str,
    code: u16,
}

pub fn json_error(status: u16, message: &str) -> Vec<u8> {
    serde_json::to_vec(&ApiError {
        error: ErrorBody {
            message: message.to_owned(),
            r#type: "api_error",
            code: status,
        },
    })
    .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_401() {
        let body = json_error(401, "unauthorized");
        assert_eq!(
            std::str::from_utf8(&body).unwrap(),
            r#"{"error":{"message":"unauthorized","type":"api_error","code":401}}"#,
        );
    }

    #[test]
    fn format_404() {
        let body = json_error(404, "not found");
        assert_eq!(
            std::str::from_utf8(&body).unwrap(),
            r#"{"error":{"message":"not found","type":"api_error","code":404}}"#,
        );
    }

    #[test]
    fn format_500() {
        let body = json_error(500, "server error");
        assert_eq!(
            std::str::from_utf8(&body).unwrap(),
            r#"{"error":{"message":"server error","type":"api_error","code":500}}"#,
        );
    }

    #[test]
    fn message_with_quotes_escaped() {
        let body = json_error(400, r#"invalid "input""#);
        let s = std::str::from_utf8(&body).unwrap();
        assert!(s.contains(r#"\"input\""#));
    }

    #[test]
    fn message_with_backslash_escaped() {
        let body = json_error(400, r"path\name");
        let s = std::str::from_utf8(&body).unwrap();
        assert!(s.contains(r#"path\\name"#));
    }
}
