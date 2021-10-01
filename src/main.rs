use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::Arc;
use std::io::Write;

use similar::{ChangeTag, TextDiff};
use askama::Template;
use chrono::{SubsecRound, DateTime, Utc};
use clap::{App, Arg};
use comrak::plugins::syntect::SyntectAdapter;
use comrak::{
    format_html_with_plugins, parse_document, Arena, ComrakOptions, ComrakPlugins,
    ComrakRenderPlugins,
};
use hyper::server::conn::AddrStream;
use hyper::service::{make_service_fn, service_fn};
use hyper::Method;
use hyper::{header, Body, Response};
use hyper::{Request, Server, StatusCode};
use tokio::sync::RwLock;
use tokio_postgres::NoTls;
use tracing::{event, Level};
use tracing_subscriber::filter::LevelFilter as TracingLevelFilter;
use tracing_subscriber::FmtSubscriber;

const CARGO_PKG_VERSION: &str = env!("CARGO_PKG_VERSION");
const CARGO_PKG_NAME: &str = env!("CARGO_PKG_NAME");

type DynResult<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

mod routes;
pub mod views;

use self::routes::*;

struct Renderer;

impl Renderer {
    fn render(&self, markdown: &str) -> DynResult<String> {
        let arena = Arena::new();

        let mut options = ComrakOptions::default();
        options.extension.strikethrough = true;
        options.extension.footnotes = true;

        let root = parse_document(&arena, markdown, &options);
        let adapter = SyntectAdapter::new("base16-ocean.light");
        let plugins = ComrakPlugins {
            render: ComrakRenderPlugins {
                codefence_syntax_highlighter: Some(&adapter),
            },
        };

        //
        let mut html = vec![];
        format_html_with_plugins(root, &options, &mut html, &plugins)?;
        Ok(String::from_utf8(html)?)
    }
}

#[derive(Clone)]
struct Handler {
    inner: Arc<RwLock<HandlerInner>>,
}

struct HandlerInner {
    db: tokio_postgres::Client,
}

impl Handler {
    async fn serve_wiki_page(
        &self,
        req: Request<Body>,
        rw: &RouteWiki<'_>,
    ) -> DynResult<Response<Body>> {
        if req.method() == Method::GET {
            return self.serve_wiki_page_get(req, rw).await;
        }
        if req.method() == Method::PUT {
            return self.serve_wiki_page_put(req, rw).await;
        }

        let response = Response::builder()
            .header("Content-Type", "text/html; charset=utf8")
            .status(StatusCode::METHOD_NOT_ALLOWED)
            .body(Body::from("Method Not Allowed"))?;

        Ok(response)
    }

    async fn serve_wiki_page_history_get(
        &self,
        _req: Request<Body>,
        rw: &RouteWiki<'_>,
    ) -> DynResult<Response<Body>> {
        if let RouteWikiSubview::History = rw.subview {
            // nothing
        } else {
            return Err(RouteError::NotFound.into());
        }

        let locked = self.inner.read().await;
        let rows = locked
            .db
            .query(
                r#"
                    SELECT created_at, document_history.id, modified_by FROM document_history
                    INNER JOIN document ON document.id = document_history.document_id
                    WHERE document.name = $1
                    LIMIT 50
                "#,
                &[&rw.name],
            )
            .await?;

        if rows.len() == 0 {
            return Err(RouteError::NotFound.into());
        }

        let mut history_records = Vec::new();
        for row in rows {
            let document_history_id: i64 = row.try_get(1)?;
            let created_at: DateTime<Utc> = row.try_get(0)?;
            history_records.push(views::wiki::HistoryRecord {
                created_at: created_at.trunc_subsecs(0),
                document_history_id,
                created_by: row.try_get(2)?,
                link: RouteWiki::to_revision(&rw.name, document_history_id).to_owned(),
            });
        }
        let hist = views::wiki::History {
            page_title: &rw.name,
            history_records,
        };

        let response = Response::builder()
            .header("Content-Type", "text/html; charset=utf8")
            .status(StatusCode::OK)
            .body(Body::from(hist.render()?))?;

        Ok(response)
    }

    async fn serve_wiki_page_diff_get(
        &self,
        _req: Request<Body>,
        rw: &RouteWiki<'_>,
    ) -> DynResult<Response<Body>> {
        let first;
        let second;
        if let RouteWikiSubview::Diff(first_tmp, second_tmp) = rw.subview {
            first = first_tmp;
            second = second_tmp;
        } else {
            return Err(RouteError::NotFound.into());
        }

        let locked = self.inner.read().await;
        let first_row = locked
            .db
            .query_opt(
                r#"
                    SELECT created_at, document_history.id, modified_by, document_data FROM document_history
                    INNER JOIN document ON document.id = document_history.document_id
                    WHERE document.name = $1 AND document_history.id = $2
                    LIMIT 50
                "#,
                &[&rw.name, &first],
            )
            .await?
            .ok_or_else(|| RouteError::NotFound)?;

        let document_history_id = first_row.try_get(1)?;
        let created_at: DateTime<Utc> = first_row.try_get(0)?;
        let first_document: String = first_row.try_get(3)?;
        let first_spec = views::wiki::RevisionSpec {
            document_history_id,
            created_at: created_at.trunc_subsecs(0),
            created_by: first_row.try_get(2)?,
            history_link: RouteWiki::to_revision(&rw.name, document_history_id).to_owned(),
        };

        drop(first_row);

        let second_row = locked
            .db
            .query_opt(
                r#"
                    SELECT created_at, document_history.id, modified_by, document_data FROM document_history
                    INNER JOIN document ON document.id = document_history.document_id
                    WHERE document.name = $1 AND document_history.id = $2
                    LIMIT 50
                "#,
                &[&rw.name, &second],
            )
            .await?
            .ok_or_else(|| RouteError::NotFound)?;

        let document_history_id = second_row.try_get(1)?;
        let created_at: DateTime<Utc> = second_row.try_get(0)?;
        let second_document: String = second_row.try_get(3)?;
        let second_spec = views::wiki::RevisionSpec {
            document_history_id,
            created_at: created_at.trunc_subsecs(0),
            created_by: second_row.try_get(2)?,
            history_link: RouteWiki::to_revision(&rw.name, document_history_id).to_owned(),
        };

        drop(second_row);

        // #[derive(Template)]
        // #[template(path = "wiki/diff.html")]
        // struct Diff {
        //     pub page_title: &'a str,
        //     pub first: RevisionSpec,
        //     pub second: RevisionSpec,
        //     pub rendered: String,
        // }

        let mut diffed_data = Vec::new();
        writeln!(&mut diffed_data, "````diff").unwrap();
        let diff = TextDiff::from_lines(&first_document, &second_document);
        for change in diff.iter_all_changes() {
            let sign = match change.tag() {
                ChangeTag::Delete => "-",
                ChangeTag::Insert => "+",
                ChangeTag::Equal => " ",
            };
            write!(&mut diffed_data, "{}{}", sign, change).unwrap();
        }
        writeln!(&mut diffed_data, "````").unwrap();

        let diffed_data = String::from_utf8_lossy(&diffed_data);
        let diff = views::wiki::Diff {
            page_title: &rw.name,
            first: first_spec,
            second: second_spec,
            rendered: Renderer.render(&diffed_data)?,
        };

        let response = Response::builder()
            .header("Content-Type", "text/html; charset=utf8")
            .status(StatusCode::OK)
            .body(Body::from(diff.render()?))?;

        Ok(response)
    }

    async fn serve_wiki_page_get(
        &self,
        req: Request<Body>,
        rw: &RouteWiki<'_>,
    ) -> DynResult<Response<Body>> {
        event!(Level::DEBUG, "rendering {:?}", rw);
        if let RouteWikiSubview::History = rw.subview {
            return self.serve_wiki_page_history_get(req, rw).await;
        }
        if let RouteWikiSubview::Diff(..) = rw.subview {
            return self.serve_wiki_page_diff_get(req, rw).await;
        }

        let locked = self.inner.read().await;

        let row = match rw.subview {
            RouteWikiSubview::History => unreachable!(),
            RouteWikiSubview::Revision(r) => locked
                    .db
                    .query_opt(
                        r#"
                            SELECT
                                document_data,
                                document_history.created_at,
                                document_history.modified_by
                            FROM document_history
                            INNER JOIN document ON document.id = document_history.document_id
                            WHERE document.name = $1 AND document_history.id = $2
                        "#,
                        &[&rw.name, &r],
                    ).await?,
            _ => locked
                    .db
                    .query_opt(
                        r#"
                            SELECT
                                document_data,
                                document_history.created_at,
                                document_history.modified_by
                            FROM document_history
                            INNER JOIN document ON document.current_revision_id = document_history.id
                            WHERE document.name = $1
                        "#,
                        &[&rw.name],
                    ).await?
        }.ok_or_else(|| RouteError::NotFound)?;

        let document_data: String = row.try_get(0)?;
        match rw.subview {
            // #[derive(Template)]
            // #[template(path = "wiki/view.html")]
            // pub struct View<'a> {
            //     pub page_title: &'a str,
            //     pub last_modified_at: DateTime<Utc>,
            //     pub last_modified_by: String,
            //     pub history_link: Route<'static>,
            //     pub edit_link: Route<'static>,
            //     pub rendered: String,
            // }
            RouteWikiSubview::View | RouteWikiSubview::Revision(..) => {
                let last_modified_at: DateTime<Utc> = row.try_get(1)?;
                let rendered = Renderer.render(&document_data)?;
                let view = views::wiki::View {
                    page_title: &rw.name,
                    last_modified_at: last_modified_at.trunc_subsecs(0),
                    last_modified_by: row.try_get(2)?,
                    history_link: RouteWiki::to_history(&rw.name).to_owned(),
                    edit_link: RouteWiki::to_edit(&rw.name).to_owned(),
                    rendered: rendered,
                };

                let response = Response::builder()
                    .header("Content-Type", "text/html; charset=utf8")
                    .status(StatusCode::OK)
                    .body(Body::from(view.render()?))?;

                Ok(response)
            }
            RouteWikiSubview::Edit => {
                let xx = format!("<textarea>{}</textarea>", document_data);
                let response = Response::builder()
                    .header("Content-Type", "text/html; charset=utf8")
                    .status(StatusCode::OK)
                    .body(Body::from(xx))?;

                Ok(response)
            }
            RouteWikiSubview::History | RouteWikiSubview::Diff(..) => unreachable!(),
        }
    }

    async fn serve_wiki_page_put(
        &self,
        req: Request<Body>,
        rw: &RouteWiki<'_>,
        // document_data: &str,
    ) -> DynResult<Response<Body>> {
        let user_id = "Anonymous";

        let body_bytes = hyper::body::to_bytes(req).await?;
        let document_data = String::from_utf8_lossy(&body_bytes);

        let mut locked = self.inner.write().await;
        let tx = locked.db.transaction().await?;

        let now = chrono::offset::Utc::now();
        let row = tx
            .query_opt(
                r#"
                    INSERT INTO document
                    (name, last_modified) VALUES ($1, $2)
                    ON CONFLICT (name) DO UPDATE SET last_modified = EXCLUDED.last_modified
                    RETURNING id
                "#,
                &[&rw.name, &now],
            )
            .await?
            .ok_or_else(|| RouteError::NotFound)?;

        let document_id: i64 = row.try_get(0).ok().ok_or_else(|| RouteError::NotFound)?;

        let row = tx
            .query_one(
                r#"
                    INSERT INTO document_history (created_at, document_id, modified_by, document_data)
                    VALUES (NOW(), $1, $2, $3)
                    RETURNING id
                "#,
                &[&document_id, &user_id, &document_data],
            )
            .await?;

        let document_history_id: i64 = row.try_get(0).ok().ok_or_else(|| RouteError::NotFound)?;

        tx.execute(
            r#"
                INSERT INTO document
                (name, last_modified, current_revision_id) VALUES ($1, $2, $3)
                ON CONFLICT (name) DO UPDATE SET
                    current_revision_id = EXCLUDED.current_revision_id,
                    last_modified = EXCLUDED.last_modified
                RETURNING id
            "#,
            &[&rw.name, &now, &document_history_id],
        )
        .await?;

        tx.commit().await?;

        let res = Response::builder()
            .status(StatusCode::FOUND)
            .header(header::LOCATION, RouteWiki::to(&rw.name).to_string())
            .body(Body::empty())
            .expect("unable to build response");
        Ok(res)
    }

    async fn login_page(&self, _req: Request<Body>) -> DynResult<Response<Body>> {
        let res = Response::builder()
            .status(StatusCode::FOUND)
            .header(header::LOCATION, RouteWiki::to("sample_doc").to_string())
            .body(Body::empty())
            .expect("unable to build response");
        Ok(res)
    }

    async fn handle(
        &self,
        _remote_addr: SocketAddr,
        req: Request<Body>,
    ) -> DynResult<Response<Body>> {
        let decoded = decode_percents(req.uri().path())?;
        let route = Route::router(&decoded)?.to_owned();

        match route {
            Route::Root => {
                let res = Response::builder()
                    .status(StatusCode::FOUND)
                    .header(header::LOCATION, "/login")
                    .body(Body::empty())
                    .expect("unable to build response");
                Ok(res)
            }
            Route::Login => self.login_page(req).await,
            Route::Wiki(ref article) => self.serve_wiki_page(req, article).await,
        }
    }
}

fn decode_percents<'a>(string: &'a str) -> Result<std::borrow::Cow<'a, str>, std::str::Utf8Error> {
    percent_encoding::percent_decode_str(string).decode_utf8()
}

fn main() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(main2()).unwrap();
}

async fn main2() -> DynResult<()> {
    let mut my_subscriber_builder = FmtSubscriber::builder();

    let app = App::new(CARGO_PKG_NAME)
        .version(CARGO_PKG_VERSION)
        .author("Stacey Ell <software@e.staceyell.com>")
        .arg(
            Arg::with_name("v")
                .short("v")
                .multiple(true)
                .help("Sets the level of verbosity"),
        );

    let matches = app.get_matches();

    let verbosity = matches.occurrences_of("v");
    let should_print_test_logging = 4 < verbosity;

    my_subscriber_builder = my_subscriber_builder.with_max_level(match verbosity {
        0 => TracingLevelFilter::ERROR,
        1 => TracingLevelFilter::WARN,
        2 => TracingLevelFilter::INFO,
        3 => TracingLevelFilter::DEBUG,
        _ => TracingLevelFilter::TRACE,
    });

    tracing::subscriber::set_global_default(my_subscriber_builder.finish())
        .expect("setting tracing default failed");

    if should_print_test_logging {
        print_test_logging();
    }

    let db_uri = "postgresql://quassel@localhost/quassel";

    let (db_client, connection) = tokio_postgres::connect(db_uri, NoTls).await?;
    tokio::spawn(async move {
        if let Err(e) = connection.await {
            eprintln!("connection error: {}", e);
        }
    });

    let handler = Handler {
        inner: Arc::new(RwLock::new(HandlerInner { db: db_client })),
    };

    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));

    // And a MakeService to handle each connection...
    let make_service = make_service_fn(move |conn: &AddrStream| {
        let handler = handler.clone();
        let addr: SocketAddr = conn.remote_addr();

        let service = service_fn(move |req| {
            let handler = handler.clone();
            let addr = addr.clone();

            async move { handler.handle(addr, req).await }
        });

        // Return the service to hyper.
        async move { Ok::<_, Infallible>(service) }
    });

    let server = Server::bind(&addr).serve(make_service);

    // And run forever...
    if let Err(e) = server.await {
        eprintln!("server error: {}", e);
    }

    Ok(())
}

#[allow(clippy::cognitive_complexity)] // macro bug around event!()
fn print_test_logging() {
    event!(Level::TRACE, "logger initialized - trace check");
    event!(Level::DEBUG, "logger initialized - debug check");
    event!(Level::INFO, "logger initialized - info check");
    event!(Level::WARN, "logger initialized - warn check");
    event!(Level::ERROR, "logger initialized - error check");
}
