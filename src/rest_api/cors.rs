use rocket::fairing::{Fairing, Info, Kind};
use rocket::http::{Header, ContentType};
use rocket::{Request, Response};
use rocket::http::Method;
use std::io::Cursor;
use std::path::PathBuf;

pub struct CORS();

#[rocket::async_trait]
impl Fairing for CORS {
	fn info(&self) -> Info {
		Info {
			name: "Add CORS headers to requests",
			kind: Kind::Response
		}
	}

	async fn on_response<'r>(&self, request: &'r Request<'_>, response: &mut Response<'r>) {
		response.set_header(Header::new("Access-Control-Allow-Origin", "http://localhost:8080"));
		response.set_header(Header::new("Access-Control-Allow-Methods", "POST, GET, OPTIONS"));
		response.set_header(Header::new("Access-Control-Allow-Headers", "Content-Type"));
		response.set_header(Header::new("Access-Control-Expose-Headers", "Location"));
		response.set_header(Header::new("Access-Control-Allow-Credentials", "true"));

		if request.method() == Method::Options {
			response.set_header(ContentType::Plain);
			response.set_sized_body(0, Cursor::new(""));
		}
	}
}

#[options("/<_path..>")]
pub fn options(_path: PathBuf) {
}

