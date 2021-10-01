use std::borrow::Cow;

const WIKI_PREFIX: &'static str = "/wiki/";

#[derive(Debug)]
pub enum RouteError {
    NotFound,
}

impl std::fmt::Display for RouteError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl std::error::Error for RouteError {}

pub enum Route<'a> {
    Root,
    Login,
    Wiki(RouteWiki<'a>),
}

impl<'a> std::fmt::Display for Route<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}", self.into_uri_path())
    }
}

#[derive(Debug, Clone)]
pub struct RouteWiki<'a> {
    pub name: Cow<'a, str>,
    pub subview: RouteWikiSubview,
}

#[derive(Debug, Clone, Copy)]
pub enum RouteWikiSubview {
    View,
    Edit,
    History,
    Revision(i64),
    Diff(i64, i64),
}

impl<'a> RouteWiki<'a> {
    pub fn to(name: &'a str) -> Route<'a> {
        Route::Wiki(RouteWiki {
            name: name.into(),
            subview: RouteWikiSubview::View,
        })
    }

    pub fn to_edit(name: &'a str) -> Route<'a> {
        Route::Wiki(RouteWiki {
            name: name.into(),
            subview: RouteWikiSubview::Edit,
        })
    }

    pub fn to_revision(name: &'a str, revision: i64) -> Route<'a> {
        Route::Wiki(RouteWiki {
            name: name.into(),
            subview: RouteWikiSubview::Revision(revision),
        })
    }

    pub fn to_history(name: &'a str) -> Route<'a> {
        Route::Wiki(RouteWiki {
            name: name.into(),
            subview: RouteWikiSubview::History,
        })
    }

    pub fn to_owned(&self) -> RouteWiki<'static> {
        RouteWiki {
            name: Cow::Owned(self.name[..].to_string()),
            subview: self.subview,
        }
    }
}

impl<'a> Route<'a> {
    pub fn to_owned(&self) -> Route<'static> {
        match self {
            Route::Root => Route::Root,
            Route::Login => Route::Login,
            Route::Wiki(ref s) => Route::Wiki(s.to_owned()),
        }
    }

    pub fn into_uri_path(&self) -> String {
        match self {
            Route::Root => "/".to_string(),
            Route::Login => "/login".to_string(),
            Route::Wiki(ref s) => match s.subview {
                RouteWikiSubview::View => format!("{}{}", WIKI_PREFIX, s.name),
                RouteWikiSubview::Edit => format!("{}{}/edit", WIKI_PREFIX, s.name),
                RouteWikiSubview::History => format!("{}{}/history", WIKI_PREFIX, s.name),
                RouteWikiSubview::Revision(r) => format!("{}{}/rev/{}", WIKI_PREFIX, s.name, r),
                RouteWikiSubview::Diff(a, b) => format!("{}{}/diff/{}-{}", WIKI_PREFIX, s.name, a, b),
            },
        }
    }

    pub fn router(path: &'a str) -> std::result::Result<Self, RouteError> {
        if path == "/" {
            return Ok(Route::Root);
        }

        if path == "/login" {
            return Ok(Route::Login);
        }

        if path.starts_with(WIKI_PREFIX) {
            let mut doc_paths = path[WIKI_PREFIX.len()..].split('/');
            let name = doc_paths.next().unwrap();

            match (doc_paths.next(), doc_paths.next()) {
                (Some("edit"), None) => {
                    return Ok(Route::Wiki(RouteWiki {
                        name: name.into(),
                        subview: RouteWikiSubview::Edit,
                    }));
                }
                (Some("history"), None) => {
                    return Ok(Route::Wiki(RouteWiki {
                        name: name.into(),
                        subview: RouteWikiSubview::History,
                    }));
                }
                (Some("rev"), Some(rev)) => {
                    if doc_paths.next().is_some() {
                        return Err(RouteError::NotFound);
                    }
                    let r: i64 = match rev.parse() {
                        Ok(r) => r,
                        Err(..) => return Err(RouteError::NotFound),
                    };
                    return Ok(Route::Wiki(RouteWiki {
                        name: name.into(),
                        subview: RouteWikiSubview::Revision(r),
                    }));
                }
                (Some("diff"), Some(diffrevs)) => {
                    let mut parts = diffrevs.splitn(2, '-');
                    let first = parts.next().ok_or_else(|| RouteError::NotFound)?.parse().map_err(|_| RouteError::NotFound)?;
                    let second = parts.next().ok_or_else(|| RouteError::NotFound)?.parse().map_err(|_| RouteError::NotFound)?;
                    if doc_paths.next().is_some() {
                        return Err(RouteError::NotFound);
                    }
                    return Ok(Route::Wiki(RouteWiki {
                        name: name.into(),
                        subview: RouteWikiSubview::Diff(first, second),
                    }));
                }
                (Some(_), _) => return Err(RouteError::NotFound),
                (None, _) => {
                    return Ok(Route::Wiki(RouteWiki {
                        name: name.into(),
                        subview: RouteWikiSubview::View,
                    }));
                }
            };
        }

        return Err(RouteError::NotFound);
    }
}
