//! Separate SQLite connections meet one freshly created owner database at once.
use aikit_store::index::Index;
use std::sync::{Arc,Barrier};

#[test]
fn concurrent_first_open_applies_each_migration_once_atomically() {
    let root=tempfile::tempdir().unwrap();
    for round in 0..8 {
        let path=root.path().join(format!("index-{round}.sqlite"));
        let barrier=Arc::new(Barrier::new(8));
        let workers=(0..8).map(|_| {
            let path=path.clone();let barrier=barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                let index=Index::open(&path).expect("concurrent native open must wait for complete schema");
                (index.schema_version().unwrap(),index.applied_migrations().unwrap().len())
            })
        }).collect::<Vec<_>>();
        let results=workers.into_iter().map(|worker|worker.join().unwrap()).collect::<Vec<_>>();
        assert!(results.iter().all(|result|*result==results[0]));
        assert_eq!(results[0].0 as usize,results[0].1,"one receipt per applied migration");
    }
}
