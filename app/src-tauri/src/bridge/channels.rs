//! Fetch-backed binary channels keep live JSON out of WebView eval source.
use std::{collections::VecDeque, sync::Mutex};
use tauri::ipc::{Channel, InvokeResponseBody};

struct Subscription {
    id: u32,
    label: String,
    event: String,
    channel: Channel<InvokeResponseBody>,
}

#[derive(Default)]
struct Subscriptions {
    next_id: u32,
    rows: VecDeque<Subscription>,
}

#[derive(Default)]
pub struct LivePayloads(Mutex<Subscriptions>);

impl LivePayloads {
    pub fn subscribe(&self, label: &str, event: String, channel: Channel) -> Result<u32, String> {
        if !matches!(event.as_str(), "live-snapshot" | "engine-event") {
            return Err(super::DELIVERY_FAILED.into());
        }
        let mut subscriptions = self.0.lock().map_err(|_| super::DELIVERY_FAILED)?;
        // Native ownership IDs survive JS counter resets on reload and are
        // independent of channel IDs reused by another webview.
        let mut id = subscriptions.next_id;
        while subscriptions.rows.iter().any(|row| row.id == id) {
            id = id.wrapping_add(1);
        }
        subscriptions.next_id = id.wrapping_add(1);
        // Lifecycle cleanup is primary; a missed cleanup must remain recoverable.
        if subscriptions.rows.len() >= 16 {
            subscriptions.rows.pop_front();
        }
        subscriptions.rows.push_back(Subscription {
            id,
            label: label.into(),
            event,
            channel,
        });
        Ok(id)
    }

    pub fn unsubscribe(&self, label: &str, id: u32) -> Result<(), String> {
        self.0
            .lock()
            .map_err(|_| super::DELIVERY_FAILED)?
            .rows
            .retain(|row| row.id != id || row.label != label);
        Ok(())
    }

    pub fn purge(&self, label: &str) -> Result<(), String> {
        self.0
            .lock()
            .map_err(|_| super::DELIVERY_FAILED)?
            .rows
            .retain(|row| row.label != label);
        Ok(())
    }

    pub fn page_load(
        &self,
        label: &str,
        event: tauri::webview::PageLoadEvent,
    ) -> Result<(), String> {
        if event == tauri::webview::PageLoadEvent::Started {
            self.purge(label)?;
        }
        Ok(())
    }

    pub(super) fn send(&self, event: &str, value: &impl serde::Serialize) -> Result<(), ()> {
        let mut subscriptions = self.0.lock().map_err(|_| ())?;
        if !subscriptions.rows.iter().any(|row| row.event == event) {
            return Ok(());
        }
        let mut bytes = serde_json::to_vec(value).map_err(|_| ())?;
        // Tauri 2.12 sends raw bodies below 1024 bytes as inline JS arrays.
        // JSON whitespace keeps even tiny live messages on its fetch path.
        if bytes.len() < 1024 {
            bytes.resize(1024, b' ');
        }
        // A destroyed recipient must not stop delivery to the others or latch
        // the publisher's global failure. Remove only the failed ownership.
        subscriptions.rows.retain(|row| {
            row.event != event
                || row
                    .channel
                    .send(InvokeResponseBody::Raw(bytes.clone()))
                    .is_ok()
        });
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn abandoned_subscriptions_do_not_permanently_exhaust_capacity() {
        let payloads = LivePayloads::default();
        let ids: Vec<_> = (0..40)
            .map(|_| {
                payloads
                    .subscribe("main", "engine-event".into(), Channel::new(|_| Ok(())))
                    .unwrap()
            })
            .collect();
        let retained: Vec<_> = payloads
            .0
            .lock()
            .unwrap()
            .rows
            .iter()
            .map(|row| row.id)
            .collect();
        assert_eq!(
            retained,
            ids[24..],
            "only the oldest abandoned ownerships are evicted"
        );
    }

    #[test]
    fn dead_channel_is_removed_without_interrupting_healthy_delivery() {
        let payloads = LivePayloads::default();
        let dead = payloads
            .subscribe(
                "main",
                "engine-event".into(),
                Channel::new(|_| {
                    Err(tauri::Error::Io(std::io::Error::other(
                        "closed test channel",
                    )))
                }),
            )
            .unwrap();
        let received = Arc::new(Mutex::new(0));
        let sink = received.clone();
        payloads
            .subscribe(
                "main",
                "engine-event".into(),
                Channel::new(move |_| {
                    *sink.lock().unwrap() += 1;
                    Ok(())
                }),
            )
            .unwrap();
        assert!(payloads.send("engine-event", &1).is_ok());
        assert!(payloads.send("engine-event", &2).is_ok());
        assert_eq!(*received.lock().unwrap(), 2);
        assert!(!payloads
            .0
            .lock()
            .unwrap()
            .rows
            .iter()
            .any(|row| row.id == dead));
    }

    #[test]
    fn reload_and_destroy_purge_only_the_owning_webview() {
        use tauri::webview::PageLoadEvent;
        let payloads = LivePayloads::default();
        let channel = Channel::new(|_| Ok(()));
        let other = payloads
            .subscribe("other", "engine-event".into(), channel.clone())
            .unwrap();
        for _ in 0..40 {
            for event in ["engine-event", "live-snapshot"] {
                payloads
                    .subscribe("main", event.into(), channel.clone())
                    .unwrap();
            }
            payloads.page_load("main", PageLoadEvent::Finished).unwrap();
            assert_eq!(payloads.0.lock().unwrap().rows.len(), 3);
            payloads.page_load("main", PageLoadEvent::Started).unwrap();
            assert_eq!(payloads.0.lock().unwrap().rows.len(), 1);
        }
        payloads.unsubscribe("main", other).unwrap();
        assert_eq!(payloads.0.lock().unwrap().rows.len(), 1);
        // The owning WebviewWindow's Destroyed hook uses the same label purge.
        payloads.purge("other").unwrap();
        assert!(payloads.0.lock().unwrap().rows.is_empty());
    }

    #[test]
    fn late_release_after_reload_cannot_remove_a_new_subscription() {
        let payloads = LivePayloads::default();
        let channel = Channel::new(|_| Ok(()));
        let old = payloads
            .subscribe("main", "engine-event".into(), channel.clone())
            .unwrap();
        payloads.purge("main").unwrap();
        let current = payloads
            .subscribe("main", "engine-event".into(), channel)
            .unwrap();
        assert_ne!(old, current);
        payloads.unsubscribe("main", old).unwrap();
        assert_eq!(payloads.0.lock().unwrap().rows.len(), 1);
        assert_eq!(payloads.0.lock().unwrap().rows[0].id, current);
    }

    #[test]
    fn subscriptions_preserve_order_use_fetch_bodies_and_release_ownership() {
        let received = Arc::new(Mutex::new(Vec::new()));
        let sink = received.clone();
        let channel = Channel::new(move |body| {
            let InvokeResponseBody::Raw(bytes) = body else {
                panic!("inline live payload");
            };
            assert!(bytes.len() >= 1024);
            sink.lock()
                .unwrap()
                .push(serde_json::from_slice::<u64>(&bytes).unwrap());
            Ok(())
        });
        let payloads = LivePayloads::default();
        let id = payloads
            .subscribe("main", "engine-event".into(), channel)
            .unwrap();
        payloads.send("live-snapshot", &9).unwrap();
        payloads.send("engine-event", &1).unwrap();
        payloads.send("engine-event", &2).unwrap();
        payloads.unsubscribe("main", id).unwrap();
        payloads.send("engine-event", &3).unwrap();
        assert_eq!(*received.lock().unwrap(), [1, 2]);
        assert!(payloads.0.lock().unwrap().rows.is_empty());
    }
}
