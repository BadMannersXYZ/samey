use std::{
    net::{IpAddr, Ipv6Addr},
    path::PathBuf,
};

use clap::{Parser, Subcommand};
use samey::{create_user, get_router};
use samey_migration::{Migrator, MigratorTrait};
use sea_orm::Database;

#[derive(Parser)]
struct Config {
    #[arg(short, long, default_value = "sqlite:db.sqlite3?mode=rwc")]
    database: String,

    #[arg(short, long, default_value = "files")]
    files_directory: PathBuf,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    Run {
        #[arg(short, long, default_value_t = IpAddr::V6(Ipv6Addr::UNSPECIFIED))]
        address: IpAddr,

        #[arg(short, long, default_value_t = 3000)]
        port: u16,
    },

    Migrate,

    AddAdminUser {
        #[arg(short, long)]
        username: String,

        #[arg(short, long)]
        password: String,
    },
}

impl Default for Commands {
    fn default() -> Self {
        Commands::Run {
            address: IpAddr::V6(Ipv6Addr::UNSPECIFIED),
            port: 3000,
        }
    }
}

#[tokio::main]
async fn main() {
    let config = Config::parse();
    let db = Database::connect(config.database)
        .await
        .expect("Unable to connect to database");
    match config.command.unwrap_or_default() {
        Commands::Migrate => {
            Migrator::up(&db, None)
                .await
                .expect("Unable to apply migrations");
        }

        Commands::AddAdminUser { username, password } => {
            create_user(db, &username, &password, true)
                .await
                .expect("Unable to add admin user");
        }

        Commands::Run { address, port } => {
            Migrator::up(&db, None)
                .await
                .expect("Unable to apply migrations");
            let app = get_router(db, config.files_directory)
                .await
                .expect("Unable to start router");
            let listener = tokio::net::TcpListener::bind((address, port))
                .await
                .expect("Unable to bind TCP listener");
            if address.is_ipv6() {
                println!("Listening on http://[{}]:{}", address, port);
            } else {
                println!("Listening on http://{}:{}", address, port);
            }
            axum::serve(listener, app).await.unwrap();
        }
    }
}
