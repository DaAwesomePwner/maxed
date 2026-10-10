use super::*;
use std::{
    collections::VecDeque,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
};

#[derive(Default)]
pub(crate) struct Script {
    pub replies: Mutex<VecDeque<Vec<u8>>>,
    pub gate: Mutex<Option<Arc<tokio::sync::Semaphore>>>,
    pub requests: AtomicUsize,
}
impl Script {
    pub(super) async fn get(&self) -> Result<Response<HttpsBody>> {
        let reply = self
            .replies
            .lock()
            .unwrap()
            .pop_front()
            .ok_or(Error::Network)?;
        let gate = self.gate.lock().unwrap().clone();
        self.requests.fetch_add(1, Ordering::AcqRel);
        if let Some(gate) = gate {
            gate.acquire().await.unwrap().forget();
        }
        Ok(Response {
            status: 200,
            location: None,
            length: Some(reply.len() as u64),
            body: HttpsBody::Fake(Some(Bytes::from(reply))),
        })
    }
}
pub(crate) enum HttpsBody {
    Network(reqwest::Response),
    Fake(Option<Bytes>),
}
impl From<reqwest::Response> for HttpsBody {
    fn from(value: reqwest::Response) -> Self {
        Self::Network(value)
    }
}
impl Body for HttpsBody {
    async fn chunk(&mut self) -> Result<Option<Bytes>> {
        match self {
            Self::Network(body) => Body::chunk(body).await,
            Self::Fake(bytes) => Ok(bytes.take()),
        }
    }
}
