use migration::{Migrator, MigratorTrait};
use samey::get_router;
use sea_orm::Database;

#[tokio::main]
async fn main() {
    let db = Database::connect("sqlite:db.sqlite3?mode=rwc")
        .await
        .expect("Unable to connect to database");
    Migrator::up(&db, None)
        .await
        .expect("Unable to apply migrations");
    let app = get_router(db, "files")
        .await
        .expect("Unable to start router");
    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000")
        .await
        .expect("Unable to listen to port");
    println!("Listening on http://localhost:3000");
    axum::serve(listener, app).await.unwrap();
}
