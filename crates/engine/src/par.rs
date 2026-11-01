//! Work done side by side on a few threads (encoding a file's layers, packing tiles), its results
//! in order.

/// How many threads work is spread over: one to a core, 8 at most.
pub fn cores() -> usize {
    std::thread::available_parallelism().map_or(1, |n| n.get()).clamp(1, 8)
}

/// How much memory work done side by side may hold at once (as many work together as fit): less
/// on a tablet than on a computer.
const BUDGET: usize = if cfg!(target_os = "android") { 256 << 20 } else { 1 << 30 };

/// `f` of each of `items`, worked out side by side on a few threads (each taking the next item
/// there is), in their order; none for one whose thread stopped.
pub fn side_by_side<T: Sync, R: Send>(items: &[T], f: impl Fn(&T) -> R + Sync) -> Vec<Option<R>> {
    side_by_side_within(items, 0, f)
}

/// [`side_by_side`] for work holding about `each` bytes an item while it is done: no more of them
/// at once than fit in the budget (one, at least).
pub fn side_by_side_within<T: Sync, R: Send>(items: &[T], each: usize, f: impl Fn(&T) -> R + Sync) -> Vec<Option<R>> {
    let fit = BUDGET.checked_div(each).unwrap_or(usize::MAX).max(1);
    let workers = cores().min(items.len()).min(fit);
    let next = std::sync::atomic::AtomicUsize::new(0);
    let mut out: Vec<Option<R>> = std::iter::repeat_with(|| None).take(items.len()).collect();
    std::thread::scope(|scope| {
        let (next, f) = (&next, &f);
        let workers: Vec<_> = (0..workers)
            .map(|_| {
                scope.spawn(move || {
                    let mut done = Vec::new();
                    loop {
                        let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        let Some(item) = items.get(i) else { break };
                        done.push((i, f(item)));
                    }
                    done
                })
            })
            .collect();
        for (i, r) in workers.into_iter().filter_map(|w| w.join().ok()).flatten() {
            if let Some(slot) = out.get_mut(i) {
                *slot = Some(r);
            }
        }
    });
    out
}

/// `f` of each `part`-long piece of `items` (the last perhaps shorter), side by side on a few
/// threads (each taking the next piece there is); no threads for items of one piece or less.
pub fn each_part<T: Send>(items: &mut [T], part: usize, f: impl Fn(&mut [T]) + Sync) {
    let part = part.max(1);
    if items.len() <= part {
        f(items);
        return;
    }
    let workers = cores().min(items.len().div_ceil(part));
    let parts = std::sync::Mutex::new(items.chunks_mut(part));
    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| {
                while let Some(piece) = parts.lock().ok().and_then(|mut p| p.next()) {
                    f(piece);
                }
            });
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn results_come_back_in_order() {
        let items: Vec<u32> = (0..1000).collect();
        let out = side_by_side(&items, |v| v * 2);
        assert!(out.iter().enumerate().all(|(i, r)| *r == Some(i as u32 * 2)));
        assert!(side_by_side(&[] as &[u32], |v| *v).is_empty());
        // work too big for two at once still comes back whole, in order
        let out = side_by_side_within(&items, BUDGET / 2 + 1, |v| v + 1);
        assert!(out.iter().enumerate().all(|(i, r)| *r == Some(i as u32 + 1)));
    }

    #[test]
    fn each_part_is_done_once() {
        for (len, part) in [(1000, 7), (1000, 1000), (5, 0), (0, 3)] {
            let mut items: Vec<u32> = (0..len).collect();
            each_part(&mut items, part, |piece| piece.iter_mut().for_each(|v| *v = *v * 3 + 1));
            assert!(items.iter().enumerate().all(|(i, v)| *v == i as u32 * 3 + 1), "{len} {part}");
        }
    }
}
