use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sqlx::{migrate, migrate::MigrateDatabase, prelude::*, Sqlite, SqlitePool};
use time::OffsetDateTime;
use tracing::info;

/// Database for storing/retrieving handshakes
#[derive(Debug, Clone)]
pub struct Database {
	/// Connection pool to use for queries
	pool: SqlitePool,
}

impl Database {
	/// Opens the database, creating it if it doesn't exist
	#[tracing::instrument("Opening database", level = "info")]
	pub async fn open(db_url: &str) -> Result<Self> {
		// Create the database if it doesn't exist
		if !Sqlite::database_exists(db_url).await? {
			info!("Database doesn't exist; creating");
			Sqlite::create_database(db_url).await?;
			info!("Created database");
		}

		// Open the database
		let pool = SqlitePool::connect(db_url).await?;
		Ok(Self { pool })
	}

	/// Runs pending migrations against the database
	#[tracing::instrument("Migrating database", level = "info", skip(self))]
	pub async fn migrate(&self) -> Result<()> {
		migrate!("./migrations").run(&self.pool).await?;
		Ok(())
	}

	/// Retrieves a single user record by its ID
	#[tracing::instrument("Database::get_user", level = "debug", skip(self))]
	pub async fn get_user(&self, id: i64) -> Result<Option<User>> {
		Ok(sqlx::query_as!(User, "SELECT * FROM users WHERE id = ?1", id)
			.fetch_optional(&self.pool)
			.await?)
	}

	/// Retrieves a single user record by its Resonite ID
	#[tracing::instrument("Database::get_user_by_resonite_id", level = "debug", skip(self))]
	pub async fn get_user_by_resonite_id(&self, id: &str) -> Result<Option<User>> {
		Ok(sqlx::query_as!(User, "SELECT * FROM users WHERE resonite_id = ?1", id)
			.fetch_optional(&self.pool)
			.await?)
	}

	/// Retrieves a single user record by its Resonite username
	#[tracing::instrument("Database::get_user_by_resonite_name", level = "debug", skip(self))]
	pub async fn get_user_by_resonite_name(&self, name: &str) -> Result<Option<User>> {
		Ok(
			sqlx::query_as!(User, "SELECT * FROM users WHERE resonite_name = ?1", name)
				.fetch_optional(&self.pool)
				.await?,
		)
	}

	/// Retrieves a single user record by its Resonite ID if it exists. If no record is found, it is instead retrieved
	/// by its Resonite username. If that also fails, then no record is returned.
	#[tracing::instrument("Database::get_user_by_resonite_info", level = "debug", skip(self))]
	pub async fn get_user_by_resonite_info(&self, info: &UserResoniteInfo) -> Result<Option<User>> {
		// Retrieve the user by its Resonite ID if it's provided
		let user = if let Some(user) = self.get_user_by_resonite_id(&info.id).await? {
			Some(user)
		} else {
			self.get_user_by_resonite_name(&info.name).await?
		};

		Ok(user)
	}

	/// Retrieves all user records
	#[tracing::instrument("Database::get_all_users", level = "debug", skip(self))]
	pub async fn get_all_users(&self) -> Result<Vec<User>> {
		Ok(sqlx::query_as!(User, "SELECT * FROM users")
			.fetch_all(&self.pool)
			.await?)
	}

	/// Retrieves the Resonite usernames of all user records
	#[tracing::instrument("Database::get_all_user_resonite_names", level = "debug", skip(self))]
	pub async fn get_all_user_resonite_names(&self) -> Result<Vec<String>> {
		Ok(sqlx::query_scalar!("SELECT resonite_name FROM users")
			.fetch_all(&self.pool)
			.await?)
	}

	/// Stores a new user
	#[tracing::instrument("Creating user", level = "info", skip(self))]
	pub async fn create_user(&self, info: &UserResoniteInfo) -> Result<User> {
		// Create the user record
		let id = sqlx::query!(
			"INSERT INTO users (resonite_id, resonite_name) VALUES (?1, ?2)",
			info.id,
			info.name
		)
		.execute(&self.pool)
		.await?
		.last_insert_rowid();

		// Return the newly-created record
		self.get_user(id)
			.await?
			.with_context(|| format!("Unable to retrieve newly-created user with ID {id}"))
	}

	/// Updates an existing user record
	#[tracing::instrument("Updating user", level = "info", skip(self))]
	pub async fn update_user(&self, user: &User) -> Result<bool> {
		let result = sqlx::query!(
			"UPDATE users SET resonite_id = ?2, resonite_name = ?3 WHERE id = ?1",
			user.id,
			user.resonite_id,
			user.resonite_name,
		)
		.execute(&self.pool)
		.await?;

		Ok(result.rows_affected() > 0)
	}

	/// Counts the number of user records
	#[tracing::instrument("Database::count_users", level = "debug", skip(self))]
	pub async fn count_users(&self) -> Result<i64> {
		Ok(sqlx::query_scalar!(r#"SELECT COUNT(*) AS "count: i64" FROM users"#)
			.fetch_optional(&self.pool)
			.await?
			.unwrap_or(0))
	}

	/// Retrieves a single handshake record by its ID
	#[tracing::instrument("Database::get_handshake", level = "debug", skip(self))]
	pub async fn get_handshake(&self, id: i64) -> Result<Option<Handshake>> {
		Ok(sqlx::query_as!(Handshake, "SELECT * FROM handshakes WHERE id = ?1", id)
			.fetch_optional(&self.pool)
			.await?)
	}

	/// Retrieves all handshake records
	#[tracing::instrument("Database::get_all_handshakes", level = "debug", skip(self))]
	pub async fn get_all_handshakes(&self) -> Result<Vec<Handshake>> {
		Ok(sqlx::query_as!(Handshake, "SELECT * FROM handshakes")
			.fetch_all(&self.pool)
			.await?)
	}

	/// Stores a new handshake, creating/updating its corresponding user if necessary
	#[tracing::instrument("Creating handshake", level = "info", skip(self))]
	pub async fn create_handshake(&self, shake: HandshakeContext) -> Result<Handshake> {
		let info = UserResoniteInfo {
			id: shake.id,
			name: shake.name,
		};

		// Retrieve the corresponding user and update it if necessary, or create it if it doesn't already exist
		let user = if let Some(mut user) = self.get_user_by_resonite_info(&info).await? {
			if user.resonite_id.is_none() || user.resonite_name != info.name {
				user.resonite_id = Some(info.id);
				user.resonite_name = info.name;
				self.update_user(&user).await?;
			}
			user
		} else {
			self.create_user(&info).await?
		};

		// Create the handshake record
		let id = sqlx::query!(
			"INSERT INTO handshakes (user_id, world_name) VALUES (?1, ?2)",
			user.id,
			shake.world,
		)
		.execute(&self.pool)
		.await?
		.last_insert_rowid();

		// Return the newly-created record
		self.get_handshake(id)
			.await?
			.with_context(|| format!("Unable to retrieve newly-created handshake with ID {id}"))
	}

	/// Counts the number of handshake records
	#[tracing::instrument("Database::count_handshakes", level = "debug", skip(self))]
	pub async fn count_handshakes(&self) -> Result<i64> {
		Ok(
			sqlx::query_scalar!(r#"SELECT COUNT(*) AS "count: i64" FROM handshakes"#)
				.fetch_optional(&self.pool)
				.await?
				.unwrap_or(0),
		)
	}

	/// Counts the number of handshake records for a specific user
	#[tracing::instrument("Database::count_user_handshakes", level = "debug", skip(self))]
	pub async fn count_user_handshakes(&self, id: i64) -> Result<i64> {
		Ok(sqlx::query_scalar!(
			r#"SELECT COUNT(*) AS "count: i64" FROM handshakes WHERE user_id = ?1"#,
			id
		)
		.fetch_optional(&self.pool)
		.await?
		.unwrap_or(0))
	}
}

/// User that has shaken hands
#[derive(Debug, Clone, FromRow, Serialize)]
pub struct User {
	/// Unique database ID for the user
	pub id: i64,

	/// Resonite user ID
	pub resonite_id: Option<String>,

	/// Resonite username (last known)
	pub resonite_name: String,

	/// Date/time the user was created
	#[serde(with = "time::serde::iso8601")]
	pub created_at: OffsetDateTime,
}

/// Handshake that has occurred
#[derive(Debug, Clone, FromRow, Serialize)]
pub struct Handshake {
	/// Unique ID for the handshake
	pub id: i64,

	/// ID of the user that shook hands
	pub user_id: i64,

	/// World the handshake took place in
	pub world_name: Option<String>,

	/// Date/time the handshake took place
	#[serde(with = "time::serde::iso8601")]
	pub created_at: OffsetDateTime,
}

/// Context for a new handshake
#[derive(Debug, Clone, Deserialize)]
pub struct HandshakeContext {
	/// Resonite ID of the user shaking hands
	pub id: String,

	/// Resonite username of the user shaking hands
	pub name: String,

	/// Name of the Resonite world the handshake is taking place in
	pub world: String,
}

/// Resonite user information
#[derive(Debug, Clone, Deserialize)]
pub struct UserResoniteInfo {
	/// Resonite ID of the user
	pub id: String,

	/// Resonite username of the user
	pub name: String,
}
