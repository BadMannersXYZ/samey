use sea_orm_migration::{prelude::*, schema::*};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(SameySession::Table)
                    .if_not_exists()
                    .col(pk_auto(SameySession::Id))
                    .col(string_uniq(SameySession::SessionId))
                    .col(json(SameySession::Data))
                    .col(big_integer(SameySession::ExpiryDate))
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(SameyConfig::Table)
                    .if_not_exists()
                    .col(pk_auto(SameyConfig::Id))
                    .col(string_uniq(SameyConfig::Key))
                    .col(json(SameyConfig::Data))
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(SameyUser::Table)
                    .if_not_exists()
                    .col(pk_auto(SameyUser::Id))
                    .col(string_len_uniq(SameyUser::Username, 50))
                    .col(string(SameyUser::Password))
                    .col(boolean(SameyUser::IsAdmin).default(false))
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(SameyTag::Table)
                    .if_not_exists()
                    .col(pk_auto(SameyTag::Id))
                    .col(string_len(SameyTag::Name, 100))
                    .col(string_len_uniq(SameyTag::NormalizedName, 100))
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(SameyPool::Table)
                    .if_not_exists()
                    .col(pk_auto(SameyPool::Id))
                    .col(string_len_uniq(SameyPool::Name, 100))
                    .col(integer(SameyPool::UploaderId))
                    .col(boolean(SameyPool::IsPublic).default(false))
                    .foreign_key(
                        ForeignKeyCreateStatement::new()
                            .name("fk-samey_pool-samey_user-uploader_id")
                            .from(SameyPool::Table, SameyPool::UploaderId)
                            .to(SameyUser::Table, SameyUser::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(SameyPost::Table)
                    .if_not_exists()
                    .col(pk_auto(SameyPost::Id))
                    .col(integer(SameyPost::UploaderId))
                    .col(string_len(SameyPost::Media, 255))
                    .col(integer(SameyPost::Width))
                    .col(integer(SameyPost::Height))
                    .col(string_len(SameyPost::Thumbnail, 255))
                    .col(string_len_null(SameyPost::Title, 100))
                    .col(text_null(SameyPost::Description))
                    .col(boolean(SameyPost::IsPublic).default(false))
                    .col(
                        enumeration(
                            SameyPost::Rating,
                            Rating::Enum,
                            [
                                Rating::Unrated,
                                Rating::Safe,
                                Rating::Questionable,
                                Rating::Explicit,
                            ],
                        )
                        .default(Rating::Unrated.into_iden().to_string()),
                    )
                    .col(date_time(SameyPost::UploadedAt))
                    .col(integer_null(SameyPost::ParentId))
                    .foreign_key(
                        ForeignKeyCreateStatement::new()
                            .name("fk-samey_post-samey_user-uploader_id")
                            .from(SameyPost::Table, SameyPost::UploaderId)
                            .to(SameyUser::Table, SameyUser::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKeyCreateStatement::new()
                            .name("fk-samey_post-samey_post-parent_id")
                            .from(SameyPost::Table, SameyPost::ParentId)
                            .to(SameyPost::Table, SameyPost::Id)
                            .on_delete(ForeignKeyAction::SetNull),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(SameyPostSource::Table)
                    .if_not_exists()
                    .col(pk_auto(SameyPostSource::Id))
                    .col(string_len(SameyPostSource::Url, 200))
                    .col(integer(SameyPostSource::PostId))
                    .foreign_key(
                        ForeignKeyCreateStatement::new()
                            .name("fk-samey_post_source-samey_post-post_id")
                            .from(SameyPostSource::Table, SameyPostSource::PostId)
                            .to(SameyPost::Table, SameyPostSource::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(SameyTagPost::Table)
                    .if_not_exists()
                    .col(pk_auto(SameyTagPost::Id))
                    .col(integer(SameyTagPost::TagId))
                    .col(integer(SameyTagPost::PostId))
                    .foreign_key(
                        ForeignKeyCreateStatement::new()
                            .name("fk-samey_tag_post-samey_tag-tag_id")
                            .from(SameyTagPost::Table, SameyTagPost::TagId)
                            .to(SameyTag::Table, SameyTag::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKeyCreateStatement::new()
                            .name("fk-samey_tag_post-samey_post-post_id")
                            .from(SameyTagPost::Table, SameyTagPost::PostId)
                            .to(SameyPost::Table, SameyPost::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .index(
                        Index::create()
                            .unique()
                            .col(SameyTagPost::PostId)
                            .col(SameyTagPost::TagId),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(SameyPoolPost::Table)
                    .if_not_exists()
                    .col(pk_auto(SameyPoolPost::Id))
                    .col(integer(SameyPoolPost::PoolId))
                    .col(integer(SameyPoolPost::PostId))
                    .col(float(SameyPoolPost::Position))
                    .foreign_key(
                        ForeignKeyCreateStatement::new()
                            .name("fk-samey_pool_post-samey_pool-pool_id")
                            .from(SameyPoolPost::Table, SameyPoolPost::PoolId)
                            .to(SameyPool::Table, SameyPool::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKeyCreateStatement::new()
                            .name("fk-samey_pool_post-samey_post-post_id")
                            .from(SameyPoolPost::Table, SameyPoolPost::PostId)
                            .to(SameyPost::Table, SameyPost::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .index(
                        Index::create()
                            .unique()
                            .col(SameyPoolPost::PoolId)
                            .col(SameyPoolPost::PostId),
                    )
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Replace the sample below with your own migration scripts

        manager
            .drop_table(Table::drop().table(SameyPoolPost::Table).to_owned())
            .await?;

        manager
            .drop_table(Table::drop().table(SameyTagPost::Table).to_owned())
            .await?;

        manager
            .drop_table(Table::drop().table(SameyPostSource::Table).to_owned())
            .await?;

        manager
            .drop_table(Table::drop().table(SameyPost::Table).to_owned())
            .await?;

        manager
            .drop_table(Table::drop().table(SameyPool::Table).to_owned())
            .await?;

        manager
            .drop_table(Table::drop().table(SameyTag::Table).to_owned())
            .await?;

        manager
            .drop_table(Table::drop().table(SameyUser::Table).to_owned())
            .await?;

        manager
            .drop_table(Table::drop().table(SameyConfig::Table).to_owned())
            .await?;

        manager
            .drop_table(Table::drop().table(SameySession::Table).to_owned())
            .await?;

        Ok(())
    }
}

#[derive(DeriveIden)]
enum SameySession {
    #[sea_orm(iden = "samey_session")]
    Table,
    Id,
    SessionId,
    Data,
    ExpiryDate,
}

#[derive(DeriveIden)]
enum SameyConfig {
    #[sea_orm(iden = "samey_config")]
    Table,
    Id,
    Key,
    Data,
}

#[derive(DeriveIden)]
enum SameyUser {
    #[sea_orm(iden = "samey_user")]
    Table,
    Id,
    Username,
    Password,
    IsAdmin,
}

#[derive(DeriveIden)]
enum SameyPost {
    #[sea_orm(iden = "samey_post")]
    Table,
    Id,
    UploaderId,
    Media,
    Width,
    Height,
    Thumbnail,
    Title,
    Description,
    IsPublic,
    Rating,
    UploadedAt,
    ParentId,
}

#[derive(DeriveIden)]
#[sea_orm(enum_name = "rating")]
pub enum Rating {
    #[sea_orm(iden = "rating")]
    Enum,
    #[sea_orm(iden = "u")]
    Unrated,
    #[sea_orm(iden = "s")]
    Safe,
    #[sea_orm(iden = "q")]
    Questionable,
    #[sea_orm(iden = "e")]
    Explicit,
}

#[derive(DeriveIden)]
enum SameyPostSource {
    #[sea_orm(iden = "samey_post_source")]
    Table,
    Id,
    Url,
    PostId,
}

#[derive(DeriveIden)]
enum SameyTag {
    #[sea_orm(iden = "samey_tag")]
    Table,
    Id,
    Name,
    NormalizedName,
}

#[derive(DeriveIden)]
enum SameyTagPost {
    #[sea_orm(iden = "samey_tag_post")]
    Table,
    Id,
    TagId,
    PostId,
}

#[derive(DeriveIden)]
enum SameyPool {
    #[sea_orm(iden = "samey_pool")]
    Table,
    Id,
    Name,
    UploaderId,
    IsPublic,
}

#[derive(DeriveIden)]
enum SameyPoolPost {
    #[sea_orm(iden = "samey_pool_post")]
    Table,
    Id,
    PoolId,
    PostId,
    Position,
}
