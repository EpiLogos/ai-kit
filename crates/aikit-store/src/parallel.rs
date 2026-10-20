//! Independent reads of separate trees, a few at a time.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, PoisonError};

/// How many reads run at once: they wait on the file system and share nothing.
const WORKERS: usize = 8;

/// `map` over `items`, up to [`WORKERS`] at a time, the answers in the order of `items` — so a
/// caller that went through them in turn and now goes through them at once sees the same
/// answers in the same order.
pub(crate) fn in_parallel<T: Sync, R: Send>(items: &[T], map: impl Fn(&T) -> R + Sync) -> Vec<R> {
    if items.len() < 2 {
        return items.iter().map(map).collect();
    }
    let next = AtomicUsize::new(0);
    let answers: Vec<Mutex<Option<R>>> = items.iter().map(|_| Mutex::new(None)).collect();
    std::thread::scope(|scope| {
        for _ in 0..WORKERS.min(items.len()) {
            scope.spawn(|| loop {
                let position = next.fetch_add(1, Ordering::Relaxed);
                let Some(item) = items.get(position) else {
                    break;
                };
                *answers[position]
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner) = Some(map(item));
            });
        }
    });
    answers
        .into_iter()
        .map(|answer| {
            answer
                .into_inner()
                .unwrap_or_else(PoisonError::into_inner)
                .expect("every item is mapped")
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::in_parallel;

    #[test]
    fn answers_come_back_in_the_order_asked_though_later_items_finish_first() {
        let items: Vec<usize> = (0..100).collect();
        let doubled = in_parallel(&items, |item| {
            std::thread::sleep(std::time::Duration::from_micros((100 - *item as u64) * 20));
            item * 2
        });
        assert_eq!(doubled, (0..100).map(|item| item * 2).collect::<Vec<_>>());
        assert!(in_parallel::<usize, usize>(&[], |item| *item).is_empty());
        assert_eq!(in_parallel(&[7], |item| item + 1), vec![8]);
    }
}
