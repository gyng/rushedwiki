use askama::Template;
use chrono::offset::Utc;
use chrono::DateTime;

use crate::routes::{Route, RouteWiki};

#[derive(Template)]
#[template(path = "wiki/history.html")]
pub struct History<'a> {
    pub page_title: &'a str,
    pub history_records: Vec<HistoryRecord>,
}

impl<'a> History<'a> {
    pub fn route_view(&self) -> Route<'a> {
        RouteWiki::to(self.page_title)
    }
}

pub struct HistoryRecord {
    pub created_at: DateTime<Utc>,
    pub document_history_id: i64,
    pub created_by: String,
    pub link: Route<'static>,
}


#[derive(Template)]
#[template(path = "wiki/view.html")]
pub struct View<'a> {
    pub page_title: &'a str,
    pub last_modified_at: DateTime<Utc>,
    pub last_modified_by: String,
    pub history_link: Route<'static>,
    pub edit_link: Route<'static>,
    pub rendered: String,
}

#[derive(Template)]
#[template(path = "wiki/diff.html")]
pub struct Diff<'a> {
    pub page_title: &'a str,
    pub first: RevisionSpec,
    pub second: RevisionSpec,
    pub rendered: String,
}

pub struct RevisionSpec {
    pub document_history_id: i64,
    pub created_at: DateTime<Utc>,
    pub created_by: String,
    pub history_link: Route<'static>,
}
