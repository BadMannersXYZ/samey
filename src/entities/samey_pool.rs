//! `SeaORM` Entity, @generated by sea-orm-codegen 1.1.8

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "samey_pool")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    #[sea_orm(unique)]
    pub name: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(has_many = "super::samey_pool_post::Entity")]
    SameyPoolPost,
}

impl Related<super::samey_pool_post::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::SameyPoolPost.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
