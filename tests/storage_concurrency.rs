use std::sync::{
    Arc, Barrier,
    atomic::{AtomicBool, Ordering},
};
use timetoll::{engine::Ledger, storage::Store};

#[test]
fn checkpoints_survive_concurrent_status_reads() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::new(Some(dir.path().to_owned())).unwrap();
    store.init().unwrap();
    store.save_ledger(&Ledger::default()).unwrap();
    let start = Arc::new(Barrier::new(2));
    let finished = AtomicBool::new(false);
    std::thread::scope(|scope| {
        let reader = scope.spawn(|| {
            start.wait();
            let mut reads = 0;
            loop {
                // These are the same reads status --watch performs each second.
                let config = store.config().unwrap();
                let mut ledger = store.ledger().unwrap();
                ledger.set_ratio(config.ratio);
                assert!(ledger.balance_ms <= 250);
                reads += 1;
                if finished.load(Ordering::Acquire) {
                    return reads;
                }
            }
        });
        start.wait();
        let result = (|| -> anyhow::Result<()> {
            for balance_ms in 1..=250 {
                store.save_ledger(&Ledger {
                    balance_ms,
                    ..Ledger::default()
                })?;
            }
            Ok(())
        })();
        // Signal even on an I/O error so a failure cannot strand the reader.
        finished.store(true, Ordering::Release);
        result.unwrap();
        assert!(reader.join().unwrap() > 0);
    });
    assert_eq!(store.ledger().unwrap().balance_ms, 250);
}
