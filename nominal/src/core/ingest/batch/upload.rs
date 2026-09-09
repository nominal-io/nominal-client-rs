use super::{BatchItemFailure, BatchSourceFailure, FailurePolicy, items::PendingItem};
use crate::Result;
use futures::{StreamExt, stream::FuturesUnordered};
use std::{collections::BTreeMap, future::Future};

pub(super) struct UploadReport {
    pub completed: Vec<nominal_api::tonic::nominal::ingest::v2::IngestItem>,
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
    let mut completed = Vec::new();
    let mut failures = Vec::new();
    for (index, item) in items.iter().enumerate() {
        // Encoding succeeds only when every source has an uploaded location.
        // Unscheduled sources in fail-fast mode also make their item incomplete.
        if let Some(encoded) = super::encode::encode(item, &locations) {
            completed.push(encoded);
        } else {
            failures.push(BatchItemFailure {
                item_index: index,
                failed_sources: failed.remove(&index).unwrap_or_default(),
                uploaded_sources: item
                    .uploads()
                    .into_iter()
                    .filter(|u| locations.contains_key(&u.id))
                    .map(|u| u.path.clone())
                    .collect(),
            });
        }
    }
    UploadReport {
        completed,
        failures,
    }
}
