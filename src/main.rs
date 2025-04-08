use clap::{Parser, Subcommand};
use migration::{Migrator, MigratorTrait};
use samey::{create_user, get_router};
use sea_orm::Database;

#[derive(Parser)]
struct Config {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    Run,
    Migrate,
    AddUser {
        #[arg(short, long)]
        username: String,
        #[arg(short, long)]
        password: String,
    },
    AddAdminUser {
        #[arg(short, long)]
        username: String,
        #[arg(short, long)]
        password: String,
    },
}

#[tokio::main]
async fn main() {
    let db = Database::connect("sqlite:db.sqlite3?mode=rwc")
        .await
        .expect("Unable to connect to database");
    let config = Config::parse();
    match config.command {
        Some(Commands::Migrate) => {
            Migrator::up(&db, None)
                .await
                .expect("Unable to apply migrations");
        }
        Some(Commands::AddUser { username, password }) => {
            create_user(db, username, password, false)
                .await
                .expect("Unable to add user");
        }
        Some(Commands::AddAdminUser { username, password }) => {
            create_user(db, username, password, true)
                .await
                .expect("Unable to add admin");
        }
        Some(Commands::Run) | None => {
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
    }
}
