//! `SeaORM` Entity, @generated by sea-orm-codegen 1.1.8

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "samey_pool")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    #[sea_orm(unique)]
    pub name: String,
    pub uploader_id: i32,
    pub is_public: bool,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(has_many = "super::samey_pool_post::Entity")]
    SameyPoolPost,
    #[sea_orm(
        belongs_to = "super::samey_user::Entity",
        from = "Column::UploaderId",
        to = "super::samey_user::Column::Id",
        on_update = "NoAction",
        on_delete = "Cascade"
    )]
    SameyUser,
}

impl Related<super::samey_pool_post::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::SameyPoolPost.def()
    }
}

impl Related<super::samey_user::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::SameyUser.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
