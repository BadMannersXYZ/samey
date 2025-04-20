use std::{
    any::Any,
    collections::{HashMap, HashSet},
    fs::OpenOptions,
    io::{BufReader, Seek, Write},
    mem,
    num::NonZero,
    str::FromStr,
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
use migration::{Expr, OnConflict, Query as MigrationQuery};
use rand::Rng;
use sea_orm::{
    ActiveValue::Set, ColumnTrait, Condition, EntityTrait, FromQueryResult, IntoSimpleExpr,
    ModelTrait, PaginatorTrait, QueryFilter, QuerySelect,
};
use serde::Deserialize;
use tokio::{task::spawn_blocking, try_join};

use crate::{
    AppState, NEGATIVE_PREFIX, RATING_PREFIX,
    auth::{AuthSession, Credentials, User},
    config::{AGE_CONFIRMATION_KEY, APPLICATION_NAME_KEY, BASE_URL_KEY},
    entities::{
        prelude::{
            SameyConfig, SameyPool, SameyPoolPost, SameyPost, SameyPostSource, SameyTag,
            SameyTagPost,
        },
        samey_config, samey_pool, samey_pool_post, samey_post, samey_post_source, samey_tag,
        samey_tag_post,
    },
    error::SameyError,
    query::{
        PoolPost, PostOverview, PostPoolData, filter_posts_by_user, get_pool_data_for_post,
        get_posts_in_pool, get_tags_for_post, search_posts,
    },
    video::{generate_thumbnail, get_dimensions_for_video},
};

const MAX_THUMBNAIL_DIMENSION: u32 = 192;

// Filters

mod filters {
    pub(crate) fn markdown(
        s: impl std::fmt::Display,
    ) -> askama::Result<askama::filters::Safe<String>> {
        let s = s.to_string();
        let parser = pulldown_cmark::Parser::new(&s);
        let mut output = String::new();
        pulldown_cmark::html::push_html(&mut output, parser);
        Ok(askama::filters::Safe(output))
    }
}

// Index view

#[derive(Template)]
#[template(path = "pages/index.html")]
struct IndexTemplate {
    application_name: String,
    age_confirmation: bool,
    user: Option<User>,
}

pub(crate) async fn index(
    State(AppState { app_config, .. }): State<AppState>,
    auth_session: AuthSession,
) -> Result<impl IntoResponse, SameyError> {
    let app_config = app_config.read().await;
    let application_name = app_config.application_name.clone();
    let age_confirmation = app_config.age_confirmation;
    drop(app_config);
    Ok(Html(
        IndexTemplate {
            application_name,
            age_confirmation,
            user: auth_session.user,
        }
        .render()?,
    ))
}

// RSS view

#[derive(Template)]
#[template(path = "fragments/rss_entry.html")]
struct RssEntryTemplate<'a> {
    post: PostOverview,
    base_url: &'a str,
}

#[axum::debug_handler]
pub(crate) async fn rss_page(
    State(AppState { app_config, db, .. }): State<AppState>,
    Query(query): Query<PostsQuery>,
) -> Result<impl IntoResponse, SameyError> {
    let app_config = app_config.read().await;
    let application_name = app_config.application_name.clone();
    let base_url = app_config.base_url.clone();
    drop(app_config);

    let tags = query
        .tags
        .as_ref()
        .map(|tags| tags.split_whitespace().collect::<Vec<_>>());

    let posts = search_posts(tags.as_ref(), None)
        .paginate(&db, 20)
        .fetch_page(0)
        .await?;

    let channel = rss::ChannelBuilder::default()
        .title(&application_name)
        .link(&base_url)
        .items(
            posts
                .into_iter()
                .map(|post| {
                    rss::ItemBuilder::default()
                        .title(post.tags.clone())
                        .pub_date(post.uploaded_at.and_utc().to_rfc2822())
                        .link(format!("{}/post/{}", &base_url, post.id))
                        .content(
                            RssEntryTemplate {
                                post,
                                base_url: &base_url,
                            }
                            .render()
                            .ok(),
                        )
                        .build()
                })
                .collect_vec(),
        )
        .build();

    Ok(channel.to_string())
}

// Auth views

#[derive(Template)]
#[template(path = "pages/login.html")]
struct LoginPageTemplate {
    application_name: String,
    age_confirmation: bool,
}

pub(crate) async fn login_page(
    State(AppState { app_config, .. }): State<AppState>,
    auth_session: AuthSession,
) -> Result<impl IntoResponse, SameyError> {
    if auth_session.user.is_some() {
        return Ok(Redirect::to("/").into_response());
    }

    let app_config = app_config.read().await;
    let application_name = app_config.application_name.clone();
    let age_confirmation = app_config.age_confirmation;
    drop(app_config);

    Ok(Html(
        LoginPageTemplate {
            application_name,
            age_confirmation,
        }
        .render()?,
    )
    .into_response())
}

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

// Post upload views

#[derive(Template)]
#[template(path = "pages/upload.html")]
struct UploadPageTemplate {
    application_name: String,
    age_confirmation: bool,
}

pub(crate) async fn upload_page(
    State(AppState { app_config, .. }): State<AppState>,
    auth_session: AuthSession,
) -> Result<impl IntoResponse, SameyError> {
    if auth_session.user.is_none() {
        return Err(SameyError::Forbidden);
    }

    let app_config = app_config.read().await;
    let application_name = app_config.application_name.clone();
    let age_confirmation = app_config.age_confirmation;
    drop(app_config);

    Ok(Html(
        UploadPageTemplate {
            application_name,
            age_confirmation,
        }
        .render()?,
    )
    .into_response())
}

enum Format {
    Video(&'static str),
    Image(ImageFormat),
}

impl Format {
    fn media_type(&self) -> &'static str {
        match self {
            Format::Video(_) => "video",
            Format::Image(_) => "image",
        }
    }
}

impl FromStr for Format {
    type Err = SameyError;

    fn from_str(content_type: &str) -> Result<Self, Self::Err> {
        match content_type {
            "video/mp4" => Ok(Self::Video(".mp4")),
            "video/webm" => Ok(Self::Video(".webm")),
            "application/x-matroska" | "video/mastroska" => Ok(Self::Video(".mkv")),
            "video/quicktime" => Ok(Self::Video(".mov")),
            _ => Ok(Self::Image(
                ImageFormat::from_mime_type(content_type).ok_or(SameyError::Other(format!(
                    "Unknown content type: {}",
                    content_type
                )))?,
            )),
        }
    }
}

pub(crate) async fn upload(
    State(AppState { db, files_dir, .. }): State<AppState>,
    auth_session: AuthSession,
    mut multipart: Multipart,
) -> Result<impl IntoResponse, SameyError> {
    let user = match auth_session.user {
        Some(user) => user,
        None => return Err(SameyError::Forbidden),
    };

    let mut upload_tags: Option<Vec<samey_tag::Model>> = None;
    let mut source_file: Option<String> = None;
    let mut media_type: Option<&'static str> = None;
    let mut width: Option<NonZero<i32>> = None;
    let mut height: Option<NonZero<i32>> = None;
    let mut thumbnail_file: Option<String> = None;
    let mut thumbnail_width: Option<NonZero<i32>> = None;
    let mut thumbnail_height: Option<NonZero<i32>> = None;
    let base_path = files_dir.as_ref();

    // Read multipart form data
    while let Some(mut field) = multipart.next_field().await.unwrap() {
        match field.name().unwrap() {
            "tags" => {
                if let Ok(tags) = field.text().await {
                    let tags: HashSet<String> = tags
                        .split_whitespace()
                        .filter_map(|tag| {
                            if tag.starts_with(NEGATIVE_PREFIX) || tag.starts_with(RATING_PREFIX) {
                                None
                            } else {
                                Some(String::from(tag))
                            }
                        })
                        .collect();
                    let normalized_tags: HashSet<String> =
                        tags.iter().map(|tag| tag.to_lowercase()).collect();
                    if tags.is_empty() {
                        upload_tags = Some(vec![]);
                    } else {
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
            }

            "media-file" => {
                let content_type = field
                    .content_type()
                    .ok_or(SameyError::Other("Missing content type".into()))?;
                match Format::from_str(content_type)? {
                    format @ Format::Video(video_format) => {
                        media_type = Some(format.media_type());
                        let thumbnail_format = ImageFormat::Png;
                        let (file_name, thumbnail_file_name) = {
                            let mut rng = rand::rng();
                            let mut file_name: String = (0..8)
                                .map(|_| rng.sample(rand::distr::Alphanumeric) as char)
                                .collect();
                            let thumbnail_file_name = format!(
                                "thumb-{}.{}",
                                file_name,
                                thumbnail_format.extensions_str()[0]
                            );
                            file_name.push_str(video_format);
                            (file_name, thumbnail_file_name)
                        };
                        let file_path = base_path.join(&file_name);
                        let mut file = OpenOptions::new()
                            .read(true)
                            .write(true)
                            .create(true)
                            .truncate(true)
                            .open(&file_path)?;
                        while let Some(chunk) = field.chunk().await? {
                            file.write_all(&chunk)?;
                        }
                        let file_path_2 = file_path.to_string_lossy().into_owned();
                        let thumbnail_path = base_path.join(&thumbnail_file_name);
                        let jh_thumbnail = spawn_blocking(move || {
                            generate_thumbnail(
                                &file_path_2,
                                &thumbnail_path.to_string_lossy(),
                                MAX_THUMBNAIL_DIMENSION,
                            )?;
                            let mut image = ImageReader::new(BufReader::new(
                                OpenOptions::new().read(true).open(thumbnail_path)?,
                            ));
                            image.set_format(thumbnail_format);
                            Ok(image.into_dimensions()?)
                        });
                        let file_path_2 = file_path.to_string_lossy().into_owned();
                        let jh_video =
                            spawn_blocking(move || get_dimensions_for_video(&file_path_2));
                        let (dim_thumbnail, dim_video) = match try_join!(jh_thumbnail, jh_video)? {
                            (Ok(dim_thumbnail), Ok(dim_video)) => (dim_thumbnail, dim_video),
                            (Err(err), _) | (_, Err(err)) => return Err(err),
                        };
                        width = NonZero::new(dim_video.0.try_into()?);
                        height = NonZero::new(dim_video.1.try_into()?);
                        thumbnail_width = NonZero::new(dim_thumbnail.0.try_into()?);
                        thumbnail_height = NonZero::new(dim_thumbnail.1.try_into()?);
                        source_file = Some(file_name);
                        thumbnail_file = Some(thumbnail_file_name);
                    }

                    format @ Format::Image(image_format) => {
                        media_type = Some(format.media_type());
                        let file_name = {
                            let mut rng = rand::rng();
                            let mut file_name: String = (0..8)
                                .map(|_| rng.sample(rand::distr::Alphanumeric) as char)
                                .collect();
                            file_name.push('.');
                            file_name.push_str(image_format.extensions_str()[0]);
                            file_name
                        };
                        let thumbnail_file_name = format!("thumb-{}", file_name);
                        let file_path = base_path.join(&file_name);
                        let mut file = OpenOptions::new()
                            .read(true)
                            .write(true)
                            .create(true)
                            .truncate(true)
                            .open(&file_path)?;
                        while let Some(chunk) = field.chunk().await? {
                            file.write_all(&chunk)?;
                        }
                        let base_path_2 = base_path.to_owned();
                        let thumbnail_path = base_path_2.join(&thumbnail_file_name);
                        let (w, h, tw, th) = spawn_blocking(move || -> Result<_, SameyError> {
                            file.seek(std::io::SeekFrom::Start(0))?;
                            let mut image = ImageReader::new(BufReader::new(file));
                            image.set_format(image_format);
                            let image = image.decode()?;
                            let (w, h) = image.dimensions();
                            let width = NonZero::new(w.try_into()?);
                            let height = NonZero::new(h.try_into()?);
                            let thumbnail = image.resize(
                                MAX_THUMBNAIL_DIMENSION,
                                MAX_THUMBNAIL_DIMENSION,
                                image::imageops::FilterType::CatmullRom,
                            );
                            thumbnail.save(thumbnail_path)?;
                            let (tw, th) = image.dimensions();
                            let thumbnail_width = NonZero::new(tw.try_into()?);
                            let thumbnail_height = NonZero::new(th.try_into()?);
                            Ok((width, height, thumbnail_width, thumbnail_height))
                        })
                        .await??;
                        width = w;
                        height = h;
                        thumbnail_width = tw;
                        thumbnail_height = th;
                        source_file = Some(file_name);
                        thumbnail_file = Some(thumbnail_file_name);
                    }
                }
            }
            _ => (),
        }
    }

    if let (
        Some(upload_tags),
        Some(source_file),
        Some(media_type),
        Some(thumbnail_file),
        Some(width),
        Some(height),
        Some(thumbnail_width),
        Some(thumbnail_height),
    ) = (
        upload_tags,
        source_file,
        media_type,
        thumbnail_file,
        width.map(|w| w.get()),
        height.map(|h| h.get()),
        thumbnail_width.map(|w| w.get()),
        thumbnail_height.map(|h| h.get()),
    ) {
        let uploaded_post = SameyPost::insert(samey_post::ActiveModel {
            uploader_id: Set(user.id),
            media: Set(source_file),
            media_type: Set(media_type.into()),
            width: Set(width),
            height: Set(height),
            thumbnail: Set(thumbnail_file),
            thumbnail_width: Set(thumbnail_width),
            thumbnail_height: Set(thumbnail_height),
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
        if !upload_tags.is_empty() {
            SameyTagPost::insert_many(upload_tags.into_iter().map(|tag| {
                samey_tag_post::ActiveModel {
                    post_id: Set(uploaded_post),
                    tag_id: Set(tag.id),
                    ..Default::default()
                }
            }))
            .exec(&db)
            .await?;
        }

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
#[template(path = "fragments/search_tags.html")]
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
    let tags = match body.tags[..body.selection_end].split(' ').next_back() {
        Some(mut tag) => {
            tag = tag.trim();
            if tag.is_empty() {
                vec![]
            } else if let Some(stripped_tag) = tag.strip_prefix(NEGATIVE_PREFIX) {
                if stripped_tag.starts_with(RATING_PREFIX) {
                    [
                        format!("{}u", RATING_PREFIX),
                        format!("{}s", RATING_PREFIX),
                        format!("{}q", RATING_PREFIX),
                        format!("{}e", RATING_PREFIX),
                    ]
                    .into_iter()
                    .filter(|t| t.starts_with(stripped_tag))
                    .map(|tag| SearchTag {
                        value: format!("-{}", &tag),
                        name: tag,
                    })
                    .collect()
                } else {
                    SameyTag::find()
                        .filter(Expr::cust_with_expr(
                            "LOWER(\"samey_tag\".\"name\") LIKE CONCAT(?, '%')",
                            stripped_tag.to_lowercase(),
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
#[template(path = "fragments/select_tag.html")]
struct SelectTagTemplate {
    tags_value: String,
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
        if !tag.is_empty() {
            if !tags.is_empty() {
                tags.push(' ');
            }
            tags.push_str(tag);
        }
    }
    if !tags.is_empty() {
        tags.push(' ');
    }
    tags.push_str(&body.new_tag);
    for tag in body.tags[body.selection_end..].split(' ') {
        if !tag.is_empty() {
            tags.push(' ');
            tags.push_str(tag);
        }
    }
    tags.push(' ');
    Ok(Html(SelectTagTemplate { tags_value: tags }.render()?))
}

// Post list views

#[derive(Template)]
#[template(path = "pages/posts.html")]
struct PostsTemplate<'a> {
    application_name: String,
    age_confirmation: bool,
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
    State(AppState { db, app_config, .. }): State<AppState>,
    auth_session: AuthSession,
    Query(query): Query<PostsQuery>,
    Path(page): Path<u32>,
) -> Result<impl IntoResponse, SameyError> {
    let app_config = app_config.read().await;
    let application_name = app_config.application_name.clone();
    let age_confirmation = app_config.age_confirmation;
    drop(app_config);
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
            let tags: Option<String> = post.tags.map(|tags| {
                let mut tags_vec = tags.split_ascii_whitespace().collect::<Vec<&str>>();
                tags_vec.sort();
                tags_vec.into_iter().join(" ")
            });
            PostOverview { tags, ..post }
        })
        .collect();

    Ok(Html(
        PostsTemplate {
            application_name,
            age_confirmation,
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

#[derive(Template)]
#[template(path = "pages/create_pool.html")]
struct CreatePoolPageTemplate {
    application_name: String,
    age_confirmation: bool,
}

pub(crate) async fn create_pool_page(
    State(AppState { app_config, .. }): State<AppState>,
    auth_session: AuthSession,
) -> Result<impl IntoResponse, SameyError> {
    if auth_session.user.is_none() {
        return Err(SameyError::Forbidden);
    }

    let app_config = app_config.read().await;
    let application_name = app_config.application_name.clone();
    let age_confirmation = app_config.age_confirmation;
    drop(app_config);

    Ok(Html(
        CreatePoolPageTemplate {
            application_name,
            age_confirmation,
        }
        .render()?,
    )
    .into_response())
}

pub(crate) async fn get_pools(
    state: State<AppState>,
    auth_session: AuthSession,
) -> Result<impl IntoResponse, SameyError> {
    get_pools_page(state, auth_session, Path(1)).await
}

#[derive(Template)]
#[template(path = "pages/pools.html")]
struct GetPoolsTemplate {
    application_name: String,
    age_confirmation: bool,
    pools: Vec<samey_pool::Model>,
    page: u32,
    page_count: u64,
}

pub(crate) async fn get_pools_page(
    State(AppState { db, app_config, .. }): State<AppState>,
    auth_session: AuthSession,
    Path(page): Path<u32>,
) -> Result<impl IntoResponse, SameyError> {
    let app_config = app_config.read().await;
    let application_name = app_config.application_name.clone();
    let age_confirmation = app_config.age_confirmation;
    drop(app_config);
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
            application_name,
            age_confirmation,
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
#[template(path = "pages/pool.html")]
struct ViewPoolTemplate {
    application_name: String,
    age_confirmation: bool,
    pool: samey_pool::Model,
    posts: Vec<PoolPost>,
    can_edit: bool,
}

pub(crate) async fn view_pool(
    State(AppState { db, app_config, .. }): State<AppState>,
    auth_session: AuthSession,
    Path(pool_id): Path<i32>,
) -> Result<impl IntoResponse, SameyError> {
    let app_config = app_config.read().await;
    let application_name = app_config.application_name.clone();
    let age_confirmation = app_config.age_confirmation;
    drop(app_config);
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
            application_name,
            age_confirmation,
            pool,
            can_edit,
            posts,
        }
        .render()?,
    ))
}

#[derive(Debug, Deserialize)]
pub(crate) struct ChangePoolNameForm {
    pool_name: String,
}

#[derive(Template)]
#[template(path = "fragments/change_pool_name.html")]
struct ChangePoolNameTemplate {
    pool_name: String,
}

pub(crate) async fn change_pool_name(
    State(AppState { db, .. }): State<AppState>,
    auth_session: AuthSession,
    Path(pool_id): Path<i32>,
    Form(body): Form<ChangePoolNameForm>,
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

    if body.pool_name.trim().is_empty() {
        return Err(SameyError::BadRequest("Pool name cannot be empty".into()));
    }

    SameyPool::update(samey_pool::ActiveModel {
        id: Set(pool.id),
        name: Set(body.pool_name.clone()),
        ..Default::default()
    })
    .exec(&db)
    .await?;

    Ok(Html(
        ChangePoolNameTemplate {
            pool_name: body.pool_name,
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
        .ok_or(SameyError::NotFound)?;

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
struct PoolWithMaxPosition {
    id: i32,
    uploader_id: i32,
    max_position: Option<f32>,
}

#[derive(Template)]
#[template(path = "fragments/add_post_to_pool.html")]
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

    let post = filter_posts_by_user(
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
#[template(path = "fragments/pool_posts.html")]
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
        } else if body.new_index < body.old_index {
            Some(body.new_index)
        } else {
            Some(body.new_index + 1)
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

pub(crate) async fn delete_pool(
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

    if !can_edit {
        return Err(SameyError::Forbidden);
    }

    SameyPool::delete_by_id(pool_id).exec(&db).await?;

    Ok(Redirect::to("/"))
}

// Bulk edit tag views

enum BulkEditTagMessage {
    None,
    Success,
    Failure(String),
}

#[derive(Template)]
#[template(path = "pages/bulk_edit_tag.html")]
struct BulkEditTagTemplate {
    application_name: String,
    age_confirmation: bool,
    message: BulkEditTagMessage,
}

pub(crate) async fn bulk_edit_tag(
    State(AppState { app_config, .. }): State<AppState>,
    auth_session: AuthSession,
) -> Result<impl IntoResponse, SameyError> {
    if auth_session.user.is_none_or(|user| !user.is_admin) {
        return Err(SameyError::Forbidden);
    }

    let app_config = app_config.read().await;
    let application_name = app_config.application_name.clone();
    let age_confirmation = app_config.age_confirmation;
    drop(app_config);

    Ok(Html(
        BulkEditTagTemplate {
            application_name,
            age_confirmation,
            message: BulkEditTagMessage::None,
        }
        .render()?,
    ))
}

#[derive(Debug, Deserialize)]
pub(crate) struct EditTagForm {
    tags: String,
    new_tag: String,
}

pub(crate) async fn edit_tag(
    State(AppState { db, app_config, .. }): State<AppState>,
    auth_session: AuthSession,
    Form(body): Form<EditTagForm>,
) -> Result<impl IntoResponse, SameyError> {
    if auth_session.user.is_none_or(|user| !user.is_admin) {
        return Err(SameyError::Forbidden);
    }

    let app_config = app_config.read().await;
    let application_name = app_config.application_name.clone();
    let age_confirmation = app_config.age_confirmation;
    drop(app_config);

    let old_tag: Vec<_> = body.tags.split_whitespace().collect();
    if old_tag.len() != 1 {
        return Ok(Html(
            BulkEditTagTemplate {
                application_name,
                age_confirmation,
                message: BulkEditTagMessage::Failure("expected single tag to edit".into()),
            }
            .render()?,
        ));
    }
    let old_tag = old_tag.first().unwrap();
    let normalized_old_tag = old_tag.to_lowercase();

    let new_tag: Vec<_> = body.new_tag.split_whitespace().collect();
    if new_tag.len() != 1 {
        return Ok(Html(
            BulkEditTagTemplate {
                application_name,
                age_confirmation,
                message: BulkEditTagMessage::Failure("expected single new tag".into()),
            }
            .render()?,
        ));
    }
    let new_tag = new_tag.first().unwrap();
    let normalized_new_tag = new_tag.to_lowercase();

    let old_tag_db = SameyTag::find()
        .filter(samey_tag::Column::NormalizedName.eq(&normalized_old_tag))
        .one(&db)
        .await?
        .ok_or(SameyError::NotFound)?;

    if let Some(new_tag_db) = SameyTag::find()
        .filter(samey_tag::Column::NormalizedName.eq(&normalized_new_tag))
        .one(&db)
        .await?
    {
        let subquery = MigrationQuery::select()
            .column((SameyTagPost, samey_tag_post::Column::PostId))
            .from(SameyTagPost)
            .and_where(samey_tag_post::Column::TagId.eq(new_tag_db.id))
            .to_owned();
        SameyTagPost::update_many()
            .filter(samey_tag_post::Column::TagId.eq(old_tag_db.id))
            .filter(samey_tag_post::Column::PostId.not_in_subquery(subquery))
            .set(samey_tag_post::ActiveModel {
                tag_id: Set(new_tag_db.id),
                ..Default::default()
            })
            .exec(&db)
            .await?;
        SameyTag::delete_by_id(old_tag_db.id).exec(&db).await?;
    } else {
        SameyTag::update(samey_tag::ActiveModel {
            id: Set(old_tag_db.id),
            name: Set(new_tag.to_string()),
            normalized_name: Set(normalized_new_tag),
        })
        .exec(&db)
        .await?;
    }

    Ok(Html(
        BulkEditTagTemplate {
            application_name,
            age_confirmation,
            message: BulkEditTagMessage::Success,
        }
        .render()?,
    ))
}

// Settings views

#[derive(Template)]
#[template(path = "pages/settings.html")]
struct SettingsTemplate {
    application_name: String,
    base_url: String,
    age_confirmation: bool,
}

pub(crate) async fn settings(
    State(AppState { db, app_config, .. }): State<AppState>,
    auth_session: AuthSession,
) -> Result<impl IntoResponse, SameyError> {
    if auth_session.user.is_none_or(|user| !user.is_admin) {
        return Err(SameyError::Forbidden);
    }

    let app_config = app_config.read().await;
    let application_name = app_config.application_name.clone();
    let base_url = app_config.base_url.clone();
    let age_confirmation = app_config.age_confirmation;
    drop(app_config);

    let config = SameyConfig::find().all(&db).await?;

    let values: HashMap<&str, Box<dyn Any>> = config
        .iter()
        .filter_map(|row| match row.key.as_str() {
            key if key == APPLICATION_NAME_KEY => row
                .data
                .as_str()
                .map::<(&str, Box<dyn Any>), _>(|data| (&row.key, Box::new(data.to_owned()))),
            _ => None,
        })
        .collect();

    Ok(Html(
        SettingsTemplate {
            application_name,
            base_url,
            age_confirmation,
        }
        .render_with_values(&values)?,
    ))
}

#[derive(Debug, Deserialize)]
pub(crate) struct UpdateSettingsForm {
    application_name: String,
    base_url: String,
    favicon_post_id: String,
    age_confirmation: Option<bool>,
}

pub(crate) async fn update_settings(
    State(AppState {
        db,
        app_config,
        files_dir,
        ..
    }): State<AppState>,
    auth_session: AuthSession,
    Form(body): Form<UpdateSettingsForm>,
) -> Result<impl IntoResponse, SameyError> {
    if auth_session.user.is_none_or(|user| !user.is_admin) {
        return Err(SameyError::Forbidden);
    }

    let mut configs = vec![];

    if !body.application_name.is_empty() {
        let _ = mem::replace(
            &mut app_config.write().await.application_name,
            body.application_name.clone(),
        );
        configs.push(samey_config::ActiveModel {
            key: Set(APPLICATION_NAME_KEY.into()),
            data: Set(body.application_name.into()),
            ..Default::default()
        });
    }

    let _ = mem::replace(
        &mut app_config.write().await.base_url,
        body.base_url.clone(),
    );
    configs.push(samey_config::ActiveModel {
        key: Set(BASE_URL_KEY.into()),
        data: Set(body.base_url.into()),
        ..Default::default()
    });

    let age_confirmation = body.age_confirmation.is_some();
    let _ = mem::replace(
        &mut app_config.write().await.age_confirmation,
        age_confirmation,
    );
    configs.push(samey_config::ActiveModel {
        key: Set(AGE_CONFIRMATION_KEY.into()),
        data: Set(age_confirmation.into()),
        ..Default::default()
    });

    if !configs.is_empty() {
        SameyConfig::insert_many(configs)
            .on_conflict(
                OnConflict::column(samey_config::Column::Key)
                    .update_column(samey_config::Column::Data)
                    .to_owned(),
            )
            .exec(&db)
            .await?;
    }

    if let Some(favicon_post_id) = body.favicon_post_id.split_whitespace().next() {
        match favicon_post_id.parse::<i32>() {
            Ok(favicon_post_id) => {
                let post = SameyPost::find_by_id(favicon_post_id)
                    .one(&db)
                    .await?
                    .ok_or(SameyError::NotFound)?;
                ImageReader::open(files_dir.join(post.thumbnail))?
                    .decode()?
                    .save_with_format(files_dir.join("favicon.png"), ImageFormat::Png)?;
            }
            Err(err) => return Err(SameyError::IntParse(err)),
        }
    }

    Ok(Redirect::to("/"))
}

// Single post views

#[derive(Template)]
#[template(path = "pages/view_post.html")]
struct ViewPostPageTemplate {
    application_name: String,
    age_confirmation: bool,
    post: samey_post::Model,
    pool_data: Vec<PostPoolData>,
    tags: Vec<samey_tag::Model>,
    tags_text: Option<String>,
    tags_post: String,
    sources: Vec<samey_post_source::Model>,
    can_edit: bool,
    parent_post: Option<PostOverview>,
    children_posts: Vec<PostOverview>,
}

pub(crate) async fn view_post_page(
    State(AppState { db, app_config, .. }): State<AppState>,
    auth_session: AuthSession,
    Query(query): Query<PostsQuery>,
    Path(post_id): Path<i32>,
) -> Result<impl IntoResponse, SameyError> {
    let app_config = app_config.read().await;
    let application_name = app_config.application_name.clone();
    let age_confirmation = app_config.age_confirmation;
    drop(app_config);

    let post = SameyPost::find_by_id(post_id)
        .one(&db)
        .await?
        .ok_or(SameyError::NotFound)?;

    let can_edit = match auth_session.user.as_ref() {
        None => false,
        Some(user) => user.is_admin || post.uploader_id == user.id,
    };

    if !post.is_public && !can_edit {
        return Err(SameyError::NotFound);
    }

    let tags = get_tags_for_post(post_id).all(&db).await?;
    let tags_post = tags.iter().map(|tag| &tag.name).join(" ");

    let sources = SameyPostSource::find()
        .filter(samey_post_source::Column::PostId.eq(post_id))
        .all(&db)
        .await?;

    let parent_post = if let Some(parent_id) = post.parent_id {
        match filter_posts_by_user(SameyPost::find_by_id(parent_id), auth_session.user.as_ref())
            .one(&db)
            .await?
        {
            Some(parent_post) => Some(PostOverview {
                id: parent_id,
                thumbnail: parent_post.thumbnail,
                title: parent_post.title,
                description: parent_post.description,
                uploaded_at: parent_post.uploaded_at,
                media: parent_post.media,
                tags: Some(
                    get_tags_for_post(post_id)
                        .all(&db)
                        .await?
                        .iter()
                        .map(|tag| &tag.name)
                        .join(" "),
                ),
                rating: parent_post.rating,
                media_type: parent_post.media_type,
            }),
            None => None,
        }
    } else {
        None
    };

    let children_posts_models = filter_posts_by_user(
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
            title: child_post.title,
            description: child_post.description,
            uploaded_at: child_post.uploaded_at,
            media: child_post.media,
            tags: Some(
                get_tags_for_post(child_post.id)
                    .all(&db)
                    .await?
                    .iter()
                    .map(|tag| &tag.name)
                    .join(" "),
            ),
            rating: child_post.rating,
            media_type: child_post.media_type,
        });
    }

    let pool_data = get_pool_data_for_post(&db, post_id, auth_session.user.as_ref()).await?;

    Ok(Html(
        ViewPostPageTemplate {
            application_name,
            age_confirmation,
            post,
            pool_data,
            tags,
            tags_text: query.tags,
            tags_post,
            sources,
            can_edit,
            parent_post,
            children_posts,
        }
        .render()?,
    ))
}

#[derive(Template)]
#[template(path = "fragments/post_details.html")]
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
#[template(path = "fragments/submit_post_details.html")]
struct SubmitPostDetailsTemplate {
    post: samey_post::Model,
    parent_post: Option<PostOverview>,
    sources: Vec<samey_post_source::Model>,
    tags: Vec<samey_tag::Model>,
    tags_text: String,
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
        "" => None,
        title => Some(title.to_owned()),
    };
    let description = match body.description.trim() {
        "" => None,
        description => Some(description.to_owned()),
    };
    let parent_post = if let Ok(parent_id) = body.parent_post.trim().parse() {
        match filter_posts_by_user(SameyPost::find_by_id(parent_id), auth_session.user.as_ref())
            .one(&db)
            .await?
        {
            Some(parent_post) => Some(PostOverview {
                id: parent_id,
                thumbnail: parent_post.thumbnail,
                title: parent_post.title,
                description: parent_post.description,
                uploaded_at: parent_post.uploaded_at,
                media: parent_post.media,
                tags: Some(
                    get_tags_for_post(post_id)
                        .all(&db)
                        .await?
                        .iter()
                        .map(|tag| &tag.name)
                        .join(" "),
                ),
                rating: parent_post.rating,
                media_type: parent_post.media_type,
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
    let tags = if tags.is_empty() {
        vec![]
    } else {
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
        upload_tags
    };
    let mut tags_text = String::new();
    for tag in &tags {
        if !tags_text.is_empty() {
            tags_text.push(' ');
        }
        tags_text.push_str(&tag.name);
    }

    let sources = SameyPostSource::find()
        .filter(samey_post_source::Column::PostId.eq(post_id))
        .all(&db)
        .await?;

    Ok(Html(
        SubmitPostDetailsTemplate {
            post,
            sources,
            tags,
            tags_text,
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
#[template(path = "fragments/edit_post_details.html")]
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
#[template(path = "fragments/post_source.html")]
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

pub(crate) async fn delete_post(
    State(AppState { db, files_dir, .. }): State<AppState>,
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
        let _ = std::fs::remove_file(files_dir.join(post.media));
        let _ = std::fs::remove_file(files_dir.join(post.thumbnail));
    });

    Ok(Redirect::to("/"))
}
