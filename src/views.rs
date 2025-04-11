use std::{
    collections::HashSet,
    fs::OpenOptions,
    io::{BufReader, Seek, Write},
    num::NonZero,
};

use askama::Template;
use axum::{
    extract::{Multipart, Path, Query, State},
    response::{Html, IntoResponse, Redirect},
};
use axum_extra::extract::Form;
use chrono::Utc;
use image::{GenericImageView, ImageFormat, ImageReader};
use itertools::Itertools;
use migration::{Expr, OnConflict};
use rand::Rng;
use sea_orm::{
    ActiveValue::Set, ColumnTrait, Condition, EntityTrait, FromQueryResult, IntoSimpleExpr,
    ModelTrait, PaginatorTrait, QueryFilter, QuerySelect,
};
use serde::Deserialize;
use tokio::task::spawn_blocking;

use crate::{
    AppState, NEGATIVE_PREFIX, RATING_PREFIX,
    auth::{AuthSession, Credentials, User},
    entities::{
        prelude::{SameyPool, SameyPoolPost, SameyPost, SameyPostSource, SameyTag, SameyTagPost},
        samey_pool, samey_pool_post, samey_post, samey_post_source, samey_tag, samey_tag_post,
    },
    error::SameyError,
    query::{
        PoolPost, PostOverview, filter_by_user, get_posts_in_pool, get_tags_for_post, search_posts,
    },
};

const MAX_THUMBNAIL_DIMENSION: u32 = 192;

// Index view

#[derive(Template)]
#[template(path = "index.html")]
struct IndexTemplate {
    user: Option<User>,
}

pub(crate) async fn index(auth_session: AuthSession) -> Result<impl IntoResponse, SameyError> {
    Ok(Html(
        IndexTemplate {
            user: auth_session.user,
        }
        .render()?,
    ))
}

// Auth views

pub(crate) async fn login(
    mut auth_session: AuthSession,
    Form(credentials): Form<Credentials>,
) -> Result<impl IntoResponse, SameyError> {
    let user = match auth_session.authenticate(credentials).await {
        Ok(Some(user)) => user,
        Ok(None) => return Err(SameyError::Authentication("Invalid credentials".into())),
        Err(_) => return Err(SameyError::Other("Auth session error".into())),
    };

    auth_session
        .login(&user)
        .await
        .map_err(|_| SameyError::Other("Login failed".into()))?;
    Ok(Redirect::to("/"))
}

pub(crate) async fn logout(mut auth_session: AuthSession) -> Result<impl IntoResponse, SameyError> {
    auth_session
        .logout()
        .await
        .map_err(|_| SameyError::Other("Logout error".into()))?;
    Ok(Redirect::to("/"))
}

// Post upload view

pub(crate) async fn upload(
    State(AppState { db, files_dir }): State<AppState>,
    auth_session: AuthSession,
    mut multipart: Multipart,
) -> Result<impl IntoResponse, SameyError> {
    let user = match auth_session.user {
        Some(user) => user,
        None => return Err(SameyError::Forbidden),
    };

    let mut upload_tags: Option<Vec<samey_tag::Model>> = None;
    let mut source_file: Option<String> = None;
    let mut thumbnail_file: Option<String> = None;
    let mut width: Option<NonZero<i32>> = None;
    let mut height: Option<NonZero<i32>> = None;
    let base_path = std::path::Path::new(files_dir.as_ref());

    // Read multipart form data
    while let Some(mut field) = multipart.next_field().await.unwrap() {
        match field.name().unwrap() {
            "tags" => {
                if let Ok(tags) = field.text().await {
                    let tags: HashSet<String> = tags.split_whitespace().map(String::from).collect();
                    let normalized_tags: HashSet<String> =
                        tags.iter().map(|tag| tag.to_lowercase()).collect();
                    SameyTag::insert_many(tags.into_iter().map(|tag| samey_tag::ActiveModel {
                        normalized_name: Set(tag.to_lowercase()),
                        name: Set(tag),
                        ..Default::default()
                    }))
                    .on_conflict(
                        OnConflict::column(samey_tag::Column::NormalizedName)
                            .do_nothing()
                            .to_owned(),
                    )
                    .exec_without_returning(&db)
                    .await?;
                    upload_tags = Some(
                        SameyTag::find()
                            .filter(samey_tag::Column::NormalizedName.is_in(normalized_tags))
                            .all(&db)
                            .await?,
                    );
                }
            }
            "media-file" => {
                let content_type = field
                    .content_type()
                    .ok_or(SameyError::Other("Missing content type".into()))?;
                let format = ImageFormat::from_mime_type(content_type).ok_or(SameyError::Other(
                    format!("Unknown content type: {}", content_type),
                ))?;
                let file_name = {
                    let mut rng = rand::rng();
                    let mut file_name: String = (0..8)
                        .map(|_| rng.sample(rand::distr::Alphanumeric) as char)
                        .collect();
                    file_name.push('.');
                    file_name.push_str(format.extensions_str()[0]);
                    file_name
                };
                let thumbnail_file_name = format!("thumb-{}", file_name);
                let file_path = base_path.join(&file_name);
                let mut file = OpenOptions::new()
                    .read(true)
                    .write(true)
                    .create(true)
                    .open(&file_path)?;
                while let Some(chunk) = field.chunk().await? {
                    file.write_all(&chunk)?;
                }
                let base_path_2 = base_path.to_owned();
                let (w, h, thumbnail_file_name) =
                    spawn_blocking(move || -> Result<_, SameyError> {
                        file.seek(std::io::SeekFrom::Start(0))?;
                        let mut image = ImageReader::new(BufReader::new(file));
                        image.set_format(format);
                        let image = image.decode()?;
                        let (w, h) = image.dimensions();
                        let width = NonZero::new(w.try_into()?);
                        let height = NonZero::new(h.try_into()?);
                        let thumbnail = image.resize(
                            MAX_THUMBNAIL_DIMENSION,
                            MAX_THUMBNAIL_DIMENSION,
                            image::imageops::FilterType::CatmullRom,
                        );
                        thumbnail.save(base_path_2.join(&thumbnail_file_name))?;
                        Ok((width, height, thumbnail_file_name))
                    })
                    .await??;
                width = w;
                height = h;
                source_file = Some(file_name);
                thumbnail_file = Some(thumbnail_file_name);
            }
            _ => (),
        }
    }

    if let (Some(upload_tags), Some(source_file), Some(thumbnail_file), Some(width), Some(height)) = (
        upload_tags,
        source_file,
        thumbnail_file,
        width.map(|w| w.get()),
        height.map(|h| h.get()),
    ) {
        let uploaded_post = SameyPost::insert(samey_post::ActiveModel {
            uploader_id: Set(user.id),
            media: Set(source_file),
            width: Set(width),
            height: Set(height),
            thumbnail: Set(thumbnail_file),
            title: Set(None),
            description: Set(None),
            rating: Set("u".to_owned()),
            uploaded_at: Set(Utc::now().naive_utc()),
            parent_id: Set(None),
            ..Default::default()
        })
        .exec(&db)
        .await?
        .last_insert_id;

        // Add tags to post
        SameyTagPost::insert_many(
            upload_tags
                .into_iter()
                .map(|tag| samey_tag_post::ActiveModel {
                    post_id: Set(uploaded_post),
                    tag_id: Set(tag.id),
                    ..Default::default()
                }),
        )
        .exec(&db)
        .await?;

        Ok(Redirect::to(&format!("/post/{}", uploaded_post)))
    } else {
        Err(SameyError::Other("Missing parameters for upload".into()))
    }
}

// Search fields views

struct SearchTag {
    name: String,
    value: String,
}

#[derive(Template)]
#[template(path = "search_tags.html")]
struct SearchTagsTemplate {
    tags: Vec<SearchTag>,
    selection_end: usize,
}

#[derive(Debug, Deserialize)]
pub(crate) struct SearchTagsForm {
    tags: String,
    selection_end: usize,
}

pub(crate) async fn search_tags(
    State(AppState { db, .. }): State<AppState>,
    Form(body): Form<SearchTagsForm>,
) -> Result<impl IntoResponse, SameyError> {
    let tags = match body.tags[..body.selection_end].split(' ').last() {
        Some(mut tag) => {
            tag = tag.trim();
            if tag.is_empty() {
                vec![]
            } else if tag.starts_with(NEGATIVE_PREFIX) {
                if tag[NEGATIVE_PREFIX.len()..].starts_with(RATING_PREFIX) {
                    [
                        format!("{}u", RATING_PREFIX),
                        format!("{}s", RATING_PREFIX),
                        format!("{}q", RATING_PREFIX),
                        format!("{}e", RATING_PREFIX),
                    ]
                    .into_iter()
                    .filter(|t| t.starts_with(&tag[NEGATIVE_PREFIX.len()..]))
                    .map(|tag| SearchTag {
                        value: format!("-{}", &tag),
                        name: tag,
                    })
                    .collect()
                } else {
                    SameyTag::find()
                        .filter(Expr::cust_with_expr(
                            "LOWER(\"samey_tag\".\"name\") LIKE CONCAT(?, '%')",
                            tag[NEGATIVE_PREFIX.len()..].to_lowercase(),
                        ))
                        .limit(10)
                        .all(&db)
                        .await?
                        .into_iter()
                        .map(|tag| SearchTag {
                            value: format!("-{}", &tag.name),
                            name: tag.name,
                        })
                        .collect()
                }
            } else if tag.starts_with(RATING_PREFIX) {
                [
                    format!("{}u", RATING_PREFIX),
                    format!("{}s", RATING_PREFIX),
                    format!("{}q", RATING_PREFIX),
                    format!("{}e", RATING_PREFIX),
                ]
                .into_iter()
                .filter(|t| t.starts_with(tag))
                .map(|tag| SearchTag {
                    value: tag.clone(),
                    name: tag,
                })
                .collect()
            } else {
                SameyTag::find()
                    .filter(Expr::cust_with_expr(
                        "LOWER(\"samey_tag\".\"name\") LIKE CONCAT(?, '%')",
                        tag.to_lowercase(),
                    ))
                    .limit(10)
                    .all(&db)
                    .await?
                    .into_iter()
                    .map(|tag| SearchTag {
                        value: tag.name.clone(),
                        name: tag.name,
                    })
                    .collect()
            }
        }
        _ => vec![],
    };
    Ok(Html(
        SearchTagsTemplate {
            tags,
            selection_end: body.selection_end,
        }
        .render()?,
    ))
}

#[derive(Template)]
#[template(path = "select_tag.html")]
struct SelectTagTemplate {
    tags: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct SelectTagForm {
    tags: String,
    new_tag: String,
    selection_end: usize,
}

pub(crate) async fn select_tag(
    Form(body): Form<SelectTagForm>,
) -> Result<impl IntoResponse, SameyError> {
    let mut tags = String::new();
    for (tag, _) in body.tags[..body.selection_end].split(' ').tuple_windows() {
        if !tags.is_empty() {
            tags.push(' ');
        }
        tags.push_str(tag);
    }
    if !tags.is_empty() {
        tags.push(' ');
    }
    tags.push_str(&body.new_tag);
    for tag in body.tags[body.selection_end..].split(' ') {
        if !tags.is_empty() {
            tags.push(' ');
        }
        tags.push_str(tag);
    }
    tags.push(' ');
    Ok(Html(SelectTagTemplate { tags }.render()?))
}

// Post list views

#[derive(Template)]
#[template(path = "posts.html")]
struct PostsTemplate<'a> {
    tags: Option<Vec<&'a str>>,
    tags_text: Option<String>,
    posts: Vec<PostOverview>,
    page: u32,
    page_count: u64,
}

#[derive(Debug, Deserialize)]
pub(crate) struct PostsQuery {
    tags: Option<String>,
}

pub(crate) async fn posts(
    state: State<AppState>,
    auth_session: AuthSession,
    query: Query<PostsQuery>,
) -> Result<impl IntoResponse, SameyError> {
    posts_page(state, auth_session, query, Path(1)).await
}

pub(crate) async fn posts_page(
    State(AppState { db, .. }): State<AppState>,
    auth_session: AuthSession,
    Query(query): Query<PostsQuery>,
    Path(page): Path<u32>,
) -> Result<impl IntoResponse, SameyError> {
    let tags = query
        .tags
        .as_ref()
        .map(|tags| tags.split_whitespace().collect::<Vec<_>>());
    let pagination = search_posts(tags.as_ref(), auth_session.user.as_ref()).paginate(&db, 50);
    let page_count = pagination.num_pages().await?;
    let posts = pagination.fetch_page(page.saturating_sub(1) as u64).await?;
    let posts = posts
        .into_iter()
        .map(|post| {
            let mut tags_vec: Vec<_> = post.tags.split_ascii_whitespace().collect();
            tags_vec.sort();
            PostOverview {
                tags: tags_vec.into_iter().join(" "),
                ..post
            }
        })
        .collect();

    Ok(Html(
        PostsTemplate {
            tags_text: tags.as_ref().map(|tags| tags.iter().join(" ")),
            tags,
            posts,
            page,
            page_count,
        }
        .render()?,
    ))
}

// Pool views

pub(crate) async fn get_pools(
    state: State<AppState>,
    auth_session: AuthSession,
) -> Result<impl IntoResponse, SameyError> {
    get_pools_page(state, auth_session, Path(1)).await
}

#[derive(Template)]
#[template(path = "pools.html")]
struct GetPoolsTemplate {
    pools: Vec<samey_pool::Model>,
    page: u32,
    page_count: u64,
}

pub(crate) async fn get_pools_page(
    State(AppState { db, .. }): State<AppState>,
    auth_session: AuthSession,
    Path(page): Path<u32>,
) -> Result<impl IntoResponse, SameyError> {
    let query = match auth_session.user {
        None => SameyPool::find().filter(samey_pool::Column::IsPublic.into_simple_expr()),
        Some(user) if user.is_admin => SameyPool::find(),
        Some(user) => SameyPool::find().filter(
            Condition::any()
                .add(samey_pool::Column::IsPublic.into_simple_expr())
                .add(samey_pool::Column::UploaderId.eq(user.id)),
        ),
    };

    let pagination = query.paginate(&db, 25);
    let page_count = pagination.num_pages().await?;

    let pools = pagination.fetch_page(page.saturating_sub(1) as u64).await?;

    Ok(Html(
        GetPoolsTemplate {
            pools,
            page,
            page_count,
        }
        .render()?,
    ))
}

#[derive(Debug, Deserialize)]
pub(crate) struct CreatePoolForm {
    pool: String,
}

pub(crate) async fn create_pool(
    State(AppState { db, .. }): State<AppState>,
    auth_session: AuthSession,
    Form(body): Form<CreatePoolForm>,
) -> Result<impl IntoResponse, SameyError> {
    let user = match auth_session.user {
        Some(user) => user,
        None => return Err(SameyError::Forbidden),
    };

    let pool_id = SameyPool::insert(samey_pool::ActiveModel {
        name: Set(body.pool),
        uploader_id: Set(user.id),
        ..Default::default()
    })
    .exec(&db)
    .await?
    .last_insert_id;

    Ok(Redirect::to(&format!("/pool/{}", pool_id)))
}

#[derive(Template)]
#[template(path = "pool.html")]
struct ViewPoolTemplate {
    pool: samey_pool::Model,
    posts: Vec<PoolPost>,
    can_edit: bool,
}

pub(crate) async fn view_pool(
    State(AppState { db, .. }): State<AppState>,
    auth_session: AuthSession,
    Path(pool_id): Path<i32>,
) -> Result<impl IntoResponse, SameyError> {
    let pool = SameyPool::find_by_id(pool_id)
        .one(&db)
        .await?
        .ok_or(SameyError::NotFound)?;

    let can_edit = match auth_session.user.as_ref() {
        None => false,
        Some(user) => user.is_admin || pool.uploader_id == user.id,
    };

    if !pool.is_public && !can_edit {
        return Err(SameyError::NotFound);
    }

    let posts = get_posts_in_pool(pool_id, auth_session.user.as_ref())
        .all(&db)
        .await?;

    Ok(Html(
        ViewPoolTemplate {
            pool,
            can_edit,
            posts,
        }
        .render()?,
    ))
}

#[derive(Debug, Deserialize)]
pub(crate) struct ChangePoolVisibilityForm {
    is_public: Option<String>,
}

pub(crate) async fn change_pool_visibility(
    State(AppState { db, .. }): State<AppState>,
    auth_session: AuthSession,
    Path(pool_id): Path<i32>,
    Form(body): Form<ChangePoolVisibilityForm>,
) -> Result<impl IntoResponse, SameyError> {
    let pool = SameyPool::find_by_id(pool_id)
        .one(&db)
        .await?
        .expect("Pool for samey_pool_post must exist");

    let can_edit = match auth_session.user.as_ref() {
        None => false,
        Some(user) => user.is_admin || pool.uploader_id == user.id,
    };

    if !can_edit {
        return Err(SameyError::Forbidden);
    }

    SameyPool::update(samey_pool::ActiveModel {
        id: Set(pool.id),
        is_public: Set(body.is_public.is_some()),
        ..Default::default()
    })
    .exec(&db)
    .await?;

    Ok("")
}

#[derive(Debug, Deserialize)]
pub(crate) struct AddPostToPoolForm {
    post_id: i32,
}

#[derive(Debug, FromQueryResult)]
pub(crate) struct PoolWithMaxPosition {
    id: i32,
    uploader_id: i32,
    max_position: Option<f32>,
}

#[derive(Template)]
#[template(path = "add_post_to_pool.html")]
struct AddPostToPoolTemplate {
    pool: PoolWithMaxPosition,
    posts: Vec<PoolPost>,
    can_edit: bool,
}

pub(crate) async fn add_post_to_pool(
    State(AppState { db, .. }): State<AppState>,
    auth_session: AuthSession,
    Path(pool_id): Path<i32>,
    Form(body): Form<AddPostToPoolForm>,
) -> Result<impl IntoResponse, SameyError> {
    let pool = SameyPool::find_by_id(pool_id)
        .select_only()
        .column(samey_pool::Column::Id)
        .column(samey_pool::Column::UploaderId)
        .column_as(samey_pool_post::Column::Position.max(), "max_position")
        .left_join(SameyPoolPost)
        .group_by(samey_pool::Column::Id)
        .into_model::<PoolWithMaxPosition>()
        .one(&db)
        .await?
        .ok_or(SameyError::NotFound)?;

    let can_edit_pool = match auth_session.user.as_ref() {
        None => false,
        Some(user) => user.is_admin || pool.uploader_id == user.id,
    };

    if !can_edit_pool {
        return Err(SameyError::Forbidden);
    }

    let post = filter_by_user(
        SameyPost::find_by_id(body.post_id),
        auth_session.user.as_ref(),
    )
    .one(&db)
    .await?
    .ok_or(SameyError::NotFound)?;

    SameyPoolPost::insert(samey_pool_post::ActiveModel {
        pool_id: Set(pool.id),
        post_id: Set(post.id),
        position: Set(pool.max_position.unwrap_or(0.0).floor() + 1.0),
        ..Default::default()
    })
    .exec(&db)
    .await?;

    let posts = get_posts_in_pool(pool.id, auth_session.user.as_ref())
        .all(&db)
        .await?;

    Ok(Html(
        AddPostToPoolTemplate {
            pool,
            posts,
            can_edit: true,
        }
        .render()?,
    ))
}

pub(crate) async fn remove_pool_post(
    State(AppState { db, .. }): State<AppState>,
    auth_session: AuthSession,
    Path(pool_post_id): Path<i32>,
) -> Result<impl IntoResponse, SameyError> {
    let pool_post = SameyPoolPost::find_by_id(pool_post_id)
        .one(&db)
        .await?
        .ok_or(SameyError::NotFound)?;
    let pool = SameyPool::find_by_id(pool_post.pool_id)
        .one(&db)
        .await?
        .expect("Pool for samey_pool_post must exist");

    let can_edit = match auth_session.user.as_ref() {
        None => false,
        Some(user) => user.is_admin || pool.uploader_id == user.id,
    };

    if !can_edit {
        return Err(SameyError::Forbidden);
    }

    pool_post.delete(&db).await?;

    Ok("")
}

#[derive(Debug, Deserialize)]
pub(crate) struct SortPoolForm {
    old_index: usize,
    new_index: usize,
}

#[derive(Template)]
#[template(path = "pool_posts.html")]
struct PoolPostsTemplate {
    pool: samey_pool::Model,
    posts: Vec<PoolPost>,
    can_edit: bool,
}

pub(crate) async fn sort_pool(
    State(AppState { db, .. }): State<AppState>,
    auth_session: AuthSession,
    Path(pool_id): Path<i32>,
    Form(body): Form<SortPoolForm>,
) -> Result<impl IntoResponse, SameyError> {
    let pool = SameyPool::find_by_id(pool_id)
        .one(&db)
        .await?
        .ok_or(SameyError::NotFound)?;

    let can_edit = match auth_session.user.as_ref() {
        None => false,
        Some(user) => user.is_admin || pool.uploader_id == user.id,
    };

    if !can_edit {
        return Err(SameyError::Forbidden);
    }

    if body.old_index != body.new_index {
        let posts = get_posts_in_pool(pool_id, auth_session.user.as_ref())
            .all(&db)
            .await?;
        let changed_post = posts.get(body.old_index).ok_or(SameyError::NotFound)?;
        let min_index = if body.new_index < body.old_index {
            body.new_index.checked_sub(1)
        } else {
            Some(body.new_index)
        };
        let max_index = if body.new_index == posts.len().saturating_sub(1) {
            None
        } else {
            if body.new_index < body.old_index {
                Some(body.new_index)
            } else {
                Some(body.new_index + 1)
            }
        };
        let min = min_index.map(|index| posts[index].position).unwrap_or(0.0);
        let max = max_index
            .map(|index| posts[index].position)
            .unwrap_or_else(|| posts.last().map(|post| post.position).unwrap_or(min) + 2.0);
        SameyPoolPost::update(samey_pool_post::ActiveModel {
            id: Set(changed_post.pool_post_id),
            position: Set((min + max) / 2.0),
            ..Default::default()
        })
        .exec(&db)
        .await?;
    }

    let posts = get_posts_in_pool(pool_id, auth_session.user.as_ref())
        .all(&db)
        .await?;
    Ok(Html(
        PoolPostsTemplate {
            pool,
            posts,
            can_edit: true,
        }
        .render()?,
    ))
}

// Single post views

#[derive(Template)]
#[template(path = "view_post.html")]
struct ViewPostTemplate {
    post: samey_post::Model,
    tags: Vec<samey_tag::Model>,
    tags_text: String,
    sources: Vec<samey_post_source::Model>,
    can_edit: bool,
    parent_post: Option<PostOverview>,
    children_posts: Vec<PostOverview>,
}

pub(crate) async fn view_post(
    State(AppState { db, .. }): State<AppState>,
    auth_session: AuthSession,
    Path(post_id): Path<i32>,
) -> Result<impl IntoResponse, SameyError> {
    let post_id = post_id;
    let tags = get_tags_for_post(post_id).all(&db).await?;
    let tags_text = tags.iter().map(|tag| &tag.name).join(" ");

    let sources = SameyPostSource::find()
        .filter(samey_post_source::Column::PostId.eq(post_id))
        .all(&db)
        .await?;

    let post = SameyPost::find_by_id(post_id)
        .one(&db)
        .await?
        .ok_or(SameyError::NotFound)?;

    let parent_post = if let Some(parent_id) = post.parent_id {
        match filter_by_user(SameyPost::find_by_id(parent_id), auth_session.user.as_ref())
            .one(&db)
            .await?
        {
            Some(parent_post) => Some(PostOverview {
                id: parent_id,
                thumbnail: parent_post.thumbnail,
                tags: get_tags_for_post(post_id)
                    .all(&db)
                    .await?
                    .iter()
                    .map(|tag| &tag.name)
                    .join(" "),
                rating: parent_post.rating,
            }),
            None => None,
        }
    } else {
        None
    };

    let children_posts_models = filter_by_user(
        SameyPost::find().filter(samey_post::Column::ParentId.eq(post_id)),
        auth_session.user.as_ref(),
    )
    .all(&db)
    .await?;
    let mut children_posts = Vec::with_capacity(children_posts_models.capacity());

    for child_post in children_posts_models.into_iter() {
        children_posts.push(PostOverview {
            id: child_post.id,
            thumbnail: child_post.thumbnail,
            tags: get_tags_for_post(child_post.id)
                .all(&db)
                .await?
                .iter()
                .map(|tag| &tag.name)
                .join(" "),
            rating: child_post.rating,
        });
    }

    let can_edit = match auth_session.user {
        None => false,
        Some(user) => user.is_admin || post.uploader_id == user.id,
    };

    if !post.is_public && !can_edit {
        return Err(SameyError::NotFound);
    }

    Ok(Html(
        ViewPostTemplate {
            post,
            tags,
            tags_text,
            sources,
            can_edit,
            parent_post,
            children_posts,
        }
        .render()?,
    ))
}

#[derive(Template)]
#[template(path = "post_details.html")]
struct PostDetailsTemplate {
    post: samey_post::Model,
    sources: Vec<samey_post_source::Model>,
    can_edit: bool,
}

pub(crate) async fn post_details(
    State(AppState { db, .. }): State<AppState>,
    auth_session: AuthSession,
    Path(post_id): Path<i32>,
) -> Result<impl IntoResponse, SameyError> {
    let sources = SameyPostSource::find()
        .filter(samey_post_source::Column::PostId.eq(post_id))
        .all(&db)
        .await?;

    let post = SameyPost::find_by_id(post_id)
        .one(&db)
        .await?
        .ok_or(SameyError::NotFound)?;

    let can_edit = match auth_session.user {
        None => false,
        Some(user) => user.is_admin || post.uploader_id == user.id,
    };

    if !post.is_public && !can_edit {
        return Err(SameyError::NotFound);
    }

    Ok(Html(
        PostDetailsTemplate {
            post,
            sources,
            can_edit,
        }
        .render()?,
    ))
}

#[derive(Debug, Deserialize)]
pub(crate) struct SubmitPostDetailsForm {
    title: String,
    description: String,
    is_public: Option<String>,
    rating: String,
    #[serde(rename = "source")]
    sources: Option<Vec<String>>,
    tags: String,
    parent_post: String,
}

#[derive(Template)]
#[template(path = "submit_post_details.html")]
struct SubmitPostDetailsTemplate {
    post: samey_post::Model,
    parent_post: Option<PostOverview>,
    sources: Vec<samey_post_source::Model>,
    tags: Vec<samey_tag::Model>,
    can_edit: bool,
}

pub(crate) async fn submit_post_details(
    State(AppState { db, .. }): State<AppState>,
    auth_session: AuthSession,
    Path(post_id): Path<i32>,
    Form(body): Form<SubmitPostDetailsForm>,
) -> Result<impl IntoResponse, SameyError> {
    let post = SameyPost::find_by_id(post_id)
        .one(&db)
        .await?
        .ok_or(SameyError::NotFound)?;

    match auth_session.user.as_ref() {
        None => return Err(SameyError::Forbidden),
        Some(user) => {
            if !user.is_admin && post.uploader_id != user.id {
                return Err(SameyError::Forbidden);
            }
        }
    }

    let title = match body.title.trim() {
        title if title.is_empty() => None,
        title => Some(title.to_owned()),
    };
    let description = match body.description.trim() {
        description if description.is_empty() => None,
        description => Some(description.to_owned()),
    };
    let parent_post = if let Some(parent_id) = body.parent_post.trim().parse().ok() {
        match filter_by_user(SameyPost::find_by_id(parent_id), auth_session.user.as_ref())
            .one(&db)
            .await?
        {
            Some(parent_post) => Some(PostOverview {
                id: parent_id,
                thumbnail: parent_post.thumbnail,
                tags: get_tags_for_post(post_id)
                    .all(&db)
                    .await?
                    .iter()
                    .map(|tag| &tag.name)
                    .join(" "),
                rating: parent_post.rating,
            }),
            None => None,
        }
    } else {
        None
    };
    let is_public = body.is_public.is_some();
    let post = SameyPost::update(samey_post::ActiveModel {
        id: Set(post_id),
        title: Set(title),
        description: Set(description),
        is_public: Set(is_public),
        rating: Set(body.rating),
        parent_id: Set(parent_post.as_ref().map(|post| post.id)),
        ..Default::default()
    })
    .exec(&db)
    .await?;

    // TODO: Improve this to not delete sources without necessity
    SameyPostSource::delete_many()
        .filter(samey_post_source::Column::PostId.eq(post_id))
        .exec(&db)
        .await?;
    // TODO: Improve this to not recreate existing sources (see above)
    if let Some(sources) = body.sources {
        let sources: Vec<_> = sources
            .into_iter()
            .filter(|source| !source.is_empty())
            .map(|source| samey_post_source::ActiveModel {
                url: Set(source),
                post_id: Set(post_id),
                ..Default::default()
            })
            .collect();
        if !sources.is_empty() {
            SameyPostSource::insert_many(sources).exec(&db).await?;
        }
    };

    let tags: HashSet<String> = body.tags.split_whitespace().map(String::from).collect();
    let normalized_tags: HashSet<String> = tags.iter().map(|tag| tag.to_lowercase()).collect();
    // TODO: Improve this to not delete tag-post entries without necessity
    SameyTagPost::delete_many()
        .filter(samey_tag_post::Column::PostId.eq(post_id))
        .exec(&db)
        .await?;
    // TODO: Improve this to not recreate existing tag-post entries (see above)
    SameyTag::insert_many(tags.into_iter().map(|tag| samey_tag::ActiveModel {
        normalized_name: Set(tag.to_lowercase()),
        name: Set(tag),
        ..Default::default()
    }))
    .on_conflict(
        OnConflict::column(samey_tag::Column::NormalizedName)
            .do_nothing()
            .to_owned(),
    )
    .exec_without_returning(&db)
    .await?;
    let mut upload_tags = SameyTag::find()
        .filter(samey_tag::Column::NormalizedName.is_in(normalized_tags))
        .all(&db)
        .await?;
    SameyTagPost::insert_many(upload_tags.iter().map(|tag| samey_tag_post::ActiveModel {
        post_id: Set(post_id),
        tag_id: Set(tag.id),
        ..Default::default()
    }))
    .exec(&db)
    .await?;
    upload_tags.sort_by(|a, b| a.name.cmp(&b.name));

    let sources = SameyPostSource::find()
        .filter(samey_post_source::Column::PostId.eq(post_id))
        .all(&db)
        .await?;

    Ok(Html(
        SubmitPostDetailsTemplate {
            post,
            sources,
            tags: upload_tags,
            parent_post,
            can_edit: true,
        }
        .render()?,
    ))
}

struct EditPostSource {
    url: Option<String>,
}

#[derive(Template)]
#[template(path = "edit_post_details.html")]
struct EditDetailsTemplate {
    post: samey_post::Model,
    sources: Vec<EditPostSource>,
    tags: String,
}

pub(crate) async fn edit_post_details(
    State(AppState { db, .. }): State<AppState>,
    auth_session: AuthSession,
    Path(post_id): Path<i32>,
) -> Result<impl IntoResponse, SameyError> {
    let post = SameyPost::find_by_id(post_id)
        .one(&db)
        .await?
        .ok_or(SameyError::NotFound)?;

    match auth_session.user {
        None => return Err(SameyError::Forbidden),
        Some(user) => {
            if !user.is_admin && post.uploader_id != user.id {
                return Err(SameyError::Forbidden);
            }
        }
    }

    let sources = SameyPostSource::find()
        .filter(samey_post_source::Column::PostId.eq(post_id))
        .all(&db)
        .await?
        .into_iter()
        .map(|source| EditPostSource {
            url: Some(source.url),
        })
        .collect();

    let tags = get_tags_for_post(post_id)
        .select_only()
        .column(samey_tag::Column::Name)
        .into_tuple::<String>()
        .all(&db)
        .await?
        .join(" ");

    Ok(Html(
        EditDetailsTemplate {
            post,
            sources,
            tags,
        }
        .render()?,
    ))
}

#[derive(Template)]
#[template(path = "post_source.html")]
struct AddPostSourceTemplate {
    source: EditPostSource,
}

pub(crate) async fn add_post_source() -> Result<impl IntoResponse, SameyError> {
    Ok(Html(
        AddPostSourceTemplate {
            source: EditPostSource { url: None },
        }
        .render()?,
    ))
}

pub(crate) async fn remove_field() -> impl IntoResponse {
    ""
}

#[derive(Template)]
#[template(path = "get_media.html")]
struct GetMediaTemplate {
    post: samey_post::Model,
}

pub(crate) async fn get_media(
    State(AppState { db, .. }): State<AppState>,
    auth_session: AuthSession,
    Path(post_id): Path<i32>,
) -> Result<impl IntoResponse, SameyError> {
    let post = SameyPost::find_by_id(post_id)
        .one(&db)
        .await?
        .ok_or(SameyError::NotFound)?;

    let can_edit = match auth_session.user {
        None => false,
        Some(user) => user.is_admin || post.uploader_id == user.id,
    };

    if !post.is_public && !can_edit {
        return Err(SameyError::NotFound);
    }

    Ok(Html(GetMediaTemplate { post }.render()?))
}

#[derive(Template)]
#[template(path = "get_full_media.html")]
struct GetFullMediaTemplate {
    post: samey_post::Model,
}

pub(crate) async fn get_full_media(
    State(AppState { db, .. }): State<AppState>,
    auth_session: AuthSession,
    Path(post_id): Path<i32>,
) -> Result<impl IntoResponse, SameyError> {
    let post = SameyPost::find_by_id(post_id)
        .one(&db)
        .await?
        .ok_or(SameyError::NotFound)?;

    let can_edit = match auth_session.user {
        None => false,
        Some(user) => user.is_admin || post.uploader_id == user.id,
    };

    if !post.is_public && !can_edit {
        return Err(SameyError::NotFound);
    }

    Ok(Html(GetFullMediaTemplate { post }.render()?))
}

pub(crate) async fn delete_post(
    State(AppState { db, files_dir }): State<AppState>,
    auth_session: AuthSession,
    Path(post_id): Path<i32>,
) -> Result<impl IntoResponse, SameyError> {
    let post = SameyPost::find_by_id(post_id)
        .one(&db)
        .await?
        .ok_or(SameyError::NotFound)?;

    match auth_session.user {
        None => return Err(SameyError::Forbidden),
        Some(user) => {
            if !user.is_admin && post.uploader_id != user.id {
                return Err(SameyError::Forbidden);
            }
        }
    }

    SameyPost::delete_by_id(post.id).exec(&db).await?;

    tokio::spawn(async move {
        let base_path = std::path::Path::new(files_dir.as_ref());
        let _ = std::fs::remove_file(base_path.join(post.media));
        let _ = std::fs::remove_file(base_path.join(post.thumbnail));
    });

    Ok(Redirect::to("/"))
}
