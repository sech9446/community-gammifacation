
use actix_web::{get, post, web, App, HttpServer, HttpResponse, Responder, error::ResponseError, http::StatusCode};
use std::time::Duration;

use err_derive::Error;
use dcinside_crawler::error::*;

use std::convert::TryInto;


use dcinside_crawler::crawler::Crawler;
use dcinside_crawler::model::*;

use serde::Serialize;

use actix_web_prom::PrometheusMetrics;
use prometheus::{IntGauge};

use log::{info, error};

use actix_web::client::{PayloadError, SendRequestError};

#[derive(Error, Debug)]
pub enum WorkerError {
    #[error(display = "crawler error")]
    Crawler(#[source] CrawlerError),
    #[error(display = "actix client send")]
    SendRequest(#[source] SendRequestError),
    #[error(display = "acitx client payload")]
    Payload(#[source] PayloadError),
    #[error(display = "serde")]
    Serde(#[source] serde_json::Error),
}

#[derive(Serialize)]
pub struct ListPartQuery {
   part: u64,
   total: u64,
}

#[derive(Clone)]
struct State {
    crawler: Crawler,
    live_directory_url: String,
    data_broker_url: String,
    part: u64,
    total: u64,
    start_page: usize,
}

#[derive(Default)]
struct ResultMetric {
    gallery_success: usize,
    document_success: usize,
    comment_success: usize,
    gallery_error: usize,
    document_error: usize,
    comment_error: usize,
}
impl State {
    fn new(
        live_directory_url: &str, 
        data_broker_url: &str, 
        total: u64,
        part: u64) 
        -> Self { 
        State {
            crawler: Crawler::new(),
            live_directory_url: live_directory_url.to_string(),
            data_broker_url: data_broker_url.to_string(),
            total,
            part,
            start_page: 2,
        }
    }
    fn start_page(mut self, v: usize) -> Self { self.start_page = v; self }
    async fn fetch_gallery_list(&self) -> Result<Vec<GalleryState>, WorkerError> {
        let bytes = self.crawler.client.get(format!("{}/list", self.live_directory_url)).query(&ListPartQuery{ total: self.total, part: self.part }).unwrap().send().await?.body().await?;
        Ok(serde_json::from_slice(&bytes)?)
    }
    async fn report(&self, form: GalleryCrawlReportForm) -> Result<(), WorkerError> {
        self.crawler.client.post(format!("{}/list", self.live_directory_url)).send_json(&form).await?.body().await?;
        Ok(())
    }
    async fn send_data(&self, data: &Document) -> Result<(), WorkerError> {
        self.crawler.client.post(&self.data_broker_url).send_json(data).await?.body().await?;
        Ok(())
    }
    async fn run(&mut self) -> Result<ResultMetric, WorkerError> {
        let gallery_states = self.fetch_gallery_list().await?;
        let mut metric = ResultMetric::default();
        let len = gallery_states.len();
        for (i, gallery_state) in gallery_states.into_iter().enumerate() {
            info!("{}/{} start", i, len);
            let now = chrono::Utc::now();
            let res = if let Some(last_crawled_document_id) = gallery_state.last_crawled_document_id {
                self.crawler.documents_after(&gallery_state.index, last_crawled_document_id, self.start_page).await
            } else {
                self.crawler.documents(&gallery_state.index, self.start_page).await
            };
            match &res {
                Ok(res) => {
                    metric.gallery_success += 1;
                    let mut last_document_id = 0usize;
                    for r in res {
                        match r {
                            Ok(doc) => {
                                metric.document_success += 1;
                                metric.comment_success += 1;
                                if last_document_id < doc.index.id {
                                    last_document_id = doc.index.id;
                                }
                                if let Err(e) = self.send_data(doc).await {
                                    error!("error while send data: {}", e.to_string());
                                }
                            },
                            Err(CrawlerError::DocumentParseError(err)) => {
                                error!("document parse error of {}: {}", &gallery_state.index.id, err.to_string());
                                metric.document_error += 1;
                            },
                            Err(CrawlerError::CommentParseError(err)) => {
                                error!("coments parse error of {}: {}", &gallery_state.index.id, err.to_string());
                                metric.comment_error += 1;
                            },
                            Err(err) => {
                                error!("document crawl of {}: {}", &gallery_state.index.id, err.to_string());
                                metric.document_error += 1;
                            }
                        };
                    }
                    if last_document_id > 0usize {
                        if let Err(e) = self.report(GalleryCrawlReportForm {
                            id: gallery_state.index.id.clone(),
                            last_crawled_at: Some(now),
                            last_crawled_document_id: Some(last_document_id),
                        }).await {
                            error!("error while report: {}", e.to_string());
                        };
                    } else {
                        error!("no document of {}", &gallery_state.index.id);
                    }
                }
                Err(err) => {
                    error!("get index of {} fail: {}", &gallery_state.index.id, err.to_string());
                    metric.gallery_error += 1;
                }
            };
        };
        Ok(metric)
    }
}

#[derive(Clone)]
struct ResultMetricGauges {
    gallery_success: IntGauge,
    document_success: IntGauge,
    comment_success: IntGauge,
    gallery_error: IntGauge,
    document_error: IntGauge,
    comment_error: IntGauge,
}
async fn crawl_forever(mut state: State, delay: Duration, gauges: ResultMetricGauges) -> Result<(), WorkerError> {
    loop {
        let metric = state.run().await?;
        gauges.gallery_success.set(metric.gallery_success.try_into().unwrap()); 
        gauges.document_success.set(metric.document_success.try_into().unwrap()); 
        gauges.comment_success.set(metric.comment_success.try_into().unwrap()); 
        gauges.gallery_error.set(metric.gallery_error.try_into().unwrap()); 
        gauges.document_error.set(metric.document_error.try_into().unwrap()); 
        gauges.comment_error.set(metric.comment_error.try_into().unwrap()); 
        info!("crawl done. wait {} seconds..", delay.as_secs());
        actix::clock::delay_for(delay).await;
    }
    Ok(())
}


#[get("/health")]
async fn health() -> impl Responder {
    "ok"
}

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.service(health);
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    pretty_env_logger::init();

    let port = std::env::var("PORT").unwrap_or("8080".to_string());

    let live_directory_url = std::env::var("LIVE_DIRECTORY_URL").expect("LIVE_DIRECTORY_URL");
    let data_broker_url = std::env::var("DATA_BROKER_URL").expect("DATA_BROKER_URL");

    let part: u64 = std::env::var("PART").expect("PART").parse().expect("PART");
    let total: u64 = std::env::var("TOTAL").expect("TOTAL").parse().expect("TOTAL");

    let prometheus = PrometheusMetrics::new("api", Some("/metrics"), None);
    let metrics = ResultMetricGauges {
        gallery_success: IntGauge::new("gallery_success", "gallery_success").unwrap(),
        gallery_error: IntGauge::new("gallery_error", "gallery_error").unwrap(),
        document_success: IntGauge::new("document_success", "document_success").unwrap(),
        document_error: IntGauge::new("document_error", "document_error").unwrap(),
        comment_success: IntGauge::new("comment_success", "comment_success").unwrap(),
        comment_error: IntGauge::new("comment_error", "comment_error").unwrap(),
    };

    let reg = prometheus.clone().registry;
    reg.register(Box::new(metrics.gallery_success.clone())).unwrap();
    reg.register(Box::new(metrics.gallery_error.clone())).unwrap();
    reg.register(Box::new(metrics.document_success.clone())).unwrap();
    reg.register(Box::new(metrics.document_error.clone())).unwrap();
    reg.register(Box::new(metrics.comment_success.clone())).unwrap();
    reg.register(Box::new(metrics.comment_error.clone())).unwrap();

    actix_rt::spawn(async move { 
        loop {
            let state = State::new(
                &live_directory_url, 
                &data_broker_url,
                total,
                part);
            let res = crawl_forever(
                state, 
                Duration::from_secs(60), 
                metrics.clone()).await;
            if let Err(e) = res {
                error!("crawler restart due to: {}", e.to_string());
            }
        }
    });
    HttpServer::new(move || {
        App::new()
            .wrap(prometheus.clone())
            .configure(config)
    })
        .bind(format!("0.0.0.0:{}", port))?
        .workers(1)
        .run()
        .await
}


#[cfg(test)]
mod tests {
    
    

    /*
    #[actix_rt::test]
    async fn state_update_list_part() {
        let mut state = State::new(
            live_directory_url: "",
            data_broker_url: "",
            total: 1u64,
            part: 1u64);
        state.update().await.unwrap();
        let res1 = state.list_part(2, 0);
        let res2 = state.list_part(2, 1);
        assert!(res1.len() > 0);
        assert!(res2.len() > 0);
        let mut h = std::collections::HashSet::new();
        for t in res1.iter() { h.insert(t.index.id.clone()); }
        for t in res2.iter() { h.insert(t.index.id.clone()); }
        assert_eq!(h.len(), res1.len() + res2.len());
    }
    #[actix_rt::test]
    async fn state_report() {
        let mut state = State::new("");
        state.update().await.unwrap();
        let res1 = state.list_part(2, 0);
        assert!(res1[0].last_crawled_at.is_none());
        let now = Utc::now();
        state.report(GalleryCrawlReportForm{
            id: res1[0].index.id.clone(),
            last_crawled_at: Some(now.clone()),
            last_crawled_document_id: Some(1),
        }).unwrap();
        let res1 = state.list_part(2, 0);
        assert_eq!(res1[0].last_crawled_at, Some(now));
        assert_eq!(res1[0].last_crawled_document_id, Some(1));
    }

    #[actix_rt::test]
    async fn test_health() {
        let mut app = test::init_service(App::new().configure(config)).await;
        let req = test::TestRequest::get().uri("/health").to_request();
        let resp = test::call_service(&mut app, req).await;
        assert_eq!(resp.status(), http::StatusCode::OK);
    }
    */
}