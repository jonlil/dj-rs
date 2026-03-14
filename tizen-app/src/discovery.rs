use futures::future::{join_all, select, Either};
use gloo_timers::future::TimeoutFuture;
use js_sys::Promise;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_name = getLocalIp)]
    fn get_local_ip_js() -> Promise;
}

pub async fn get_local_ip() -> Option<String> {
    let val = JsFuture::from(get_local_ip_js()).await.ok()?;
    val.as_string().filter(|s| !s.is_empty())
}

pub async fn find_desktop() -> Option<String> {
    let local_ip = get_local_ip().await?;
    let parts: Vec<&str> = local_ip.split('.').collect();
    if parts.len() != 4 {
        return None;
    }
    let subnet = format!("{}.{}.{}", parts[0], parts[1], parts[2]);

    let checks: Vec<_> = (1u32..=254)
        .map(|i| {
            let subnet = subnet.clone();
            let url = format!("http://{}.{}:7879/ping", subnet, i);
            async move {
                let fetch = gloo_net::http::Request::get(&url).send();
                let timeout = TimeoutFuture::new(800);
                match select(Box::pin(fetch), Box::pin(timeout)).await {
                    Either::Left((Ok(resp), _)) if resp.status() == 200 => {
                        let text = resp.text().await.ok()?;
                        if text.contains("dj-rs") {
                            Some(format!("http://{}.{}:7879", subnet, i))
                        } else {
                            None
                        }
                    }
                    _ => None,
                }
            }
        })
        .collect();

    let results = join_all(checks).await;
    results.into_iter().flatten().next()
}
