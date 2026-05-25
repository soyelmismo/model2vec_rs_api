use crate::server::Response;

pub fn handle() -> Response {
    Response::json(200, br#"{"status":"ok"}"#.to_vec())
}
