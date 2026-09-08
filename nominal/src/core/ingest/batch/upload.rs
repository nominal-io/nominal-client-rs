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
/// Stops scheduling after the first failure in fail-fast mode and waits for active uploads.
pub(super) async fn upload_all<F, Fut>(
    items: &[PendingItem],
    limit: usize,
    policy: FailurePolicy,
    upload: F,
) -> UploadReport
where
    F: Fn(std::path::PathBuf, &'static str) -> Fut,
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
            let future = upload(input.path.clone(), input.mime);
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
    // Include items with unscheduled sources in the failure report.
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

pub(super) fn completed_items(
    items: &[PendingItem],
    report: &UploadReport,
    policy: FailurePolicy,
) -> Option<Vec<nominal_api::tonic::nominal::ingest::v2::IngestItem>> {
    if policy == FailurePolicy::FailFast && !report.failures.is_empty() {
        return None;
    }
    let items: Vec<_> = items
        .iter()
        .enumerate()
        .filter(|(index, _)| {
            !report
                .failures
                .iter()
                .any(|failure| failure.item_index == *index)
        })
        .map(|(_, item)| super::encode::encode(item, &report.locations))
        .collect();
    if items.is_empty() { None } else { Some(items) }
}
