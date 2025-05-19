/// Code related to asynchronous features.
///
/// As wasm32 and most other platforms behave differently, there are these wrappers.
use futures_core::Future;
use log::info;
use serde::Deserialize;

#[derive(Debug, Deserialize, PartialEq, Eq)]
pub enum AsyncJob {
    SaveState,
}

// Wasm doesn't seem to support std::thread, so this spawns a thread where we can
// but runs the work sequentially where we can not.
pub fn perform_work<F>(f: F)
where
    F: FnOnce() + Send + 'static,
{
    #[cfg(target_arch = "wasm32")]
    {
        wasm_bindgen_futures::spawn_local(async {
            info!("Starting async task");
            f();
        });
        info!("Returning from perform work")
    }

    #[cfg(not(target_arch = "wasm32"))]
    {
        tokio::spawn(async {
            info!("Starting async task");
            f();
        });
        info!("Returning from perform work");
    }
}

// NOTE: wasm32 does not require a Send bound.
#[cfg(target_arch = "wasm32")]
pub fn perform_async_work<F>(f: F)
where
    F: Future<Output = ()> + 'static,
{
    wasm_bindgen_futures::spawn_local(f);
}

// NOTE: not wasm32 requires a Send bound too.
#[cfg(not(target_arch = "wasm32"))]
pub fn perform_async_work<F>(f: F)
where
    F: Future<Output = ()> + Send + 'static,
{
    tokio::spawn(f);
}

#[cfg(target_arch = "wasm32")]
pub async fn sleep_ms(delay: u64) {
    use wasm_bindgen_futures::js_sys;

    let mut cb = |resolve: js_sys::Function, _reject: js_sys::Function| {
        web_sys::window()
            .unwrap()
            .set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, delay as i32)
            .unwrap();
    };

    let p = js_sys::Promise::new(&mut cb);

    wasm_bindgen_futures::JsFuture::from(p).await.unwrap();
}

#[cfg(not(target_arch = "wasm32"))]
pub async fn sleep_ms(delay_ms: u64) {
    tokio::time::sleep(tokio::time::Duration::from_millis(delay_ms)).await;
}
