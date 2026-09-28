//! Byte-channel transport to the radio.
//!
//! Native builds open one UDP socket per channel. The browser cannot open raw
//! UDP sockets, so the wasm build opens a single multiplexed WebSocket to a
//! `scu-bridge` process that relays frames to the four UDP sockets. Each
//! WebSocket binary frame is tagged with the physical channel: `0 = ctrl`,
//! `1 = cat`, `2 = audio`, `3 = scope`.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use crate::config::ConnectConfig;
use crate::ClientError;

/// A boxed future with an explicit lifetime (dyn-safe async methods).
///
/// Native futures must be `Send` (the engine runs on its own thread); browser
/// futures need not be, and some Web APIs (`gloo-timers`) return non-`Send`
/// futures.
#[cfg(not(target_arch = "wasm32"))]
pub type BoxFut<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;
#[cfg(target_arch = "wasm32")]
pub type BoxFut<'a, T> = Pin<Box<dyn Future<Output = T> + 'a>>;

/// A single physical channel (one UDP socket, or one WebSocket tag).
pub trait Channel: Send + Sync {
    /// Best-effort synchronous send. Safe to call from the UI thread.
    fn send_now(&self, data: &[u8]) -> std::io::Result<()>;

    /// Reliable-ish async send (used during the handshake).
    fn send<'a>(&'a self, data: &'a [u8]) -> BoxFut<'a, std::io::Result<()>>;

    /// Await the next datagram into `buf`, returning its length in bytes.
    fn recv<'a>(&'a self, buf: &'a mut [u8]) -> BoxFut<'a, std::io::Result<usize>>;
}

/// A reference-counted channel usable from both the UI and the engine.
pub type SharedChannel = Arc<dyn Channel>;

/// The four physical channels to the radio.
#[derive(Clone)]
pub struct Channels {
    pub ctrl: SharedChannel,
    pub cat: SharedChannel,
    pub audio: SharedChannel,
    pub scope: SharedChannel,
}

/// Open the four channels described by `config`.
pub async fn open(config: &ConnectConfig) -> Result<Channels, ClientError> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        udp::open(config).await
    }
    #[cfg(target_arch = "wasm32")]
    {
        ws::open(config).await
    }
}

/// Sleep for `duration` on either runtime.
pub async fn sleep(duration: Duration) {
    #[cfg(not(target_arch = "wasm32"))]
    tokio::time::sleep(duration).await;
    #[cfg(target_arch = "wasm32")]
    gloo_timers::future::TimeoutFuture::new(duration.as_millis() as u32).await;
}

/// Run `future`, returning `None` if `duration` elapses first.
pub async fn timeout<F>(duration: Duration, future: F) -> Option<F::Output>
where
    F: Future,
{
    let future = std::pin::pin!(future);
    let timer = std::pin::pin!(sleep(duration));
    match futures::future::select(future, timer).await {
        futures::future::Either::Left((output, _)) => Some(output),
        futures::future::Either::Right(_) => None,
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod udp {
    use super::*;
    use tokio::net::UdpSocket;

    struct UdpChannel(UdpSocket);

    impl Channel for UdpChannel {
        fn send_now(&self, data: &[u8]) -> std::io::Result<()> {
            self.0.try_send(data).map(|_| ())
        }

        fn send<'a>(&'a self, data: &'a [u8]) -> BoxFut<'a, std::io::Result<()>> {
            Box::pin(async move { self.0.send(data).await.map(|_| ()) })
        }

        fn recv<'a>(&'a self, buf: &'a mut [u8]) -> BoxFut<'a, std::io::Result<usize>> {
            Box::pin(self.0.recv(buf))
        }
    }

    async fn bind(host: &str, port: u16) -> std::io::Result<Arc<UdpChannel>> {
        let sock = UdpSocket::bind(("0.0.0.0", 0)).await?;
        sock.connect((host, port)).await?;
        Ok(Arc::new(UdpChannel(sock)))
    }

    pub async fn open(config: &ConnectConfig) -> Result<Channels, ClientError> {
        Ok(Channels {
            ctrl: bind(&config.host, config.ctrl_port()).await?,
            cat: bind(&config.host, config.cat_port()).await?,
            audio: bind(&config.host, config.audio_port()).await?,
            scope: bind(&config.host, config.scope_port()).await?,
        })
    }
}

#[cfg(target_arch = "wasm32")]
mod ws {
    use std::collections::VecDeque;
    use std::sync::Mutex;

    use futures::task::AtomicWaker;
    use js_sys::Uint8Array;
    use wasm_bindgen::prelude::*;
    use wasm_bindgen_futures::JsFuture;
    use web_sys::{BinaryType, MessageEvent};

    use super::*;

    const TAGS: usize = 4;

    struct WsInner {
        socket: web_sys::WebSocket,
        queues: Mutex<[VecDeque<Vec<u8>>; TAGS]>,
        wakers: [AtomicWaker; TAGS],
    }

    // The browser is single-threaded: the socket is only ever touched from the
    // one thread that owns the wasm instance, so sharing it is sound.
    unsafe impl Send for WsInner {}
    unsafe impl Sync for WsInner {}

    struct WsChannel {
        inner: Arc<WsInner>,
        tag: u8,
    }

    impl Channel for WsChannel {
        fn send_now(&self, data: &[u8]) -> std::io::Result<()> {
            let mut frame = Vec::with_capacity(data.len() + 1);
            frame.push(self.tag);
            frame.extend_from_slice(data);
            self.inner
                .socket
                .send_with_u8_array(&frame)
                .map_err(|e| js_error(&e))
        }

        fn send<'a>(&'a self, data: &'a [u8]) -> BoxFut<'a, std::io::Result<()>> {
            Box::pin(async move { self.send_now(data) })
        }

        fn recv<'a>(&'a self, buf: &'a mut [u8]) -> BoxFut<'a, std::io::Result<usize>> {
            Box::pin(Recv { channel: self, buf })
        }
    }

    struct Recv<'a> {
        channel: &'a WsChannel,
        buf: &'a mut [u8],
    }

    impl std::future::Future for Recv<'_> {
        type Output = std::io::Result<usize>;

        fn poll(
            self: std::pin::Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<Self::Output> {
            let this = self.get_mut();
            let tag = this.channel.tag as usize;
            let inner = &this.channel.inner;

            if let Some(n) = take(inner, tag, this.buf) {
                return std::task::Poll::Ready(Ok(n));
            }
            inner.wakers[tag].register(cx.waker());
            if let Some(n) = take(inner, tag, this.buf) {
                return std::task::Poll::Ready(Ok(n));
            }
            std::task::Poll::Pending
        }
    }

    fn take(inner: &WsInner, tag: usize, buf: &mut [u8]) -> Option<usize> {
        let mut queues = inner.queues.lock().unwrap();
        let message = queues[tag].pop_front()?;
        let n = message.len().min(buf.len());
        buf[..n].copy_from_slice(&message[..n]);
        Some(n)
    }

    fn js_error(value: &JsValue) -> std::io::Error {
        std::io::Error::other(format!("{value:?}"))
    }

    fn format_error(value: &JsValue) -> String {
        value.as_string().unwrap_or_else(|| format!("{value:?}"))
    }

    pub async fn open(config: &ConnectConfig) -> Result<Channels, ClientError> {
        let Some(url) = config.bridge_url.as_deref() else {
            return Err(ClientError::BridgeUrl);
        };
        if !url.starts_with("ws://") && !url.starts_with("wss://") {
            return Err(ClientError::BridgeUrl);
        }

        let socket = web_sys::WebSocket::new(url)
            .map_err(|_| ClientError::Bridge(format!("invalid WebSocket URL: {url}")))?;
        socket.set_binary_type(BinaryType::Arraybuffer);

        let inner = Arc::new(WsInner {
            socket: socket.clone(),
            queues: Mutex::new(std::array::from_fn(|_| VecDeque::new())),
            wakers: std::array::from_fn(|_| AtomicWaker::new()),
        });

        let handler_inner = Arc::clone(&inner);
        let onmessage = Closure::<dyn FnMut(MessageEvent)>::new(move |event: MessageEvent| {
            let data = event.data();
            let Some(buffer) = data.dyn_ref::<js_sys::ArrayBuffer>() else {
                return;
            };
            let bytes = Uint8Array::new(buffer).to_vec();
            let Some((&tag, payload)) = bytes.split_first() else {
                return;
            };
            let tag = tag as usize;
            if tag >= TAGS {
                return;
            }
            handler_inner.queues.lock().unwrap()[tag].push_back(payload.to_vec());
            handler_inner.wakers[tag].wake();
        });
        socket.set_onmessage(Some(onmessage.as_ref().unchecked_ref()));
        onmessage.forget();

        await_open(&socket).await?;

        let hello = serde_json::json!({
            "host": config.host,
            "base_port": config.base_port,
        });
        socket
            .send_with_str(&hello.to_string())
            .map_err(|e| ClientError::Bridge(format_error(&e)))?;

        Ok(Channels {
            ctrl: Arc::new(WsChannel {
                inner: Arc::clone(&inner),
                tag: 0,
            }),
            cat: Arc::new(WsChannel {
                inner: Arc::clone(&inner),
                tag: 1,
            }),
            audio: Arc::new(WsChannel {
                inner: Arc::clone(&inner),
                tag: 2,
            }),
            scope: Arc::new(WsChannel {
                inner: Arc::clone(&inner),
                tag: 3,
            }),
        })
    }

    async fn await_open(socket: &web_sys::WebSocket) -> Result<(), ClientError> {
        if socket.ready_state() == web_sys::WebSocket::OPEN {
            return Ok(());
        }
        let promise = js_sys::Promise::new(&mut |resolve, reject| {
            let onopen = Closure::once_into_js(move || {
                let _ = resolve.call0(&JsValue::UNDEFINED);
            });
            socket.set_onopen(Some(onopen.unchecked_ref()));
            let onerror = Closure::once_into_js(move |event: JsValue| {
                let _ = reject.call1(&JsValue::UNDEFINED, &event);
            });
            socket.set_onerror(Some(onerror.unchecked_ref()));
        });
        JsFuture::from(promise)
            .await
            .map(|_| ())
            .map_err(|e| ClientError::Bridge(format_error(&e)))
    }
}
