use crate::server::Response;

pub fn handle() -> Response {
    Response::json(200, br#"{"status":"ok"}"#.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn health_returns_ok() {
        let resp = handle();
        assert_eq!(resp.status, 200);
        assert_eq!(resp.content_type, "application/json");
        assert_eq!(resp.body, br#"{"status":"ok"}"#);
    }
}
