//! `SeaORM` Entity, @generated by sea-orm-codegen 1.1.8

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "samey_post")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub media: String,
    pub width: i32,
    pub height: i32,
    pub thumbnail: String,
    pub title: Option<String>,
    #[sea_orm(column_type = "Text", nullable)]
    pub description: Option<String>,
    pub is_public: bool,
    #[sea_orm(column_type = "custom(\"enum_text\")")]
    pub rating: String,
    pub uploaded_at: DateTime,
    pub parent_id: Option<i32>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "Entity",
        from = "Column::ParentId",
        to = "Column::Id",
        on_update = "NoAction",
        on_delete = "SetNull"
    )]
    SelfRef,
    #[sea_orm(has_many = "super::samey_post_source::Entity")]
    SameyPostSource,
    #[sea_orm(has_many = "super::samey_tag_post::Entity")]
    SameyTagPost,
}

impl Related<super::samey_post_source::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::SameyPostSource.def()
    }
}

impl Related<super::samey_tag_post::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::SameyTagPost.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
