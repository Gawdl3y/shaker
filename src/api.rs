use anyhow::Result;
use axum::{
	async_trait,
	extract::{Form, FromRef, FromRequestParts, Query, State},
	http::{request::Parts, StatusCode},
	routing::{get, post},
	Router,
};
use secrecy::{ExposeSecret, Secret};
use serde::Deserialize;
use tokio::net::TcpListener;
use tracing::warn;

use crate::{db, Config};

/// Runs the API server
#[tracing::instrument("Running API server", level = "info")]
pub async fn run(cfg: Config, db: db::Database) -> Result<()> {
	if cfg.token.is_none() {
		warn!("No token provided in configuration - requests will not be required to provide a token to authenticate");
	}

	let app = Router::new()
		.route("/users/count", get(count_users))
		.route("/users/names", get(list_user_names))
		.route("/handshakes", post(create_handshake))
		.route("/handshakes/count", get(count_handshakes))
		.with_state(AppState { token: cfg.token, db });

	let listener = TcpListener::bind(cfg.api).await?;
	axum::serve(listener, app).await?;

	Ok(())
}

/// State for the API
#[derive(Debug, Clone)]
pub struct AppState {
	/// Token required to authenticate
	token: Option<Secret<String>>,

	/// Database to store/retrieve records
	db: db::Database,
}

impl FromRef<AppState> for db::Database {
	fn from_ref(state: &AppState) -> db::Database {
		state.db.clone()
	}
}

/// Authenticated session for a request
#[derive(Debug, Clone, Deserialize)]
pub struct Session {
	/// Token being used to authenticate
	token: Option<Secret<String>>,
}

#[async_trait]
impl FromRequestParts<AppState> for Session {
	type Rejection = (StatusCode, String);

	async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
		// If we aren't expecting a token, then go ahead and return an empty session
		let Some(expected_token) = &state.token else {
			return Ok(Session { token: None });
		};

		// Parse the session from the query string
		let Query(session): Query<Session> =
			Query::try_from_uri(&parts.uri).map_err(|_| (StatusCode::BAD_REQUEST, "missing token".to_owned()))?;

		// Ensure the given token matches
		match &session.token {
			Some(secret) if secret.expose_secret() == expected_token.expose_secret() => Ok(session),
			Some(_) => Err((StatusCode::UNAUTHORIZED, "invalid token".to_owned())),
			None => Err((StatusCode::BAD_REQUEST, "missing token".to_owned())),
		}
	}
}

/// Returns the number of unique users that have shaken hands
#[tracing::instrument(level = "debug", skip(_session, db))]
async fn count_users(_session: Session, State(db): State<db::Database>) -> Result<String, (StatusCode, String)> {
	let count = db
		.count_users()
		.await
		.map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
	Ok(count.to_string())
}

/// Returns a newline-delimited list of the usernames of all unique users that have shaken hands
#[tracing::instrument(level = "debug", skip(_session, db))]
async fn list_user_names(_session: Session, State(db): State<db::Database>) -> Result<String, (StatusCode, String)> {
	let names = db
		.get_all_user_resonite_names()
		.await
		.map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
	Ok(names.join("\n"))
}

/// Stores record of a new handshake
#[tracing::instrument(level = "debug", skip(_session, db))]
async fn create_handshake(
	_session: Session,
	State(db): State<db::Database>,
	Form(shake): Form<db::HandshakeContext>,
) -> Result<Form<db::Handshake>, (StatusCode, String)> {
	let created = db
		.create_handshake(shake)
		.await
		.map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
	Ok(Form(created))
}

/// Returns the total number of handshakes that have occurred
#[tracing::instrument(level = "debug", skip(_session, db))]
async fn count_handshakes(_session: Session, State(db): State<db::Database>) -> Result<String, (StatusCode, String)> {
	let count = db
		.count_handshakes()
		.await
		.map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
	Ok(count.to_string())
}
