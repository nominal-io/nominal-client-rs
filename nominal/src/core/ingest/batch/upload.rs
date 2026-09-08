use super::{BatchItemFailure, BatchSourceFailure, FailurePolicy, items::PendingItem};
use crate::Result;
use futures::{StreamExt, stream::FuturesUnordered};
use std::{
    collections::{BTreeMap, BTreeSet},
    future::Future,
};

pub(super) struct UploadReport {
    pub locations: BTreeMap<usize, String>,
    pub failures: Vec<BatchItemFailure>,
}
/// Stop scheduling on the first observed error in fail-fast mode, then settle all active uploads.
pub(super) async fn upload_all<F, Fut>(
    items: &[PendingItem],
    limit: usize,
    policy: FailurePolicy,
    upload: F,
) -> UploadReport
where
    F: Fn(std::path::PathBuf) -> Fut,
    Fut: Future<Output = Result<String>>,
{
    let mut pending = items
        .iter()
        .enumerate()
        .flat_map(|(index, item)| item.uploads().into_iter().map(move |u| (index, u)));
    let mut active = FuturesUnordered::new();
    let mut locations = BTreeMap::new();
    let mut failed: BTreeMap<usize, Vec<BatchSourceFailure>> = BTreeMap::new();
    let mut stopped = false;
    loop {
        while !stopped && active.len() < limit {
            let Some((index, input)) = pending.next() else {
                break;
            };
            let future = upload(input.path.clone());
            active.push(async move { (index, input, future.await) });
        }
        let Some((index, input, result)) = active.next().await else {
            break;
        };
        match result {
            Ok(location) => {
                locations.insert(input.id, location);
            }
            Err(error) => {
                failed.entry(index).or_default().push(BatchSourceFailure {
                    name: input.name.clone(),
                    path: input.path.clone(),
                    error,
                });
                if policy == FailurePolicy::FailFast {
                    stopped = true;
                }
            }
        }
    }
    // Unscheduled sources also identify the items omitted by fail-fast termination.
    let mut incomplete = BTreeSet::new();
    for (index, item) in items.iter().enumerate() {
        if item
            .uploads()
            .iter()
            .any(|u| !locations.contains_key(&u.id))
        {
            incomplete.insert(index);
        }
    }
    let failures = incomplete
        .into_iter()
        .map(|index| BatchItemFailure {
            item_index: index,
            failed_sources: failed.remove(&index).unwrap_or_default(),
            uploaded_sources: items[index]
                .uploads()
                .into_iter()
                .filter(|u| locations.contains_key(&u.id))
                .map(|u| u.path.clone())
                .collect(),
        })
        .collect();
    UploadReport {
        locations,
        failures,
    }
}
