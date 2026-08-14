pub mod keyword_hinter;
pub mod levenshtein_distance;
pub mod logger;
pub mod text_file;

#[cfg(feature = "extra")]
pub mod cache;
#[cfg(feature = "extra")]
pub mod date;
#[cfg(feature = "extra")]
pub mod fetch;
#[cfg(feature = "extra")]
pub mod zlib;

#[cfg(feature = "crawler")]
pub mod http;

use url::Url;

pub fn chunks_exact<T>(v: Vec<T>, n: usize) -> impl Iterator<Item = Vec<T>> {
    assert!(n > 0);
    let mut iter = v.into_iter();

    std::iter::from_fn(move || {
        let chunk: Vec<_> = iter.by_ref().take(n).collect();
        (chunk.len() == n).then_some(chunk)
    })
}

pub trait StrIterExt<'a>: Iterator<Item = &'a str> {
    fn trimmed(self) -> impl Iterator<Item = &'a str>
    where
        Self: Sized,
    {
        self.map(str::trim).filter(|s| !s.is_empty())
    }
}

impl<'a, I> StrIterExt<'a> for I where I: Iterator<Item = &'a str> {}

pub fn url_host(url: &Url) -> Option<String> {
    let host = url.host_str()?.trim_start_matches("www.");
    Some(host.to_ascii_lowercase())
}

#[cfg(feature = "extra")]
pub fn normalize_url(url: &Url) -> crate::Result<Url> {
    let mut url = url.clone();
    url.set_fragment(None);

    let path = url.path().trim_end_matches('/').to_string();
    url.set_path(&path);

    Ok(url)
}

#[cfg(feature = "extra")]
pub fn resolve_url(base: &Url, input: &str) -> crate::Result<Url> {
    match Url::parse(input) {
        Ok(url) => Ok(normalize_url(&url)?),
        Err(_) => normalize_url(&base.join(input)?),
    }
}
