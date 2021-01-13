use err_derive::Error;
use actix_web::client::{PayloadError, SendRequestError};


#[derive(Error, Debug)]
pub enum DocumentParseError {
    #[error(display = "fail to select `{}`", path)]
    Select { path: &'static str },
    #[error(display = "fail to parse `{}`", path)]
    NumberParse { path: &'static str },
    #[error(display = "fail to parse `{}`", path)]
    DatetimeParse { path: &'static str },
    #[error(display = "fail to parse" )]
    JsonParse(#[source] serde_json::Error),
}

#[derive(Error, Debug)]
pub enum CommentParseError {
    #[error(display = "fail to select `{}`", path)]
    Select { path: &'static str },
    #[error(display = "fail to parse `{}`", path)]
    NumberParse { path: &'static str },
    #[error(display = "fail to parse `{}`", path)]
    DatetimeParse { path: &'static str },
    #[error(display = "fail to parse at {}.{} due to {}. body: {}", gallery_id, doc_id, source, target )]
    JsonParse{
        source: serde_json::Error,
        target: String,
        doc_id: usize, 
        gallery_id: String,
    },
}
#[derive(Error, Debug)]
pub enum DocumentBodyParseError {
    #[error(display = "fail to select `{}`", path)]
    Select { path: &'static str },
    #[error(display = "fail to parse page")]
    DocumentParseError(#[source] DocumentParseError),
}

#[derive(Error, Debug)]
pub enum CrawlerError {
    #[error(display = "actix client send")]
    SendRequest(#[source] SendRequestError),
    #[error(display = "acitx client payload")]
    Payload(#[source] PayloadError),
    #[error(display = "serde")]
    Serde(#[source] serde_json::Error),
    #[error(display = "fmt")]
    Fmt(#[source] core::fmt::Error),
    #[error(display = "utf8")]
    Utf8(#[source] std::str::Utf8Error),
    #[error(display = "fail to parse root page")]
    DocumentParseError(#[source] DocumentParseError),
    #[error(display = "fail to parse comment")]
    CommentParseError(#[source] CommentParseError),
    #[error(display = "fail to parse body")]
    DocumentBodyParseError(#[source] DocumentBodyParseError),
}

#[derive(Error, Debug)]
pub enum LiveDirectoryError {
    #[error(display = "crawler error")]
    Crawler(#[source] CrawlerError),
    #[error(display = "sled")]
    Sled(#[source] sled::Error),
}