use crate::{
    Result,
    utils::{cache::Cache, normalize_url},
};
use log::info;
use reqwest::blocking::Client;
use url::Url;

const CACHE_PATH: &str = "software-companies-in-bangladesh";

pub fn clear_cache() -> Result {
    Cache::clear(CACHE_PATH)
}

pub fn fetch(url: &Url) -> Result<String> {
    let url = normalize_url(url)?;

    let cache = Cache::open(CACHE_PATH, url.as_str())?;

    if let Some(data) = cache.get()? {
        return Ok(data);
    }

    info!("[FETCH] {}", url);

    let data = client().get(url).send()?.error_for_status()?.text()?;

    cahce.set(&data)?;

    Ok(data)
}

fn client() -> &'static Client {
    use reqwest::{
        blocking::Client,
        header::{HeaderMap, HeaderValue, USER_AGENT},
        redirect::Policy,
    };
    use std::{sync::OnceLock, time::Duration};

    static CLIENT: OnceLock<Client> = OnceLock::new();

    let agents = HeaderValue::from_static(
        "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/139.0.0.0 Safari/537.36",
    );

    CLIENT.get_or_init(|| {
        let mut headers = HeaderMap::new();

        headers.insert(USER_AGENT, agents);

        Client::builder()
            .default_headers(headers)
            .redirect(Policy::limited(10))
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(15))
            .tcp_keepalive(Some(Duration::from_secs(30)))
            .gzip(true)
            .zstd(true)
            .build()
            .expect("failed to build http client")
    })
}
