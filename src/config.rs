use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter};

use crate::{
    SameyError,
    entities::{prelude::SameyConfig, samey_config},
};

pub(crate) const APPLICATION_NAME_KEY: &str = "APPLICATION_NAME";
pub(crate) const AGE_CONFIRMATION_KEY: &str = "AGE_CONFIRMATION";

#[derive(Clone)]
pub(crate) struct AppConfig {
    pub(crate) application_name: String,
    pub(crate) age_confirmation: bool,
}

impl AppConfig {
    pub(crate) async fn new(db: &DatabaseConnection) -> Result<Self, SameyError> {
        let application_name = match SameyConfig::find()
            .filter(samey_config::Column::Key.eq(APPLICATION_NAME_KEY))
            .one(db)
            .await?
        {
            Some(row) => row.data.as_str().unwrap_or("Samey").to_owned(),
            None => "Samey".to_owned(),
        };
        let age_confirmation = match SameyConfig::find()
            .filter(samey_config::Column::Key.eq(AGE_CONFIRMATION_KEY))
            .one(db)
            .await?
        {
            Some(row) => row.data.as_bool().unwrap_or(false),
            None => false,
        };
        Ok(Self {
            application_name,
            age_confirmation,
        })
    }
}
