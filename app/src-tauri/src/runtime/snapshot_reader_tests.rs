//! Reader shutdown and restart coverage with an explicitly pending reply.
use super::*;

#[test]
fn native_snapshot_reads_survive_stop_and_a_new_capture() {
    let runtime = Arc::new(CaptureRuntime::default());
    let timeout = Duration::from_secs(5);
    let mut previous_stream = None;
    for _ in 0..2 {
        let (pending, waiting) = bounded(1);
        *runtime.shared.before_worker_loop.lock().unwrap() =
            Some(Arc::new(move |requests, stopped| {
                // Claim the request without replying. The reader must now wait
                // for this reply sender to disconnect when the worker exits.
                let reply = requests.recv_timeout(timeout).unwrap();
                pending.send(()).unwrap();
                stopped.recv_timeout(timeout).unwrap();
                reply
            }));
        let (sink, _) = sink();
        let (_frames, _) = start_fake(&runtime, sink);
        let reading = runtime.clone();
        let (done, reads) = bounded(1);
        let reader = thread::spawn(move || done.send(reading.snapshot()).unwrap());
        waiting.recv_timeout(timeout).unwrap();
        assert!(
            reads.try_recv().is_err(),
            "the reader is waiting for a reply"
        );
        let stream = runtime
            .shared
            .views
            .snapshot
            .lock()
            .unwrap()
            .stream_id
            .clone();
        assert_ne!(Some(&stream), previous_stream.as_ref());
        previous_stream = Some(stream.clone());

        let stopping = runtime.clone();
        let (done, stops) = bounded(1);
        let stopper = thread::spawn(move || done.send(stopping.stop()).unwrap());
        let fallback = reads
            .recv_timeout(timeout)
            .expect("worker exit releases reader");
        assert_eq!(fallback.stream_id, stream);
        // The hook never sent a snapshot, so this value can only come from cache.
        assert!(fallback.session.is_none());
        stops
            .recv_timeout(timeout)
            .expect("Stop joins the worker")
            .unwrap();
        reader.join().unwrap();
        stopper.join().unwrap();
        assert_eq!(runtime.snapshot().capture.state, CaptureState::Stopped);
    }
    // A subsequent worker must replace the disconnected request channel.
    let (sink, _) = sink();
    let (frames, _) = start_fake(&runtime, sink);
    frames.send(frame(10, true, &[0, 0, 0, 5, 105])).unwrap();
    wait_until(|| runtime.status().state == CaptureState::Live);
    let (done, reads) = bounded(1);
    let reading = runtime.clone();
    let reader = thread::spawn(move || done.send(reading.snapshot()).unwrap());
    let fresh = reads.recv_timeout(timeout).unwrap();
    assert!(fresh.session.is_some());
    assert_ne!(Some(&fresh.stream_id), previous_stream.as_ref());
    reader.join().unwrap();
    runtime.stop().unwrap();
}
