use sea_orm_migration::{prelude::*, schema::*};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(SameyPost::Table)
                    .if_not_exists()
                    .col(pk_auto(SameyPost::Id))
                    .col(string_len(SameyPost::Media, 255))
                    .col(integer(SameyPost::Width))
                    .col(integer(SameyPost::Height))
                    .col(string_len(SameyPost::Thumbnail, 255))
                    .col(string_len_null(SameyPost::Title, 100))
                    .col(text_null(SameyPost::Description))
                    .col(boolean(SameyPost::IsPublic))
                    .col(enumeration(
                        SameyPost::Rating,
                        Rating::Enum,
                        [
                            Rating::Unrated,
                            Rating::Safe,
                            Rating::Questionable,
                            Rating::Explicit,
                        ],
                    ))
                    .col(date_time(SameyPost::UploadedAt))
                    .col(integer_null(SameyPost::ParentId))
                    .foreign_key(
                        ForeignKeyCreateStatement::new()
                            .name("fk-samey_post-samey_post-parent")
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
                            .name("fk-samey_post_source-samey_post")
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
                    .table(SameyTagPost::Table)
                    .if_not_exists()
                    .col(pk_auto(SameyTagPost::Id))
                    .col(integer(SameyTagPost::TagId))
                    .col(integer(SameyTagPost::PostId))
                    .foreign_key(
                        ForeignKeyCreateStatement::new()
                            .name("fk-samey_tag_post-samey_tag")
                            .from(SameyTagPost::Table, SameyTagPost::TagId)
                            .to(SameyTag::Table, SameyTag::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKeyCreateStatement::new()
                            .name("fk-samey_tag_post-samey_post")
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

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Replace the sample below with your own migration scripts

        manager
            .drop_table(Table::drop().table(SameyTagPost::Table).to_owned())
            .await?;

        manager
            .drop_table(Table::drop().table(SameyTag::Table).to_owned())
            .await?;

        manager
            .drop_table(Table::drop().table(SameyPostSource::Table).to_owned())
            .await?;

        manager
            .drop_table(Table::drop().table(SameyPost::Table).to_owned())
            .await?;

        Ok(())
    }
}

#[derive(DeriveIden)]
enum SameyPost {
    #[sea_orm(iden = "samey_post")]
    Table,
    Id,
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
